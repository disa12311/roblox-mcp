/// http_server.rs — MCP Streamable HTTP (spec 2025-03-26)
///
/// Flow Claude dùng khi connect:
/// 1. POST / với body {"method":"initialize",...}
///    → Server trả 200 + InitializeResult + Mcp-Session-Id header
/// 2. POST / với body {"method":"notifications/initialized",...}
///    → Server trả 202 Accepted (no body)
/// 3. POST / với body {"method":"tools/list",...}
///    → Server trả danh sách tools
/// 4. POST / với body {"method":"tools/call",...}
///    → Server chạy tool và trả kết quả

use crate::bridge::{BridgeState, CommandKind};
use anyhow::Result;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::IntoResponse,
    routing::any,
};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    bridge: BridgeState,
}

async fn mcp_handler(
    method: Method,
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    match method {
        Method::POST    => handle_post(s, headers, body).await.into_response(),
        Method::GET     => handle_get().await.into_response(),
        Method::HEAD    => handle_head().await.into_response(),
        Method::OPTIONS => handle_options().await.into_response(),
        _               => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

// HEAD / — protocol discovery
async fn handle_head() -> impl IntoResponse {
    let mut h = HeaderMap::new();
    h.insert("MCP-Protocol-Version", HeaderValue::from_static("2025-03-26"));
    h.insert(header::CONTENT_TYPE,   HeaderValue::from_static("application/json"));
    (StatusCode::OK, h)
}

// GET / — trả 405 để Claude biết đây là POST-only Streamable HTTP server
// (không phải SSE server — SSE server dùng GET để stream)
async fn handle_get() -> impl IntoResponse {
    let mut h = HeaderMap::new();
    h.insert(header::ALLOW,          HeaderValue::from_static("POST, HEAD, OPTIONS"));
    h.insert("MCP-Protocol-Version", HeaderValue::from_static("2025-03-26"));
    (StatusCode::METHOD_NOT_ALLOWED, h)
}

// OPTIONS / — CORS preflight
async fn handle_options() -> impl IntoResponse {
    let mut h = HeaderMap::new();
    h.insert(header::ALLOW,                        HeaderValue::from_static("POST, HEAD, OPTIONS"));
    h.insert("Access-Control-Allow-Origin",        HeaderValue::from_static("*"));
    h.insert("Access-Control-Allow-Methods",       HeaderValue::from_static("POST, HEAD, OPTIONS"));
    h.insert("Access-Control-Allow-Headers",       HeaderValue::from_static("Content-Type, Mcp-Session-Id, Accept"));
    h.insert("MCP-Protocol-Version",               HeaderValue::from_static("2025-03-26"));
    (StatusCode::NO_CONTENT, h)
}

// POST / — endpoint chính xử lý tất cả JSON-RPC
async fn handle_post(
    s: AppState,
    req_headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Parse JSON-RPC
    let req: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return make_response(StatusCode::BAD_REQUEST, None, serde_json::json!({
                "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": format!("Parse error: {e}") }
            })).into_response();
        }
    };

    let method = req["method"].as_str().unwrap_or("");
    let id     = req["id"].clone();

    // Notifications — trả 202 Accepted, không cần body
    if method.starts_with("notifications/") {
        let mut h = HeaderMap::new();
        h.insert("MCP-Protocol-Version", HeaderValue::from_static("2025-03-26"));
        return (StatusCode::ACCEPTED, h).into_response();
    }

    // Session ID từ request header hoặc tạo mới khi initialize
    let session_id = req_headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    tracing::debug!("MCP [{session_id}] → {method}");

    let result = match method {
        "initialize" => {
            // Trả đúng protocolVersion mà client gửi lên (hoặc version mình support)
            let client_version = req["params"]["protocolVersion"]
                .as_str()
                .unwrap_or("2025-03-26");
            let proto = if client_version >= "2025-03-26" {
                "2025-03-26"
            } else {
                "2024-11-05"
            };
            serde_json::json!({
                "protocolVersion": proto,
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "roblox-studio-mcp",
                    "version": "0.1.0"
                },
                "instructions": "\
                    QUAN TRỌNG: \
                    1. Gọi snapshot() ĐẦU TIÊN để lấy toàn bộ context game. \
                    2. Dùng batch_run([...]) thay vì nhiều run_code() riêng lẻ. \
                    3. Gộp nhiều thay đổi vào 1 đoạn code khi có thể."
            })
        }

        "tools/list" => serde_json::json!({
            "tools": tools_schema()
        }),

        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            let args = req["params"]["arguments"].clone();
            let out  = dispatch(s.bridge, name, args).await;
            serde_json::json!({
                "content": [{ "type": "text", "text": out }],
                "isError": false
            })
        }

        "ping" => serde_json::json!({}),

        _ => {
            return make_response(
                StatusCode::OK,
                Some(&session_id),
                serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {method}")
                    }
                }),
            ).into_response();
        }
    };

    make_response(
        StatusCode::OK,
        Some(&session_id),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }),
    )
    .into_response()
}

