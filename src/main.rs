#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod config;
mod gui;
mod http_server;
mod state;
mod tunnel;

use anyhow::Result;
use config::AppConfig;
use eframe::egui::{self, Stroke};
use gui::{McpApp, BG, BG_PANEL, BG_INPUT, BORDER, BORDER_ACT};
use state::{LogLine, Shared, SharedState, push};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ── Runtime config ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Config {
    pub mcp_port:        u16,
    pub bridge_port:     u16,
    pub cf_tunnel_token: Option<String>,
}

impl Config {
    pub fn from_app(app: &AppConfig) -> Self {
        use config::TunnelMode;
        Self {
            mcp_port:        env_port("MCP_PORT",    app.mcp_port),
            bridge_port:     env_port("BRIDGE_PORT", app.bridge_port),
            cf_tunnel_token: match app.tunnel_mode {
                TunnelMode::Named if !app.tunnel_token.is_empty() =>
                    Some(app.tunnel_token.clone()),
                _ => None,
            },
        }
    }
}

fn env_port(key: &str, default: u16) -> u16 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

// ── Entry point ───────────────────────────────────────────────────

fn main() -> Result<()> {
    let app_cfg = AppConfig::load();
    let cfg     = Config::from_app(&app_cfg);
    let shared: Shared = Arc::new(Mutex::new(SharedState::default()));

    {
        let shared_bg = shared.clone();
        let cfg_bg    = cfg.clone();
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .expect("tokio runtime")
                .block_on(run_backend(cfg_bg, shared_bg));
        });
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Roblox Studio Bridge  v0.2")
            .with_inner_size([700.0, 460.0])
            .with_min_inner_size([500.0, 320.0])
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "roblox-studio-bridge",
        options,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill           = BG;
            visuals.window_fill          = BG_PANEL;
            visuals.window_stroke        = Stroke::new(1.0, BORDER);
            visuals.window_corner_radius = egui::CornerRadius::same(6u8);
            visuals.widgets.noninteractive.bg_fill   = BG_PANEL;
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, BG_INPUT);
            visuals.widgets.inactive.bg_fill         = BG_INPUT;
            visuals.widgets.active.bg_stroke         = Stroke::new(1.0, BORDER_ACT);
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::new(McpApp::new(shared.clone(), app_cfg)))
        }),
    ).map_err(|e| anyhow::anyhow!("eframe: {e}"))?;

    Ok(())
}

// ── Backend ───────────────────────────────────────────────────────

async fn run_backend(config: Config, shared: Shared) {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("roblox_studio_bridge=info,tower_http=warn")
        .with_ansi(false)
        .init();

    start_with_config(config, shared.clone()).await;

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let new_cfg = { shared.lock().unwrap().restart_config.take() };
        if let Some(cfg) = new_cfg {
            push(&shared, LogLine::dim("Đang restart…"));
            tokio::time::sleep(Duration::from_millis(500)).await;
            start_with_config(cfg, shared.clone()).await;
        }
    }
}

async fn start_with_config(config: Config, shared: Shared) {
    let p = |line: LogLine| push(&shared, line);

    p(LogLine::dim("Roblox Studio Bridge  v0.2"));
    p(LogLine::sep());
    p(LogLine::info(format!("MCP port   →  localhost:{}", config.mcp_port)));
    p(LogLine::info(format!("Bridge     →  localhost:{}", config.bridge_port)));
    p(LogLine::info(format!("Tunnel     →  {}",
        if config.cf_tunnel_token.is_some() { "Named (URL cố định)" } else { "Quick (auto-restart)" }
    )));
    p(LogLine::sep());
    p(LogLine::dim("Đang khởi động…"));

    let bridge = bridge::BridgeState::new();

    {
        let b = bridge.clone(); let port = config.bridge_port; let s = shared.clone();
        tokio::spawn(async move {
            push(&s, LogLine::success(format!("Bridge server  localhost:{port}")));
            if let Err(e) = bridge::run_bridge_server(b, port).await {
                push(&s, LogLine::error(format!("Bridge error: {e}")));
            }
        });
    }
    {
        let b = bridge.clone(); let port = config.mcp_port; let s = shared.clone();
        tokio::spawn(async move {
            push(&s, LogLine::success(format!("MCP server     localhost:{port}")));
            if let Err(e) = http_server::run_mcp_http_server(b, port).await {
                push(&s, LogLine::error(format!("MCP error: {e}")));
            }
        });
    }

    tokio::time::sleep(Duration::from_millis(300)).await;

    match tunnel::start_tunnel(&config).await {
        Ok((url, mut url_rx)) => {
            p(LogLine::sep());
            p(LogLine::success("✅  READY"));
            p(LogLine::dim(format!("Public URL  →  {url}")));
            p(LogLine::url(url.clone()));
            p(LogLine::sep());
            p(LogLine::dim("Thêm URL vào: claude.ai → Settings → Connectors → Add custom"));
            p(LogLine::sep());
            {
                let mut s = shared.lock().unwrap();
                s.tunnel_url    = Some(url.clone());
                s.ready         = true;
                s.auto_copy_url = Some(url);
            }
            let shared2 = shared.clone();
            tokio::spawn(async move {
                loop {
                    if url_rx.changed().await.is_err() { break; }
                    let new_url = url_rx.borrow().clone();
                    match new_url {
                        None => {
                            push(&shared2, LogLine::warn("Tunnel mất kết nối, đang restart…"));
                            if let Ok(mut s) = shared2.lock() { s.tunnel_url = None; }
                        }
                        Some(u) => {
                            push(&shared2, LogLine::success(format!("Tunnel mới: {u}")));
                            push(&shared2, LogLine::url(u.clone()));
                            push(&shared2, LogLine::dim("⚠ URL đã đổi — cập nhật connector trong claude.ai"));
                            if let Ok(mut s) = shared2.lock() {
                                s.tunnel_url    = Some(u.clone());
                                s.auto_copy_url = Some(u);
                            }
                        }
                    }
                }
            });
        }
        Err(e) => {
            p(LogLine::error(format!("Tunnel lỗi: {e}")));
            if let Ok(mut s) = shared.lock() { s.error = Some(e.to_string()); }
        }
    }

    {
        let bridge2 = bridge.clone();
        let shared3 = shared.clone();
        tokio::spawn(async move {
            loop {
                bridge2.refresh_online_status();
                if let Ok(mut s) = shared3.lock() {
                    s.plugin_online = bridge2.is_plugin_online();
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });
    }
}