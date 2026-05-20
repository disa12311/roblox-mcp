// Ẩn console window ở release build — chỉ hiện cửa sổ egui
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod http_server;
mod tunnel;

use anyhow::Result;
use eframe::egui::{self, Color32, FontFamily, FontId, Margin, RichText, ScrollArea, Stroke, Vec2};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ── Config ────────────────────────────────────────────────────────

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
            mcp_port:    env_port("MCP_PORT",    3000),
            bridge_port: env_port("BRIDGE_PORT", 7878),
            cf_tunnel_token:  std::env::var("CF_TUNNEL_TOKEN").ok(),
            use_quick_tunnel: std::env::var("QUICK_TUNNEL")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(true),
        }
    }
}

fn env_port(key: &str, default: u16) -> u16 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

// ── Log line ──────────────────────────────────────────────────────

#[derive(Clone)]
enum LogKind {
    Info,
    Success,
    Warn,
    Error,
    Dim,
    Url,
    Separator,
}

#[derive(Clone)]
struct LogLine {
    kind:    LogKind,
    text:    String,
    time:    Option<String>, // HH:MM:SS prefix, None cho separator/header
}

impl LogLine {
    fn info(text: impl Into<String>)    -> Self { Self::new(LogKind::Info,    text) }
    fn success(text: impl Into<String>) -> Self { Self::new(LogKind::Success, text) }
    fn warn(text: impl Into<String>)    -> Self { Self::new(LogKind::Warn,    text) }
    fn error(text: impl Into<String>)   -> Self { Self::new(LogKind::Error,   text) }
    fn dim(text: impl Into<String>)     -> Self { Self::new(LogKind::Dim,     text) }
    fn url(text: impl Into<String>)     -> Self { Self::new(LogKind::Url,     text) }
    fn sep()                            -> Self {
        Self { kind: LogKind::Separator, text: String::new(), time: None }
    }

    fn new(kind: LogKind, text: impl Into<String>) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs  = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let h     = (secs / 3600) % 24;
        let m     = (secs / 60) % 60;
        let s     = secs % 60;
        Self { kind, text: text.into(), time: Some(format!("{h:02}:{m:02}:{s:02}")) }
    }
}

// ── Shared app state (giữa tokio tasks và egui thread) ────────────

#[derive(Default)]
struct SharedState {
    lines:         Vec<LogLine>,
    tunnel_url:    Option<String>,
    plugin_online: bool,
    ready:         bool,
    error:         Option<String>,
}

type Shared = Arc<Mutex<SharedState>>;

fn push(shared: &Shared, line: LogLine) {
    if let Ok(mut s) = shared.lock() {
        s.lines.push(line);
    }
}

// ── egui App ──────────────────────────────────────────────────────

// Màu sắc — tông terminal tối
const BG:           Color32 = Color32::from_rgb(13,  15,  22);
const BG_PANEL:     Color32 = Color32::from_rgb(20,  22,  32);
const BORDER:       Color32 = Color32::from_rgb(40,  44,  64);
const COL_TIME:     Color32 = Color32::from_rgb(55,  60,  85);
const COL_INFO:     Color32 = Color32::from_rgb(180, 185, 210);
const COL_SUCCESS:  Color32 = Color32::from_rgb(72,  210, 140);
const COL_WARN:     Color32 = Color32::from_rgb(240, 190,  60);
const COL_ERROR:    Color32 = Color32::from_rgb(232,  80,  80);
const COL_DIM:      Color32 = Color32::from_rgb(70,  74, 100);
const COL_URL:      Color32 = Color32::from_rgb(80,  180, 255);
const COL_SEP:      Color32 = Color32::from_rgb(35,  38,  58);

struct McpApp {
    shared:       Shared,
    last_online:  bool,
    scroll_to_bottom: bool,
}

impl McpApp {
    fn new(shared: Shared) -> Self {
        Self { shared, last_online: false, scroll_to_bottom: true }
    }
}

