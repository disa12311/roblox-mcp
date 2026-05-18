/// tunnel.rs — Tìm cloudflared.exe và spawn quick tunnel
/// Tải cloudflared tại: https://github.com/cloudflare/cloudflared/releases/latest

use crate::Config;
use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::info;

fn find_cloudflared() -> Result<PathBuf> {
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
        info!("Found cloudflared: {}", path.display());
        Ok(path)
    } else {
        anyhow::bail!(
            "Không tìm thấy {name} cạnh roblox-mcp.exe!\n\
            Tải tại: https://github.com/cloudflare/cloudflared/releases/latest\n\
            File: cloudflared-windows-amd64.exe → đổi tên thành cloudflared.exe\n\
            Đặt cạnh roblox-mcp.exe rồi chạy lại."
        )
    }
}

pub async fn start_tunnel(config: &Config) -> Result<String> {
    let cf   = find_cloudflared()?;
    let port = config.mcp_port;

    info!("Quick tunnel → http://localhost:{port}");

    let mut child = tokio::process::Command::new(&cf)
        .args(["tunnel", "--no-autoupdate", "--url", &format!("http://localhost:{port}")])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Failed to spawn cloudflared")?;

    let stderr = child.stderr.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    // Drain stdout
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(_)) = lines.next_line().await {}
    });

    // Parse URL từ stderr — cloudflared in "https://xxxx.trycloudflare.com"
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
            anyhow::bail!("cloudflared thoát mà không in URL")
        },
    )
    .await
    .context("Timeout 30s — cloudflared không phản hồi")??;

    // Keep alive
    tokio::spawn(async move { let _ = child.wait().await; });
    tokio::spawn(async move {
        while let Ok(Some(_)) = lines.next_line().await {}
    });

    Ok(url)
}

fn extract_url(line: &str) -> Option<String> {
    // Chỉ lấy URL dạng https://xxxx.trycloudflare.com
    if let Some(i) = line.find("https://") {
        let rest = &line[i..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '|' || c == '"' || c == ')')
            .unwrap_or(rest.len());
        let url = &rest[..end];
        // Chỉ accept trycloudflare.com — bỏ qua mọi URL khác
        if url.ends_with(".trycloudflare.com") {
            return Some(url.to_string());
        }
    }
    None
}