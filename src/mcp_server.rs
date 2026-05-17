/// mcp_server.rs

use crate::bridge::{BridgeState, CommandKind};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo},
    schemars,
    tool, tool_handler, tool_router,
    ServerHandler,
};

// ── Params — #[allow(dead_code)] vì macro tool_router dùng qua proc-macro ──

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RunCodeParams {
    #[schemars(description = "Luau code để chạy trong Studio")]
    pub code: String,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetInstancesParams {
    #[schemars(description = "Path trong DataModel. Ví dụ: 'game.Workspace'")]
    pub path: String,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InsertPartParams {
    #[schemars(description = "Tên Part")]
    pub name: String,
    #[schemars(description = "Parent path. Ví dụ: 'game.Workspace'")]
    pub parent: String,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InsertScriptParams {
    #[schemars(description = "Tên script")]
    pub name: String,
    #[schemars(description = "Loại: 'Script', 'LocalScript', 'ModuleScript'")]
    pub script_type: String,
    #[schemars(description = "Parent path")]
    pub parent: String,
    #[schemars(description = "Source code")]
    pub source: String,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BatchRunParams {
    #[schemars(description = "Danh sách Luau code chạy tuần tự")]
    pub codes: Vec<String>,
}

// ── Server ────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Clone)]
pub struct RobloxMcpServer {
    pub bridge: BridgeState,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl RobloxMcpServer {
    pub fn new(bridge: BridgeState) -> Self {
        Self { bridge, tool_router: Self::tool_router() }
    }

    #[tool(description = "\
        Lấy toàn bộ context game 1 lần: version, instances tất cả services, \
        scripts kèm source. LUÔN gọi đầu tiên trước mọi thao tác khác.")]
    async fn snapshot(&self) -> String {
        self.bridge
            .send_command(CommandKind::Snapshot {})
            .await
            .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
    }

    #[tool(description = "Kiểm tra kết nối nhanh với Studio plugin.")]
    async fn status(&self) -> String {
        match self.bridge.send_command(CommandKind::RunCode {
            code: r#"print("✅ "..tostring(version()))"#.to_string(),
        }).await {
            Ok(o) => format!("🟢 Connected\n{o}"),
            Err(e) => format!("🔴 Not connected: {e}"),
        }
    }

    #[tool(description = "Lấy tất cả scripts trong game kèm source code.")]
    async fn get_scripts(&self) -> String {
        self.bridge
            .send_command(CommandKind::GetScripts {})
            .await
            .unwrap_or_else(|e| format!("❌ {e}"))
    }

    #[tool(description = "\
        Chạy nhiều đoạn Luau tuần tự trong 1 lần gọi. \
        Tiết kiệm token hơn nhiều lần run_code(). \
        Trả JSON array kết quả từng đoạn.")]
    async fn batch_run(&self, Parameters(p): Parameters<BatchRunParams>) -> String {
        self.bridge
            .send_command(CommandKind::BatchRun { codes: p.codes })
            .await
            .unwrap_or_else(|e| format!("❌ {e}"))
    }

    #[tool(description = "\
        Chạy 1 đoạn Luau trong Studio. \
        Dùng batch_run() nếu cần nhiều thao tác.")]
    async fn run_code(&self, Parameters(p): Parameters<RunCodeParams>) -> String {
        self.bridge
            .send_command(CommandKind::RunCode { code: p.code })
            .await
            .unwrap_or_else(|e| format!("❌ {e}"))
    }

    #[tool(description = "Xem children của 1 path. Dùng snapshot() nếu cần tổng thể.")]
    async fn get_instances(&self, Parameters(p): Parameters<GetInstancesParams>) -> String {
        self.bridge
            .send_command(CommandKind::GetInstances { path: p.path })
            .await
            .unwrap_or_else(|e| format!("❌ {e}"))
    }

    #[tool(description = "Tạo Part mới. Hoặc dùng run_code() để tạo kèm properties luôn.")]
    async fn insert_part(&self, Parameters(p): Parameters<InsertPartParams>) -> String {
        self.bridge
            .send_command(CommandKind::InsertPart { name: p.name, parent: p.parent })
            .await
            .unwrap_or_else(|e| format!("❌ {e}"))
    }

    #[tool(description = "Tạo Script/LocalScript/ModuleScript với source code.")]
    async fn insert_script(&self, Parameters(p): Parameters<InsertScriptParams>) -> String {
        let code = format!(
            "local s=Instance.new(\"{t}\")\ns.Name=\"{n}\"\ns.Source=[=[{src}]=]\ns.Parent={par}\nprint(\"✅ \"..s.Name)",
            t = p.script_type, n = p.name, src = p.source, par = p.parent,
        );
        self.bridge
            .send_command(CommandKind::RunCode { code })
            .await
            .unwrap_or_else(|e| format!("❌ {e}"))
    }
}

#[tool_handler]
impl ServerHandler for RobloxMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "QUAN TRỌNG: \
                1. Gọi snapshot() ĐẦU TIÊN để lấy toàn bộ context game. \
                2. Dùng batch_run([...]) thay vì nhiều run_code() riêng lẻ. \
                3. Gộp nhiều thay đổi vào 1 đoạn code khi có thể."
                    .into(),
            ),
        }
    }
}