impl eframe::App for McpApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Repaint liên tục để log update real-time
        ctx.request_repaint_after(Duration::from_millis(200));

        let state = self.shared.lock().unwrap().clone_for_ui();
        let now_online = state.plugin_online;

        // Detect plugin connect/disconnect → thêm log line
        if now_online != self.last_online {
            if now_online {
                push(&self.shared, LogLine::success("Plugin Roblox Studio đã kết nối"));
            } else if self.last_online {
                push(&self.shared, LogLine::warn("Plugin mất kết nối"));
            }
            self.last_online = now_online;
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::same(0i8)))
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());

                // ── Header bar ─────────────────────────────────
                egui::Frame::new()
                    .fill(BG_PANEL)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(Margin::symmetric(14i8, 10i8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Dot status
                            let (dot_col, dot_tip) = if state.error.is_some() {
                                (COL_ERROR, "Error")
                            } else if !state.ready {
                                (COL_WARN, "Starting…")
                            } else if now_online {
                                (COL_SUCCESS, "Plugin connected")
                            } else {
                                (COL_DIM, "Waiting for plugin")
                            };

                            ui.add(egui::Label::new(
                                RichText::new("●").color(dot_col).size(13.0)
                            )).on_hover_text(dot_tip);

                            ui.add_space(8.0);

                            ui.label(RichText::new("Roblox Studio Bridge")
                                .color(COL_INFO)
                                .font(FontId::new(13.0, FontFamily::Monospace))
                                .strong());

                            ui.add_space(6.0);
                            ui.label(RichText::new("v0.2")
                                .color(COL_DIM)
                                .font(FontId::new(11.0, FontFamily::Monospace)));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if let Some(ref url) = state.tunnel_url {
                                    // Copy button
                                    if ui.small_button(
                                        RichText::new("⎘ copy").color(COL_DIM).size(11.0)
                                    ).on_hover_text("Copy URL to clipboard").clicked() {
                                        ctx.copy_text(url.clone());
                                    }
                                    ui.add_space(6.0);
                                    ui.label(RichText::new(url)
                                        .color(COL_URL)
                                        .font(FontId::new(11.0, FontFamily::Monospace)));
                                } else {
                                    ui.label(RichText::new("đang khởi động…")
                                        .color(COL_DIM)
                                        .font(FontId::new(11.0, FontFamily::Monospace)));
                                }
                            });
                        });
                    });

                ui.add_space(1.0);

                // ── Log area ────────────────────────────────────
                let log_frame = egui::Frame::new()
                    .fill(BG)
                    .inner_margin(Margin::symmetric(14i8, 8i8));

                log_frame.show(ui, |ui| {
                    let lines = &state.lines;

                    let scroll = ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(self.scroll_to_bottom);

                    scroll.show(ui, |ui| {
                        ui.set_min_width(ui.available_width());

                        for line in lines {
                            match line.kind {
                                LogKind::Separator => {
                                    ui.add_space(4.0);
                                    ui.painter().line_segment(
                                        [
                                            ui.cursor().min,
                                            ui.cursor().min + Vec2::new(ui.available_width(), 0.0),
                                        ],
                                        Stroke::new(1.0, COL_SEP),
                                    );
                                    ui.add_space(5.0);
                                    continue;
                                }
                                _ => {}
                            }

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;

                                // Timestamp
                                if let Some(ref t) = line.time {
                                    ui.add(egui::Label::new(
                                        RichText::new(t)
                                            .color(COL_TIME)
                                            .font(FontId::new(11.0, FontFamily::Monospace))
                                    ).wrap_mode(egui::TextWrapMode::Extend));
                                    ui.add_space(10.0);
                                }

                                // Text
                                let (color, size) = match line.kind {
                                    LogKind::Info    => (COL_INFO,    12.0),
                                    LogKind::Success => (COL_SUCCESS, 12.0),
                                    LogKind::Warn    => (COL_WARN,    12.0),
                                    LogKind::Error   => (COL_ERROR,   12.0),
                                    LogKind::Dim     => (COL_DIM,     11.0),
                                    LogKind::Url     => (COL_URL,     12.0),
                                    LogKind::Separator => unreachable!(),
                                };
                                ui.add(egui::Label::new(
                                    RichText::new(&line.text)
                                        .color(color)
                                        .font(FontId::new(size, FontFamily::Monospace))
                                ).wrap_mode(egui::TextWrapMode::Wrap));
                            });

                            ui.add_space(2.0);
                        }
                    });
                });

                // ── Status bar ──────────────────────────────────
                egui::TopBottomPanel::bottom("statusbar")
                    .frame(egui::Frame::new()
                        .fill(BG_PANEL)
                        .stroke(Stroke::new(1.0, BORDER))
                        .inner_margin(Margin::symmetric(14i8, 6i8)))
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Plugin status pill
                            let (pill_col, pill_text) = if now_online {
                                (COL_SUCCESS, "● plugin online")
                            } else {
                                (COL_DIM, "○ plugin offline")
                            };
                            ui.label(RichText::new(pill_text)
                                .color(pill_col)
                                .font(FontId::new(11.0, FontFamily::Monospace)));

                            ui.separator();

                            let port_text = format!("mcp :{}  bridge :{}", 
                                std::env::var("MCP_PORT").unwrap_or("3000".into()),
                                std::env::var("BRIDGE_PORT").unwrap_or("7878".into()));
                            ui.label(RichText::new(port_text)
                                .color(COL_DIM)
                                .font(FontId::new(11.0, FontFamily::Monospace)));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let scroll_icon = if self.scroll_to_bottom { "⬇ auto" } else { "⬇ manual" };
                                if ui.small_button(
                                    RichText::new(scroll_icon).color(COL_DIM).size(10.0)
                                ).on_hover_text("Toggle auto-scroll").clicked() {
                                    self.scroll_to_bottom = !self.scroll_to_bottom;
                                }
                            });
                        });
                    });
            });
    }
}

