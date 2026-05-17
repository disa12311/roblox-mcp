/// tunnel.rs
/// Tự động download cloudflared binary nếu chưa có, rồi spawn tunnel
/// Người dùng không cần cài gì — chỉ cần chạy roblox-mcp.exe

use crate::Config;
use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{info, warn};

// URL download cloudflared binary theo platform
#[cfg(target_os = "windows")]
const CLOUDFLARED_URL: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe";

#[cfg(target_os = "linux")]
const CLOUDFLARED_URL: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64";

#[cfg(target_os = "macos")]
const CLOUDFLARED_URL: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-amd64";

/// Trả về path tới cloudflared binary (download nếu chưa có)
async fn get_cloudflared_path() -> Result<PathBuf> {
    // Lưu vào thư mục cùng chỗ với exe, hoặc temp dir
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(std::env::temp_dir);

    #[cfg(target_os = "windows")]
    let binary_name = "cloudflared.exe";
    #[cfg(not(target_os = "windows"))]
    let binary_name = "cloudflared";

    let cf_path = exe_dir.join(binary_name);

    // Nếu đã có thì dùng luôn
    if cf_path.exists() {
        info!("Found cloudflared at: {}", cf_path.display());
        return Ok(cf_path);
    }

    // Download
    info!("cloudflared not found, downloading from GitHub...");
    info!("URL: {}", CLOUDFLARED_URL);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let resp = client
        .get(CLOUDFLARED_URL)
        .send()
        .await
        .context("Failed to download cloudflared")?;

    if !resp.status().is_success() {
        anyhow::bail!("Download failed: HTTP {}", resp.status());
    }

    let bytes = resp.bytes().await?;
    tokio::fs::write(&cf_path, &bytes).await?;

    // Set executable bit trên Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&cf_path).await?.permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&cf_path, perms).await?;
    }

    info!("Downloaded cloudflared ({} MB)", bytes.len() / 1024 / 1024);
    Ok(cf_path)
}

/// Khởi động Cloudflare tunnel và trả về public URL
pub async fn start_tunnel(config: &Config) -> Result<String> {
    let cf_path = get_cloudflared_path().await?;
    let mcp_port = config.mcp_port;

    // Nếu có CF_TUNNEL_TOKEN → named tunnel với subdomain cố định
    // Nếu không → quick tunnel với URL random (*.trycloudflare.com)
    let mut cmd = if let Some(token) = &config.cf_tunnel_token {
        info!("Starting named Cloudflare tunnel (fixed URL)...");
        let mut c = tokio::process::Command::new(&cf_path);
        c.args(["tunnel", "--no-autoupdate", "run", "--token", token]);
        c
    } else {
        info!("Starting quick tunnel (random URL, resets on restart)...");
        info!("Để có URL cố định, set CF_TUNNEL_TOKEN=<your_token>");
        let mut c = tokio::process::Command::new(&cf_path);
        c.args([
            "tunnel",
            "--no-autoupdate",
            "--url",
            &format!("http://localhost:{mcp_port}"),
        ]);
        c
    };

    // Redirect stderr để đọc URL
    cmd.stdout(std::process::Stdio::piped())
       .stderr(std::process::Stdio::piped())
       .kill_on_drop(true);

    let mut child = cmd.spawn().context("Failed to spawn cloudflared")?;

    // Đọc output để lấy URL
    let stderr = child.stderr.take().expect("stderr should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");

    // Spawn background task đọc stdout
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!("[cloudflared stdout] {}", line);
        }
    });

    // Đọc stderr để tìm URL (cloudflared log URL vào stderr)
    let mut reader = BufReader::new(stderr);
    let mut lines = reader.lines();

    // Timeout 30s chờ tunnel khởi động
    let url = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        async {
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("[cloudflared] {}", line);

                // Tìm URL trong log output
                // Named tunnel: "https://your.domain.com"
                // Quick tunnel: "https://xxxx.trycloudflare.com"
                if let Some(url) = extract_tunnel_url(&line) {
                    return Ok::<String, anyhow::Error>(url);
                }
            }
            anyhow::bail!("cloudflared exited without providing URL")
        }
    )
    .await
    .context("Timeout waiting for tunnel URL")??;

    // Keep child alive bằng cách spawn vào background
    tokio::spawn(async move {
        let status = child.wait().await;
        tracing::warn!("cloudflared exited: {:?}", status);
    });

    // Background task tiếp tục đọc logs
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!("[cloudflared] {}", line);
        }
    });

    Ok(url)
}

/// Extract URL từ một dòng log của cloudflared
fn extract_tunnel_url(line: &str) -> Option<String> {
    // Quick tunnel: "Your quick tunnel is ready! | URL: https://xxx.trycloudflare.com"
    // Named tunnel: "Registered tunnel connection ... https://your.domain.com"
    // Cũng check: "https://" ở bất kỳ đâu trong dòng có "tunnel" hoặc "url"
    let lower = line.to_lowercase();

    if lower.contains("trycloudflare.com") || lower.contains("url") || lower.contains("tunnel") {
        // Extract https://... từ line
        if let Some(start) = line.find("https://") {
            let rest = &line[start..];
            // Lấy tới whitespace hoặc pipe
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '|' || c == '"' || c == '\'')
                .unwrap_or(rest.len());
            let url = &rest[..end];
            if url.contains('.') && url.len() > 10 {
                return Some(url.to_string());
            }
        }
    }
    None
}