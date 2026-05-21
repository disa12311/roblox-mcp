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

// ── Log ───────────────────────────────────────────────────────────

#[derive(Clone)]
enum LogKind { Info, Success, Warn, Error, Dim, Url, Separator }

#[derive(Clone)]
struct LogLine {
    kind: LogKind,
    text: String,
    time: Option<String>,
}

impl LogLine {
    fn info(t: impl Into<String>)    -> Self { Self::new(LogKind::Info,    t) }
    fn success(t: impl Into<String>) -> Self { Self::new(LogKind::Success, t) }
    fn warn(t: impl Into<String>)    -> Self { Self::new(LogKind::Warn,    t) }
    fn error(t: impl Into<String>)   -> Self { Self::new(LogKind::Error,   t) }
    fn dim(t: impl Into<String>)     -> Self { Self::new(LogKind::Dim,     t) }
    fn url(t: impl Into<String>)     -> Self { Self::new(LogKind::Url,     t) }
    fn sep() -> Self { Self { kind: LogKind::Separator, text: String::new(), time: None } }

    fn new(kind: LogKind, text: impl Into<String>) -> Self {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        Self {
            kind, text: text.into(),
            time: Some(format!("{:02}:{:02}:{:02}", (secs/3600)%24, (secs/60)%60, secs%60)),
        }
    }
}

// ── Toast notification ────────────────────────────────────────────

#[derive(Clone)]
struct Toast {
    text:       String,
    born_secs:  f64,  // egui time
    duration:   f64,
}

impl Toast {
    fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), born_secs: 0.0, duration: 3.0 }
    }
    fn is_alive(&self, now: f64) -> bool {
        now - self.born_secs < self.duration
    }
    fn alpha(&self, now: f64) -> f32 {
        let age = (now - self.born_secs) as f32;
        let fade_start = (self.duration - 0.6) as f32;
        if age < fade_start { 1.0 } else { 1.0 - (age - fade_start) / 0.6 }
    }
}

// ── Shared state ──────────────────────────────────────────────────

#[derive(Default)]
struct SharedState {
    lines:         Vec<LogLine>,
    tunnel_url:    Option<String>,
    plugin_online: bool,
    ready:         bool,
    error:         Option<String>,
    auto_copy_url: Option<String>,
    /// Set true khi user click "Thoát" trong tray menu
    should_quit:   bool,
}

type Shared = Arc<Mutex<SharedState>>;

fn push(shared: &Shared, line: LogLine) {
    if let Ok(mut s) = shared.lock() { s.lines.push(line); }
}

// ── Colors ────────────────────────────────────────────────────────

const BG:          Color32 = Color32::from_rgb(13,  15,  22);
const BG_PANEL:    Color32 = Color32::from_rgb(20,  22,  32);
const BORDER:      Color32 = Color32::from_rgb(40,  44,  64);
const COL_TIME:    Color32 = Color32::from_rgb(55,  60,  85);
const COL_INFO:    Color32 = Color32::from_rgb(180, 185, 210);
const COL_SUCCESS: Color32 = Color32::from_rgb(72,  210, 140);
const COL_WARN:    Color32 = Color32::from_rgb(240, 190,  60);
const COL_ERROR:   Color32 = Color32::from_rgb(232,  80,  80);
const COL_DIM:     Color32 = Color32::from_rgb(70,  74, 100);
const COL_URL:     Color32 = Color32::from_rgb(80,  180, 255);
const COL_SEP:     Color32 = Color32::from_rgb(35,  38,  58);

// ── App ───────────────────────────────────────────────────────────

struct McpApp {
    shared:           Shared,
    last_online:      bool,
    scroll_to_bottom: bool,
    toast:            Option<Toast>,
    /// Minimized to tray
    hidden:           bool,
}

impl McpApp {
    fn new(shared: Shared) -> Self {
        Self {
            shared,
            last_online:      false,
            scroll_to_bottom: true,
            toast:            None,
            hidden:           false,
        }
    }

    fn show_toast(&mut self, text: impl Into<String>, now: f64) {
        let mut t = Toast::new(text);
        t.born_secs = now;
        self.toast = Some(t);
    }
}