// Clone chỉ những gì cần cho UI (tránh clone toàn bộ Vec lớn mỗi frame)
struct UiSnapshot {
    lines:         Vec<LogLine>,
    tunnel_url:    Option<String>,
    plugin_online: bool,
    ready:         bool,
    error:         Option<String>,
}

impl SharedState {
    fn clone_for_ui(&self) -> UiSnapshot {
        UiSnapshot {
            lines:         self.lines.clone(),
            tunnel_url:    self.tunnel_url.clone(),
            plugin_online: self.plugin_online,
            ready:         self.ready,
            error:         self.error.clone(),
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────

fn main() -> Result<()> {
    let shared: Shared = Arc::new(Mutex::new(SharedState::default()));

    let config = Config::from_env();

    // Khởi động tokio runtime trong background thread
    // (eframe chiếm main thread cho GUI loop)
    let shared_bg = shared.clone();
    let config_bg = config.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            run_backend(config_bg, shared_bg).await;
        });
    });

    // egui window
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Roblox Studio Bridge  v0.2")
            .with_inner_size([680.0, 420.0])
            .with_min_inner_size([480.0, 280.0])
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "roblox-studio-bridge",
        options,
        Box::new(|cc| {
            // Font monospace mặc định egui đã có — chỉ cần tune visuals
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill             = BG;
            visuals.window_fill            = BG_PANEL;
            visuals.window_stroke          = Stroke::new(1.0, BORDER);
            visuals.window_corner_radius   = egui::CornerRadius::same(6u8);
            visuals.widgets.noninteractive.bg_fill   = BG_PANEL;
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, COL_DIM);
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::new(McpApp::new(shared.clone())))
        }),
    ).map_err(|e| anyhow::anyhow!("eframe: {e}"))?;

    Ok(())
}

// ── Backend (chạy trong tokio thread riêng) ───────────────────────

async fn run_backend(config: Config, shared: Shared) {
    // Setup tracing → ghi vào shared log thay vì stderr
    // (dùng tracing_subscriber đơn giản để không block; log qua closure)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr) // fallback stderr, chủ yếu dùng push()
        .with_env_filter("roblox_studio_bridge=info,tower_http=warn")
        .with_ansi(false)
        .init();

    let p = |line: LogLine| push(&shared, line);

    p(LogLine::dim("Roblox Studio Bridge  v0.2"));
    p(LogLine::sep());

    let c = config.clone();
    p(LogLine::info(format!("MCP port   →  localhost:{}", c.mcp_port)));
    p(LogLine::info(format!("Bridge     →  localhost:{}", c.bridge_port)));
    p(LogLine::info(format!("Tunnel     →  {}",
        if c.cf_tunnel_token.is_some() { "Named (URL cố định)" }
        else { "Quick (URL random)" }
    )));
    p(LogLine::sep());
    p(LogLine::dim("Đang khởi động…"));

    let bridge = bridge::BridgeState::new();

    // Bridge server
    {
        let b = bridge.clone(); let port = config.bridge_port;
        let s = shared.clone();
        tokio::spawn(async move {
            push(&s, LogLine::success(format!("Bridge server  localhost:{port}")));
            if let Err(e) = bridge::run_bridge_server(b, port).await {
                push(&s, LogLine::error(format!("Bridge error: {e}")));
            }
        });
    }

    // MCP HTTP server
    {
        let b = bridge.clone(); let port = config.mcp_port;
        let s = shared.clone();
        tokio::spawn(async move {
            push(&s, LogLine::success(format!("MCP server     localhost:{port}")));
            if let Err(e) = http_server::run_mcp_http_server(b, port).await {
                push(&s, LogLine::error(format!("MCP error: {e}")));
            }
        });
    }

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Cloudflare tunnel
    match tunnel::start_tunnel(&config).await {
        Ok(url) => {
            push(&shared, LogLine::sep());
            push(&shared, LogLine::success("✅  READY"));
            push(&shared, LogLine::dim(format!("Public URL  →  {url}")));
            push(&shared, LogLine::url(url.clone()));
            push(&shared, LogLine::sep());
            push(&shared, LogLine::dim("Thêm URL vào: claude.ai → Settings → Connectors → Add custom"));
            push(&shared, LogLine::sep());
            if let Ok(mut s) = shared.lock() {
                s.tunnel_url = Some(url);
                s.ready      = true;
            }
        }
        Err(e) => {
            push(&shared, LogLine::error(format!("Tunnel lỗi: {e}")));
            if let Ok(mut s) = shared.lock() {
                s.error = Some(e.to_string());
            }
        }
    }

    // Plugin online monitor
    {
        let bridge  = bridge.clone();
        let shared2 = shared.clone();
        tokio::spawn(async move {
            loop {
                let online = bridge.is_plugin_online();
                if let Ok(mut s) = shared2.lock() {
                    s.plugin_online = online;
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });
    }

    // Giữ runtime sống
    loop { tokio::time::sleep(Duration::from_secs(60)).await; }
}