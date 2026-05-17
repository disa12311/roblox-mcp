--[[
    RobloxMCP Plugin v0.3
    Kết nối Roblox Studio với Claude qua MCP server

    Cài đặt:
    1. Copy file này vào: %LOCALAPPDATA%\Roblox\Plugins\RobloxMCP.lua
    2. Bật: Game Settings → Security → Allow HTTP Requests ✓
    3. Chạy roblox-mcp.exe trước, rồi click "MCP" trong toolbar
--]]

local HttpService = game:GetService("HttpService")

local CFG = {
    HOST        = "http://127.0.0.1",
    BRIDGE_PORT = 7878,
    RETRY_DELAY = 2,
}

local function url(path)
    return CFG.HOST .. ":" .. CFG.BRIDGE_PORT .. path
end

-- ── State ─────────────────────────────────────────────────────────

local State = {
    running      = false,
    connected    = false,
    cmdCount     = 0,
    lastCmd      = "—",
    errorMsg     = "",
    startClock   = 0,
}

-- ── Helpers ───────────────────────────────────────────────────────

local function log(msg) print("[MCP] " .. tostring(msg)) end

local function enc(t)
    local ok, r = pcall(HttpService.JSONEncode, HttpService, t)
    return ok and r or "{}"
end

local function dec(s)
    local ok, r = pcall(HttpService.JSONDecode, HttpService, s)
    return ok and r or nil
end

