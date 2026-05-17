# Roblox Studio MCP Server

Kết nối Claude web (claude.ai) với Roblox Studio — không cần cài thêm gì ngoài file `.exe` này.

## Kiến trúc

```
claude.ai (web)
    ↕ HTTPS (MCP protocol)
Cloudflare Tunnel ← tự động download + chạy
    ↕
roblox-mcp.exe (localhost:3000)
    ↕ HTTP long-poll (localhost:7878)
Roblox Studio Plugin (RobloxMCP.lua)
    ↕
Roblox Studio DataModel
```

---

## Cài đặt

## Build

### Yêu cầu

```powershell
# 1. Cài Rust (nếu chưa có)
winget install Rustlang.Rustup
# Restart terminal

# 2. Thêm Windows GNU target
rustup target add x86_64-pc-windows-gnu

# 3. Cài MinGW linker (nếu build trên Linux/WSL)
sudo apt install gcc-mingw-w64-x86-64
```

### Build lệnh

```powershell
# Build release (Windows .exe) — dùng từ Windows hoặc WSL
cargo build --release --target x86_64-pc-windows-gnu

# Binary ở:
# target\x86_64-pc-windows-gnu\release\roblox-mcp.exe
```

> **Lưu ý:** `.cargo/config.toml` đã set default target là `x86_64-pc-windows-gnu`
> nên chỉ cần `cargo build --release` cũng được nếu đang ở Windows với MinGW.

### Bước 2 — Cài plugin vào Roblox Studio

Copy file `plugin/RobloxMCP.lua` vào:
```
%LOCALAPPDATA%\Roblox\Plugins\RobloxMCP.lua
```

Hoặc trong Studio: **Plugins → Plugins Folder** → paste file vào.

Bật HTTP trong Studio: **Game Settings → Security → Allow HTTP Requests** ✓

### Bước 3 — Cấu hình Cloudflare Tunnel (URL cố định)

Bạn cần subdomain riêng trỏ vào Cloudflare.

**3a. Tạo tunnel trên Cloudflare dashboard:**
1. Vào https://one.dash.cloudflare.com
2. **Networks → Tunnels → Create a tunnel**
3. Chọn **Cloudflared** → đặt tên (ví dụ: `roblox-mcp`)
4. Copy **tunnel token** (dạng `eyJ...`)

**3b. Add public hostname:**
- Subdomain: `roblox-mcp` (hoặc tên bạn muốn)
- Domain: `yourdomain.com`
- Service: `http://localhost:3000`
→ URL sẽ là: `https://roblox-mcp.yourdomain.com`

**3c. Tạo file `.env` hoặc set environment variable:**
```bash
# Windows (PowerShell)
$env:CF_TUNNEL_TOKEN = "eyJ..."

# Hoặc tạo file .env cùng chỗ với .exe:
CF_TUNNEL_TOKEN=eyJ...
```

### Bước 4 — Chạy

```bash
# Với token (URL cố định)
set CF_TUNNEL_TOKEN=eyJ...
roblox-mcp.exe

# Không có token (URL random, test thôi)
set QUICK_TUNNEL=1
roblox-mcp.exe
```

Output sẽ hiện:
```
╔══════════════════════════════════════════════════╗
║         ROBLOX STUDIO MCP — READY                ║
╠══════════════════════════════════════════════════╣
║  Public URL: https://roblox-mcp.yourdomain.com  ║
╠══════════════════════════════════════════════════╣
║  Thêm vào claude.ai:                             ║
║  Settings → Connectors → Add custom connector   ║
╚══════════════════════════════════════════════════╝
```

### Bước 5 — Add vào claude.ai

1. Vào https://claude.ai
2. **Settings → Connectors → Add custom connector**
3. Dán URL vào: `https://roblox-mcp.yourdomain.com`
4. Tên: `Roblox Studio`
5. Save

### Bước 6 — Bật plugin trong Studio

1. Mở Roblox Studio
2. Tìm tab **"Claude MCP"** trong toolbar
3. Click button **MCP** để bật (icon sẽ highlight)
4. Widget status ở dưới hiện `🟢 MCP Running`

---

## Sử dụng trong claude.ai

Khi chat, bật connector **Roblox Studio** (click dấu `+` → Connectors).

Ví dụ prompt:
```
Kiểm tra kết nối Studio
→ Claude gọi: status()

Tạo một Part màu đỏ trong Workspace tên là "RedBlock"
→ Claude gọi: insert_part() + run_code()

Xem cấu trúc game của tôi
→ Claude gọi: get_instances("game")

Đọc tất cả scripts trong game
→ Claude gọi: get_scripts()

Tạo script chạy mỗi giây và in thời gian
→ Claude gọi: insert_script()
```

---

## Environment Variables

| Biến | Mặc định | Mô tả |
|------|----------|-------|
| `MCP_PORT` | `3000` | Port MCP HTTP server |
| `BRIDGE_PORT` | `7878` | Port bridge nhận lệnh từ plugin |
| `CF_TUNNEL_TOKEN` | _(none)_ | Token named tunnel (URL cố định) |
| `QUICK_TUNNEL` | `false` | Dùng quick tunnel (URL random) |

---

## Troubleshooting

**Plugin không kết nối được:**
- Kiểm tra `roblox-mcp.exe` đang chạy
- Kiểm tra **Allow HTTP Requests** đã bật trong Studio
- Thử `http://127.0.0.1:7878/health` trong browser → phải hiện "OK"

**Claude không thấy tools:**
- Kiểm tra connector URL trong claude.ai settings
- Thử `https://your-url.com/` trong browser → phải hiện JSON info

**cloudflared không download được:**
- Tự download từ: https://github.com/cloudflare/cloudflared/releases/latest
- Đặt `cloudflared.exe` cùng thư mục với `roblox-mcp.exe`