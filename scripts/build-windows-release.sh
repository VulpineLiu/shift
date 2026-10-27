#!/bin/bash
# Build the Windows x64 release installer from current source.
#
#   scripts/build-windows-release.sh [--skip-build]
#
# Produces artifacts/Shift-Setup-<version>-win-x64.exe and refreshes
# artifacts/SHA256SUMS.txt. Cross-compiled on macOS with cargo-xwin; makensis is
# the Homebrew formula of the same name. Windows ARM64 runs this x64 package
# through the update manifest's normal `windows` fallback.
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/opt/homebrew/opt/llvm/bin:$PATH"
export RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin
export RUSTC="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc"

ROOT="$PWD"
VERSION=$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")

SKIP_BUILD=0
usage() { echo "usage: $0 [--skip-build]" >&2 && exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1 ;;
    *) usage ;;
  esac
  shift
done

# A build can only verify a signed channel if the release key is compiled in. Derive
# it from the signing key when the caller gave us one, and never do this quietly: the
# alternative is two installers with the same version number and different trust.
if [ -z "${SHIFT_UPDATE_PUBKEY:-}" ] && [ -n "${SHIFT_RELEASE_KEY:-}" ]; then
  SHIFT_UPDATE_PUBKEY=$(python3 scripts/release-key.py pubkey "${SHIFT_RELEASE_KEY%.ed25519}.pub")
  export SHIFT_UPDATE_PUBKEY
fi
if [ -n "${SHIFT_UPDATE_PUBKEY:-}" ]; then
  echo "== release key baked in: ${SHIFT_UPDATE_PUBKEY:0:12}… =="
else
  echo "== no release key: this build will report the feed as unsigned =="
fi

if [ "$SKIP_BUILD" != "1" ]; then
  echo "== frontend (Windows target: chrome105) =="
  TAURI_ENV_PLATFORM=windows pnpm build
fi

ARCH=x64
TARGET=x86_64-pc-windows-msvc
PE_MACHINE=8664
EXE="src-tauri/target/$TARGET/release/shift.exe"
INSTALLER="artifacts/Shift-Setup-$VERSION-win-x64.exe"
NOTES="artifacts/README-Windows-x64.txt"

if [ "$SKIP_BUILD" != "1" ]; then
  echo "== rust cross build ($TARGET) =="
  # --features tauri/custom-protocol is mandatory: without it Tauri derives
  # cfg(dev) and every window loads build.devUrl instead of the bundled assets.
  CROSS_LOG=$(mktemp /tmp/shift-win-build.XXXXXX)
  if ! (
    cd src-tauri
    cargo xwin build --target "$TARGET" --release \
      --features tauri/custom-protocol >"$CROSS_LOG" 2>&1
  ); then
    sed -n '1,240p' "$CROSS_LOG"
    exit 1
  fi
  grep -Ev "^warning:|^\s+\||^\s+=|LNK4099|failed to load|^$|^\s+-->|^note:" \
    "$CROSS_LOG" || true
fi

test -f "$EXE"

echo "== PE machine check (x64) =="
python3 - "$EXE" "$PE_MACHINE" <<PE
import pathlib, struct, sys
exe = pathlib.Path(sys.argv[1]).read_bytes()
machine = struct.unpack_from("<H", exe, struct.unpack_from("<I", exe, 0x3C)[0] + 4)[0]
expected = int(sys.argv[2], 16)
if machine != expected:
    sys.exit(f"{sys.argv[1]} is PE machine 0x{machine:04x}, expected 0x{expected:04x}")
print(f"pe machine ok: 0x{machine:04x}")
PE

echo "== verify the frontend is embedded =="
python3 - "$EXE" <<PY
import pathlib, sys
exe = pathlib.Path(sys.argv[1]).read_bytes()
missing = [p.name for p in pathlib.Path("dist/assets").iterdir() if p.name.encode() not in exe]
for extra in ("index.html", "view=radial"):
    if extra.encode() not in exe:
        missing.append(extra)
# The runtime hint is rendered by the webview, so it must be present in the
# asset bundle even though the PE itself does not keep readable UTF-8 strings.
asset_text = "".join(
    path.read_text(encoding="utf-8") for path in pathlib.Path("dist/assets").iterdir()
)
required = [
    "以下是本机安装组件的情况，未安装的组件请点击安装。",
    "正在转换第 {done}/{total} 页…",
    "正在生成文档…",
]
missing.extend(text for text in required if text not in asset_text)
old = "以下结果来自当前这台机器"
if old in asset_text:
    sys.exit(f"stale frontend copy is still embedded: {old}")
