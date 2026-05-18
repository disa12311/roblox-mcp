# Roblox Studio MCP Server

Kết nối Claude web (claude.ai) với Roblox Studio qua MCP protocol.

## Kiến trúc

```
claude.ai (web)
    ↕ HTTPS — MCP Streamable HTTP
Cloudflare Quick Tunnel (*.trycloudflare.com)
    ↕
roblox-mcp.exe  ←  localhost:3000 (MCP server)
                ←  localhost:7878 (Bridge)
    ↕ HTTP long-poll
RobloxMCP.luau (Plugin trong Roblox Studio)
    ↕
Roblox DataModel
```

---

## Yêu cầu

- **Rust** — https://rustup.rs
- **cloudflared.exe** — https://github.com/cloudflare/cloudflared/releases/latest
  - Tải `cloudflared-windows-amd64.exe` → đổi tên thành `cloudflared.exe`
  - Đặt **cùng thư mục** với `roblox-mcp.exe`

---

## Build

```powershell
# Thêm Windows target (1 lần duy nhất)
rustup target add x86_64-pc-windows-gnu

# Build
cargo build --release --target x86_64-pc-windows-gnu

# Binary ở:
# target\x86_64-pc-windows-gnu\release\roblox-mcp.exe
```

---

## Cài plugin vào Roblox Studio

Copy `plugin/RobloxMCP.luau` vào:
```
%LOCALAPPDATA%\Roblox\Plugins\RobloxMCP.luau
```

Bật HTTP trong Studio:
**Game Settings → Security → Allow HTTP Requests ✓**

---

## Chạy

```powershell
.\roblox-mcp.exe
```

Lần đầu chạy sẽ thấy:

```
╔══════════════════════════════════════════╗
║     Roblox Studio MCP Server v0.1        ║
╚══════════════════════════════════════════╝

  ✓  MCP port    localhost:3000
  ✓  Bridge      localhost:7878
  ✓  Tunnel      Quick (URL random — thay đổi mỗi lần restart)

  ⟳ Đang khởi động...

┌──────────────────────────────────────────┐
│              ✅  READY                    │
│  Public URL: https://xxxx.trycloudflare.com  │
│  Bridge:     localhost:7878              │
├──────────────────────────────────────────┤
│  Thêm vào claude.ai:                     │
│  Settings → Connectors → Add custom      │
└──────────────────────────────────────────┘
```

> ⚠️ URL thay đổi mỗi lần restart — phải cập nhật lại connector trong claude.ai.

---

## Thêm connector vào claude.ai

1. Vào **https://claude.ai → Settings → Connectors → Add custom connector**
2. Dán URL từ terminal vào (dạng `https://xxxx.trycloudflare.com`)
3. Save

---

## Bật plugin trong Studio

1. Mở Roblox Studio
2. Toolbar → tab **"Claude MCP"** → click button **MCP**
3. Widget hiện `● CONNECTED` màu xanh là OK

---

## Thứ tự khởi động

```
1. Chạy roblox-mcp.exe      ← đợi hiện READY + URL
2. Copy URL → paste vào claude.ai Connectors
3. Mở Roblox Studio
4. Click MCP button trong toolbar
5. Chat trên claude.ai → bật connector Roblox Studio
```

---

## Sử dụng trong claude.ai

Khi chat, click `+` → **Connectors** → bật **Roblox Studio**.

### Tools

| Tool | Mô tả | Ghi chú |
|------|-------|---------|
| `snapshot()` | Lấy toàn bộ context game 1 lần | **Gọi đầu tiên** |
| `batch_run([...])` | Chạy nhiều đoạn Luau trong 1 call | Tiết kiệm token |
| `run_code(code)` | Chạy 1 đoạn Luau | Thao tác đơn lẻ |
| `get_instances(path)` | Xem children của 1 object | Inspect cụ thể |
| `insert_part(name, parent)` | Tạo Part mới | Tạo nhanh |
| `insert_script(...)` | Tạo Script với source code | Thêm logic |
| `get_scripts()` | Đọc tất cả scripts | Review code |
| `status()` | Kiểm tra kết nối | Debug |

### Flow tối ưu (ít token nhất)

```
1. snapshot()        ← 1 call, biết hết cấu trúc game
2. batch_run([...])  ← 1 call, làm mọi thứ cùng lúc
= 2 calls tổng
```

### Ví dụ prompt

```
Xem game của tôi có gì rồi tạo obstacle course đơn giản
Thêm script đếm điểm khi chạm vào coin
Đổi màu tất cả Parts trong Workspace thành đỏ
Debug tại sao script không chạy
```

---

## Cấu hình (Environment Variables)

| Biến | Mặc định | Mô tả |
|------|----------|-------|
| `MCP_PORT` | `3000` | Port MCP HTTP server |
| `BRIDGE_PORT` | `7878` | Port bridge nhận lệnh từ plugin |

---

## Troubleshooting

**`cloudflared.exe` không tìm thấy:**
```
Tải: https://github.com/cloudflare/cloudflared/releases/latest
File: cloudflared-windows-amd64.exe → đổi tên cloudflared.exe
Đặt cùng thư mục với roblox-mcp.exe
```

**Plugin hiện `● CONNECTING...` mãi:**
```powershell
# Kiểm tra bridge server
curl http://127.0.0.1:7878/health
# Phải trả: OK
```

**claude.ai không connect được:**
```powershell
# Kiểm tra MCP server
curl https://xxxx.trycloudflare.com/health
# Phải trả: OK

# Test initialize
curl -X POST https://xxxx.trycloudflare.com/ ^
  -H "Content-Type: application/json" ^
  -H "Accept: application/json, text/event-stream" ^
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}"
# Phải trả JSON có protocolVersion và serverInfo
```

**URL thay đổi sau khi restart:**
- Cập nhật lại connector trong claude.ai Settings → Connectors
- Xóa connector cũ → Add lại với URL mới