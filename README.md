# Roblox Studio Bridge

Cho phép Claude (claude.ai) điều khiển Roblox Studio trực tiếp qua chat — tạo parts, viết scripts, chỉnh sửa game mà không cần rời khỏi trình duyệt.

---

## Cách hoạt động

Có 3 thành phần:

```
claude.ai  ←→  roblox-studio-bridge.exe  ←→  Plugin trong Studio
              (chạy trên máy bạn)
```

1. **`roblox-studio-bridge.exe`** chạy trên máy, mở tunnel ra internet để Claude kết nối vào
2. **Plugin** cài trong Roblox Studio, kết nối với exe qua localhost
3. Khi chat trên Claude, các lệnh đi qua tunnel → exe → plugin → thực thi trong game

---

## Lần đầu cài đặt

### Bước 1 — Cài Rust (chỉ cần 1 lần)

```powershell
winget install Rustlang.Rustup
# Khởi động lại terminal sau khi cài xong
rustup target add x86_64-pc-windows-gnu
```

### Bước 2 — Build

```powershell
cargo build --release --target x86_64-pc-windows-gnu
```

File build xong ở:
```
target\x86_64-pc-windows-gnu\release\roblox-studio-bridge.exe
```

### Bước 3 — Cài cloudflared

cloudflared tạo tunnel để Claude kết nối vào máy bạn từ internet.

**Cách nhanh nhất:** Mở app → tab Settings → click **⬇ Tải tự động**. App tự tải và cài.

**Hoặc cài thủ công:**
```powershell
winget install Cloudflare.cloudflared
# Hoặc tải cloudflared-windows-amd64.exe tại github.com/cloudflare/cloudflared/releases
# Đổi tên thành cloudflared.exe, đặt cùng thư mục với roblox-studio-bridge.exe
```

### Bước 4 — Cài plugin vào Roblox Studio

**Cách 1 — Copy file (đơn giản nhất):**
```
plugin\RobloxStudioBridge.luau  →  %LOCALAPPDATA%\Roblox\Plugins\RobloxStudioBridge.luau
```

**Cách 2 — Import file .rbxmx:**
```powershell
python tools\luau_to_rbxmx.py plugin\RobloxStudioBridge.luau --name RobloxStudioBridge
# Tạo ra plugin\RobloxStudioBridge.rbxmx
# Mở Studio → File → Open → chọn file .rbxmx
```

**Bật HTTP trong Studio** (bắt buộc, chỉ cần làm 1 lần):
```
Game Settings → Security → Allow HTTP Requests ✓
```

---

## Cách dùng mỗi ngày

### 1. Chạy app

```powershell
.\roblox-studio-bridge.exe
```

Cửa sổ mở ra, đợi đến khi log hiện **✅ READY** và URL tunnel xuất hiện ở header. URL tự động copy vào clipboard.

### 2. Thêm connector vào claude.ai

> Chỉ cần làm lại khi URL thay đổi (mỗi lần restart app với Quick Tunnel).

1. URL đã được copy tự động — hoặc click **⎘ copy** trong header
2. Vào **claude.ai → Settings → Connectors → Add custom connector**
3. Dán URL → Save

### 3. Kết nối plugin trong Studio

Mở Studio → toolbar có tab **"Studio Bridge"** → widget hiện ở dưới màn hình:

```
● STOPPED
Bridge URL
[ http://127.0.0.1:7878 ]  [ Connect ]
```

Click **Connect** → chờ chuyển sang `● CONNECTED` màu xanh.

> Bridge URL mặc định `http://127.0.0.1:7878` — không cần đổi nếu Studio và app chạy cùng máy.

### 4. Chat trên claude.ai

Click `+` → **Connectors** → bật **Roblox Studio Bridge** → bắt đầu chat.

---

## Tunnel: Quick vs Named

Mở tab **Settings** trong app để chọn chế độ tunnel.

| | Quick Tunnel | Named Tunnel |
|---|---|---|
| **Tài khoản** | Không cần | Cần Cloudflare account |
| **URL** | Đổi mỗi lần restart | Cố định, không bao giờ đổi |
| **Setup** | Zero config | Cần tạo tunnel trên dashboard |
| **Phù hợp** | Dùng thử, casual | Dùng hàng ngày |

**Dùng Named Tunnel:** Vào Settings → chọn tile **🔒 Named Tunnel** → dán token vào ô → Lưu & Restart.

Lấy token tại: `dash.cloudflare.com → Zero Trust → Tunnels → Create tunnel`

---

## Giao diện app

```
┌─ ● Roblox Studio Bridge  v0.2 ─── [─]  https://xxxx.trycloudflare.com  [⎘ copy] ─┐
│                                                                                      │
│  📋 Log  ⚙ Settings                                                                 │
│  ──────────────────────────────────────────────────────────                         │
│  00:00:01  MCP port   →  localhost:3000                                             │
│  00:00:01  Bridge     →  localhost:7878                                             │
│  00:00:02  ✅  READY                                                                │
│  00:00:02  https://xxxx.trycloudflare.com                                           │
│  00:00:15  Plugin Roblox Studio đã kết nối                                          │
│                                                                                      │
├── ● plugin online  │  mcp :3000  bridge :7878  quick tunnel  ──── [⬇ auto] ────────┤
└──────────────────────────────────────────────────────────────────────────────────────┘
```

