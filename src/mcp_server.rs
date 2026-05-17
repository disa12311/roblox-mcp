/// mcp_server.rs — MCP tools cho Roblox Studio
/// Tool snapshot() trả toàn bộ context 1 lần, batch_run() gộp nhiều lệnh

use crate::bridge::{BridgeState, CommandKind};
use rmcp::{
    ErrorData as McpError,
    handler::server::tool::ToolRouter,
    model::{CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo},
    schemars,
    tool, tool_handler, tool_router,
    ServerHandler,
};

// ── Params ────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RunCodeParams {
    #[schemars(description = "Luau code để chạy trong Studio")]
    pub code: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetInstancesParams {
    #[schemars(description = "Path trong DataModel. Ví dụ: 'game.Workspace'")]
    pub path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InsertPartParams {
    #[schemars(description = "Tên Part")]
    pub name: String,
    #[schemars(description = "Parent path. Ví dụ: 'game.Workspace'")]
    pub parent: String,
}

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

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BatchRunParams {
    #[schemars(description = "Danh sách Luau code chạy tuần tự. Mỗi item là 1 đoạn code độc lập.")]
    pub codes: Vec<String>,
}

// ── Server ────────────────────────────────────────────────────────

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

    // ── snapshot: 1 call trả hết context ────────────────────────

    #[tool(description = "\
        Lấy toàn bộ context của game trong 1 lần gọi: \
        danh sách instances (Workspace/SSS/RS/StarterGui), \
        tất cả scripts kèm source code, và version Studio. \
        LUÔN gọi tool này đầu tiên trước khi làm bất cứ điều gì khác. \
        Trả JSON với các key: version, services, scripts.")]
    async fn snapshot(&self) -> Result<CallToolResult, McpError> {
        let result = self
            .bridge
            .send_command(CommandKind::Snapshot {})
            .await
            .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // ── batch_run: gộp nhiều lệnh thành 1 round-trip ────────────

    #[tool(description = "\
        Chạy nhiều đoạn Luau code tuần tự trong 1 lần gọi duy nhất. \
        Dùng thay cho nhiều lần gọi run_code riêng lẻ để tiết kiệm token. \
        Trả về output của từng đoạn code theo thứ tự.")]
    async fn batch_run(
        &self,
        #[tool(aggr)] params: BatchRunParams,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .bridge
            .send_command(CommandKind::BatchRun { codes: params.codes })
            .await
            .unwrap_or_else(|e| format!("❌ {e}"));
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // ── run_code ─────────────────────────────────────────────────

    #[tool(description = "\
        Chạy 1 đoạn Luau code trong Studio. \
        Nếu cần chạy nhiều thao tác, dùng batch_run() thay thế để tiết kiệm token.")]
    async fn run_code(
        &self,
        #[tool(aggr)] params: RunCodeParams,
    ) -> Result<CallToolResult, McpError> {
        let r = self
            .bridge
            .send_command(CommandKind::RunCode { code: params.code })
            .await
            .unwrap_or_else(|e| format!("❌ {e}"));
        Ok(CallToolResult::success(vec![Content::text(r)]))
    }

    // ── get_instances ─────────────────────────────────────────────

    #[tool(description = "Xem children của 1 object. Dùng snapshot() nếu cần xem tổng thể.")]
    async fn get_instances(
        &self,
        #[tool(aggr)] params: GetInstancesParams,
    ) -> Result<CallToolResult, McpError> {
        let r = self
            .bridge
            .send_command(CommandKind::GetInstances { path: params.path })
            .await
            .unwrap_or_else(|e| format!("❌ {e}"));
        Ok(CallToolResult::success(vec![Content::text(r)]))
    }

    // ── insert_part ───────────────────────────────────────────────

    #[tool(description = "Tạo Part mới. Hoặc dùng run_code() để tạo kèm properties luôn.")]
    async fn insert_part(
        &self,
        #[tool(aggr)] params: InsertPartParams,
    ) -> Result<CallToolResult, McpError> {
        let r = self
            .bridge
            .send_command(CommandKind::InsertPart { name: params.name, parent: params.parent })
            .await
            .unwrap_or_else(|e| format!("❌ {e}"));
        Ok(CallToolResult::success(vec![Content::text(r)]))
    }

    // ── insert_script ─────────────────────────────────────────────

    #[tool(description = "Tạo Script/LocalScript/ModuleScript với source code.")]
    async fn insert_script(
        &self,
        #[tool(aggr)] params: InsertScriptParams,
    ) -> Result<CallToolResult, McpError> {
        let code = format!(
            "local s=Instance.new(\"{t}\")\ns.Name=\"{n}\"\ns.Source=[=[{src}]=]\ns.Parent={p}\nprint(\"✅ \"..s.Name)",
            t = params.script_type,
            n = params.name,
            src = params.source,
            p = params.parent,
        );
        let r = self
            .bridge
            .send_command(CommandKind::RunCode { code })
            .await
            .unwrap_or_else(|e| format!("❌ {e}"));
        Ok(CallToolResult::success(vec![Content::text(r)]))
    }

    // ── status ────────────────────────────────────────────────────

    #[tool(description = "Kiểm tra nhanh kết nối plugin. snapshot() đã bao gồm thông tin này.")]
    async fn status(&self) -> Result<CallToolResult, McpError> {
        let r = self
            .bridge
            .send_command(CommandKind::RunCode {
                code: r#"print("✅ "..tostring(version()))"#.to_string(),
            })
            .await;
        let msg = match r {
            Ok(o) => format!("🟢 Connected\n{o}"),
            Err(e) => format!("🔴 Not connected\n{e}"),
        };
        Ok(CallToolResult::success(vec![Content::text(msg)]))
    }
}

#[tool_handler]
impl ServerHandler for RobloxMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "roblox-studio-mcp".into(),
                version: "0.1.0".into(),
            },
            instructions: Some(
                "QUAN TRỌNG: Luôn gọi snapshot() ĐẦU TIÊN để lấy toàn bộ context game. \
                Gộp nhiều thao tác vào batch_run() hoặc 1 run_code() duy nhất thay vì gọi nhiều lần. \
                Tools: snapshot, batch_run, run_code, get_instances, insert_part, insert_script, status."
                    .into(),
            ),
        }
    }
}