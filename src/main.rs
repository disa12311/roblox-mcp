mod bridge;
mod http_server;
mod tunnel;

use anyhow::Result;
use colored::Colorize;

#[derive(Debug, Clone)]
pub struct Config {
    pub mcp_port:         u16,
    pub bridge_port:      u16,
    pub cf_tunnel_token:  Option<String>,
    pub use_quick_tunnel: bool,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            mcp_port:         std::env::var("MCP_PORT").ok()
                                .and_then(|p| p.parse().ok()).unwrap_or(3000),
            bridge_port:      std::env::var("BRIDGE_PORT").ok()
                                .and_then(|p| p.parse().ok()).unwrap_or(7878),
            cf_tunnel_token:  std::env::var("CF_TUNNEL_TOKEN").ok(),
            use_quick_tunnel: std::env::var("QUICK_TUNNEL")
                                .map(|v| v == "1" || v == "true").unwrap_or(true),
        }
    }
}

fn print_banner() {
    println!();
    println!("{}", "╔══════════════════════════════════════════╗".cyan());
    println!("{}", "║     Roblox Studio MCP Server v0.1        ║".cyan());
    println!("{}", "╚══════════════════════════════════════════╝".cyan());
    println!();
}

fn print_status(label: &str, value: &str, ok: bool) {
    let icon = if ok { "✓".green().bold() } else { "✗".red().bold() };
    println!("  {}  {:<10}  {}", icon, label.white(), value.bright_white());
}

fn print_ready(url: &str, bridge_port: u16) {
    println!();
    println!("{}", "┌──────────────────────────────────────────┐".bright_green());
    println!("{}", "│              ✅  READY                    │".bright_green());
    println!("{}", "├──────────────────────────────────────────┤".bright_green());
    println!("{}  {:<28}{}",
        "│  Public URL:".bright_green(),
        url.bright_white(),
        "│".bright_green()
    );
    println!("{}  {:<28}{}",
        "│  Bridge:    ".bright_green(),
        format!("localhost:{bridge_port}").bright_white(),
        "│".bright_green()
    );
    println!("{}", "├──────────────────────────────────────────┤".bright_green());
    println!("{}", "│  Thêm vào claude.ai:                     │".bright_green());
    println!("{}", "│  Settings → Connectors → Add custom      │".bright_green());
    println!("{}", "└──────────────────────────────────────────┘".bright_green());
    println!();
    println!("  {} Ctrl+C để dừng\n", "→".yellow());
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("roblox_mcp=info".parse()?)
                .add_directive("tower_http=warn".parse()?),
        )
        .with_ansi(false)
        .init();

    print_banner();

    let config = Config::from_env();

    print_status("MCP port",  &format!("localhost:{}", config.mcp_port),    true);
    print_status("Bridge",    &format!("localhost:{}", config.bridge_port), true);

    if config.cf_tunnel_token.is_some() {
        print_status("Tunnel", "Named (URL cố định)", true);
    } else {
        print_status("Tunnel", "Quick (URL random — thay đổi mỗi lần restart)", true);
    }

    println!("\n  {} Đang khởi động...", "⟳".yellow());

    let bridge = bridge::BridgeState::new();

    // Bridge server — nhận long-poll từ Studio plugin
    let b = bridge.clone();
    let port = config.bridge_port;
    tokio::spawn(async move {
        if let Err(e) = bridge::run_bridge_server(b, port).await {
            eprintln!("Bridge error: {e}");
        }
    });

    // MCP HTTP server — Claude web kết nối vào đây
    let b = bridge.clone();
    let port = config.mcp_port;
    tokio::spawn(async move {
        if let Err(e) = http_server::run_mcp_http_server(b, port).await {
            eprintln!("MCP HTTP error: {e}");
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Cloudflare tunnel
    match tunnel::start_tunnel(&config).await {
        Ok(url) => print_ready(&url, config.bridge_port),
        Err(e) => {
            println!("\n  {} Tunnel lỗi: {}", "✗".red().bold(), e.to_string().red());
            std::process::exit(1);
        }
    }

    println!("  {} Chờ Roblox Studio plugin kết nối...\n", "◉".bright_blue());

    tokio::signal::ctrl_c().await?;
    println!("\n  {} Đã dừng.", "✓".green());
    Ok(())
}