/// mcp_server.rs
/// Định nghĩa các tools Claude có thể gọi để điều khiển Roblox Studio

use crate::bridge::{BridgeState, CommandKind};
use anyhow::Result;
use rmcp::{
    ServerHandler,
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_box, tool_handler,
};
use schemars::JsonSchema;
use serde::Deserialize;

// ── Tham số cho từng tool ──────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunCodeParams {
    #[schemars(description = "Luau code để chạy trong Roblox Studio. Ví dụ: print('Hello')")]
    pub code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetInstancesParams {
    #[schemars(
        description = "Path trong DataModel. Ví dụ: 'game.Workspace', 'game.ServerStorage', 'game'"
    )]
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InsertPartParams {
    #[schemars(description = "Tên của Part mới")]
    pub name: String,
    #[schemars(description = "Parent path. Ví dụ: 'game.Workspace'")]
    pub parent: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InsertScriptParams {
    #[schemars(description = "Tên script")]
    pub name: String,
    #[schemars(description = "Loại script: 'Script', 'LocalScript', 'ModuleScript'")]
    pub script_type: String,
    #[schemars(description = "Parent path. Ví dụ: 'game.ServerScriptService'")]
    pub parent: String,
    #[schemars(description = "Nội dung source code của script")]
    pub source: String,
}

// ── MCP Server implementation ──────────────────────────────────────

#[derive(Clone)]
pub struct RobloxMcpServer {
    bridge: BridgeState,
}

#[tool_box]
impl RobloxMcpServer {
    pub fn new(bridge: BridgeState) -> Self {
        Self { bridge }
    }

    // ── Tool: chạy Luau code tùy ý ──────────────────────────────

    #[tool(description = "Chạy Luau code tùy ý trong Roblox Studio và trả về output. \
        Dùng để test logic, tạo objects, sửa properties, v.v.")]
    async fn run_code(
        &self,
        #[tool(aggr)] params: RunCodeParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        match self.bridge.send_command(CommandKind::RunCode { code: params.code }).await {
            Ok(output) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Err(e) => Ok(CallToolResult::success(vec![
                Content::text(format!("❌ Lỗi: {e}"))
            ])),
        }
    }

    // ── Tool: xem cây instances ──────────────────────────────────

    #[tool(description = "Xem danh sách con (children) của một object trong DataModel. \
        Trả về tên và ClassName của từng child. \
        Ví dụ path: 'game', 'game.Workspace', 'game.ServerStorage'")]
    async fn get_instances(
        &self,
        #[tool(aggr)] params: GetInstancesParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        match self.bridge.send_command(CommandKind::GetInstances { path: params.path }).await {
            Ok(output) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Err(e) => Ok(CallToolResult::success(vec![
                Content::text(format!("❌ Lỗi: {e}"))
            ])),
        }
    }

    // ── Tool: tạo Part ──────────────────────────────────────────

    #[tool(description = "Tạo một Part mới trong Roblox Studio Workspace. \
        Part sẽ xuất hiện ngay trong viewport của Studio.")]
    async fn insert_part(
        &self,
        #[tool(aggr)] params: InsertPartParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        match self.bridge.send_command(CommandKind::InsertPart {
            name: params.name,
            parent: params.parent,
        }).await {
            Ok(output) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Err(e) => Ok(CallToolResult::success(vec![
                Content::text(format!("❌ Lỗi: {e}"))
            ])),
        }
    }

    // ── Tool: tạo Script ────────────────────────────────────────

    #[tool(description = "Tạo Script/LocalScript/ModuleScript trong Roblox Studio \
        với source code được cung cấp. \
        script_type: 'Script' (server), 'LocalScript' (client), 'ModuleScript'")]
    async fn insert_script(
        &self,
        #[tool(aggr)] params: InsertScriptParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        // Tạo script bằng Luau code
        let code = format!(
            r#"
local parent = {}
local script = Instance.new("{}")
script.Name = "{}"
script.Source = [[{}]]
script.Parent = parent
print("Created script: " .. script.Name .. " in " .. parent:GetFullName())
"#,
            params.parent,
            params.script_type,
            params.name,
            params.source.replace("]]", "]] .. \"]]\" .. [["), // escape
        );

        match self.bridge.send_command(CommandKind::RunCode { code }).await {
            Ok(output) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Err(e) => Ok(CallToolResult::success(vec![
                Content::text(format!("❌ Lỗi: {e}"))
            ])),
        }
    }

    // ── Tool: lấy danh sách scripts ─────────────────────────────

    #[tool(description = "Lấy danh sách tất cả Scripts trong game, kèm theo source code. \
        Hữu ích để Claude đọc hiểu codebase hiện tại.")]
    async fn get_scripts(&self) -> Result<CallToolResult, rmcp::Error> {
        match self.bridge.send_command(CommandKind::GetScripts {}).await {
            Ok(output) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Err(e) => Ok(CallToolResult::success(vec![
                Content::text(format!("❌ Lỗi: {e}"))
            ])),
        }
    }

    // ── Tool: status ────────────────────────────────────────────

    #[tool(description = "Kiểm tra trạng thái kết nối với Roblox Studio plugin")]
    async fn status(&self) -> Result<CallToolResult, rmcp::Error> {
        let code = r#"print("✅ Roblox Studio connected! Version: " .. tostring(version()))"#.to_string();
        match self.bridge.send_command(CommandKind::RunCode { code }).await {
            Ok(output) => Ok(CallToolResult::success(vec![Content::text(
                format!("🟢 Studio connected\n{output}")
            )])),
            Err(_) => Ok(CallToolResult::success(vec![Content::text(
                "🔴 Studio plugin chưa kết nối. Mở Roblox Studio và bật plugin MCP.".to_string()
            )])),
        }
    }
}

#[tool_handler]
impl ServerHandler for RobloxMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Roblox Studio MCP Server. Dùng các tools để:\n\
                - run_code: Chạy Luau code trong Studio\n\
                - get_instances: Xem cấu trúc DataModel\n\
                - insert_part: Tạo Part mới\n\
                - insert_script: Tạo Script với code\n\
                - get_scripts: Đọc scripts hiện có\n\
                - status: Kiểm tra kết nối\n\
                Luôn gọi status() trước khi dùng các tool khác.".into()
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            ..Default::default()
        }
    }
}