--[[
    RobloxMCP Plugin
    Kết nối Roblox Studio với MCP server để Claude có thể điều khiển Studio
    
    Cách cài:
    1. Lưu file này vào: %LOCALAPPDATA%\Roblox\Plugins\RobloxMCP.lua
    2. Hoặc trong Studio: Plugins → Plugins Folder → paste file vào
    3. Bật "Allow HTTP Requests" trong: Game Settings → Security
--]]

local HttpService = game:GetService("HttpService")
local RunService = game:GetService("RunService")
local ScriptEditorService = game:GetService("ScriptEditorService")

-- Cấu hình
local CONFIG = {
    BRIDGE_URL = "http://127.0.0.1:7878",  -- Port bridge server của MCP
    POLL_INTERVAL = 0.5,                    -- Giây giữa mỗi poll
    TIMEOUT = 25,                           -- Giây timeout cho long-poll
    AUTO_RECONNECT = true,
}

-- State
local isRunning = false
local connectionStatus = "Disconnected"

-- ── Utility ───────────────────────────────────────────────────────

local function log(msg)
    print("[RobloxMCP] " .. tostring(msg))
end

local function logError(msg)
    warn("[RobloxMCP] ❌ " .. tostring(msg))
end

-- Safe JSON encode
local function jsonEncode(data)
    local ok, result = pcall(HttpService.JSONEncode, HttpService, data)
    if ok then return result end
    return '{"error":"encode failed"}'
end

-- Safe JSON decode
local function jsonDecode(str)
    local ok, result = pcall(HttpService.JSONDecode, HttpService, str)
    if ok then return result end
    return nil
end

-- ── Command handlers ──────────────────────────────────────────────

--- Chạy Luau code và capture output
local function handleRunCode(params)
    local code = params.code or ""
    
    -- Capture print output bằng cách override print tạm thời
    local outputs = {}
    local originalPrint = print
    local originalWarn = warn
    
    -- Override để capture
    local env = setmetatable({
        print = function(...)
            local args = {...}
            local parts = {}
            for _, v in ipairs(args) do
                table.insert(parts, tostring(v))
            end
            local line = table.concat(parts, "\t")
            table.insert(outputs, line)
            originalPrint("[MCP]", line)
        end,
        warn = function(...)
            local args = {...}
            local parts = {}
            for _, v in ipairs(args) do
                table.insert(parts, tostring(v))
            end
            local line = "⚠️ " .. table.concat(parts, "\t")
            table.insert(outputs, line)
            originalWarn("[MCP]", line)
        end,
        game = game,
        workspace = workspace,
        script = script,
        Instance = Instance,
        Vector3 = Vector3,
        CFrame = CFrame,
        Color3 = Color3,
        BrickColor = BrickColor,
        Enum = Enum,
        task = task,
        wait = task.wait,
    }, {__index = _G})
    
    -- Load và chạy code
    local fn, loadErr = loadstring(code)
    if not fn then
        return false, "Syntax error: " .. tostring(loadErr)
    end
    
    setfenv(fn, env)
    local ok, runErr = pcall(fn)
    
    if not ok then
        return false, "Runtime error: " .. tostring(runErr)
    end
    
    local output = table.concat(outputs, "\n")
    if output == "" then
        output = "(no output)"
    end
    return true, output
end

--- Lấy danh sách children của một path
local function handleGetInstances(params)
    local path = params.path or "game"
    
    -- Resolve path
    local obj
    local ok, err = pcall(function()
        -- Thay thế "game." prefix
        if path == "game" then
            obj = game
        else
            -- Parse path như "game.Workspace.Model"
            local parts = string.split(path, ".")
            obj = game
            for i = 2, #parts do  -- bỏ "game" ở đầu
                obj = obj:FindFirstChild(parts[i])
                if not obj then
                    error("Not found: " .. parts[i])
                end
            end
        end
    end)
    
    if not ok then
        return false, "Path error: " .. tostring(err)
    end
    
    if not obj then
        return false, "Object not found: " .. path
    end
    
    -- Build danh sách children
    local results = {
        path = path,
        className = obj.ClassName,
        children = {}
    }
    
    for _, child in ipairs(obj:GetChildren()) do
        table.insert(results.children, {
            name = child.Name,
            className = child.ClassName,
            fullPath = child:GetFullName(),
        })
    end
    
    return true, jsonEncode(results)
end

