# Windows build

## Requirements

- Windows 10/11 x64
- Visual Studio Build Tools with the Desktop development with C++ workload
- WebView2 Runtime (included with current Windows releases)
- Node.js 20+ and pnpm 8+
- Rust stable through rustup
- FFmpeg on `PATH` for screen recording and media conversion
- Pandoc on `PATH` for document conversion
- Poppler (`pdftoppm`) on `PATH` for PDF-to-image rendering
- ImageMagick (`magick`) if HEIC/AVIF export is required

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

## Runtime behavior

- Hold `Shift` while starting a file drag, then move past the Windows drag threshold. Shift places a transparent radial window under the pointer and receives the native OLE drop.
- Hold `Ctrl + Shift` for the tool ring.
- Global shortcuts use `Alt + Shift + 2`, `Alt + Shift + 3`, and `Alt + Shift + 4`.
- The first screen capture uses the same local-only processing paths as macOS; no cloud service is involved.

## Cross-compile check from macOS

The Windows-specific Rust code can be type-checked from macOS:

```bash
rustup target add x86_64-pc-windows-msvc
PATH="/opt/homebrew/opt/llvm/bin:$PATH" \
  cargo check --target x86_64-pc-windows-msvc
```

This proves `cfg(windows)` code compiles. Interactive drag/drop and FFmpeg recording still need a real Windows host for runtime QA.

## Cross-build a Windows EXE from macOS

```bash
cargo install cargo-xwin --locked
cd src-tauri
cargo +stable xwin build --target x86_64-pc-windows-msvc --release
```

The executable is written to `src-tauri/target/x86_64-pc-windows-msvc/release/shift.exe`.
It needs the Microsoft WebView2 Runtime on the target machine. FFmpeg, Poppler and Pandoc remain optional external tools for recording, PDF rendering and document conversion.
