# Roblox Studio Bridge

Kết nối Claude web (claude.ai) với Roblox Studio — điều khiển game trực tiếp từ chat.

---

## Kiến trúc

```
claude.ai (web)
    ↕ HTTPS — MCP Streamable HTTP (spec 2025-03-26)
Cloudflare Quick Tunnel
(https://xxxx.trycloudflare.com)
    ↕
roblox-studio-bridge.exe
├── GUI Window (egui)       ← Log console, trạng thái kết nối, copy URL
├── MCP HTTP Server  :3000  ← Claude kết nối vào
└── Bridge Server    :7878  ← Plugin kết nối vào
    ↕ HTTP long-poll
RobloxStudioBridge.luau (Plugin trong Roblox Studio)
    ↕
Roblox DataModel
```

---

## Cấu trúc project

```
roblox-studio-bridge/
├── src/
│   ├── main.rs         — Entry point, egui GUI
│   ├── bridge.rs       — HTTP server nhận lệnh từ plugin (port 7878)
│   ├── http_server.rs  — MCP HTTP server cho Claude (port 3000)
│   └── tunnel.rs       — Cloudflare Quick Tunnel
├── plugin/
│   └── RobloxStudioBridge.luau  — Plugin Roblox Studio (Luau strict)
├── tools/
│   └── luau_to_rbxmx.py         — Tool chuyển .luau → .rbxmx
├── .cargo/
│   └── config.toml     — Cross-compile Windows GNU target
├── Cargo.toml
└── README.md
```

---

## Yêu cầu

### Rust
```powershell
winget install Rustlang.Rustup
# Restart terminal sau khi cài
rustup target add x86_64-pc-windows-gnu
```

### cloudflared
Tải tại: https://github.com/cloudflare/cloudflared/releases/latest

- File cần tải: `cloudflared-windows-amd64.exe`
- Đổi tên thành: `cloudflared.exe`
- Đặt **cùng thư mục** với `roblox-studio-bridge.exe`
- Hoặc cài system-wide: `winget install Cloudflare.cloudflared`

---

## Build

```powershell
cargo build --release --target x86_64-pc-windows-gnu
```

Binary sau khi build:
```
target\x86_64-pc-windows-gnu\release\roblox-studio-bridge.exe
```

Copy 2 file vào cùng 1 thư mục:
```
📁 bất kỳ
├── roblox-studio-bridge.exe
└── cloudflared.exe
```

---

## Cài plugin vào Roblox Studio

### Cách 1 — Copy file Luau (đơn giản nhất)
```
plugin\RobloxStudioBridge.luau
→ %LOCALAPPDATA%\Roblox\Plugins\RobloxStudioBridge.luau
```

### Cách 2 — Dùng file .rbxmx (import vào Studio)
```powershell
python tools\luau_to_rbxmx.py plugin\RobloxStudioBridge.luau --name RobloxStudioBridge
# → tạo ra plugin\RobloxStudioBridge.rbxmx
```
Sau đó mở Studio → **File → Open** → chọn file `.rbxmx`

**Bật HTTP trong Studio (bắt buộc):**
```
Game Settings → Security → Allow HTTP Requests ✓
```

---

## Chạy

```powershell
.\roblox-studio-bridge.exe
```

Cửa sổ GUI mở ra:

```
┌─ Roblox Studio Bridge  v0.2 ─────────── ● waiting  [url]  [⎘ copy] ─┐
│                                                                        │
│  00:00:01  Roblox Studio Bridge  v0.2                                 │
│  ─────────────────────────────────────────────────                    │
│  00:00:01  MCP port   →  localhost:3000                               │
│  00:00:01  Bridge     →  localhost:7878                               │
│  00:00:01  Bridge server  localhost:7878                              │
│  00:00:01  MCP server     localhost:3000                              │
│  ─────────────────────────────────────────────────                    │
│  00:00:02  ✅  READY                                                  │
│  00:00:02  https://xxxx.trycloudflare.com                             │
│  ─────────────────────────────────────────────────                    │
│  00:00:15  Plugin Roblox Studio đã kết nối                            │
│                                                                        │
├── ● plugin online  │  mcp :3000  bridge :7878  ──── [⬇ auto] ────────┤
└────────────────────────────────────────────────────────────────────────┘
```

