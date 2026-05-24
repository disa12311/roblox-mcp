/// tunnel.rs — Cloudflare Quick Tunnel với auto-restart

use crate::Config;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::watch;
use tracing::{debug, info, warn};

#[cfg(target_os = "windows")]
const CF_BIN: &str = "cloudflared.exe";
#[cfg(not(target_os = "windows"))]
const CF_BIN: &str = "cloudflared";

fn find_cloudflared() -> Result<PathBuf> {
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        let path = dir.join(CF_BIN);
        if path.exists() {
            info!("Found cloudflared: {}", path.display());
            return Ok(path);
        }
    }
    if let Ok(output) = std::process::Command::new(CF_BIN).arg("--version").output() {
        if output.status.success() {
            info!("Found cloudflared in PATH");
            return Ok(PathBuf::from(CF_BIN));
        }
    }
    bail!(
        "Không tìm thấy {CF_BIN}!\n\
        Cách fix:\n\
        1. Tải tại: https://github.com/cloudflare/cloudflared/releases/latest\n\
        2. File cần tải: cloudflared-windows-amd64.exe → đổi tên thành {CF_BIN}\n\
        3. Đặt cạnh roblox-studio-bridge.exe hoặc: winget install Cloudflare.cloudflared"
    )
}

/// Spawn một tunnel, đọc URL từ stderr, trả về URL + child handle.
async fn spawn_tunnel(cf: &PathBuf, port: u16) -> Result<(String, tokio::process::Child)> {
    let mut child = tokio::process::Command::new(cf)
        .args(["tunnel", "--no-autoupdate", "--url", &format!("http://localhost:{port}")])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Failed to spawn cloudflared")?;

    let stderr = child.stderr.take().context("Failed to capture stderr")?;
    let stdout = child.stdout.take().context("Failed to capture stdout")?;

    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await { debug!("[cf stdout] {line}"); }
    });

    let mut stderr_lines = BufReader::new(stderr).lines();

    let url = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        async {
            while let Ok(Some(line)) = stderr_lines.next_line().await {
                debug!("[cf] {line}");
                if let Some(u) = extract_tunnel_url(&line) { return Ok::<_, anyhow::Error>(u); }
                if line.contains("failed") || line.contains("error") { warn!("[cf] {line}"); }
            }
            bail!("cloudflared thoát mà không in URL")
        },
    )
    .await
    .context("Timeout 30s — cloudflared không phản hồi")??;

    // Drain stderr tiếp trong background
    tokio::spawn(async move {
        while let Ok(Some(line)) = stderr_lines.next_line().await { debug!("[cf] {line}"); }
    });

    Ok((url, child))
}

/// Khởi động tunnel và trả về `watch::Receiver<Option<String>>` để nhận URL mới
/// mỗi khi tunnel restart. Giá trị ban đầu là URL đầu tiên sau khi kết nối.
pub async fn start_tunnel(
    config: &Config,
) -> Result<(String, watch::Receiver<Option<String>>)> {
    let cf   = find_cloudflared()?;
    let port = config.mcp_port;

    if let Some(ref token) = config.cf_tunnel_token {
        // ── Named tunnel — URL cố định, không cần parse stdout ────
        info!("Starting Named Tunnel");
        let mut child = tokio::process::Command::new(&cf)
            .args(["tunnel", "--no-autoupdate", "run", "--token", token])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn cloudflared named tunnel")?;

        // Named tunnel URL được cấu hình trên Cloudflare dashboard
        // App không biết URL — trả placeholder để user điền
        let url = format!("(named tunnel — xem dashboard.cloudflare.com)");
        let (tx, rx) = watch::channel::<Option<String>>(Some(url.clone()));

        tokio::spawn(async move {
            let _ = child.wait().await;
            let _ = tx.send(None);
        });

        return Ok((url, rx));
    }

    // ── Quick tunnel ──────────────────────────────────────────────
    info!("Starting Quick Tunnel → http://localhost:{port}");
    let (url, child) = spawn_tunnel(&cf, port).await?;
    let (tx, rx) = watch::channel::<Option<String>>(Some(url.clone()));

    tokio::spawn(async move {
        let mut current_child = child;
        loop {
            match current_child.wait().await {
                Ok(status) => warn!("cloudflared exited: {status}"),
                Err(e)     => warn!("cloudflared wait error: {e}"),
            }
            let _ = tx.send(None);
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            info!("Restarting cloudflared tunnel…");
            match spawn_tunnel(&cf, port).await {
                Ok((new_url, new_child)) => {
                    info!("Tunnel restarted: {new_url}");
                    let _ = tx.send(Some(new_url));
                    current_child = new_child;
                }
                Err(e) => {
                    warn!("Failed to restart tunnel: {e}");
                    break;
                }
            }
        }
    });

    Ok((url, rx))
}

fn extract_tunnel_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest  = &line[start..];
    let end   = rest
        .find(|c: char| c.is_whitespace() || matches!(c, '|' | '"' | ')' | '>'))
        .unwrap_or(rest.len());
    let url = &rest[..end];
    if url.ends_with(".trycloudflare.com") { Some(url.to_string()) } else { None }
}

#[cfg(test)]
mod tests {
    use super::extract_tunnel_url;

    #[test]
    fn extracts_valid_tunnel_url() {
        assert_eq!(extract_tunnel_url("2024-01-01 INF +----+"), None);
        assert_eq!(
            extract_tunnel_url("2024-01-01 INF  |  https://abc123.trycloudflare.com  |"),
            Some("https://abc123.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn rejects_non_trycloudflare_urls() {
        assert_eq!(extract_tunnel_url("https://cloudflare.com/something"), None);
        assert_eq!(extract_tunnel_url("https://evil.com https://abc.trycloudflare.com"), None);
    }

    #[test]
    fn extracts_from_various_formats() {
        assert_eq!(
            extract_tunnel_url("Tunnel created at https://xyz789.trycloudflare.com"),
            Some("https://xyz789.trycloudflare.com".to_string())
        );
    }
}