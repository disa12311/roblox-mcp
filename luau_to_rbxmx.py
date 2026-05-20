#!/usr/bin/env python3
"""
luau_to_rbxmx.py — Chuyển đổi file .luau / .lua sang .rbxmx (Roblox XML Model)

Hỗ trợ:
  - Script, LocalScript, ModuleScript
  - Tự động detect loại script từ header comment hoặc tên file
  - Nhiều file đầu vào → một file .rbxmx duy nhất (hoặc nhiều file riêng)

Cách dùng:
  python luau_to_rbxmx.py plugin.luau
  python luau_to_rbxmx.py plugin.luau -o output.rbxmx
  python luau_to_rbxmx.py *.luau --merge -o bundle.rbxmx
  python luau_to_rbxmx.py plugin.luau --type LocalScript
  python luau_to_rbxmx.py plugin.luau --name MyPlugin
"""

import argparse
import re
import sys
import uuid
from pathlib import Path


# ── Script type detection ─────────────────────────────────────────

SCRIPT_TYPES = {"Script", "LocalScript", "ModuleScript"}

# Header comment: -- @type LocalScript
HEADER_TYPE_RE = re.compile(
    r"--\s*@type\s+(Script|LocalScript|ModuleScript)", re.IGNORECASE
)
SUFFIX_MAP = {
    ".server": "Script",
    ".client": "LocalScript",
    ".module": "ModuleScript",
}


def detect_script_type(path: Path, source: str) -> str:
    """Ưu tiên: header comment → suffix → mặc định Script."""
    m = HEADER_TYPE_RE.search(source[:500])
    if m:
        return m.group(1)
    stem = path.stem
    for suffix, stype in SUFFIX_MAP.items():
        if stem.endswith(suffix):
            return stype
    return "Script"


def detect_script_name(path: Path) -> str:
    """Tên file bỏ .server/.client/.module nếu có."""
    stem = path.stem
    for suffix in SUFFIX_MAP:
        if stem.endswith(suffix):
            return stem[: -len(suffix)]
    return stem


# ── CDATA / XML helpers ───────────────────────────────────────────

def make_referent() -> str:
    return "RBX" + uuid.uuid4().hex.upper()


def cdata_wrap(source: str) -> str:
    """
    Bọc source trong CDATA. Nếu source chứa ']]>' thì split để tránh lỗi.
    """
    return "<![CDATA[" + source.replace("]]>", "]]]]><![CDATA[>") + "]]>"


def xml_attr(value: str) -> str:
    """Escape attribute value."""
    return value.replace("&", "&amp;").replace('"', "&quot;")


def xml_text(value: str) -> str:
    """Escape text node."""
    return value.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


# ── Serializer ────────────────────────────────────────────────────
# Dùng string builder thay vì ET để kiểm soát CDATA trực tiếp.

INDENT = "\t"


def build_item_xml(
    name: str,
    script_type: str,
    source: str,
    disabled: bool = False,
) -> list[str]:
    """Trả về list các dòng XML cho một <Item>."""
    ref = make_referent()
    lines: list[str] = []
    a = lines.append

    a(f'{INDENT}<Item class="{script_type}" referent="{ref}">')
    a(f'{INDENT*2}<Properties>')

    # Name
    a(f'{INDENT*3}<string name="Name">{xml_text(name)}</string>')

    # Source — CDATA, không escape
    a(f'{INDENT*3}<ProtectedString name="Source">')
    a(f'{INDENT*4}{cdata_wrap(source)}')
    a(f'{INDENT*3}</ProtectedString>')

    # Disabled
    a(f'{INDENT*3}<bool name="Disabled">{"true" if disabled else "false"}</bool>')

    # RunContext — chỉ Script
    if script_type == "Script":
        a(f'{INDENT*3}<token name="RunContext">0</token>')

    a(f'{INDENT*2}</Properties>')
    a(f'{INDENT}</Item>')
    return lines


RBXMX_NS = (
    'xmlns:xmime="http://www.w3.org/2005/05/xmlmime" '
    'xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" '
    'xsi:noNamespaceSchemaLocation="https://raw.githubusercontent.com/'
    'MaximumADHD/Roblox-Client-Tracker/roblox/Mini-Schema.xsd" '
    'version="4"'
)


def build_rbxmx(items_xml: list[list[str]]) -> str:
    """Ghép các item lines vào root <roblox>."""
    lines: list[str] = [
        '<?xml version="1.0" encoding="utf-8"?>',
        f'<roblox {RBXMX_NS}>',
        "",
    ]
    for item_lines in items_xml:
        lines.extend(item_lines)
        lines.append("")
    lines.append("</roblox>")
    return "\n".join(lines)


