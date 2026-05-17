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
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, info};
use uuid::Uuid;

// ── Command kinds ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandKind {
    /// Chạy Luau code tùy ý
    RunCode { code: String },
    /// Lấy children của một path
    GetInstances { path: String },
    /// Tạo Part mới
    InsertPart { name: String, parent: String },
    /// Lấy tất cả scripts
    GetScripts {},
    /// Snapshot toàn bộ game state — 1 lần trả hết context
    Snapshot {},
    /// Chạy nhiều đoạn code tuần tự, trả về từng kết quả
    BatchRun { codes: Vec<String> },
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingCommand {
    pub id: String,
    pub kind: CommandKind,
}

#[derive(Debug, Deserialize)]
pub struct CommandResult {
    pub id: String,
    pub output: String,
    pub success: bool,
}

// ── Shared state ──────────────────────────────────────────────────

#[derive(Clone)]
pub struct BridgeState {
    inner: Arc<BridgeInner>,
}

struct BridgeInner {
    pending: Mutex<Option<PendingCommand>>,
    waiters: Mutex<HashMap<String, oneshot::Sender<CommandResult>>>,
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(BridgeInner {
                pending: Mutex::new(None),
                waiters: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub async fn send_command(&self, kind: CommandKind) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        self.inner.waiters.lock().await.insert(id.clone(), tx);
        *self.inner.pending.lock().await = Some(PendingCommand { id: id.clone(), kind });

        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(r)) if r.success => Ok(r.output),
            Ok(Ok(r)) => anyhow::bail!("Studio error: {}", r.output),
            Ok(Err(_)) => anyhow::bail!("Channel closed"),
            Err(_) => {
                self.inner.waiters.lock().await.remove(&id);
                self.inner.pending.lock().await.take();
                anyhow::bail!("Timeout: Studio plugin không phản hồi (30s)")
            }
        }
    }
}

// ── HTTP handlers ─────────────────────────────────────────────────

async fn handle_poll(State(s): State<BridgeState>) -> impl IntoResponse {
    // Long-poll tối đa 10s, trả lệnh khi có
    for _ in 0..100 {
        if let Some(cmd) = s.inner.pending.lock().await.take() {
            debug!("→ plugin: {}", cmd.id);
            return (StatusCode::OK, Json(Some(cmd))).into_response();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    (StatusCode::OK, Json::<Option<PendingCommand>>(None)).into_response()
}

async fn handle_result(
    State(s): State<BridgeState>,
    Json(r): Json<CommandResult>,
) -> impl IntoResponse {
    debug!("← plugin: {} ok={}", r.id, r.success);
    if let Some(tx) = s.inner.waiters.lock().await.remove(&r.id) {
        let _ = tx.send(r);
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn handle_health() -> &'static str { "OK" }

pub async fn run_bridge_server(state: BridgeState, port: u16) -> Result<()> {
    let app = Router::new()
        .route("/poll", get(handle_poll))
        .route("/result", post(handle_result))
        .route("/health", get(handle_health))
        .with_state(state);

    let addr = format!("127.0.0.1:{port}");
    info!("Bridge server on http://{addr}");
    axum::serve(tokio::net::TcpListener::bind(&addr).await?, app).await?;
    Ok(())
}