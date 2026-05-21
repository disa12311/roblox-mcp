/// bridge.rs — HTTP server local, long-poll cho Roblox Studio plugin

use anyhow::Result;
use axum::{
    Router,
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}},
    time::Duration,
};
use tokio::sync::{oneshot, Mutex, Notify};
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── Command kinds ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandKind {
    RunCode      { code: String },
    GetInstances { path: String },
    InsertPart   { name: String, parent: String },
    GetScripts   {},
    Snapshot     {},
    BatchRun     { codes: Vec<String> },
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingCommand {
    pub id:   String,
    pub kind: CommandKind,
}

#[derive(Debug, Deserialize)]
pub struct CommandResult {
    pub id:      String,
    pub output:  String,
    pub success: bool,
}

// ── Shared state ──────────────────────────────────────────────────

#[derive(Clone)]
pub struct BridgeState {
    inner: Arc<BridgeInner>,
}

struct BridgeInner {
    pending:        Mutex<Option<PendingCommand>>,
    pending_notify: Notify,
    waiters:        Mutex<HashMap<String, oneshot::Sender<CommandResult>>>,
    /// true khi plugin đang kết nối
    plugin_online:  AtomicBool,
    /// Unix timestamp (giây) của lần cuối plugin gọi /poll hoặc /health
    last_seen_secs: AtomicU64,
}

/// Plugin được coi là offline nếu không thấy trong 15 giây
const OFFLINE_TIMEOUT_SECS: u64 = 15;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(BridgeInner {
                pending:        Mutex::new(None),
                pending_notify: Notify::new(),
                waiters:        Mutex::new(HashMap::new()),
                plugin_online:  AtomicBool::new(false),
                last_seen_secs: AtomicU64::new(0),
            }),
        }
    }

    /// Gửi lệnh đến plugin và chờ kết quả (timeout 30s).
    pub async fn send_command(&self, kind: CommandKind) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        self.inner.waiters.lock().await.insert(id.clone(), tx);
        *self.inner.pending.lock().await = Some(PendingCommand { id: id.clone(), kind });
        self.inner.pending_notify.notify_one();

        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(r)) if r.success => Ok(r.output),
            Ok(Ok(r))              => anyhow::bail!("Studio error: {}", r.output),
            Ok(Err(_))             => anyhow::bail!("Channel closed unexpectedly"),
            Err(_) => {
                self.inner.waiters.lock().await.remove(&id);
                self.inner.pending.lock().await.take();
                anyhow::bail!("Timeout: Studio plugin không phản hồi trong 30s")
            }
        }
    }

    /// Tính lại online dựa trên last_seen — gọi từ monitor task.
    pub fn refresh_online_status(&self) {
        let elapsed = now_secs().saturating_sub(
            self.inner.last_seen_secs.load(Ordering::Relaxed)
        );
        let online = elapsed < OFFLINE_TIMEOUT_SECS
            && self.inner.last_seen_secs.load(Ordering::Relaxed) != 0;
        self.inner.plugin_online.store(online, Ordering::Relaxed);
    }

    pub fn is_plugin_online(&self) -> bool {
        self.inner.plugin_online.load(Ordering::Relaxed)
    }

    fn touch(&self) {
        self.inner.last_seen_secs.store(now_secs(), Ordering::Relaxed);
        self.inner.plugin_online.store(true, Ordering::Relaxed);
    }
}

// ── HTTP handlers ─────────────────────────────────────────────────

async fn handle_poll(State(s): State<BridgeState>) -> impl IntoResponse {
    s.touch();

    if let Some(cmd) = s.inner.pending.lock().await.take() {
        debug!("→ plugin (immediate): {}", cmd.id);
        return (StatusCode::OK, Json(Some(cmd))).into_response();
    }

    let notified = s.inner.pending_notify.notified();
    match tokio::time::timeout(Duration::from_secs(10), notified).await {
        Ok(()) => {
            let cmd = s.inner.pending.lock().await.take();
            if let Some(ref c) = cmd { debug!("→ plugin (notified): {}", c.id); }
            (StatusCode::OK, Json(cmd)).into_response()
        }
        Err(_) => (StatusCode::OK, Json::<Option<PendingCommand>>(None)).into_response(),
    }
}

async fn handle_result(
    State(s): State<BridgeState>,
    Json(r): Json<CommandResult>,
) -> impl IntoResponse {
    debug!("← plugin: {} ok={}", r.id, r.success);
    let mut waiters = s.inner.waiters.lock().await;
    match waiters.remove(&r.id) {
        Some(tx) => { let _ = tx.send(r); StatusCode::OK }
        None     => { warn!("Không tìm thấy waiter cho id={}", r.id); StatusCode::NOT_FOUND }
    }
}

async fn handle_health(State(s): State<BridgeState>) -> impl IntoResponse {
    s.touch();
    (StatusCode::OK, "OK")
}

pub async fn run_bridge_server(state: BridgeState, port: u16) -> Result<()> {
    let app = Router::new()
        .route("/poll",   get(handle_poll))
        .route("/result", post(handle_result))
        .route("/health", get(handle_health))
        .with_state(state);

    let addr = format!("127.0.0.1:{port}");
    info!("Bridge server on http://{addr}");
    axum::serve(tokio::net::TcpListener::bind(&addr).await?, app).await?;
    Ok(())
}