/// tunnel.rs — Tự download cloudflared nếu chưa có, rồi spawn tunnel

use crate::Config;
use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::info;

#[cfg(target_os = "windows")]
const CF_URL: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe";
#[cfg(target_os = "linux")]
const CF_URL: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64";
#[cfg(target_os = "macos")]
const CF_URL: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-amd64";

async fn get_cloudflared() -> Result<PathBuf> {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(std::env::temp_dir);

    #[cfg(target_os = "windows")]
    let name = "cloudflared.exe";
    #[cfg(not(target_os = "windows"))]
    let name = "cloudflared";

    let path = dir.join(name);
    if path.exists() {
        info!("cloudflared found: {}", path.display());
        return Ok(path);
    }

    info!("Downloading cloudflared (~30MB)...");
    let bytes = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?
        .get(CF_URL)
        .send()
        .await
        .context("Download failed")?
        .bytes()
        .await?;

    tokio::fs::write(&path, &bytes).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = tokio::fs::metadata(&path).await?.permissions();
        p.set_mode(0o755);
        tokio::fs::set_permissions(&path, p).await?;
    }

    info!("Downloaded ({} MB)", bytes.len() / 1024 / 1024);
    Ok(path)
}

pub async fn start_tunnel(config: &Config) -> Result<String> {
    let cf = get_cloudflared().await?;
    let port = config.mcp_port;

    let mut cmd = if let Some(token) = &config.cf_tunnel_token {
        info!("Named tunnel (URL cố định)...");
        let mut c = tokio::process::Command::new(&cf);
        c.args(["tunnel", "--no-autoupdate", "run", "--token", token]);
        c
    } else {
        info!("Quick tunnel (URL random)...");
        let mut c = tokio::process::Command::new(&cf);
        c.args(["tunnel", "--no-autoupdate", "--url", &format!("http://localhost:{port}")]);
        c
    };

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().context("Failed to spawn cloudflared")?;
    let stderr = child.stderr.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(_)) = lines.next_line().await {}
    });

    let mut lines = BufReader::new(stderr).lines();

    let url = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        async {
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("[cf] {line}");
                if let Some(u) = extract_url(&line) {
                    return Ok::<_, anyhow::Error>(u);
                }
            }
            anyhow::bail!("cloudflared exited without URL")
        },
    )
    .await
    .context("Timeout")??;

    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    tokio::spawn(async move {
        while let Ok(Some(_)) = lines.next_line().await {}
    });

    Ok(url)
}

fn extract_url(line: &str) -> Option<String> {
    if let Some(i) = line.find("https://") {
        let rest = &line[i..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '|' || c == '"')
            .unwrap_or(rest.len());
        let url = &rest[..end];
        if url.contains('.') && url.len() > 12 {
            return Some(url.to_string());
        }
    }
    None
}