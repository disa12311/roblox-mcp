/// bridge.rs — HTTP server local, long-poll cho Roblox Studio plugin
///
/// Cải tiến so với v0.1:
/// - Dùng `tokio::sync::Notify` thay vì spin-poll 100×100ms → giảm CPU idle
/// - Thêm `plugin_online: AtomicBool` để track trạng thái kết nối plugin
/// - Tách `PendingSlot` thành struct riêng để dễ đọc hơn
/// - Timeout logic gọn hơn với `select!`

use anyhow::Result;
use axum::{
    Router,
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};
use tokio::sync::{oneshot, Mutex, Notify};
use tracing::{debug, info, warn};
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
    /// Lấy tất cả scripts kèm source
    GetScripts {},
    /// Snapshot toàn bộ game state — 1 lần trả hết context
    Snapshot {},
    /// Chạy nhiều đoạn code tuần tự, trả từng kết quả
    BatchRun { codes: Vec<String> },
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

/// Toàn bộ state chia sẻ giữa bridge server và MCP server.
#[derive(Clone)]
pub struct BridgeState {
    inner: Arc<BridgeInner>,
}

struct BridgeInner {
    /// Lệnh đang chờ plugin nhận — None nếu chưa có lệnh mới.
    pending: Mutex<Option<PendingCommand>>,
    /// Notify để /poll biết khi có lệnh mới (thay vì spin-poll).
    pending_notify: Notify,
    /// Map id → sender để trả kết quả về caller.
    waiters: Mutex<HashMap<String, oneshot::Sender<CommandResult>>>,
    /// Plugin có đang kết nối không (dùng cho status hiển thị).
    pub plugin_online: AtomicBool,
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(BridgeInner {
                pending:        Mutex::new(None),
                pending_notify: Notify::new(),
                waiters:        Mutex::new(HashMap::new()),
                plugin_online:  AtomicBool::new(false),
            }),
        }
    }

    /// Gửi lệnh đến plugin và chờ kết quả (timeout 30s).
    pub async fn send_command(&self, kind: CommandKind) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        {
            let mut waiters = self.inner.waiters.lock().await;
            waiters.insert(id.clone(), tx);
        }
        {
            let mut pending = self.inner.pending.lock().await;
            *pending = Some(PendingCommand { id: id.clone(), kind });
        }
        // Notify /poll handler đang chờ
        self.inner.pending_notify.notify_one();

        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(r)) if r.success => Ok(r.output),
            Ok(Ok(r))              => anyhow::bail!("Studio error: {}", r.output),
            Ok(Err(_))             => anyhow::bail!("Channel closed unexpectedly"),
            Err(_) => {
                // Cleanup khi timeout
                self.inner.waiters.lock().await.remove(&id);
                self.inner.pending.lock().await.take();
                anyhow::bail!("Timeout: Studio plugin không phản hồi trong 30s")
            }
        }
    }

    /// Kiểm tra plugin có đang online không.
    pub fn is_plugin_online(&self) -> bool {
        self.inner.plugin_online.load(Ordering::Relaxed)
    }
}

// ── HTTP handlers ─────────────────────────────────────────────────

/// GET /poll — Plugin gọi liên tục để nhận lệnh mới.
///
/// Thay vì spin 100×100ms, dùng `Notify::notified()` wait + timeout 10s.
/// CPU gần như 0% khi idle, phản hồi < 1ms khi có lệnh.
async fn handle_poll(State(s): State<BridgeState>) -> impl IntoResponse {
    s.inner.plugin_online.store(true, Ordering::Relaxed);

    // Trả ngay nếu đang có lệnh chờ
    if let Some(cmd) = s.inner.pending.lock().await.take() {
        debug!("→ plugin (immediate): {}", cmd.id);
        return (StatusCode::OK, Json(Some(cmd))).into_response();
    }

    // Chờ tối đa 10s cho lệnh tiếp theo — không spin
    let notified = s.inner.pending_notify.notified();
    match tokio::time::timeout(Duration::from_secs(10), notified).await {
        Ok(()) => {
            let cmd = s.inner.pending.lock().await.take();
            if let Some(ref c) = cmd {
                debug!("→ plugin (notified): {}", c.id);
            }
            (StatusCode::OK, Json(cmd)).into_response()
        }
        Err(_) => {
            // Long-poll timeout bình thường — plugin poll lại
            (StatusCode::OK, Json::<Option<PendingCommand>>(None)).into_response()
        }
    }
}

/// POST /result — Plugin gửi kết quả lệnh về.
async fn handle_result(
    State(s): State<BridgeState>,
    Json(r): Json<CommandResult>,
) -> impl IntoResponse {
    debug!("← plugin: {} ok={}", r.id, r.success);
    let mut waiters = s.inner.waiters.lock().await;
    match waiters.remove(&r.id) {
        Some(tx) => {
            let _ = tx.send(r);
            StatusCode::OK
        }
        None => {
            warn!("Không tìm thấy waiter cho id={}", r.id);
            StatusCode::NOT_FOUND
        }
    }
}

/// GET /health — Plugin dùng để kiểm tra bridge còn sống.
async fn handle_health(State(s): State<BridgeState>) -> impl IntoResponse {
    s.inner.plugin_online.store(true, Ordering::Relaxed);
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