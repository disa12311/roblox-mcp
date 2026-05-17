--!strict
--[[
    RobloxMCP Plugin v0.4 — Luau
    Kết nối Roblox Studio với Claude qua MCP server

    Cài đặt:
    1. Copy file này vào: %LOCALAPPDATA%\Roblox\Plugins\RobloxMCP.luau
    2. Bật: Game Settings → Security → Allow HTTP Requests ✓
    3. Chạy roblox-mcp.exe trước, rồi click "MCP" trong toolbar
--]]

local HttpService   = game:GetService("HttpService")

-- ── Types ─────────────────────────────────────────────────────────

type CommandKind =
    | { type: "run_code";      code: string }
    | { type: "batch_run";     codes: { string } }
    | { type: "get_instances"; path: string }
    | { type: "get_scripts" }
    | { type: "snapshot" }
    | { type: "insert_part";   name: string; parent: string }

type Command = {
    id:   string,
    kind: CommandKind,
}

type BatchResult = {
    index:   number,
    success: boolean,
    output:  string,
}

type InstanceInfo = {
    name:  string,
    class: string,
    path:  string,
}

type SnapshotData = {
    version:  string,
    services: { [string]: { InstanceInfo } },
    scripts:  { {
        path:        string,
        class:       string,
        totalLines:  number,
        source:      string,
    } },
}

-- ── Config ────────────────────────────────────────────────────────

local CFG = {
    HOST        = "http://127.0.0.1",
    BRIDGE_PORT = 7878,
    RETRY_DELAY = 2,
}

local function bridgeUrl(path: string): string
    return `{CFG.HOST}:{CFG.BRIDGE_PORT}{path}`
end

-- ── State ─────────────────────────────────────────────────────────

local State = {
    running    = false,
    connected  = false,
    cmdCount   = 0,
    lastCmd    = "—",
    errorMsg   = "",
    startClock = 0,
}

-- ── Helpers ───────────────────────────────────────────────────────

local function log(msg: string): ()
    print(`[MCP] {msg}`)
end

local function enc(t: { [string]: unknown }): string
    local ok, r = pcall(HttpService.JSONEncode, HttpService, t)
    return if ok then r :: string else "{}"
end

local function dec(s: string): { [string]: unknown }?
    local ok, r = pcall(HttpService.JSONDecode, HttpService, s)
    return if ok then r :: { [string]: unknown } else nil
end

-- Chạy Luau code và capture output
local function runCode(code: string): (boolean, string)
    local out: { string } = {}

    local env = setmetatable({
        print = function(...: unknown): ()
            local parts: { string } = {}
            for _, v in ipairs({ ... }) do
                table.insert(parts, tostring(v))
            end
            table.insert(out, table.concat(parts, "\t"))
        end,
        warn = function(...: unknown): ()
            local parts: { string } = {}
            for _, v in ipairs({ ... }) do
                table.insert(parts, tostring(v))
            end
            table.insert(out, `⚠️ {table.concat(parts, "\t")}`)
        end,
    }, { __index = _G })

    local fn: ((...unknown) -> ...unknown)?, loadErr: string? = loadstring(code)
    if fn == nil then
        return false, `Syntax error: {tostring(loadErr)}`
    end

    setfenv(fn :: any, env)
    local ok: boolean, runErr: unknown = pcall(fn :: any)
    if not ok then
        return false, `Runtime error: {tostring(runErr)}`
    end

    return true, if #out > 0 then table.concat(out, "\n") else "(no output)"
end

-- ── Command handlers ──────────────────────────────────────────────