# ── File conversion ───────────────────────────────────────────────

def convert_file(
    path: Path,
    force_type: str | None = None,
    force_name: str | None = None,
    disabled: bool = False,
) -> list[str]:
    """Đọc file .luau/.lua → trả về lines XML cho item đó."""
    source = path.read_text(encoding="utf-8")
    name   = force_name or detect_script_name(path)
    stype  = force_type or detect_script_type(path, source)

    if stype not in SCRIPT_TYPES:
        print(f"[warn] '{stype}' không hợp lệ, dùng Script.", file=sys.stderr)
        stype = "Script"

    print(f"  {path.name}  →  {stype} '{name}'")
    return build_item_xml(name, stype, source, disabled)


# ── CLI ───────────────────────────────────────────────────────────

def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Chuyển đổi .luau/.lua → .rbxmx (Roblox XML Model)",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Ví dụ:
  # 1 file, output tự động: plugin.luau → plugin.rbxmx
  python luau_to_rbxmx.py plugin.luau

  # Chỉ định output
  python luau_to_rbxmx.py plugin.luau -o dist/plugin.rbxmx

  # Nhiều file gộp vào 1 rbxmx
  python luau_to_rbxmx.py a.luau b.luau --merge -o bundle.rbxmx

  # Ép kiểu
  python luau_to_rbxmx.py plugin.luau --type LocalScript

  # Đặt tên script
  python luau_to_rbxmx.py plugin.luau --name RobloxStudioBridge

  # Detect tự động từ header comment trong file:
  --   -- @type ModuleScript
""",
    )
    p.add_argument("inputs", nargs="+", metavar="FILE",
                   help="File .luau hoặc .lua (hỗ trợ glob)")
    p.add_argument("-o", "--output", metavar="OUT",
                   help="File .rbxmx đầu ra")
    p.add_argument("--type", choices=list(SCRIPT_TYPES), metavar="TYPE",
                   help="Ép kiểu: Script | LocalScript | ModuleScript")
    p.add_argument("--name", metavar="NAME",
                   help="Tên script trong Roblox (mặc định: tên file)")
    p.add_argument("--merge", action="store_true",
                   help="Gộp nhiều file vào 1 .rbxmx (cần --output)")
    p.add_argument("--disabled", action="store_true",
                   help="Đặt Disabled=true cho script")
    return p.parse_args()


def resolve_inputs(patterns: list[str]) -> list[Path]:
    paths: list[Path] = []
    for pat in patterns:
        matched = list(Path(".").glob(pat))
        if matched:
            paths.extend(matched)
        else:
            p = Path(pat)
            if p.exists():
                paths.append(p)
            else:
                print(f"[warn] Không tìm thấy: {pat}", file=sys.stderr)
    # Lọc chỉ .luau/.lua, bỏ trùng
    seen: set[Path] = set()
    result: list[Path] = []
    for p in paths:
        if p.suffix in {".luau", ".lua"} and p not in seen:
            seen.add(p)
            result.append(p)
    return result


def main() -> None:
    args = parse_args()

    inputs = resolve_inputs(args.inputs)
    if not inputs:
        print("Lỗi: không có file .luau hoặc .lua nào hợp lệ.", file=sys.stderr)
        sys.exit(1)

    if args.merge and not args.output:
        print("Lỗi: --merge cần chỉ định --output.", file=sys.stderr)
        sys.exit(1)

    single = len(inputs) == 1

    if single or args.merge:
        # Tất cả vào 1 file
        print(f"Chuyển đổi {len(inputs)} file(s):")
        items_xml = [
            convert_file(p, args.type, args.name if single else None, args.disabled)
            for p in inputs
        ]
        out_path = Path(args.output) if args.output else inputs[0].with_suffix(".rbxmx")
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(build_rbxmx(items_xml), encoding="utf-8")
        print(f"\n✅  {out_path}")
    else:
        # Mỗi file → file riêng
        if args.output:
            print("[warn] --output bị bỏ qua khi chuyển nhiều file. Dùng --merge để gộp.",
                  file=sys.stderr)
        print(f"Chuyển đổi {len(inputs)} file(s):")
        for p in inputs:
            item_xml = convert_file(p, args.type, args.name, args.disabled)
            out_path = p.with_suffix(".rbxmx")
            out_path.write_text(build_rbxmx([item_xml]), encoding="utf-8")
            print(f"    → {out_path}")
        print(f"\n✅  Xong ({len(inputs)} file)")


if __name__ == "__main__":
    main()