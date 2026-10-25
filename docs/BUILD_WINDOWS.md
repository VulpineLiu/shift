# Windows build

## Requirements

- Windows 10/11 x64
- Visual Studio Build Tools with the Desktop development with C++ workload
- WebView2 Runtime (included with current Windows releases)
- Node.js 20+ and pnpm 8+
- Rust stable through rustup
- FFmpeg on `PATH` for screen recording and media conversion
- Pandoc and a Pandoc PDF engine (Typst is recommended) for document conversion
- Poppler (`pdftoppm`) on `PATH` for PDF-to-image and PDF-to-Word rendering
- ImageMagick (`magick`) if HEIC/AVIF export is required

These tools are optional at install time. Settings -> **Runtime components**
detects them on the current machine and shows their version and resolved path.
Missing Pandoc, Typst, FFmpeg, Poppler and ImageMagick components have an
**Install** button that runs WinGet without blocking the UI, then checks again
automatically. This is the same flow as the Homebrew-backed runtime installer
on macOS.

## Build

```powershell
pnpm install
pnpm check
pnpm build
cd src-tauri
cargo check
cd ..
pnpm tauri build
```

The installer is written below `src-tauri\target\release\bundle`.

## Release packages from macOS

`scripts/build-windows-release.sh` cross-compiles the EXE and produces both
Windows artefacts without a Windows host:

```bash
brew install makensis
scripts/build-windows-release.sh              # both architectures
scripts/build-windows-release.sh --arch arm64 # one of them
scripts/build-windows-release.sh --skip-build # repackage the EXEs already built
```

For each architecture asked for it writes
`artifacts/Shift-Setup-<version>-win-<arch>.exe` (NSIS, per-user, no administrator
rights) and `artifacts/Shift-Portable-<version>-win-<arch>.zip`, then refreshes
`artifacts/SHA256SUMS.txt`. Before packaging an EXE the script checks its PE machine
field — `0x8664` for x64, `0xAA64` for ARM64 — so a binary built for the wrong target
cannot end up in the wrong package, and each installer refuses a machine of the other
architecture. `scripts/windows-installer.nsi` installs into
`%LOCALAPPDATA%\Programs\Shift`, and its uninstaller only removes what the installer
wrote.

The ARM64 build is cross-compiled and packaged from macOS, but it has not been run on
Windows-on-ARM hardware, so treat it as unverified on the device.

## Runtime behavior

- Hold `Shift` while starting a file drag, then move past the Windows drag threshold. Shift places a transparent radial window under the pointer and receives the native OLE drop.
- Hold `Alt + Shift` for the tool ring.
- Global shortcuts use `Alt + Shift + 2`, `Alt + Shift + 3`, and `Alt + Shift + 4`.
- The first screen capture uses the same local-only processing paths as macOS; no cloud service is involved.
- Revealing a result uses `SHOpenFolderAndSelectItems`, so an Explorer window
  already showing that folder is reused. `explorer /select,…` opens a fresh
  window for every conversion and is not used.

## Cross-compile check from macOS

The Windows-specific Rust code can be type-checked from macOS:

```bash
rustup target add x86_64-pc-windows-msvc
PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/opt/homebrew/opt/llvm/bin:$PATH" \
  RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin \
  cargo check --target x86_64-pc-windows-msvc
```

Prepending the rustup toolchain is required: a Homebrew `cargo` earlier on
`PATH` ignores `RUSTUP_TOOLCHAIN` and cannot find the Windows standard library.
This proves `cfg(windows)` code compiles. Interactive drag/drop and FFmpeg
recording still need a real Windows host for runtime QA.

## Cross-build a Windows EXE from macOS

```bash
cargo install cargo-xwin --locked
cd src-tauri
PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" \
  RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin \
  cargo xwin build --target x86_64-pc-windows-msvc --release --features tauri/custom-protocol
```

`--features tauri/custom-protocol` is not optional. Tauri derives `cfg(dev)` from the
absence of that feature, and a dev binary points every window at `build.devUrl`
(`http://localhost:1420`) instead of the bundled assets, so it opens a blank window on
any machine that is not running the Vite dev server. The release script verifies that
the current `dist/assets/*` files and UI strings are embedded before it creates the
installer.

The executable is written to `src-tauri/target/x86_64-pc-windows-msvc/release/shift.exe`.
It needs the Microsoft WebView2 Runtime on the target machine. FFmpeg, Poppler and Pandoc remain optional external tools for recording, PDF rendering, PDF-to-Word and document conversion.

## Runtime components

The Windows build uses these WinGet package IDs:

| Component | Package |
| --- | --- |
| Pandoc | `JohnMacFarlane.Pandoc` |
| FFmpeg | `Gyan.FFmpeg` |
| Poppler `pdftoppm` | `oschwartz10612.Poppler` |
| ImageMagick `magick` | `ImageMagick.ImageMagick` |
| Typst PDF engine | `Typst.Typst` |

WinGet runs with:

```text
install --id <package> --exact --silent --accept-package-agreements --accept-source-agreements
```

Some packages may show a Windows UAC prompt. If WinGet is missing, the
component panel links to Microsoft's official setup guide instead of downloading
an executable from a third-party source.

Shift checks the normal `PATH`, WinGet Links and Packages directories, the
standard Pandoc/FFmpeg/ImageMagick install locations, Chocolatey and Scoop
locations. This lets tools installed while Shift is running become available
after a refresh without restarting the app.

Build the frontend for the right engine first, otherwise Vite emits Safari 13 output:

```bash
TAURI_ENV_PLATFORM=windows pnpm build
```

Interactive drag/drop, installer shortcuts and recording still need a manual pass on
a real Windows machine. The release script itself can run unattended; it does not
replace that runtime QA.