if missing:
    sys.exit(f"frontend not embedded, missing: {missing}")
print(f"embedded ok: {len(exe):,} byte exe carries the current dist/assets")
PY

echo "== windows notes (x64) =="
STAGE=$(mktemp -d /tmp/shift-windows-release.XXXXXX)
python3 - "$STAGE" "$VERSION" <<PY
import pathlib, sys
stage = pathlib.Path(sys.argv[1])
version = sys.argv[2]
text = f"""Shift {version} - Windows x64

系统要求
--------
- Windows 10 或 Windows 11（x64；ARM64 设备通过系统兼容层运行）。
- Microsoft Edge WebView2 Runtime。当前 Windows 10/11 通常已内置；如果 Shift
  启动后窗口空白或无法打开，请从微软官方页面安装：
  https://developer.microsoft.com/microsoft-edge/webview2/
- 安装与运行都不需要管理员权限。只有在“运行组件”里安装外部工具时，
  Windows 才可能弹出 UAC 确认。

安装
----
1. 运行 Shift-Setup-{version}-win-x64.exe，默认安装到
   %LOCALAPPDATA%\\Programs\\Shift，并创建开始菜单与桌面快捷方式。
2. 这是未签名构建。若 SmartScreen 显示“Windows 已保护你的电脑”，请选择
   “更多信息” > “仍要运行”。
3. 卸载请在“设置 > 应用”中移除 Shift，无需手动删除文件。
4. 首次启动若 Windows 安全中心询问网络访问，请允许 Shift 访问“专用网络”。
   如果之前拒绝过，请到“Windows 安全中心 > 防火墙和网络保护 > 允许应用通过
   防火墙”，勾选 Shift 的专用网络权限；否则“发送到设备”无法接收其它终端的信标。

运行组件
--------
打开 Shift 主窗口，进入“设置” > “运行组件”：
- 首次进入该页会自动检测，之后只在你点击“重新检测”时重新扫描；
  扫描范围包括 PATH、WinGet、Chocolatey、Scoop 与常见安装目录。
- 页面显示本机实际使用的组件、版本、路径与来源。
- 缺失组件旁有“安装”，通过 Microsoft WinGet 安装，界面不会阻塞，
  完成后自动重新检测。

WinGet 包映射：
- Pandoc: JohnMacFarlane.Pandoc
- Typst: Typst.Typst
- FFmpeg: Gyan.FFmpeg
- Poppler: oschwartz10612.Poppler
- ImageMagick: ImageMagick.ImageMagick

如果没有 WinGet，界面会给出微软官方安装说明入口。Shift 不会从第三方站点
下载运行库，也不会卸载或覆盖你已有的工具。

校验
----
PowerShell：
  Get-FileHash .\\Shift-Setup-{version}-win-x64.exe -Algorithm SHA256
"""
(stage / "README-Windows.txt").write_bytes(text.replace("\n", "\r\n").encode("utf-8"))
PY

echo "== NSIS installer (x64) =="
mkdir -p artifacts
SIZE_KB=$(( ($(stat -f%z "$EXE" 2>/dev/null || stat -c%s "$EXE") + 1023) / 1024 ))
makensis -V2 \
  -DSHIFT_EXE="$PWD/$EXE" \
  -DLICENSE_FILE="$PWD/LICENSE" \
  -DNOTICES_FILE="$PWD/THIRD_PARTY_NOTICES.md" \
  -DREADME_FILE="$STAGE/README-Windows.txt" \
  -DICONS_DIR="$PWD/src-tauri/icons" \
  -DOUT_FILE="$PWD/$INSTALLER" \
  -DVERSION="$VERSION" \
  -DEST_SIZE_KB="$SIZE_KB" \
  scripts/windows-installer.nsi
mv "$STAGE/README-Windows.txt" "$NOTES"
rm -rf "$STAGE"

echo "== checksums =="
python3 - "$VERSION" <<PY
import hashlib, pathlib, sys
version = sys.argv[1]
artifacts = pathlib.Path("artifacts")
preferred = [
    f"Shift-{version}-macOS-universal.dmg",
    f"Shift-Setup-{version}-win-x64.exe",
    "README-Windows-x64.txt",
    "README-macOS-universal.txt",
]
names = [name for name in preferred if (artifacts / name).is_file()]
lines = [f"{hashlib.sha256((artifacts / name).read_bytes()).hexdigest()}  {name}" for name in names]
(artifacts / "SHA256SUMS.txt").write_text("".join(line + "\n" for line in lines), encoding="utf-8")
print("\n".join(lines))
PY

echo "== result =="
ls -lh artifacts/