- **Header**: dot màu trạng thái + URL tunnel + nút **⎘ copy** để copy URL vào clipboard
- **Log**: timestamp `HH:MM:SS` + text màu theo loại (info / success / warn / error)
- **Status bar**: plugin online/offline, ports, toggle auto-scroll

> ⚠️ **URL thay đổi mỗi lần restart** — phải cập nhật lại connector trong claude.ai.

---

## Thêm connector vào claude.ai

1. Click **⎘ copy** trong cửa sổ để copy URL tunnel
2. Vào **claude.ai → Settings → Connectors → Add custom connector**
3. Dán URL vào → Save
4. Khi chat: click `+` → **Connectors** → bật **Roblox Studio Bridge**

---

## Kết nối plugin trong Studio

Sau khi cài plugin, mở Studio sẽ thấy tab **"Studio Bridge"** trong toolbar.

Widget hiện ra ở dưới màn hình:

```
● STOPPED
Commands: 0
Last: —
────────────────────────────────────
Bridge URL
┌──────────────────────────────┐ ┌────────────┐
│ http://127.0.0.1:7878        │ │  Connect   │
└──────────────────────────────┘ └────────────┘
Tip: snapshot() trả toàn bộ context 1 lần
```

**Bridge URL là gì?**
- Địa chỉ port 7878 của `roblox-studio-bridge.exe` chạy trên máy bạn
- Mặc định `http://127.0.0.1:7878` — **không cần đổi** nếu chạy cùng máy
- Chỉ đổi nếu chạy server trên máy khác trong LAN

**Cách kết nối:**
- Click **Connect** hoặc nhấn **Enter** trong ô URL
- Widget chuyển sang `● CONNECTED` màu xanh là OK

---

## Thứ tự khởi động đúng

```
Bước 1: Chạy roblox-studio-bridge.exe
        → Đợi cửa sổ hiện "READY" + URL

Bước 2: Click ⎘ copy → paste vào claude.ai
        Settings → Connectors → Add custom connector

Bước 3: Mở Roblox Studio + mở game bất kỳ

Bước 4: Widget "Studio Bridge" → click Connect
        → Chờ hiện ● CONNECTED

Bước 5: Chat trên claude.ai
        → click + → Connectors → bật Roblox Studio Bridge
```

---

## Sử dụng trong claude.ai

### Danh sách tools

| Tool | Mô tả | Khi nào dùng |
|------|-------|--------------|
| `snapshot()` | Lấy toàn bộ context game 1 lần: version, tất cả instances, tất cả scripts kèm source | **Luôn gọi đầu tiên** |
| `batch_run([...])` | Chạy nhiều đoạn Luau tuần tự trong 1 lần gọi, trả JSON array kết quả | Nhiều thao tác cùng lúc |
| `run_code(code)` | Chạy 1 đoạn Luau, trả output | Thao tác đơn lẻ |
| `get_instances(path)` | Xem children của 1 object (ví dụ: `game.Workspace`) | Inspect cụ thể |
| `get_scripts()` | Đọc tất cả scripts kèm toàn bộ source code | Review/debug code |
| `insert_part(name, parent)` | Tạo Part mới | Tạo nhanh |
| `insert_script(name, type, parent, source)` | Tạo Script/LocalScript/ModuleScript | Thêm logic |
| `status()` | Kiểm tra kết nối plugin còn sống không | Debug |

### Flow tối ưu (ít token nhất)

```
❌ Tệ — nhiều round trips:
   status() → get_instances() → get_scripts() → run_code() → run_code() → run_code()
   = 6 calls

✅ Tốt — gộp lại:
   snapshot()       ← 1 call, biết hết cấu trúc game
   batch_run([...]) ← 1 call, làm mọi thứ cùng lúc
   = 2 calls
```

