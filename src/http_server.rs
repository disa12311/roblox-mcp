/// http_server.rs
/// MCP Streamable HTTP transport (spec 2025-11-25)
/// Claude web dùng POST / để gửi JSON-RPC requests

use crate::bridge::BridgeState;
use crate::mcp_server::RobloxMcpServer;
use anyhow::Result;
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

#[derive(Clone)]
struct AppState {
    bridge: BridgeState,
}

/// HEAD / — Protocol discovery (bắt buộc theo MCP spec 2025-11-25)
async fn handle_head() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        "MCP-Protocol-Version",
        HeaderValue::from_static("2025-11-25"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    (StatusCode::OK, headers)
}

/// GET / — Health check + info
async fn handle_get() -> impl IntoResponse {
    let body = serde_json::json!({
        "name": "roblox-studio-mcp",
        "version": "0.1.0",
        "protocol": "2025-11-25",
        "description": "Roblox Studio MCP Server — điều khiển Studio từ Claude",
        "tools": ["run_code", "get_instances", "insert_part", "insert_script", "get_scripts", "status"]
    });
    (StatusCode::OK, axum::Json(body))
}

/// POST / — MCP JSON-RPC endpoint chính
/// Claude gửi tool calls vào đây
async fn handle_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Dùng rmcp's StreamableHttpServerTransport để xử lý
    // Tạo MCP server mới cho mỗi request (stateless là OK)
    let server = RobloxMcpServer::new(state.bridge.clone());

    // Parse JSON-RPC request
    let request: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {e}")
                    },
                    "id": null
                })),
            ).into_response();
        }
    };

    tracing::debug!("MCP request: {}", request["method"].as_str().unwrap_or("unknown"));

    // Handle các method cơ bản của MCP protocol
    let method = request["method"].as_str().unwrap_or("");
    let id = request["id"].clone();

    let response = match method {
        "initialize" => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "roblox-studio-mcp",
                        "version": "0.1.0"
                    },
                    "instructions": "Roblox Studio MCP Server. Gọi tools/list để xem danh sách tools."
                }
            })
        }

        "tools/list" => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": build_tools_list()
                }
            })
        }

        "tools/call" => {
            let params = &request["params"];
            let tool_name = params["name"].as_str().unwrap_or("");
            let arguments = params["arguments"].clone();

            let result = call_tool(server, tool_name, arguments).await;

            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": result
                        }
                    ]
                }
            })
        }

        "ping" => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            })
        }

        _ => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {method}")
                }
            })
        }
    };

    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        "MCP-Protocol-Version",
        HeaderValue::from_static("2025-11-25"),
    );

    (StatusCode::OK, resp_headers, axum::Json(response)).into_response()
}

/// Build danh sách tools với schema đầy đủ
fn build_tools_list() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "run_code",
            "description": "Chạy Luau code tùy ý trong Roblox Studio và trả về output",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "Luau code để chạy trong Roblox Studio. Ví dụ: print('Hello')"
                    }
                },
                "required": ["code"]
            }
        },
        {
            "name": "get_instances",
            "description": "Xem danh sách con (children) của một object trong DataModel",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path trong DataModel. Ví dụ: 'game', 'game.Workspace'"
                    }
                },
                "required": ["path"]
            }
        },
        {
            "name": "insert_part",
            "description": "Tạo một Part mới trong Roblox Studio Workspace",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Tên của Part mới" },
                    "parent": { "type": "string", "description": "Parent path. Ví dụ: 'game.Workspace'" }
                },
                "required": ["name", "parent"]
            }
        },
        {
            "name": "insert_script",
            "description": "Tạo Script/LocalScript/ModuleScript với source code",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Tên script" },
                    "script_type": {
                        "type": "string",
                        "enum": ["Script", "LocalScript", "ModuleScript"],
                        "description": "Loại script"
                    },
                    "parent": { "type": "string", "description": "Parent path" },
                    "source": { "type": "string", "description": "Source code của script" }
                },
                "required": ["name", "script_type", "parent", "source"]
            }
        },
        {
            "name": "get_scripts",
            "description": "Lấy danh sách tất cả Scripts trong game kèm source code",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "status",
            "description": "Kiểm tra trạng thái kết nối với Roblox Studio plugin",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }
    ])
}

/// Dispatch tool call tới đúng handler
async fn call_tool(server: RobloxMcpServer, name: &str, args: serde_json::Value) -> String {
    use crate::bridge::CommandKind;

    let result = match name {
        "run_code" => {
            let code = args["code"].as_str().unwrap_or("").to_string();
            server.bridge.send_command(CommandKind::RunCode { code }).await
        }
        "get_instances" => {
            let path = args["path"].as_str().unwrap_or("game").to_string();
            server.bridge.send_command(CommandKind::GetInstances { path }).await
        }
        "insert_part" => {
            let name = args["name"].as_str().unwrap_or("Part").to_string();
            let parent = args["parent"].as_str().unwrap_or("game.Workspace").to_string();
            server.bridge.send_command(CommandKind::InsertPart { name, parent }).await
        }
        "insert_script" => {
            let script_name = args["name"].as_str().unwrap_or("Script").to_string();
            let script_type = args["script_type"].as_str().unwrap_or("Script").to_string();
            let parent = args["parent"].as_str().unwrap_or("game.ServerScriptService").to_string();
            let source = args["source"].as_str().unwrap_or("").to_string();
            let code = format!(
                r#"
local s = Instance.new("{script_type}")
s.Name = "{script_name}"
s.Source = [=[{source}]=]
s.Parent = {parent}
print("✅ Created " .. s.ClassName .. " '" .. s.Name .. "' in " .. s.Parent:GetFullName())
"#
            );
            server.bridge.send_command(CommandKind::RunCode { code }).await
        }
        "get_scripts" => {
            server.bridge.send_command(CommandKind::GetScripts {}).await
        }
        "status" => {
            let code = r#"print("✅ Connected! Roblox Studio " .. tostring(version()))"#.to_string();
            server.bridge.send_command(CommandKind::RunCode { code }).await
                .map(|o| format!("🟢 Studio connected\n{o}"))
        }
        _ => Err(anyhow::anyhow!("Unknown tool: {name}")),
    };

    match result {
        Ok(output) => output,
        Err(e) => format!("❌ {e}"),
    }
}

/// Khởi động MCP HTTP server
pub async fn run_mcp_http_server(bridge: BridgeState, port: u16) -> Result<()> {
    let state = AppState { bridge };

    // CORS cho phép claude.ai gọi vào (thực ra Anthropic server gọi, không phải browser)
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::HEAD, Method::OPTIONS])
        .allow_headers(Any)
        .allow_origin(Any);

    let app = Router::new()
        .route("/", get(handle_get).post(handle_post).head(handle_head))
        .route("/health", get(|| async { "OK" }))
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    info!("MCP HTTP server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}