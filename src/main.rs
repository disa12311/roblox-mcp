mod bridge;
mod mcp_server;
mod tunnel;
mod http_server;

use anyhow::Result;
use tracing::info;

/// Cấu hình đọc từ CLI args hoặc env
#[derive(Debug, Clone)]
pub struct Config {
    /// Port MCP HTTP server lắng nghe
    pub mcp_port: u16,
    /// Port Roblox Studio plugin gửi request vào
    pub bridge_port: u16,
    /// Subdomain Cloudflare của bạn (ví dụ: roblox-mcp.yourdomain.com)
    pub cf_tunnel_token: Option<String>,
    /// Nếu không có token, dùng quick tunnel (URL random)
    pub use_quick_tunnel: bool,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            mcp_port: std::env::var("MCP_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
            bridge_port: std::env::var("BRIDGE_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(7878),
            cf_tunnel_token: std::env::var("CF_TUNNEL_TOKEN").ok(),
            use_quick_tunnel: std::env::var("QUICK_TUNNEL")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Log ra stderr (stdout dành riêng cho MCP stdio nếu cần)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("roblox_mcp=info".parse()?)
                .add_directive("tower_http=debug".parse()?),
        )
        .with_ansi(true)
        .init();

    let config = Config::from_env();

    info!("=== Roblox Studio MCP Server ===");
    info!("MCP HTTP port  : {}", config.mcp_port);
    info!("Studio bridge  : {}", config.bridge_port);

    // Shared state giữa bridge và MCP server
    let bridge_state = bridge::BridgeState::new();

    // Task 1: HTTP server nhận lệnh từ Roblox Studio plugin
    let bridge_state_clone = bridge_state.clone();
    let bridge_port = config.bridge_port;
    tokio::spawn(async move {
        if let Err(e) = bridge::run_bridge_server(bridge_state_clone, bridge_port).await {
            tracing::error!("Bridge server error: {e}");
        }
    });

    // Task 2: MCP HTTP server (Streamable HTTP — Claude web dùng)
    let bridge_state_clone = bridge_state.clone();
    let mcp_port = config.mcp_port;
    tokio::spawn(async move {
        if let Err(e) = http_server::run_mcp_http_server(bridge_state_clone, mcp_port).await {
            tracing::error!("MCP HTTP server error: {e}");
        }
    });

    // Task 3: Cloudflare tunnel
    let public_url = tunnel::start_tunnel(&config).await?;
    info!("");
    info!("╔══════════════════════════════════════════════════╗");
    info!("║         ROBLOX STUDIO MCP — READY                ║");
    info!("╠══════════════════════════════════════════════════╣");
    info!("║  Public URL: {:<36} ║", public_url);
    info!("╠══════════════════════════════════════════════════╣");
    info!("║  Thêm vào claude.ai:                             ║");
    info!("║  Settings → Connectors → Add custom connector   ║");
    info!("╚══════════════════════════════════════════════════╝");
    info!("");
    info!("Chờ Roblox Studio plugin kết nối trên port {}...", config.bridge_port);

    // Giữ process chạy
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");
    Ok(())
}