/// state.rs — SharedState, LogLine, Shared — dùng chung giữa gui và backend

use crate::Config;
use std::sync::{Arc, Mutex};

// ── Log ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum LogKind { Info, Success, Warn, Error, Dim, Url, Separator }

#[derive(Clone)]
pub struct LogLine {
    pub kind: LogKind,
    pub text: String,
    pub time: Option<String>,
}

impl LogLine {
    pub fn info(t: impl Into<String>)    -> Self { Self::new(LogKind::Info,    t) }
    pub fn success(t: impl Into<String>) -> Self { Self::new(LogKind::Success, t) }
    pub fn warn(t: impl Into<String>)    -> Self { Self::new(LogKind::Warn,    t) }
    pub fn error(t: impl Into<String>)   -> Self { Self::new(LogKind::Error,   t) }
    pub fn dim(t: impl Into<String>)     -> Self { Self::new(LogKind::Dim,     t) }
    pub fn url(t: impl Into<String>)     -> Self { Self::new(LogKind::Url,     t) }
    pub fn sep() -> Self {
        Self { kind: LogKind::Separator, text: String::new(), time: None }
    }

    pub fn new(kind: LogKind, text: impl Into<String>) -> Self {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            kind,
            text: text.into(),
            time: Some(format!("{:02}:{:02}:{:02}", (secs/3600)%24, (secs/60)%60, secs%60)),
        }
    }
}

// ── Shared state ──────────────────────────────────────────────────

#[derive(Default)]
pub struct SharedState {
    pub lines:          Vec<LogLine>,
    pub tunnel_url:     Option<String>,
    pub plugin_online:  bool,
    pub ready:          bool,
    pub error:          Option<String>,
    pub auto_copy_url:  Option<String>,
    /// Signal backend restart với config mới (set từ Settings tab)
    pub restart_config: Option<Config>,
    /// Đang download cloudflared
    pub downloading:    bool,
}

pub type Shared = Arc<Mutex<SharedState>>;

pub fn push(shared: &Shared, line: LogLine) {
    if let Ok(mut s) = shared.lock() { s.lines.push(line); }
}