impl eframe::App for McpApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(200));

        let now = ctx.input(|i| i.time);

        // ── Xử lý auto-copy khi tunnel ready ─────────────────────
        let auto_copy = {
            let mut s = self.shared.lock().unwrap();
            s.auto_copy_url.take()
        };
        if let Some(ref url) = auto_copy {
            ctx.copy_text(url.clone());
            self.show_toast("✓ Đã copy URL vào clipboard", now);
            push(&self.shared, LogLine::success("URL đã copy vào clipboard tự động"));
        }

        let state = self.shared.lock().unwrap().clone_for_ui();
        let now_online = state.plugin_online;

        // ── Detect plugin online/offline ──────────────────────────
        if now_online != self.last_online {
            if now_online {
                push(&self.shared, LogLine::success("Plugin Roblox Studio đã kết nối"));
            } else if self.last_online {
                push(&self.shared, LogLine::warn("Plugin mất kết nối"));
            }
            self.last_online = now_online;
        }

        // ── Close / minimize to tray ──────────────────────────────
        let should_quit = self.shared.lock().unwrap().should_quit;
        if should_quit {
            // User click "Thoát" trong tray menu → thoát thật
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            // Click X → minimize xuống tray, không thoát
            self.hidden = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        // ── Expire toast ──────────────────────────────────────────
        if let Some(ref t) = self.toast {
            if !t.is_alive(now) { self.toast = None; }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::same(0i8)))
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());

                // ── Header ────────────────────────────────────────
                egui::Frame::new()
                    .fill(BG_PANEL)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(Margin::symmetric(14i8, 10i8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
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
                                    if ui.small_button(
                                        RichText::new("⎘ copy").color(COL_DIM).size(11.0)
                                    ).on_hover_text("Copy URL to clipboard").clicked() {
                                        ctx.copy_text(url.clone());
                                        self.show_toast("✓ Đã copy URL", now);
                                    }
                                    ui.add_space(6.0);
                                    ui.label(RichText::new(url)
                                        .color(COL_URL)
                                        .font(FontId::new(11.0, FontFamily::Monospace)));
                                } else if state.ready {
                                    // Tunnel đang restart
                                    ui.label(RichText::new("⟳ tunnel restarting…")
                                        .color(COL_WARN)
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

                // ── Log area ──────────────────────────────────────
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(Margin::symmetric(14i8, 8i8))
                    .show(ui, |ui| {
                        let lines = &state.lines;
                        ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .stick_to_bottom(self.scroll_to_bottom)
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                for line in lines {
                                    if matches!(line.kind, LogKind::Separator) {
                                        ui.add_space(4.0);
                                        let rect = ui.cursor();
                                        ui.painter().line_segment(
                                            [rect.min, rect.min + Vec2::new(ui.available_width(), 0.0)],
                                            Stroke::new(1.0, COL_SEP),
                                        );
                                        ui.add_space(5.0);
                                        continue;
                                    }
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        if let Some(ref t) = line.time {
                                            ui.add(egui::Label::new(
                                                RichText::new(t).color(COL_TIME)
                                                    .font(FontId::new(11.0, FontFamily::Monospace))
                                            ).wrap_mode(egui::TextWrapMode::Extend));
                                            ui.add_space(10.0);
                                        }
                                        let (color, size) = match line.kind {
                                            LogKind::Info      => (COL_INFO,    12.0),
                                            LogKind::Success   => (COL_SUCCESS, 12.0),
                                            LogKind::Warn      => (COL_WARN,    12.0),
                                            LogKind::Error     => (COL_ERROR,   12.0),
                                            LogKind::Dim       => (COL_DIM,     11.0),
                                            LogKind::Url       => (COL_URL,     12.0),
                                            LogKind::Separator => unreachable!(),
                                        };
                                        ui.add(egui::Label::new(
                                            RichText::new(&line.text).color(color)
                                                .font(FontId::new(size, FontFamily::Monospace))
                                        ).wrap_mode(egui::TextWrapMode::Wrap));
                                    });
                                    ui.add_space(2.0);
                                }
                            });
                    });

                // ── Status bar ────────────────────────────────────
                egui::TopBottomPanel::bottom("statusbar")
                    .frame(egui::Frame::new()
                        .fill(BG_PANEL)
                        .stroke(Stroke::new(1.0, BORDER))
                        .inner_margin(Margin::symmetric(14i8, 6i8)))
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (pill_col, pill_text) = if now_online {
                                (COL_SUCCESS, "● plugin online")
                            } else {
                                (COL_DIM, "○ plugin offline")
                            };
                            ui.label(RichText::new(pill_text).color(pill_col)
                                .font(FontId::new(11.0, FontFamily::Monospace)));
                            ui.separator();
                            let port_text = format!("mcp :{}  bridge :{}",
                                std::env::var("MCP_PORT").unwrap_or("3000".into()),
                                std::env::var("BRIDGE_PORT").unwrap_or("7878".into()));
                            ui.label(RichText::new(port_text).color(COL_DIM)
                                .font(FontId::new(11.0, FontFamily::Monospace)));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Minimize to tray button
                                if ui.small_button(
                                    RichText::new("⬇ tray").color(COL_DIM).size(10.0)
                                ).on_hover_text("Minimize to system tray").clicked() {
                                    self.hidden = true;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                                }
                                ui.add_space(4.0);
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

        // ── Toast overlay ─────────────────────────────────────────
        if let Some(ref toast) = self.toast.clone() {
            let alpha = toast.alpha(now);
            let bg    = Color32::from_rgba_unmultiplied(30, 34, 50, (alpha * 230.0) as u8);
            let fg    = Color32::from_rgba_unmultiplied(72, 210, 140, (alpha * 255.0) as u8);

            egui::Area::new(egui::Id::new("toast"))
                .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -40.0])
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(bg)
                        .corner_radius(egui::CornerRadius::same(6u8))
                        .inner_margin(Margin::symmetric(14i8, 8i8))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&toast.text).color(fg)
                                .font(FontId::new(12.0, FontFamily::Monospace)));
                        });
                });
        }
    }
}

