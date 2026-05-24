/// gui.rs — egui app: McpApp, SharedState, LogLine, Toast, colors

use crate::config::{self, AppConfig, TunnelMode};
use crate::state::{LogLine, LogKind, Shared, SharedState, push};
use crate::Config;
use eframe::egui::{self, Color32, FontFamily, FontId, Margin, RichText, ScrollArea, Stroke, Vec2};
use std::time::Duration;

// ── Colors ────────────────────────────────────────────────────────

pub const BG:          Color32 = Color32::from_rgb(13,  15,  22);
pub const BG_PANEL:    Color32 = Color32::from_rgb(20,  22,  32);
pub const BG_INPUT:    Color32 = Color32::from_rgb(28,  30,  42);
pub const BORDER:      Color32 = Color32::from_rgb(40,  44,  64);
pub const BORDER_ACT:  Color32 = Color32::from_rgb(80,  100, 180);
const COL_TIME:    Color32 = Color32::from_rgb(55,  60,  85);
const COL_INFO:    Color32 = Color32::from_rgb(180, 185, 210);
const COL_SUCCESS: Color32 = Color32::from_rgb(72,  210, 140);
const COL_WARN:    Color32 = Color32::from_rgb(240, 190,  60);
const COL_ERROR:   Color32 = Color32::from_rgb(232,  80,  80);
const COL_DIM:     Color32 = Color32::from_rgb(70,  74, 100);
const COL_URL:     Color32 = Color32::from_rgb(80,  180, 255);
const COL_SEP:     Color32 = Color32::from_rgb(35,  38,  58);

// ── Toast ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct Toast {
    text:      String,
    born_secs: f64,
    duration:  f64,
}

impl Toast {
    fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), born_secs: 0.0, duration: 3.0 }
    }
    fn is_alive(&self, now: f64) -> bool { now - self.born_secs < self.duration }
    fn alpha(&self, now: f64) -> f32 {
        let age        = (now - self.born_secs) as f32;
        let fade_start = (self.duration - 0.6) as f32;
        if age < fade_start { 1.0 } else { 1.0 - (age - fade_start) / 0.6 }
    }
}

// ── Tab ───────────────────────────────────────────────────────────

#[derive(PartialEq)]
enum Tab { Log, Settings }

// ── UiSnapshot ────────────────────────────────────────────────────

pub(crate) struct UiSnapshot {
    lines:         Vec<LogLine>,
    tunnel_url:    Option<String>,
    plugin_online: bool,
    ready:         bool,
    error:         Option<String>,
    downloading:   bool,
}

impl SharedState {
    pub fn clone_for_ui(&self) -> UiSnapshot {
        UiSnapshot {
            lines:         self.lines.clone(),
            tunnel_url:    self.tunnel_url.clone(),
            plugin_online: self.plugin_online,
            ready:         self.ready,
            error:         self.error.clone(),
            downloading:   self.downloading,
        }
    }
}

// ── App ───────────────────────────────────────────────────────────

pub struct McpApp {
    shared:           Shared,
    last_online:      bool,
    scroll_to_bottom: bool,
    toast:            Option<Toast>,
    tab:              Tab,
    cfg:              AppConfig,
    token_buf:        String,
    show_token:       bool,
    cfg_dirty:        bool,
}

impl McpApp {
    pub fn new(shared: Shared, cfg: AppConfig) -> Self {
        let token_buf = cfg.tunnel_token.clone();
        Self {
            shared,
            last_online:      false,
            scroll_to_bottom: true,
            toast:            None,
            tab:              Tab::Log,
            token_buf,
            show_token:       false,
            cfg_dirty:        false,
            cfg,
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

        // Auto-copy URL khi tunnel ready
        let auto_copy = { self.shared.lock().unwrap().auto_copy_url.take() };
        if let Some(ref url) = auto_copy {
            ctx.copy_text(url.clone());
            self.show_toast("✓ Đã copy URL vào clipboard", now);
            push(&self.shared, LogLine::success("URL đã copy vào clipboard tự động"));
        }

        let state       = self.shared.lock().unwrap().clone_for_ui();
        let now_online  = state.plugin_online;
        let downloading = state.downloading;

        if now_online != self.last_online {
            if now_online {
                push(&self.shared, LogLine::success("Plugin Roblox Studio đã kết nối"));
            } else if self.last_online {
                push(&self.shared, LogLine::warn("Plugin mất kết nối"));
            }
            self.last_online = now_online;
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            std::process::exit(0);
        }
        if let Some(ref t) = self.toast.clone() {
            if !t.is_alive(now) { self.toast = None; }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::same(0i8)))
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());
                self.show_header(ui, ctx, &state, now);
                ui.add_space(1.0);
                self.show_tab_bar(ui);
                ui.add_space(1.0);

