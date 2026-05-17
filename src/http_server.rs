/// http_server.rs — MCP Streamable HTTP server cho claude.ai web

use crate::bridge::{BridgeState, CommandKind};
use anyhow::Result;
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    response::IntoResponse,
    routing::get,
};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

#[derive(Clone)]
struct AppState { bridge: BridgeState }

async fn handle_head() -> impl IntoResponse {
    let mut h = HeaderMap::new();
    h.insert("MCP-Protocol-Version", HeaderValue::from_static("2024-11-05"));
    (StatusCode::OK, h)
}

async fn handle_get() -> impl IntoResponse {
    (StatusCode::OK, axum::Json(serde_json::json!({
        "name": "roblox-studio-mcp",
        "version": "0.1.0",
        "protocol": "2024-11-05",
    })))
}

async fn handle_post(
    State(s): State<AppState>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let req: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({
            "jsonrpc":"2.0","id":null,
            "error":{"code":-32700,"message":format!("Parse error: {e}")}
        }))).into_response(),
    };

    let method = req["method"].as_str().unwrap_or("");
    let id = req["id"].clone();

    // notifications không cần response
    if method.starts_with("notifications/") {
        return (StatusCode::OK, axum::Json(serde_json::Value::Null)).into_response();
    }

    let result = match method {
        "initialize" => serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "roblox-studio-mcp", "version": "0.1.0"},
            "instructions": "LUÔN gọi snapshot() đầu tiên. Dùng batch_run() thay vì nhiều run_code()."
        }),

        "tools/list" => serde_json::json!({ "tools": tools_schema() }),

        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            let args = req["params"]["arguments"].clone();
            let out = dispatch(s.bridge, name, args).await;
            serde_json::json!({
                "content": [{"type":"text","text": out}],
                "isError": false
            })
        }

        "ping" => serde_json::json!({}),

        _ => return (StatusCode::OK, axum::Json(serde_json::json!({
            "jsonrpc":"2.0","id":id,
            "error":{"code":-32601,"message":format!("Method not found: {method}")}
        }))).into_response(),
    };

    let mut h = HeaderMap::new();
    h.insert("MCP-Protocol-Version", HeaderValue::from_static("2024-11-05"));
    (StatusCode::OK, h, axum::Json(serde_json::json!({
        "jsonrpc":"2.0","id":id,"result":result
    }))).into_response()
}

async fn dispatch(bridge: BridgeState, name: &str, args: serde_json::Value) -> String {
    let cmd = match name {
        "snapshot"   => CommandKind::Snapshot {},
        "get_scripts" => CommandKind::GetScripts {},
        "status"     => CommandKind::RunCode {
            code: r#"print("✅ "..tostring(version()))"#.to_string(),
        },
        "run_code"   => CommandKind::RunCode {
            code: args["code"].as_str().unwrap_or("").to_string(),
        },
        "get_instances" => CommandKind::GetInstances {
            path: args["path"].as_str().unwrap_or("game").to_string(),
        },
        "insert_part" => CommandKind::InsertPart {
            name: args["name"].as_str().unwrap_or("Part").to_string(),
            parent: args["parent"].as_str().unwrap_or("game.Workspace").to_string(),
        },
        "insert_script" => {
            let n = args["name"].as_str().unwrap_or("Script");
            let t = args["script_type"].as_str().unwrap_or("Script");
            let p = args["parent"].as_str().unwrap_or("game.ServerScriptService");
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
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            CommandKind::BatchRun { codes }
        }
        _ => return format!("❌ Unknown tool: {name}"),
    };

    bridge.send_command(cmd).await.unwrap_or_else(|e| format!("❌ {e}"))
}

fn tools_schema() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "snapshot",
            "description": "Lấy toàn bộ context game 1 lần: instances, scripts, version. Gọi đầu tiên.",
            "inputSchema": {"type":"object","properties":{}}
        },
        {
            "name": "batch_run",
            "description": "Chạy nhiều đoạn Luau tuần tự trong 1 lần gọi. Tiết kiệm token hơn run_code nhiều lần.",
            "inputSchema": {
                "type":"object",
                "properties": {
                    "codes": {
                        "type":"array",
                        "items":{"type":"string"},
                        "description":"Danh sách Luau code chạy theo thứ tự"
                    }
                },
                "required":["codes"]
            }
        },
        {
            "name": "run_code",
            "description": "Chạy 1 đoạn Luau. Dùng batch_run nếu cần nhiều thao tác.",
            "inputSchema": {
                "type":"object",
                "properties":{"code":{"type":"string"}},
                "required":["code"]
            }
        },
        {
            "name": "get_instances",
            "description": "Xem children của 1 path cụ thể.",
            "inputSchema": {
                "type":"object",
                "properties":{"path":{"type":"string","description":"Ví dụ: game.Workspace"}},
                "required":["path"]
            }
        },
        {
            "name": "insert_part",
            "description": "Tạo Part mới. Hoặc dùng run_code để tạo kèm properties.",
            "inputSchema": {
                "type":"object",
                "properties":{
                    "name":{"type":"string"},
                    "parent":{"type":"string"}
                },
                "required":["name","parent"]
            }
        },
        {
            "name": "insert_script",
            "description": "Tạo Script/LocalScript/ModuleScript với source code.",
            "inputSchema": {
                "type":"object",
                "properties":{
                    "name":{"type":"string"},
                    "script_type":{"type":"string","enum":["Script","LocalScript","ModuleScript"]},
                    "parent":{"type":"string"},
                    "source":{"type":"string"}
                },
                "required":["name","script_type","parent","source"]
            }
        },
        {
            "name": "status",
            "description": "Kiểm tra kết nối nhanh. snapshot() đã bao gồm thông tin này.",
            "inputSchema":{"type":"object","properties":{}}
        }
    ])
}

pub async fn run_mcp_http_server(bridge: BridgeState, port: u16) -> Result<()> {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::HEAD, Method::OPTIONS])
        .allow_headers(Any)
        .allow_origin(Any);

    let app = Router::new()
        .route("/", get(handle_get).post(handle_post).head(handle_head))
        .route("/health", get(|| async { "OK" }))
        .layer(cors)
        .with_state(AppState { bridge });

    let addr = format!("0.0.0.0:{port}");
    info!("MCP HTTP on http://{addr}");
    axum::serve(tokio::net::TcpListener::bind(&addr).await?, app).await?;
    Ok(())
}