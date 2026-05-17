/// bridge.rs
/// HTTP server chạy local, nhận long-poll từ Roblox Studio plugin
/// Plugin gọi GET /poll để nhận lệnh, POST /result để trả kết quả

use anyhow::Result;
use axum::{
    Router,
    extract::{Json, State},
    http::StatusCode,
    routing::{get, post},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, oneshot};
use tracing::{info, debug};
use uuid::Uuid;

// ── Shared state ───────────────────────────────────────────────────

/// Một lệnh đang chờ được gửi tới Studio
#[derive(Debug, Clone, Serialize)]
pub struct PendingCommand {
    pub id: String,
    pub kind: CommandKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandKind {
    RunCode { code: String },
    GetInstances { path: String },
    InsertPart { name: String, parent: String },
    GetScripts {},
}

/// Kết quả từ Studio trả về
#[derive(Debug, Deserialize)]
pub struct CommandResult {
    pub id: String,
    pub output: String,
    pub success: bool,
}

/// State chia sẻ giữa bridge server và MCP server
#[derive(Clone)]
pub struct BridgeState {
    inner: Arc<BridgeInner>,
}

struct BridgeInner {
    /// Queue lệnh chờ gửi tới plugin (chỉ 1 lệnh mỗi lần)
    pending: Mutex<Option<PendingCommand>>,
    /// Map id → sender để trả kết quả về caller
    waiters: Mutex<HashMap<String, oneshot::Sender<CommandResult>>>,
    /// Có plugin nào đang kết nối không
    plugin_connected: Mutex<bool>,
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(BridgeInner {
                pending: Mutex::new(None),
                waiters: Mutex::new(HashMap::new()),
                plugin_connected: Mutex::new(false),
            }),
        }
    }

    /// Gửi lệnh tới Studio và chờ kết quả (timeout 30s)
    pub async fn send_command(&self, kind: CommandKind) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let cmd = PendingCommand { id: id.clone(), kind };

        // Tạo channel để nhận kết quả
        let (tx, rx) = oneshot::channel();
        {
            let mut waiters = self.inner.waiters.lock().await;
            waiters.insert(id.clone(), tx);
        }

        // Đưa vào queue
        {
            let mut pending = self.inner.pending.lock().await;
            *pending = Some(cmd);
        }

        // Chờ kết quả tối đa 30 giây
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(result)) => {
                if result.success {
                    Ok(result.output)
                } else {
                    anyhow::bail!("Studio error: {}", result.output)
                }
            }
            Ok(Err(_)) => anyhow::bail!("Channel closed unexpectedly"),
            Err(_) => {
                // Cleanup
                self.inner.waiters.lock().await.remove(&id);
                self.inner.pending.lock().await.take();
                anyhow::bail!("Timeout: Studio không phản hồi sau 30 giây. Plugin có đang chạy không?")
            }
        }
    }

    pub fn is_connected(&self) -> Arc<BridgeInner> {
        self.inner.clone()
    }
}

// ── HTTP handlers cho plugin ────────────────────────────────────────

/// Plugin gọi GET /poll — long-poll chờ lệnh tiếp theo
async fn handle_poll(State(state): State<BridgeState>) -> impl IntoResponse {
    // Đánh dấu plugin đã kết nối
    *state.inner.plugin_connected.lock().await = true;

    // Poll tối đa 10 giây, kiểm tra mỗi 100ms
    for _ in 0..100 {
        let cmd = state.inner.pending.lock().await.take();
        if let Some(cmd) = cmd {
            debug!("Sending command to plugin: {}", cmd.id);
            return (StatusCode::OK, Json(Some(cmd))).into_response();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Timeout — trả về null, plugin sẽ poll lại
    (StatusCode::OK, Json::<Option<PendingCommand>>(None)).into_response()
}

/// Plugin POST /result — trả kết quả về
async fn handle_result(
    State(state): State<BridgeState>,
    Json(result): Json<CommandResult>,
) -> impl IntoResponse {
    debug!("Got result for command {}: success={}", result.id, result.success);

    let mut waiters = state.inner.waiters.lock().await;
    if let Some(tx) = waiters.remove(&result.id) {
        let _ = tx.send(result);
        StatusCode::OK
    } else {
        tracing::warn!("No waiter found for command id");
        StatusCode::NOT_FOUND
    }
}

/// Plugin GET /health — kiểm tra server còn sống
async fn handle_health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Chạy bridge server
pub async fn run_bridge_server(state: BridgeState, port: u16) -> Result<()> {
    let app = Router::new()
        .route("/poll", get(handle_poll))
        .route("/result", post(handle_result))
        .route("/health", get(handle_health))
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    info!("Bridge server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}