                match self.tab {
                    Tab::Log      => self.show_log_tab(ui, &state),
                    Tab::Settings => self.show_settings_tab(ui, ctx, &state, now, downloading),
                }

                self.show_status_bar(ui, &state);
            });

        self.show_toast_overlay(ctx, now);
    }
}

// ── Header ────────────────────────────────────────────────────────

impl McpApp {
    fn show_header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, state: &UiSnapshot, now: f64) {
        egui::Frame::new()
            .fill(BG_PANEL)
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(Margin::symmetric(14i8, 10i8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (dot_col, dot_tip) = if state.error.is_some() {
                        (COL_ERROR, "Error")
                    } else if !state.ready {
                        (COL_WARN,  "Starting…")
                    } else if state.plugin_online {
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
                        if ui.small_button(
                            RichText::new("─").color(COL_DIM).size(13.0)
                        ).on_hover_text("Thu nhỏ cửa sổ").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        ui.add_space(4.0);
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
    }

    fn show_tab_bar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(BG_PANEL)
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(Margin::symmetric(14i8, 0i8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(2.0);
                    for (label, variant) in [("📋 Log", Tab::Log), ("⚙ Settings", Tab::Settings)] {
                        let active = self.tab == variant;
                        let color  = if active { COL_INFO } else { COL_DIM };
                        let resp   = ui.add(egui::Label::new(
                            RichText::new(label)
                                .color(color)
                                .font(FontId::new(12.0, FontFamily::Monospace))
                        ).sense(egui::Sense::click()));
                        if resp.clicked() { self.tab = variant; }
                        if active {
                            let rect = resp.rect;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(rect.min.x, rect.max.y + 1.0),
                                    egui::pos2(rect.max.x, rect.max.y + 1.0),
                                ],
                                Stroke::new(2.0, COL_SUCCESS),
                            );
                        }
                        ui.add_space(12.0);
                    }
                    if self.cfg_dirty {
                        ui.label(RichText::new("● unsaved")
                            .color(COL_WARN)
                            .font(FontId::new(10.0, FontFamily::Monospace)));
                    }
                });
            });
    }

    fn show_status_bar(&mut self, ui: &mut egui::Ui, state: &UiSnapshot) {
        egui::TopBottomPanel::bottom("statusbar")
            .frame(egui::Frame::new()
                .fill(BG_PANEL)
                .stroke(Stroke::new(1.0, BORDER))
                .inner_margin(Margin::symmetric(14i8, 6i8)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    let (pill_col, pill_text) = if state.plugin_online {
                        (COL_SUCCESS, "● plugin online")
                    } else {
                        (COL_DIM, "○ plugin offline")
                    };
                    ui.label(RichText::new(pill_text).color(pill_col)
                        .font(FontId::new(11.0, FontFamily::Monospace)));
                    ui.separator();
                    let mode_text = match self.cfg.tunnel_mode {
                        TunnelMode::Quick => "quick tunnel",
                        TunnelMode::Named => "named tunnel",
                    };
                    ui.label(RichText::new(format!(
                        "mcp :{}  bridge :{}  {}",
                        self.cfg.mcp_port, self.cfg.bridge_port, mode_text
                    )).color(COL_DIM).font(FontId::new(11.0, FontFamily::Monospace)));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let icon = if self.scroll_to_bottom { "⬇ auto" } else { "⬇ manual" };
                        if ui.small_button(RichText::new(icon).color(COL_DIM).size(10.0))
                            .on_hover_text("Toggle auto-scroll").clicked()
                        {
                            self.scroll_to_bottom = !self.scroll_to_bottom;
                        }
                    });
                });
            });
    }

    fn show_toast_overlay(&self, ctx: &egui::Context, now: f64) {
        if let Some(ref toast) = self.toast {
            let alpha = toast.alpha(now);
            let bg = Color32::from_rgba_unmultiplied(30, 34, 50, (alpha * 230.0) as u8);
            let fg = Color32::from_rgba_unmultiplied(72, 210, 140, (alpha * 255.0) as u8);
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

// ── Log tab ───────────────────────────────────────────────────────

impl McpApp {
    fn show_log_tab(&mut self, ui: &mut egui::Ui, state: &UiSnapshot) {
        egui::Frame::new()
            .fill(BG)
            .inner_margin(Margin::symmetric(14i8, 8i8))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(self.scroll_to_bottom)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        for line in &state.lines {
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
    }
}

// ── Settings tab ──────────────────────────────────────────────────

impl McpApp {
    fn show_settings_tab(
        &mut self,
        ui:          &mut egui::Ui,
        _ctx:        &egui::Context,
        state:       &UiSnapshot,
        now:         f64,
        downloading: bool,
    ) {
        egui::Frame::new()
            .fill(BG)
            .inner_margin(Margin::symmetric(20i8, 16i8))
            .show(ui, |ui| {
                ui.set_max_width(520.0);
                let mono = |s: &str, col: Color32| {
                    RichText::new(s).color(col).font(FontId::new(12.0, FontFamily::Monospace))
                };

                // Tunnel mode tiles
                ui.label(mono("TUNNEL MODE", COL_DIM));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for (mode, icon, line1, line2) in [
                        (TunnelMode::Quick, "⚡ Quick Tunnel", "Không cần tài khoản",   "URL đổi mỗi restart"),
                        (TunnelMode::Named, "🔒 Named Tunnel", "Cần Cloudflare account", "URL cố định, không đổi"),
                    ] {
                        let active = self.cfg.tunnel_mode == mode;
                        let stroke = if active { Stroke::new(2.0, COL_SUCCESS) } else { Stroke::new(1.0, BORDER) };
                        let title_col = if active { COL_SUCCESS } else { COL_INFO };
                        let resp = egui::Frame::new()
                            .fill(BG_PANEL).stroke(stroke)
                            .corner_radius(egui::CornerRadius::same(6u8))
                            .inner_margin(Margin::same(12i8))
                            .show(ui, |ui| {
                                ui.set_min_width(200.0);
                                ui.label(mono(icon, title_col));
                                ui.add_space(4.0);
                                ui.label(mono(line1, COL_DIM));
                                ui.label(mono(line2, COL_DIM));
                            });
                        if resp.response.interact(egui::Sense::click()).clicked() {
                            self.cfg.tunnel_mode = mode;
                            self.cfg_dirty = true;
                        }
                        ui.add_space(12.0);
                    }
                });

                // Token input (Named only)
                if self.cfg.tunnel_mode == TunnelMode::Named {
                    ui.add_space(16.0);
                    ui.label(mono("TUNNEL TOKEN", COL_DIM));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.token_buf)
                                .password(!self.show_token)
                                .font(FontId::new(12.0, FontFamily::Monospace))
                                .desired_width(ui.available_width() - 80.0)
                                .hint_text("eyJhbGciOi…")
                                .background_color(BG_INPUT)
                        );
                        if resp.changed() {
                            self.cfg.tunnel_token = self.token_buf.clone();
                            self.cfg_dirty = true;
                        }
                        ui.add_space(6.0);
                        let eye = if self.show_token { "🙈 hide" } else { "👁 show" };
                        if ui.small_button(mono(eye, COL_DIM)).clicked() {
                            self.show_token = !self.show_token;
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(mono("Lấy token tại:", COL_DIM));
                        ui.add_space(4.0);
                        if ui.add(egui::Label::new(
                            mono("dash.cloudflare.com → Zero Trust → Tunnels", COL_URL)
                        ).sense(egui::Sense::click())).clicked() {
                            open_url("https://dash.cloudflare.com/?to=/:account/zero-trust/tunnels");
                        }
                    });
                }

                // Ports
                ui.add_space(20.0);
                ui.label(mono("PORTS", COL_DIM));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for (label, port) in [
                        ("MCP port   ", &mut self.cfg.mcp_port),
                        ("Bridge port", &mut self.cfg.bridge_port),
                    ] {
                        ui.label(mono(label, COL_INFO));
                        let mut s = port.to_string();
                        if ui.add(
                            egui::TextEdit::singleline(&mut s)
                                .desired_width(60.0)
                                .font(FontId::new(12.0, FontFamily::Monospace))
                                .background_color(BG_INPUT)
                        ).changed() {
                            if let Ok(p) = s.parse::<u16>() { *port = p; self.cfg_dirty = true; }
                        }
                        ui.add_space(20.0);
                    }
                });

                // cloudflared
                ui.add_space(20.0);
                ui.label(mono("CLOUDFLARED", COL_DIM));
                ui.add_space(8.0);
                let cf_exists = config::cloudflared_exists();
                ui.horizontal(|ui| {
                    let (icon, col) = if cf_exists {
                        ("✓ cloudflared đã có", COL_SUCCESS)
                    } else {
                        ("✗ cloudflared chưa cài", COL_ERROR)
                    };
                    ui.label(mono(icon, col));
                    if !cf_exists {
                        ui.add_space(12.0);
                        let btn_text = if downloading { "⟳ đang tải…" } else { "⬇ Tải tự động" };
                        if ui.add_enabled(!downloading, egui::Button::new(mono(btn_text, COL_INFO))).clicked() {
                            let s1 = self.shared.clone();
                            let s2 = self.shared.clone();
                            if let Ok(mut s) = s1.lock() { s.downloading = true; }
                            std::thread::spawn(move || {
                                let rt = tokio::runtime::Runtime::new().unwrap();
                                rt.block_on(async move {
                                    let sc = s2.clone();
                                    let result = config::download_cloudflared(move |msg| {
                                        push(&sc, LogLine::dim(msg));
                                    }).await;
                                    match result {
                                        Ok(_)  => push(&s2, LogLine::success("✅ cloudflared đã tải xong")),
                                        Err(e) => push(&s2, LogLine::error(format!("Download lỗi: {e}"))),
                                    }
                                    if let Ok(mut s) = s2.lock() { s.downloading = false; }
                                });
                            });
                            self.tab = Tab::Log;
                        }
                    }
                });

                // Save / Restart
                ui.add_space(24.0);
                ui.add(egui::Separator::default().horizontal());
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.cfg_dirty, egui::Button::new(
                        RichText::new("💾 Lưu config")
                            .color(if self.cfg_dirty { COL_SUCCESS } else { COL_DIM })
                            .font(FontId::new(12.0, FontFamily::Monospace))
                    )).clicked() {
                        self.cfg.save();
                        self.cfg_dirty = false;
                        self.show_toast("✓ Đã lưu config.json", now);
                    }
                    ui.add_space(8.0);
                    if ui.add_enabled(self.cfg_dirty && state.ready, egui::Button::new(
                        RichText::new("↺ Lưu & Restart")
                            .color(if self.cfg_dirty && state.ready { COL_WARN } else { COL_DIM })
                            .font(FontId::new(12.0, FontFamily::Monospace))
                    )).clicked() {
                        self.cfg.save();
                        self.cfg_dirty = false;
                        self.show_toast("↺ Đang restart…", now);
                        let new_cfg = Config::from_app(&self.cfg);
                        if let Ok(mut s) = self.shared.lock() {
                            s.restart_config = Some(new_cfg);
                            s.ready      = false;
                            s.tunnel_url = None;
                            s.lines.push(LogLine::sep());
                            s.lines.push(LogLine::warn("Restart với config mới…"));
                        }
                    }
                    if !self.cfg_dirty {
                        ui.add_space(8.0);
                        ui.label(mono("Không có thay đổi", COL_DIM));
                    }
                });
                ui.add_space(8.0);
                ui.label(mono("Thay đổi port/tunnel yêu cầu restart để áp dụng.", COL_DIM));
            });
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/c", "start", url]).spawn(); }
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(url).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(url).spawn(); }
}