fn make_response(
    status: StatusCode,
    session_id: Option<&str>,
    body: serde_json::Value,
) -> impl IntoResponse {
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE,   HeaderValue::from_static("application/json"));
    h.insert("MCP-Protocol-Version", HeaderValue::from_static("2025-03-26"));
    h.insert("Access-Control-Allow-Origin", HeaderValue::from_static("*"));
    if let Some(sid) = session_id {
        if let Ok(v) = HeaderValue::from_str(sid) {
            h.insert("Mcp-Session-Id", v);
        }
    }
    (status, h, axum::Json(body))
}

// ── Tool dispatch ─────────────────────────────────────────────────

async fn dispatch(bridge: BridgeState, name: &str, args: serde_json::Value) -> String {
    let cmd = match name {
        "snapshot"      => CommandKind::Snapshot {},
        "get_scripts"   => CommandKind::GetScripts {},
        "status"        => CommandKind::RunCode {
            code: r#"print("✅ "..tostring(version()))"#.to_string(),
        },
        "run_code"      => CommandKind::RunCode {
            code: args["code"].as_str().unwrap_or("").to_string(),
        },
        "get_instances" => CommandKind::GetInstances {
            path: args["path"].as_str().unwrap_or("game").to_string(),
        },
        "insert_part"   => CommandKind::InsertPart {
            name:   args["name"].as_str().unwrap_or("Part").to_string(),
            parent: args["parent"].as_str().unwrap_or("game.Workspace").to_string(),
        },
        "insert_script" => {
            let n   = args["name"].as_str().unwrap_or("Script");
            let t   = args["script_type"].as_str().unwrap_or("Script");
            let p   = args["parent"].as_str().unwrap_or("game.ServerScriptService");
            let src = args["source"].as_str().unwrap_or("");
            CommandKind::RunCode {
                code: format!(
                    "local s=Instance.new(\"{t}\")\ns.Name=\"{n}\"\ns.Source=[=[{src}]=]\ns.Parent={p}\nprint(\"✅ \"..s.Name)"
                ),
            }
        }
        "batch_run" => {
            let codes = args["codes"]
                .as_array()
                .map(|a| a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect())
                .unwrap_or_default();
            CommandKind::BatchRun { codes }
        }
        _ => return format!("❌ Unknown tool: {name}"),
    };

    bridge.send_command(cmd).await.unwrap_or_else(|e| format!("❌ {e}"))
}

// ── Tool schema ───────────────────────────────────────────────────

fn tools_schema() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "snapshot",
            "description": "Lấy toàn bộ context game 1 lần: instances, scripts, version. Gọi đầu tiên.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "batch_run",
            "description": "Chạy nhiều đoạn Luau tuần tự trong 1 lần gọi. Tiết kiệm token hơn run_code nhiều lần.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "codes": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Danh sách code chạy theo thứ tự"
                    }
                },
                "required": ["codes"]
            }
        },
        {
            "name": "run_code",
            "description": "Chạy 1 đoạn Luau. Dùng batch_run nếu cần nhiều thao tác.",
            "inputSchema": {
                "type": "object",
                "properties": { "code": { "type": "string" } },
                "required": ["code"]
            }
        },
        {
            "name": "get_instances",
            "description": "Xem children của 1 path. Dùng snapshot() nếu cần tổng thể.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Ví dụ: game.Workspace" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "insert_part",
            "description": "Tạo Part mới.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name":   { "type": "string" },
                    "parent": { "type": "string" }
                },
                "required": ["name", "parent"]
            }
        },
        {
            "name": "insert_script",
            "description": "Tạo Script/LocalScript/ModuleScript với source code.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name":        { "type": "string" },
                    "script_type": { "type": "string", "enum": ["Script","LocalScript","ModuleScript"] },
                    "parent":      { "type": "string" },
                    "source":      { "type": "string" }
                },
                "required": ["name","script_type","parent","source"]
            }
        },
        {
            "name": "status",
            "description": "Kiểm tra kết nối với Roblox Studio plugin.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

// ── Server entry ──────────────────────────────────────────────────

pub async fn run_mcp_http_server(bridge: BridgeState, port: u16) -> Result<()> {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::HEAD, Method::OPTIONS])
        .allow_headers(Any)
        .allow_origin(Any);

    let app = Router::new()
        .route("/",       any(mcp_handler))
        .route("/health", any(|| async { "OK" }))
        .layer(cors)
        .with_state(AppState { bridge });

    let addr = format!("0.0.0.0:{port}");
    info!("MCP HTTP on http://{addr}");
    axum::serve(tokio::net::TcpListener::bind(&addr).await?, app).await?;
    Ok(())
}