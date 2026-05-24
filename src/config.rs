/// config.rs — Đọc/ghi config.json và auto-download cloudflared

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Persist config ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Quick tunnel hoặc Named tunnel
    pub tunnel_mode: TunnelMode,
    /// Token cho Named tunnel (chỉ dùng khi mode = Named)
    pub tunnel_token: String,
    pub mcp_port:     u16,
    pub bridge_port:  u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TunnelMode {
    Quick,
    Named,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tunnel_mode:  TunnelMode::Quick,
            tunnel_token: String::new(),
            mcp_port:     3000,
            bridge_port:  7878,
        }
    }
}

fn config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("config.json")))
        .unwrap_or_else(|| PathBuf::from("config.json"))
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_path();
        if !path.exists() { return Self::default(); }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = config_path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }
}

// ── Auto-download cloudflared ─────────────────────────────────────

#[cfg(target_os = "windows")]
const CF_ASSET: &str = "cloudflared-windows-amd64.exe";
#[cfg(target_os = "linux")]
const CF_ASSET: &str = "cloudflared-linux-amd64";
#[cfg(target_os = "macos")]
const CF_ASSET: &str = "cloudflared-darwin-amd64";

#[cfg(target_os = "windows")]
const CF_BIN: &str = "cloudflared.exe";
#[cfg(not(target_os = "windows"))]
const CF_BIN: &str = "cloudflared";

pub fn cloudflared_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(CF_BIN)))
        .unwrap_or_else(|| PathBuf::from(CF_BIN))
}

pub fn cloudflared_exists() -> bool {
    let local = cloudflared_path();
    if local.exists() { return true; }
    // Tìm trong PATH
    std::process::Command::new(CF_BIN)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Download cloudflared từ GitHub releases mới nhất.
/// Gọi từ tokio async context.
pub async fn download_cloudflared(
    on_progress: impl Fn(String) + Send + 'static,
) -> Result<PathBuf> {
    use reqwest::Client;
    use tokio::io::AsyncWriteExt;

    let client = Client::builder()
        .user_agent("roblox-studio-bridge/0.2")
        .build()?;

    // Lấy release mới nhất từ GitHub API
    on_progress("Đang lấy thông tin release mới nhất…".to_string());
    let release: serde_json::Value = client
        .get("https://api.github.com/repos/cloudflare/cloudflared/releases/latest")
        .send().await?
        .json().await?;

    let asset_url = release["assets"]
        .as_array()
        .context("No assets")?
        .iter()
        .find(|a| a["name"].as_str() == Some(CF_ASSET))
        .and_then(|a| a["browser_download_url"].as_str())
        .context(format!("Asset {CF_ASSET} không tìm thấy trong release"))?
        .to_string();

    on_progress(format!("Đang download {CF_ASSET}…"));

    let bytes = client
        .get(&asset_url)
        .send().await?
        .bytes().await?;

    let dest = cloudflared_path();
    let mut file = tokio::fs::File::create(&dest).await
        .context("Không tạo được file cloudflared")?;
    file.write_all(&bytes).await?;

    // Trên Unix: chmod +x
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
    }

    on_progress(format!("✅  Đã tải cloudflared vào {}", dest.display()));
    Ok(dest)
}