-- Chạy code và capture print/warn output
local function runCode(code)
    local out = {}
    local env = setmetatable({
        print = function(...)
            local p = {}
            for _, v in ipairs({...}) do p[#p+1] = tostring(v) end
            out[#out+1] = table.concat(p, "\t")
        end,
        warn = function(...)
            local p = {}
            for _, v in ipairs({...}) do p[#p+1] = tostring(v) end
            out[#out+1] = "⚠️ " .. table.concat(p, "\t")
        end,
    }, { __index = _G })

    local fn, e = loadstring(code)
    if not fn then return false, "Syntax: " .. tostring(e) end
    setfenv(fn, env)
    local ok, e2 = pcall(fn)
    if not ok then return false, "Runtime: " .. tostring(e2) end
    return true, #out > 0 and table.concat(out, "\n") or "(no output)"
end

-- ── Handlers ──────────────────────────────────────────────────────

-- snapshot: trả toàn bộ context trong 1 lần
local function handleSnapshot()
    local data = {
        version  = tostring(version()),
        services = {},
        scripts  = {},
    }

    -- Scan top-level services
    local serviceNames = {
        "Workspace", "ServerScriptService", "ReplicatedStorage",
        "StarterGui", "StarterPack", "ServerStorage", "Teams",
    }
    for _, svcName in ipairs(serviceNames) do
        local ok, svc = pcall(function() return game:GetService(svcName) end)
        if ok and svc then
            local children = {}
            for _, c in ipairs(svc:GetChildren()) do
                children[#children+1] = {
                    name  = c.Name,
                    class = c.ClassName,
                    path  = c:GetFullName(),
                }
            end
            data.services[svcName] = children
        end
    end

    -- Scan tất cả scripts kèm source (giới hạn 200 scripts)
    local count = 0
    local function scanScripts(parent, depth)
        if depth > 8 or count >= 200 then return end
        for _, c in ipairs(parent:GetChildren()) do
            if c:IsA("LuaSourceContainer") then
                count += 1
                -- Chỉ lấy 100 dòng đầu của source để tránh quá lớn
                local lines = string.split(c.Source, "\n")
                local preview = {}
                for i = 1, math.min(100, #lines) do
                    preview[i] = lines[i]
                end
                local sourcePreview = table.concat(preview, "\n")
                if #lines > 100 then
                    sourcePreview = sourcePreview .. string.format(
                        "\n-- ... (%d more lines)", #lines - 100
                    )
                end
                data.scripts[#data.scripts+1] = {
                    path         = c:GetFullName(),
                    class        = c.ClassName,
                    totalLines   = #lines,
                    source       = sourcePreview,
                }
            end
            scanScripts(c, depth + 1)
        end
    end
    scanScripts(game, 0)

    return true, enc(data)
end

-- batch_run: chạy nhiều đoạn code, trả từng kết quả
local function handleBatchRun(kind)
    local codes = kind.codes or {}
    local results = {}
    for i, code in ipairs(codes) do
        local ok, out = runCode(code)
        results[#results+1] = {
            index   = i,
            success = ok,
            output  = out,
        }
        -- Dừng nếu có lỗi nghiêm trọng
        if not ok and string.find(out, "Syntax") then
            break
        end
    end
    return true, enc(results)
end

-- get_instances
local function handleGetInstances(kind)
    local path = kind.path or "game"
    local obj
    if path == "game" then
        obj = game
    else
        local parts = string.split(path, ".")
        obj = game
        for i = 2, #parts do
            obj = obj:FindFirstChild(parts[i])
            if not obj then return false, "Not found: " .. parts[i] end
        end
    end
    local result = { path = path, class = obj.ClassName, children = {} }
    for _, c in ipairs(obj:GetChildren()) do
        result.children[#result.children+1] = {
            name = c.Name, class = c.ClassName, path = c:GetFullName()
        }
    end
    return true, enc(result)
end

-- get_scripts
local function handleGetScripts()
    local found, count = {}, 0
    local function scan(p, d)
        if d > 8 or count >= 200 then return end
        for _, c in ipairs(p:GetChildren()) do
            if c:IsA("LuaSourceContainer") then
                count += 1
                found[#found+1] = {
                    path  = c:GetFullName(),
                    class = c.ClassName,
                    lines = #string.split(c.Source, "\n"),
                    source = c.Source,
                }
            end
            scan(c, d + 1)
        end
    end
    scan(game, 0)
    return true, enc(found)
end

-- Dispatch
local function dispatch(cmd)
    local k = cmd.kind
    if not k then return false, "No kind" end
    local t = k.type

    if t == "snapshot"      then return handleSnapshot() end
    if t == "batch_run"     then return handleBatchRun(k) end
    if t == "run_code"      then return runCode(k.code or "") end
    if t == "get_instances" then return handleGetInstances(k) end
    if t == "get_scripts"   then return handleGetScripts() end
    if t == "insert_part"   then
        local code = string.format(
            "local p=Instance.new('Part')\np.Name='%s'\np.Parent=%s\nprint('✅ '..p.Name)",
            k.name or "Part", k.parent or "game.Workspace"
        )
        return runCode(code)
    end
    return false, "Unknown: " .. tostring(t)
end

-- ── Poll loop ─────────────────────────────────────────────────────

local function pollLoop()
    log("Started → " .. url("/poll"))
    while State.running do
        -- Health check
        local healthOk = pcall(HttpService.GetAsync, HttpService, url("/health"), true)
        if not healthOk then
            State.connected = false
            State.errorMsg  = "roblox-mcp.exe không chạy"
            task.wait(CFG.RETRY_DELAY)
            continue
        end

        State.connected = true
        State.errorMsg  = ""

        -- Long-poll
        local ok, resp = pcall(HttpService.GetAsync, HttpService, url("/poll"), true)
        if not ok then
            State.connected = false
            State.errorMsg  = tostring(resp)
            task.wait(CFG.RETRY_DELAY)
            continue
        end

        local data = dec(resp)
        if data and data.id then
            State.cmdCount += 1
            State.lastCmd = (data.kind and data.kind.type) or "?"
            log(string.format("#%d %s", State.cmdCount, State.lastCmd))

            local success, output = dispatch(data)

            pcall(HttpService.PostAsync, HttpService,
                url("/result"),
                enc({ id = data.id, output = tostring(output), success = success }),
                Enum.HttpContentType.ApplicationJson,
                false
            )
        end
        task.wait(0.05) -- yield nhỏ tránh busy loop
    end
    State.connected = false
    log("Stopped")
end

-- ── UI ────────────────────────────────────────────────────────────

local toolbar = plugin:CreateToolbar("Claude MCP")
local btn = toolbar:CreateButton("MCP", "Bật/tắt Claude MCP", "")

local widgetInfo = DockWidgetPluginGuiInfo.new(
    Enum.InitialDockState.Bottom, true, false, 360, 170, 280, 130
)
local widget = plugin:CreateDockWidgetPluginGui("RobloxMCP_v3", widgetInfo)
widget.Title = "Claude MCP"

-- Root frame
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
list.Padding = UDim.new(0,5)

local function lbl(text, sz, col, order)
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

local lblSt  = lbl("● STOPPED",              17, Color3.fromRGB(140,140,155), 1)
local lblBr  = lbl("Bridge: —",              12, Color3.fromRGB(90,90,110),   2)
local lblCmd = lbl("Commands: 0",            12, Color3.fromRGB(90,90,110),   3)
local lblLst = lbl("Last: —",                12, Color3.fromRGB(90,90,110),   4)
local lblErr = lbl("",                       11, Color3.fromRGB(255,80,80),   5)

-- Divider
local div = Instance.new("Frame", root)
div.Size = UDim2.new(1,0,0,1)
div.BackgroundColor3 = Color3.fromRGB(40,40,55)
div.BorderSizePixel = 0
div.LayoutOrder = 6

lbl("Tip: snapshot() trả toàn bộ context 1 lần — ít token nhất",
    10, Color3.fromRGB(60,60,80), 7)

-- Cập nhật UI
task.spawn(function()
    while true do
        if State.running then
            if State.connected then
                lblSt.Text = "<font color='#50dd80'>●</font> CONNECTED"
            else
                lblSt.Text = "<font color='#f0b832'>●</font> CONNECTING..."
            end
            local up = string.format("%.0fs", os.clock() - State.startClock)
            lblBr.Text  = "Bridge: " .. url("")
            lblCmd.Text = string.format("Commands: %d  |  Up: %s", State.cmdCount, up)
            lblLst.Text = "Last: " .. State.lastCmd
        else
            lblSt.Text  = "● STOPPED"
            lblBr.Text  = "Bridge: —"
            lblCmd.Text = "Commands: 0"
            lblLst.Text = "Last: —"
        end
        lblErr.Text = State.errorMsg
        task.wait(0.5)
    end
end)

btn.Click:Connect(function()
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

log("v0.3 loaded")