// ── UiSnapshot ────────────────────────────────────────────────────

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

    // Tokio backend chạy trong thread riêng
    let shared_bg = shared.clone();
    let config_bg = config.clone();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(run_backend(config_bg, shared_bg));
    });

    // System tray
    let tray_ctx = shared.clone();
    let _tray    = build_tray(tray_ctx);

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
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill           = BG;
            visuals.window_fill          = BG_PANEL;
            visuals.window_stroke        = Stroke::new(1.0, BORDER);
            visuals.window_corner_radius = egui::CornerRadius::same(6u8);
            visuals.widgets.noninteractive.bg_fill   = BG_PANEL;
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, COL_DIM);
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::new(McpApp::new(shared.clone())))
        }),
    ).map_err(|e| anyhow::anyhow!("eframe: {e}"))?;

    Ok(())
}

// ── System tray ───────────────────────────────────────────────────

fn build_tray(shared: Shared) -> Option<tray_icon::TrayIcon> {
    use tray_icon::{
        TrayIconBuilder,
        menu::{Menu, MenuItem, MenuEvent},
    };

    let icon = {
        let size = 16usize;
        let mut rgba = vec![0u8; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                let i  = (y * size + x) * 4;
                let dx = x as i32 - size as i32 / 2;
                let dy = y as i32 - size as i32 / 2;
                let r  = (size as i32 / 2 - 1).pow(2);
                if dx * dx + dy * dy < r {
                    rgba[i]     = 48;
                    rgba[i + 1] = 210;
                    rgba[i + 2] = 140;
                    rgba[i + 3] = 255;
                }
            }
        }
        tray_icon::Icon::from_rgba(rgba, size as u32, size as u32).ok()?
    };

    let quit_item = MenuItem::new("Thoát", true, None);
    let quit_id   = quit_item.id().clone();

    let menu = Menu::new();
    menu.append(&MenuItem::new("Roblox Studio Bridge", false, None)).ok();
    menu.append(&tray_icon::menu::PredefinedMenuItem::separator()).ok();
    menu.append(&quit_item).ok();

    let tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("Roblox Studio Bridge")
        .with_menu(Box::new(menu))
        .build()
        .ok()?;

    // Thread lắng nghe menu events
    std::thread::spawn(move || {
        let receiver = MenuEvent::receiver();
        loop {
            if let Ok(event) = receiver.recv() {
                if event.id == quit_id {
                    if let Ok(mut s) = shared.lock() {
                        s.should_quit = true;
                    }
                }
            }
        }
    });

    Some(tray)
}

// ── Backend ───────────────────────────────────────────────────────

async fn run_backend(config: Config, shared: Shared) {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("roblox_studio_bridge=info,tower_http=warn")
        .with_ansi(false)
        .init();

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

    // Tunnel với auto-restart
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
                // Trigger auto-copy lần đầu
                s.auto_copy_url = Some(url);
            }

            // Theo dõi URL thay đổi khi tunnel restart
            let shared2 = shared.clone();
            tokio::spawn(async move {
                loop {
                    // Chờ giá trị mới
                    if url_rx.changed().await.is_err() { break; }
                    let new_url = url_rx.borrow().clone();
                    match new_url {
                        None => {
                            push(&shared2, LogLine::warn("Tunnel mất kết nối, đang restart…"));
                            if let Ok(mut s) = shared2.lock() {
                                s.tunnel_url = None;
                            }
                        }
                        Some(u) => {
                            push(&shared2, LogLine::success(format!("Tunnel mới: {u}")));
                            push(&shared2, LogLine::url(u.clone()));
                            push(&shared2, LogLine::dim("⚠ URL đã đổi — cập nhật lại connector trong claude.ai"));
                            if let Ok(mut s) = shared2.lock() {
                                s.tunnel_url    = Some(u.clone());
                                // Auto-copy URL mới
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

    // Plugin heartbeat monitor — dùng refresh_online_status() mỗi 3s
    {
        let bridge2  = bridge.clone();
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

    loop { tokio::time::sleep(Duration::from_secs(60)).await; }
}