local function handleSnapshot(): (boolean, string)
    local data: SnapshotData = {
        version  = tostring(version()),
        services = {},
        scripts  = {},
    }

    local serviceNames: { string } = {
        "Workspace", "ServerScriptService", "ReplicatedStorage",
        "StarterGui", "StarterPack", "ServerStorage", "Teams",
    }

    for _, name in serviceNames do
        local ok, svc = pcall(function(): Instance
            return game:GetService(name)
        end)
        if ok and svc then
            local children: { InstanceInfo } = {}
            for _, child in svc:GetChildren() do
                table.insert(children, {
                    name  = child.Name,
                    class = child.ClassName,
                    path  = child:GetFullName(),
                })
            end
            data.services[name] = children
        end
    end

    -- Scan scripts (giới hạn 200, preview 100 dòng)
    local scriptCount = 0
    local function scanScripts(parent: Instance, depth: number): ()
        if depth > 8 or scriptCount >= 200 then return end
        for _, child in parent:GetChildren() do
            if child:IsA("LuaSourceContainer") then
                scriptCount += 1
                local src = (child :: Script).Source
                local lines = string.split(src, "\n")
                local preview: { string } = {}
                for i = 1, math.min(100, #lines) do
                    preview[i] = lines[i]
                end
                local previewStr = table.concat(preview, "\n")
                if #lines > 100 then
                    previewStr ..= `\n-- ... ({#lines - 100} more lines)`
                end
                table.insert(data.scripts, {
                    path       = child:GetFullName(),
                    class      = child.ClassName,
                    totalLines = #lines,
                    source     = previewStr,
                })
            end
            scanScripts(child, depth + 1)
        end
    end
    scanScripts(game, 0)

    return true, HttpService:JSONEncode(data)
end

local function handleBatchRun(codes: { string }): (boolean, string)
    local results: { BatchResult } = {}
    for i, code in codes do
        local ok, output = runCode(code)
        table.insert(results, { index = i, success = ok, output = output })
        -- Dừng nếu syntax error
        if not ok and string.find(output, "Syntax") then break end
    end
    return true, HttpService:JSONEncode(results)
end

local function handleGetInstances(path: string): (boolean, string)
    local obj: Instance?
    if path == "game" then
        obj = game
    else
        local parts = string.split(path, ".")
        local cur: Instance = game
        for i = 2, #parts do
            local child = cur:FindFirstChild(parts[i])
            if child == nil then
                return false, `Not found: {parts[i]}`
            end
            cur = child
        end
        obj = cur
    end

    if obj == nil then
        return false, `Object not found: {path}`
    end

    local result = {
        path     = path,
        class    = obj.ClassName,
        children = {} :: { InstanceInfo },
    }
    for _, child in obj:GetChildren() do
        table.insert(result.children, {
            name  = child.Name,
            class = child.ClassName,
            path  = child:GetFullName(),
        })
    end
    return true, HttpService:JSONEncode(result)
end

local function handleGetScripts(): (boolean, string)
    local found: { unknown } = {}
    local count = 0
    local function scan(parent: Instance, depth: number): ()
        if depth > 8 or count >= 200 then return end
        for _, child in parent:GetChildren() do
            if child:IsA("LuaSourceContainer") then
                count += 1
                local src = (child :: Script).Source
                table.insert(found, {
                    path   = child:GetFullName(),
                    class  = child.ClassName,
                    lines  = #string.split(src, "\n"),
                    source = src,
                })
            end
            scan(child, depth + 1)
        end
    end
    scan(game, 0)
    return true, HttpService:JSONEncode(found)
end

-- ── Dispatcher ────────────────────────────────────────────────────

local function dispatch(cmd: Command): (boolean, string)
    local kind = cmd.kind

    if kind.type == "snapshot" then
        return handleSnapshot()
    elseif kind.type == "batch_run" then
        return handleBatchRun((kind :: any).codes)
    elseif kind.type == "run_code" then
        return runCode((kind :: any).code)
    elseif kind.type == "get_instances" then
        return handleGetInstances((kind :: any).path)
    elseif kind.type == "get_scripts" then
        return handleGetScripts()
    elseif kind.type == "insert_part" then
        local k = kind :: any
        local code = `local p=Instance.new("Part")\np.Name="{k.name}"\np.Parent={k.parent}\nprint("✅ "..p.Name)`
        return runCode(code)
    end

    return false, `Unknown command: {kind.type}`
end

-- ── Poll loop ─────────────────────────────────────────────────────

local function pollLoop(): ()
    log(`Started → {bridgeUrl("/poll")}`)

    while State.running do
        -- Health check
        local healthOk: boolean = pcall(
            HttpService.GetAsync, HttpService, bridgeUrl("/health"), true
        )
        if not healthOk then
            State.connected = false
            State.errorMsg  = "roblox-mcp.exe không chạy"
            task.wait(CFG.RETRY_DELAY)
            continue
        end

        State.connected = true
        State.errorMsg  = ""

        -- Long-poll chờ lệnh
        local pollOk: boolean, resp: unknown = pcall(
            HttpService.GetAsync, HttpService, bridgeUrl("/poll"), true
        )
        if not pollOk then
            State.connected = false
            State.errorMsg  = tostring(resp)
            task.wait(CFG.RETRY_DELAY)
            continue
        end

        local data = dec(tostring(resp))
        if data and data.id then
            State.cmdCount += 1
            local kind = (data.kind :: any)
            State.lastCmd = if kind then tostring(kind.type) else "?"
            log(`#{State.cmdCount} {State.lastCmd}`)

            local success: boolean, output: string = dispatch(data :: any)

            pcall(
                HttpService.PostAsync, HttpService,
                bridgeUrl("/result"),
                HttpService:JSONEncode({
                    id      = data.id,
                    output  = output,
                    success = success,
                }),
                Enum.HttpContentType.ApplicationJson,
                false
            )
        end

        task.wait(0.05)
    end

    State.connected = false
    log("Stopped")
end

-- ── UI ────────────────────────────────────────────────────────────

local toolbar = plugin:CreateToolbar("Claude MCP")
local btn     = toolbar:CreateButton("MCP", "Bật/tắt Claude MCP", "")

local widgetInfo = DockWidgetPluginGuiInfo.new(
    Enum.InitialDockState.Bottom, true, false, 360, 165, 280, 130
)
local widget = plugin:CreateDockWidgetPluginGui("RobloxMCP_v4", widgetInfo)
widget.Title = "Claude MCP"

-- Root frame
local root = Instance.new("Frame")
root.Size = UDim2.new(1, 0, 1, 0)
root.BackgroundColor3 = Color3.fromRGB(18, 18, 22)
root.BorderSizePixel = 0
root.Parent = widget

local pad = Instance.new("UIPadding", root)
pad.PaddingLeft   = UDim.new(0, 14)
pad.PaddingRight  = UDim.new(0, 14)
pad.PaddingTop    = UDim.new(0, 12)
pad.PaddingBottom = UDim.new(0, 10)

local layout = Instance.new("UIListLayout", root)
layout.SortOrder = Enum.SortOrder.LayoutOrder
layout.Padding   = UDim.new(0, 5)

local function makeLabel(text: string, size: number, col: Color3, order: number): TextLabel
    local l = Instance.new("TextLabel")
    l.Size               = UDim2.new(1, 0, 0, size)
    l.BackgroundTransparency = 1
    l.TextColor3         = col
    l.Font               = Enum.Font.Code
    l.TextSize           = size
    l.TextXAlignment     = Enum.TextXAlignment.Left
    l.Text               = text
    l.TextWrapped        = true
    l.RichText           = true
    l.LayoutOrder        = order
    l.Parent             = root
    return l
end

local lblStatus = makeLabel("● STOPPED",    17, Color3.fromRGB(140, 140, 155), 1)
local lblBridge = makeLabel("Bridge: —",    12, Color3.fromRGB(90,  90,  110), 2)
local lblCmds   = makeLabel("Commands: 0",  12, Color3.fromRGB(90,  90,  110), 3)
local lblLast   = makeLabel("Last: —",      12, Color3.fromRGB(90,  90,  110), 4)
local lblError  = makeLabel("",             11, Color3.fromRGB(255, 80,  80),  5)

local div = Instance.new("Frame", root)
div.Size             = UDim2.new(1, 0, 0, 1)
div.BackgroundColor3 = Color3.fromRGB(40, 40, 55)
div.BorderSizePixel  = 0
div.LayoutOrder      = 6

makeLabel(
    "Tip: snapshot() trả toàn bộ context 1 lần — ít token nhất",
    10, Color3.fromRGB(60, 60, 80), 7
)

-- UI update loop
task.spawn(function(): ()
    while true do
        if State.running then
            if State.connected then
                lblStatus.Text = "<font color='#50dd80'>●</font> CONNECTED"
            else
                lblStatus.Text = "<font color='#f0b832'>●</font> CONNECTING..."
            end
            local uptime = `{math.floor(os.clock() - State.startClock)}s`
            lblBridge.Text = `Bridge: {bridgeUrl("")}`
            lblCmds.Text   = `Commands: {State.cmdCount}  |  Up: {uptime}`
            lblLast.Text   = `Last: {State.lastCmd}`
        else
            lblStatus.Text = "● STOPPED"
            lblBridge.Text = "Bridge: —"
            lblCmds.Text   = "Commands: 0"
            lblLast.Text   = "Last: —"
        end
        lblError.Text = State.errorMsg
        task.wait(0.5)
    end
end)

-- Toggle
btn.Click:Connect(function(): ()
    if State.running then
        State.running = false
        btn:SetActive(false)
    else
        State.running    = true
        State.cmdCount   = 0
        State.startClock = os.clock()
        btn:SetActive(true)
        task.spawn(pollLoop)
    end
end)

log("v0.4 loaded — Luau strict mode")