--- Lấy tất cả scripts
local function handleGetScripts(params)
    local results = {}
    
    local function scanForScripts(parent, depth)
        if depth > 10 then return end  -- tránh đệ quy quá sâu
        
        for _, child in ipairs(parent:GetChildren()) do
            if child:IsA("LuaSourceContainer") then
                table.insert(results, {
                    name = child.Name,
                    className = child.ClassName,
                    path = child:GetFullName(),
                    source = child.Source,
                    sourceLength = #child.Source,
                })
            end
            scanForScripts(child, depth + 1)
        end
    end
    
    scanForScripts(game, 0)
    
    local output = string.format("Found %d scripts:\n", #results)
    for _, s in ipairs(results) do
        output = output .. string.format(
            "\n[%s] %s (%s)\n  Source: %d chars\n",
            s.className, s.path, s.name, s.sourceLength
        )
        -- Hiển thị 5 dòng đầu của source
        local lines = string.split(s.source, "\n")
        for i = 1, math.min(5, #lines) do
            output = output .. "  | " .. lines[i] .. "\n"
        end
        if #lines > 5 then
            output = output .. string.format("  ... (%d more lines)\n", #lines - 5)
        end
    end
    
    return true, output
end

-- ── Command dispatcher ────────────────────────────────────────────

local handlers = {
    run_code     = handleRunCode,
    get_instances = handleGetInstances,
    get_scripts  = handleGetScripts,
}

local function handleCommand(command)
    local cmdType = command.kind and command.kind.type
    if not cmdType then
        return false, "No command type"
    end
    
    local handler = handlers[cmdType]
    if not handler then
        return false, "Unknown command: " .. cmdType
    end
    
    return handler(command.kind)
end

-- ── Main polling loop ─────────────────────────────────────────────

local function pollLoop()
    log("Starting poll loop → " .. CONFIG.BRIDGE_URL)
    connectionStatus = "Connecting..."
    
    while isRunning do
        -- Long-poll: chờ lệnh từ MCP server
        local ok, response = pcall(function()
            return HttpService:GetAsync(
                CONFIG.BRIDGE_URL .. "/poll",
                true  -- nocache
            )
        end)
        
        if not ok then
            connectionStatus = "Disconnected"
            -- Server chưa chạy hoặc lỗi mạng
            task.wait(CONFIG.POLL_INTERVAL * 2)
        else
            connectionStatus = "Connected ✅"
            
            local data = jsonDecode(response)
            
            -- Nếu có lệnh (không phải null)
            if data and data.id then
                log("Received command: " .. (data.kind and data.kind.type or "unknown"))
                
                -- Xử lý lệnh
                local success, output = handleCommand(data)
                
                -- Gửi kết quả về
                local resultOk, resultErr = pcall(function()
                    HttpService:PostAsync(
                        CONFIG.BRIDGE_URL .. "/result",
                        jsonEncode({
                            id = data.id,
                            output = tostring(output),
                            success = success,
                        }),
                        Enum.HttpContentType.ApplicationJson,
                        false
                    )
                end)
                
                if not resultOk then
                    logError("Failed to send result: " .. tostring(resultErr))
                else
                    log("Sent result: success=" .. tostring(success))
                end
            end
            -- Nếu data == null thì poll lại ngay (server timeout, không có lệnh)
        end
    end
    
    log("Poll loop stopped")
    connectionStatus = "Stopped"
end

-- ── Plugin UI (toolbar button) ────────────────────────────────────

local toolbar = plugin:CreateToolbar("Claude MCP")
local toggleButton = toolbar:CreateButton(
    "MCP",
    "Toggle Claude MCP connection",
    "rbxassetid://6022668888"  -- icon (network icon)
)

toggleButton.Click:Connect(function()
    if isRunning then
        -- Stop
        isRunning = false
        toggleButton:SetActive(false)
        log("MCP stopped")
    else
        -- Start
        isRunning = true
        toggleButton:SetActive(true)
        log("MCP started")
        
        -- Chạy poll loop trong coroutine riêng
        task.spawn(pollLoop)
    end
end)

-- Widget hiển thị status
local widgetInfo = DockWidgetPluginGuiInfo.new(
    Enum.InitialDockState.Bottom,
    false,  -- enabled
    false,  -- override
    300,    -- width
    80,     -- height
    200,    -- min width
    60      -- min height
)

local widget = plugin:CreateDockWidgetPluginGui("RobloxMCP", widgetInfo)
widget.Title = "Claude MCP"

local statusLabel = Instance.new("TextLabel")
statusLabel.Size = UDim2.new(1, 0, 1, 0)
statusLabel.BackgroundColor3 = Color3.fromRGB(30, 30, 30)
statusLabel.TextColor3 = Color3.fromRGB(200, 255, 200)
statusLabel.Font = Enum.Font.Code
statusLabel.TextSize = 14
statusLabel.Text = "🔴 MCP Stopped\nClick toolbar button to start"
statusLabel.TextWrapped = true
statusLabel.Parent = widget

-- Update status label mỗi giây
task.spawn(function()
    while true do
        if isRunning then
            statusLabel.Text = string.format(
                "🟢 MCP Running\nStatus: %s\nBridge: %s",
                connectionStatus,
                CONFIG.BRIDGE_URL
            )
            statusLabel.TextColor3 = Color3.fromRGB(100, 255, 100)
        else
            statusLabel.Text = "🔴 MCP Stopped\nClick toolbar button to start"
            statusLabel.TextColor3 = Color3.fromRGB(255, 100, 100)
        end
        task.wait(1)
    end
end)

log("Plugin loaded! Click the 'MCP' button in toolbar to start.")
log("Make sure roblox-mcp.exe is running first.")