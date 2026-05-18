--!strict
--[[
    RobloxMCP Plugin v0.5 — Luau
    Kết nối Roblox Studio với Claude qua MCP server

    Cài đặt:
    1. Copy vào: %LOCALAPPDATA%\Roblox\Plugins\RobloxMCP.luau
    2. Bật: Game Settings → Security → Allow HTTP Requests ✓
    3. Chạy roblox-mcp.exe, copy URL hiện ra
    4. Paste URL vào ô trong widget, click Connect
--]]

local HttpService  = game:GetService("HttpService")
local plugin_env   = plugin

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

-- ── State ─────────────────────────────────────────────────────────

local State = {
    running    = false,
    connected  = false,
    cmdCount   = 0,
    lastCmd    = "—",
    errorMsg   = "",
    startClock = 0,
    bridgeUrl  = "http://127.0.0.1:7878",  -- default
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

local function url(path: string): string
    return State.bridgeUrl .. path
end

-- Chạy Luau code và capture output
local function runCode(code: string): (boolean, string)
    local out: { string } = {}
    local env = setmetatable({
        print = function(...: unknown): ()
            local p: { string } = {}
            for _, v in ipairs({...}) do p[#p+1] = tostring(v) end
            out[#out+1] = table.concat(p, "\t")
        end,
        warn = function(...: unknown): ()
            local p: { string } = {}
            for _, v in ipairs({...}) do p[#p+1] = tostring(v) end
            out[#out+1] = `⚠️ {table.concat(p, "\t")}`
        end,
    }, { __index = _G })

    local fn: ((...unknown) -> ...unknown)?, loadErr: string? = loadstring(code)
    if fn == nil then return false, `Syntax: {tostring(loadErr)}` end
    setfenv(fn :: any, env)
    local ok, runErr = pcall(fn :: any)
    if not ok then return false, `Runtime: {tostring(runErr)}` end
    return true, if #out > 0 then table.concat(out, "\n") else "(no output)"
end

-- ── Handlers ──────────────────────────────────────────────────────

local function handleSnapshot(): (boolean, string)
    local data = {
        version  = tostring(version()),
        services = {} :: { [string]: { { name: string, class: string, path: string } } },
        scripts  = {} :: { { path: string, class: string, totalLines: number, source: string } },
    }
    for _, name in { "Workspace","ServerScriptService","ReplicatedStorage","StarterGui","StarterPack","ServerStorage" } do
        local ok, svc = pcall(function() return game:GetService(name) end)
        if ok and svc then
            local children = {}
            for _, c in svc:GetChildren() do
                children[#children+1] = { name = c.Name, class = c.ClassName, path = c:GetFullName() }
            end
            data.services[name] = children
        end
    end
    local count = 0
    local function scan(parent: Instance, depth: number): ()
        if depth > 8 or count >= 200 then return end
        for _, c in parent:GetChildren() do
            if c:IsA("LuaSourceContainer") then
                count += 1
                local src = (c :: Script).Source
                local lines = string.split(src, "\n")
                local preview = table.concat(lines, "\n", 1, math.min(100, #lines))
                if #lines > 100 then preview ..= `\n-- ... ({#lines - 100} more lines)` end
                data.scripts[#data.scripts+1] = { path = c:GetFullName(), class = c.ClassName, totalLines = #lines, source = preview }
            end
            scan(c, depth + 1)
        end
    end
    scan(game, 0)
    return true, HttpService:JSONEncode(data)
end

local function handleBatchRun(codes: { string }): (boolean, string)
    local results: { BatchResult } = {}
    for i, code in codes do
        local ok, output = runCode(code)
        results[#results+1] = { index = i, success = ok, output = output }
        if not ok and string.find(output, "Syntax") then break end
    end
    return true, HttpService:JSONEncode(results)
end

local function handleGetInstances(path: string): (boolean, string)
    local obj: Instance = game
    if path ~= "game" then
        local parts = string.split(path, ".")
        for i = 2, #parts do
            local child = obj:FindFirstChild(parts[i])
            if child == nil then return false, `Not found: {parts[i]}` end
            obj = child
        end
    end
    local result = { path = path, class = obj.ClassName, children = {} :: { { name: string, class: string, path: string } } }
    for _, c in obj:GetChildren() do
        result.children[#result.children+1] = { name = c.Name, class = c.ClassName, path = c:GetFullName() }
    end
    return true, HttpService:JSONEncode(result)
end

local function handleGetScripts(): (boolean, string)
    local found: { unknown } = {}
    local count = 0
    local function scan(parent: Instance, depth: number): ()
        if depth > 8 or count >= 200 then return end
        for _, c in parent:GetChildren() do
            if c:IsA("LuaSourceContainer") then
                count += 1
                local src = (c :: Script).Source
                found[#found+1] = { path = c:GetFullName(), class = c.ClassName, lines = #string.split(src, "\n"), source = src }
            end
            scan(c, depth + 1)
        end
    end
    scan(game, 0)
    return true, HttpService:JSONEncode(found)
end

local function dispatch(cmd: Command): (boolean, string)
    local kind = cmd.kind
    if kind.type == "snapshot"      then return handleSnapshot()
    elseif kind.type == "batch_run" then return handleBatchRun((kind :: any).codes)
    elseif kind.type == "run_code"  then return runCode((kind :: any).code)
    elseif kind.type == "get_instances" then return handleGetInstances((kind :: any).path)
    elseif kind.type == "get_scripts"   then return handleGetScripts()
    elseif kind.type == "insert_part"   then
        local k = kind :: any
        return runCode(`local p=Instance.new("Part")\np.Name="{k.name}"\np.Parent={k.parent}\nprint("✅ "..p.Name)`)
    end
    return false, `Unknown: {kind.type}`
end

-- ── Poll loop ─────────────────────────────────────────────────────

local function pollLoop(): ()
    log(`Started → {url("/poll")}`)
    while State.running do
        local healthOk = pcall(HttpService.GetAsync, HttpService, url("/health"), true)
        if not healthOk then
            State.connected = false
            State.errorMsg  = "Server không chạy — kiểm tra URL"
            task.wait(2)
            continue
        end
        State.connected = true
        State.errorMsg  = ""

        local pollOk, resp = pcall(HttpService.GetAsync, HttpService, url("/poll"), true)
        if not pollOk then
            State.connected = false
            State.errorMsg  = tostring(resp)
            task.wait(2)
            continue
        end

        local data = dec(tostring(resp))
        if data and data.id then
            State.cmdCount += 1
            State.lastCmd = tostring((data.kind :: any) and (data.kind :: any).type or "?")
            log(`#{State.cmdCount} {State.lastCmd}`)
            local success, output = dispatch(data :: any)
            pcall(HttpService.PostAsync, HttpService,
                url("/result"),
                HttpService:JSONEncode({ id = data.id, output = output, success = success }),
                Enum.HttpContentType.ApplicationJson, false
            )
        end
        task.wait(0.05)
    end
    State.connected = false
    log("Stopped")
end

-- ── UI ────────────────────────────────────────────────────────────

local toolbar = plugin_env:CreateToolbar("Claude MCP")
local btn     = toolbar:CreateButton("MCP", "Bật/tắt Claude MCP", "")

local widgetInfo = DockWidgetPluginGuiInfo.new(
    Enum.InitialDockState.Bottom, true, false, 400, 220, 320, 180
)
local widget = plugin_env:CreateDockWidgetPluginGui("RobloxMCP_v5", widgetInfo)
widget.Title = "Claude MCP"

-- Root
local root = Instance.new("Frame")
root.Size = UDim2.new(1,0,1,0)
root.BackgroundColor3 = Color3.fromRGB(18,18,22)
root.BorderSizePixel = 0
root.Parent = widget

local pad = Instance.new("UIPadding", root)
pad.PaddingLeft   = UDim.new(0,14)
pad.PaddingRight  = UDim.new(0,14)
pad.PaddingTop    = UDim.new(0,12)
pad.PaddingBottom = UDim.new(0,10)

local list = Instance.new("UIListLayout", root)
list.SortOrder = Enum.SortOrder.LayoutOrder
list.Padding   = UDim.new(0,6)

local function lbl(text: string, sz: number, col: Color3, order: number): TextLabel
    local l = Instance.new("TextLabel")
    l.Size = UDim2.new(1,0,0,sz)
    l.BackgroundTransparency = 1
    l.TextColor3 = col
    l.Font = Enum.Font.Code
    l.TextSize = sz
    l.TextXAlignment = Enum.TextXAlignment.Left
    l.Text = text
    l.TextWrapped = true
    l.RichText = true
    l.LayoutOrder = order
    l.Parent = root
    return l
end

-- Status
local lblStatus = lbl("● STOPPED", 16, Color3.fromRGB(140,140,155), 1)
local lblCmds   = lbl("Commands: 0", 12, Color3.fromRGB(90,90,110), 2)
local lblLast   = lbl("Last: —", 12, Color3.fromRGB(90,90,110), 3)
local lblError  = lbl("", 11, Color3.fromRGB(255,80,80), 4)

-- Divider
local div1 = Instance.new("Frame", root)
div1.Size = UDim2.new(1,0,0,1)
div1.BackgroundColor3 = Color3.fromRGB(45,45,60)
div1.BorderSizePixel = 0
div1.LayoutOrder = 5

-- URL input label
lbl("Bridge URL (từ roblox-mcp.exe):", 11, Color3.fromRGB(100,100,130), 6)

-- URL input row
local inputRow = Instance.new("Frame", root)
inputRow.Size = UDim2.new(1,0,0,28)
inputRow.BackgroundTransparency = 1
inputRow.LayoutOrder = 7

local inputRowLayout = Instance.new("UIListLayout", inputRow)
inputRowLayout.FillDirection = Enum.FillDirection.Horizontal
inputRowLayout.SortOrder = Enum.SortOrder.LayoutOrder
inputRowLayout.Padding = UDim.new(0,6)

-- TextBox nhập URL
local urlBox = Instance.new("TextBox")
urlBox.Size = UDim2.new(1,-80,1,0)
urlBox.BackgroundColor3 = Color3.fromRGB(30,30,40)
urlBox.BorderColor3 = Color3.fromRGB(70,70,100)
urlBox.BorderSizePixel = 1
urlBox.TextColor3 = Color3.fromRGB(200,200,220)
urlBox.Font = Enum.Font.Code
urlBox.TextSize = 12
urlBox.PlaceholderText = "http://127.0.0.1:7878"
urlBox.PlaceholderColor3 = Color3.fromRGB(70,70,90)
urlBox.Text = State.bridgeUrl
urlBox.ClearTextOnFocus = false
urlBox.TextXAlignment = Enum.TextXAlignment.Left
urlBox.LayoutOrder = 1
urlBox.Parent = inputRow

local urlBoxPad = Instance.new("UIPadding", urlBox)
urlBoxPad.PaddingLeft  = UDim.new(0,6)
urlBoxPad.PaddingRight = UDim.new(0,6)

local urlBoxCorner = Instance.new("UICorner", urlBox)
urlBoxCorner.CornerRadius = UDim.new(0,4)

-- Connect button
local connectBtn = Instance.new("TextButton")
connectBtn.Size = UDim2.new(0,70,1,0)
connectBtn.BackgroundColor3 = Color3.fromRGB(50,120,220)
connectBtn.BorderSizePixel = 0
connectBtn.TextColor3 = Color3.fromRGB(255,255,255)
connectBtn.Font = Enum.Font.GothamBold
connectBtn.TextSize = 12
connectBtn.Text = "Connect"
connectBtn.LayoutOrder = 2
connectBtn.Parent = inputRow

local connectBtnCorner = Instance.new("UICorner", connectBtn)
connectBtnCorner.CornerRadius = UDim.new(0,4)

-- Tip
lbl("Tip: snapshot() trả toàn bộ context 1 lần", 10, Color3.fromRGB(55,55,75), 8)

-- ── Logic button ──────────────────────────────────────────────────

local function applyUrl(newUrl: string): ()
    -- Trim whitespace
    newUrl = string.match(newUrl, "^%s*(.-)%s*$") or newUrl
    if newUrl == "" then
        newUrl = "http://127.0.0.1:7878"
    end
    -- Bỏ trailing slash
    newUrl = string.gsub(newUrl, "/$", "")
    State.bridgeUrl = newUrl
    urlBox.Text = newUrl
    log(`Bridge URL set to: {newUrl}`)
end

-- Click Connect
connectBtn.MouseButton1Click:Connect(function(): ()
    applyUrl(urlBox.Text)

    if State.running then
        -- Đang chạy → restart với URL mới
        State.running = false
        task.wait(0.3)
    end

    State.running    = true
    State.cmdCount   = 0
    State.startClock = os.clock()
    btn:SetActive(true)
    task.spawn(pollLoop)
    log("Connected with URL: " .. State.bridgeUrl)
end)

-- Toolbar button toggle
btn.Click:Connect(function(): ()
    if State.running then
        State.running = false
        State.connected = false
        btn:SetActive(false)
        log("Stopped")
    else
        applyUrl(urlBox.Text)
        State.running    = true
        State.cmdCount   = 0
        State.startClock = os.clock()
        btn:SetActive(true)
        task.spawn(pollLoop)
        log("Started")
    end
end)

-- Enter trong urlBox → apply + connect
urlBox.FocusLost:Connect(function(enterPressed: boolean): ()
    if enterPressed then
        applyUrl(urlBox.Text)
        if not State.running then
            State.running    = true
            State.cmdCount   = 0
            State.startClock = os.clock()
            btn:SetActive(true)
            task.spawn(pollLoop)
        end
    end
end)

-- ── UI update loop ────────────────────────────────────────────────

task.spawn(function(): ()
    while true do
        if State.running then
            if State.connected then
                lblStatus.Text = "<font color='#50dd80'>●</font> CONNECTED"
                connectBtn.BackgroundColor3 = Color3.fromRGB(40,160,80)
                connectBtn.Text = "Disconnect"
            else
                lblStatus.Text = "<font color='#f0b832'>●</font> CONNECTING..."
                connectBtn.BackgroundColor3 = Color3.fromRGB(50,120,220)
                connectBtn.Text = "Connect"
            end
            local up = `{math.floor(os.clock() - State.startClock)}s`
            lblCmds.Text = `Commands: {State.cmdCount}  |  Up: {up}`
            lblLast.Text = `Last: {State.lastCmd}`
        else
            lblStatus.Text = "● STOPPED"
            connectBtn.BackgroundColor3 = Color3.fromRGB(50,120,220)
            connectBtn.Text = "Connect"
            lblCmds.Text = "Commands: 0"
            lblLast.Text = "Last: —"
        end
        lblError.Text = State.errorMsg
        task.wait(0.5)
    end
end)

log("v0.5 loaded — nhập URL vào ô rồi click Connect")