| Phần | Chức năng |
|------|-----------|
| Dot `●` ở header | Màu xanh = plugin online, vàng = đang khởi động, đỏ = lỗi |
| `[─]` | Thu nhỏ cửa sổ xuống taskbar |
| `[⎘ copy]` | Copy URL tunnel vào clipboard |
| Tab **Log** | Xem log real-time với timestamp |
| Tab **Settings** | Chọn tunnel mode, nhập token, đổi port, tải cloudflared |
| Status bar | Trạng thái plugin, ports đang dùng, toggle auto-scroll |

---

## Sử dụng trong claude.ai

### Các tool có sẵn

| Tool | Làm gì |
|------|--------|
| `snapshot()` | Lấy toàn bộ thông tin game: cấu trúc, tất cả scripts kèm source. **Gọi đầu tiên trước mọi thứ.** |
| `batch_run([code1, code2, ...])` | Chạy nhiều đoạn Luau cùng lúc, tiết kiệm token hơn gọi nhiều lần |
| `run_code(code)` | Chạy 1 đoạn Luau |
| `get_instances(path)` | Xem children của 1 object, ví dụ `game.Workspace` |
| `get_scripts()` | Đọc tất cả scripts kèm source code |
| `insert_part(name, parent)` | Tạo Part mới |
| `insert_script(name, type, parent, source)` | Tạo Script / LocalScript / ModuleScript |
| `status()` | Kiểm tra plugin còn kết nối không |

### Tip dùng hiệu quả

Thay vì gọi nhiều tool nhỏ, gộp lại:

```
❌ Chậm:  status() → get_instances() → get_scripts() → run_code() → run_code()

✅ Nhanh:  snapshot()        ← biết hết cấu trúc game trong 1 lần
           batch_run([...])  ← làm mọi thứ trong 1 lần
```

### Ví dụ prompt

```
"Xem game của tôi có gì"
→ Claude gọi snapshot() và tóm tắt cho bạn

"Tạo obstacle course 5 platforms, có script làm platforms di chuyển"
→ Claude: snapshot() → batch_run([tạo folder, tạo parts, viết script])

"Script CountdownTimer đang bị lỗi, fix giúp tôi"
→ Claude: get_scripts() → đọc code → run_code(fix)

"Đổi màu tất cả Parts thành đỏ"
→ Claude: run_code(loop qua workspace)
```

---

## Tool: luau_to_rbxmx.py

Chuyển file `.luau` / `.lua` sang `.rbxmx` để import trực tiếp vào Roblox Studio.

```powershell
# Chuyển 1 file (output tự động: RobloxStudioBridge.rbxmx)
python tools\luau_to_rbxmx.py plugin\RobloxStudioBridge.luau

# Chỉ định output
python tools\luau_to_rbxmx.py plugin\RobloxStudioBridge.luau -o dist\plugin.rbxmx

# Ép kiểu script
python tools\luau_to_rbxmx.py foo.luau --type LocalScript

# Gộp nhiều file vào 1 rbxmx
python tools\luau_to_rbxmx.py a.luau b.luau --merge -o bundle.rbxmx
```

Tool tự detect loại script theo thứ tự: header comment `-- @type LocalScript` → suffix tên file (`.server.luau` / `.client.luau` / `.module.luau`) → mặc định Script.

---

## Cấu hình nâng cao

Port mặc định có thể override bằng environment variable:

```powershell
$env:MCP_PORT    = "4000"   # mặc định 3000
$env:BRIDGE_PORT = "8888"   # mặc định 7878
.\roblox-studio-bridge.exe
```

Config tunnel mode và token lưu tự động vào `config.json` cạnh exe.

---

## Troubleshooting

**Plugin hiện `● CONNECTING...` mãi không chuyển**
```powershell
curl http://127.0.0.1:7878/health   # phải trả về: OK
# Không trả về → app chưa chạy hoặc đã crash, khởi động lại
```

**claude.ai báo "Couldn't reach the MCP server"**
```powershell
curl https://xxxx.trycloudflare.com/health   # phải trả về: OK
# Nếu OK mà vẫn lỗi → xóa connector cũ, thêm lại với URL mới
```

**Studio không nhận lệnh dù plugin CONNECTED**
1. Kiểm tra `Game Settings → Security → Allow HTTP Requests` đã bật chưa
2. Thử click Disconnect → Connect lại trong widget Studio Bridge
3. Xem `Output` window trong Studio có báo lỗi gì không

**cloudflared không tìm thấy**
- Mở tab Settings → click **⬇ Tải tự động**
- Hoặc: `winget install Cloudflare.cloudflared`

---

## Cấu trúc project

```
roblox-studio-bridge/
├── src/
│   ├── main.rs          — Entry point, backend (servers, tunnel)
│   ├── gui.rs           — Toàn bộ egui UI
│   ├── state.rs         — SharedState, LogLine dùng chung giữa GUI và backend
│   ├── config.rs        — Đọc/ghi config.json, auto-download cloudflared
│   ├── bridge.rs        — HTTP server cho plugin (port 7878)
│   ├── http_server.rs   — MCP HTTP server cho Claude (port 3000)
│   └── tunnel.rs        — Cloudflare tunnel với auto-restart
├── plugin/
│   └── RobloxStudioBridge.luau   — Plugin Roblox Studio
├── tools/
│   └── luau_to_rbxmx.py          — Chuyển .luau → .rbxmx
├── .cargo/config.toml             — Cross-compile Windows GNU
└── Cargo.toml
```