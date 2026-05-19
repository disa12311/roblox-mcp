/// tunnel.rs — Tìm cloudflared.exe và spawn Cloudflare Quick Tunnel
///
/// Cải tiến so với v0.1:
/// - `find_cloudflared()` tìm thêm ở PATH nếu không thấy bên cạnh exe
/// - Log rõ hơn khi cloudflared crash sớm
/// - Hàm `extract_tunnel_url` tách riêng, dễ test
///
/// Tải cloudflared tại: https://github.com/cloudflare/cloudflared/releases/latest

use crate::Config;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, info, warn};

// ── cloudflared discovery ─────────────────────────────────────────

#[cfg(target_os = "windows")]
const CF_BIN: &str = "cloudflared.exe";
#[cfg(not(target_os = "windows"))]
const CF_BIN: &str = "cloudflared";

/// Tìm cloudflared theo thứ tự:
/// 1. Cùng thư mục với exe đang chạy
/// 2. PATH (nếu người dùng đã cài system-wide)
fn find_cloudflared() -> Result<PathBuf> {
    // Ưu tiên: cùng thư mục với binary hiện tại
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

    // Fallback: tìm trong PATH
    if let Ok(output) = std::process::Command::new(CF_BIN).arg("--version").output() {
        if output.status.success() {
            info!("Found cloudflared in PATH");
            return Ok(PathBuf::from(CF_BIN));
        }
    }

    bail!(
        "Không tìm thấy {CF_BIN}!\n\
        \n\
        Cách fix:\n\
        1. Tải tại: https://github.com/cloudflare/cloudflared/releases/latest\n\
        2. File cần tải: cloudflared-windows-amd64.exe\n\
        3. Đổi tên thành: {CF_BIN}\n\
        4. Đặt cạnh roblox-mcp.exe (hoặc thêm vào PATH)"
    )
}

// ── Tunnel launch ─────────────────────────────────────────────────

pub async fn start_tunnel(config: &Config) -> Result<String> {
    let cf   = find_cloudflared()?;
    let port = config.mcp_port;

    info!("Starting Quick Tunnel → http://localhost:{port}");

    let mut child = tokio::process::Command::new(&cf)
        .args(["tunnel", "--no-autoupdate", "--url", &format!("http://localhost:{port}")])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Failed to spawn cloudflared")?;

    let stderr = child.stderr.take().context("Failed to capture stderr")?;
    let stdout = child.stdout.take().context("Failed to capture stdout")?;

    // Drain stdout (cloudflared ít dùng stdout nhưng cần drain để không block)
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            debug!("[cf stdout] {line}");
        }
    });

    let mut stderr_lines = BufReader::new(stderr).lines();

    let url = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        async {
            while let Ok(Some(line)) = stderr_lines.next_line().await {
                debug!("[cf] {line}");
                if let Some(u) = extract_tunnel_url(&line) {
                    return Ok::<_, anyhow::Error>(u);
                }
                // Detect early failure
                if line.contains("failed") || line.contains("error") {
                    warn!("[cf] {line}");
                }
            }
            bail!("cloudflared thoát mà không in URL — kiểm tra lại cài đặt")
        },
    )
    .await
    .context("Timeout 30s — cloudflared không phản hồi")??;

    // Giữ process và drain stderr trong background
    tokio::spawn(async move { let _ = child.wait().await; });
    tokio::spawn(async move {
        while let Ok(Some(line)) = stderr_lines.next_line().await {
            debug!("[cf] {line}");
        }
    });

    Ok(url)
}

// ── URL extraction ────────────────────────────────────────────────

/// Trích xuất URL tunnel từ một dòng output của cloudflared.
/// Chỉ accept dạng `https://*.trycloudflare.com` — bỏ mọi URL khác.
fn extract_tunnel_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest  = &line[start..];
    let end   = rest
        .find(|c: char| c.is_whitespace() || matches!(c, '|' | '"' | ')' | '>'))
        .unwrap_or(rest.len());
    let url = &rest[..end];

    if url.ends_with(".trycloudflare.com") {
        Some(url.to_string())
    } else {
        None
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::extract_tunnel_url;

    #[test]
    fn extracts_valid_tunnel_url() {
        let line = "2024-01-01 INF +--------------------------------------------------------------------------------------------+";
        assert_eq!(extract_tunnel_url(line), None);

        let line = "2024-01-01 INF  |  Your quick Tunnel has been created! Visit it at (it may take some time to be reachable):  |";
        assert_eq!(extract_tunnel_url(line), None);

        let line = "2024-01-01 INF  |  https://abc123.trycloudflare.com  |";
        assert_eq!(
            extract_tunnel_url(line),
            Some("https://abc123.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn rejects_non_trycloudflare_urls() {
        let line = "https://cloudflare.com/something";
        assert_eq!(extract_tunnel_url(line), None);

        let line = "https://evil.com https://abc.trycloudflare.com";
        // Lấy URL đầu tiên tìm thấy — evil.com không phải trycloudflare nên None
        assert_eq!(extract_tunnel_url(line), None);
    }

    #[test]
    fn extracts_from_various_formats() {
        // Format mới cloudflared
        let line = "Your quick Tunnel has been created at https://xyz789.trycloudflare.com";
        assert_eq!(
            extract_tunnel_url(line),
            Some("https://xyz789.trycloudflare.com".to_string())
        );
    }
}