### Ví dụ prompt

```
# Xem tổng quan game
"Xem game của tôi có gì"
→ Claude: snapshot()

# Tạo nhiều thứ cùng lúc
"Tạo obstacle course đơn giản gồm 5 platforms và 1 script di chuyển"
→ Claude: snapshot() → batch_run([tạo folder, tạo parts, tạo script])

# Sửa code
"Script CountdownTimer bị lỗi, fix giúp tôi"
→ Claude: get_scripts() → phân tích → run_code(fix)

# Thao tác nhanh
"Đổi màu tất cả Parts trong Workspace thành đỏ"
→ Claude: run_code("for _,p in workspace:GetDescendants() do if p:IsA('BasePart') then p.BrickColor = BrickColor.new('Bright red') end end")
```

---

## Tool: luau_to_rbxmx.py

Chuyển đổi file `.luau` / `.lua` sang `.rbxmx` để import vào Roblox Studio.

```powershell
# 1 file → rbxmx cùng tên
python tools\luau_to_rbxmx.py plugin\RobloxStudioBridge.luau

# Chỉ định output
python tools\luau_to_rbxmx.py plugin\RobloxStudioBridge.luau -o dist\plugin.rbxmx

# Ép kiểu script
python tools\luau_to_rbxmx.py foo.luau --type LocalScript

# Nhiều file gộp vào 1 rbxmx
python tools\luau_to_rbxmx.py a.luau b.luau --merge -o bundle.rbxmx
```

**Auto-detect loại script** theo thứ tự ưu tiên:
1. Header comment trong file: `-- @type LocalScript`
2. Suffix tên file: `foo.server.luau` → Script, `foo.client.luau` → LocalScript, `foo.module.luau` → ModuleScript
3. Mặc định: `Script`

---

## Cấu hình (Environment Variables)

| Biến | Mặc định | Mô tả |
|------|----------|-------|
| `MCP_PORT` | `3000` | Port MCP HTTP server (Claude kết nối) |
| `BRIDGE_PORT` | `7878` | Port Bridge (plugin kết nối) |

```powershell
$env:MCP_PORT    = "4000"
$env:BRIDGE_PORT = "8888"
.\roblox-studio-bridge.exe
```

---

## Test server hoạt động không

```powershell
# 1. Health check
curl http://127.0.0.1:3000/health
# Trả về: OK

# 2. Test MCP initialize (CMD — dùng ^ để xuống dòng)
curl -X POST http://127.0.0.1:3000/ ^
  -H "Content-Type: application/json" ^
  -H "Accept: application/json, text/event-stream" ^
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}"
# Trả về JSON có "protocolVersion":"2025-03-26" và "serverInfo"

# 3. Test qua tunnel
curl https://xxxx.trycloudflare.com/health
# Trả về: OK
```

---

## Troubleshooting

### `cloudflared.exe` không tìm thấy
```
Fix: Đặt cloudflared.exe cùng thư mục với roblox-studio-bridge.exe
     Hoặc: winget install Cloudflare.cloudflared
```

### Plugin hiện `● CONNECTING...` mãi
```powershell
# Kiểm tra bridge server còn sống
curl http://127.0.0.1:7878/health
# Phải trả: OK

# Nếu không trả OK → roblox-studio-bridge.exe chưa chạy hoặc đã crash
```

### claude.ai báo "Couldn't reach the MCP server"
```powershell
# Kiểm tra tunnel
curl https://xxxx.trycloudflare.com/health
# Phải trả: OK

# Nếu OK mà vẫn lỗi → xóa connector cũ, add lại với URL mới
# URL thay đổi sau mỗi lần restart roblox-studio-bridge.exe
```

### Studio không nhận lệnh dù plugin CONNECTED
```
1. Kiểm tra "Allow HTTP Requests" đã bật trong Game Settings → Security
2. Thử click Disconnect → Connect lại trong widget
3. Kiểm tra Output window trong Studio có lỗi gì không
```