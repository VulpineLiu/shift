# Contributing to Shift

Shift is a Tauri 2 desktop application: React/TypeScript renderers talk to a Rust
command layer, with native macOS, Windows and Linux adapters underneath. Changes
should stay local to the behaviour being fixed; the regression ledger in
[`docs/ENGINEERING-NOTES.md`](docs/ENGINEERING-NOTES.md) records paths that must
not be simplified back into earlier failures.

## Prerequisites

- Node.js 22 (the CI baseline) and pnpm 8 or newer. The repository has no
  `packageManager` pin, so use an installed pnpm rather than assuming Corepack
  selects one.
- Current stable Rust and Cargo; the CI workflow tracks the stable channel.
- macOS: macOS 13 or newer and the Xcode Command Line Tools for the Swift
  sidecars.
- Windows: Visual Studio Build Tools with the Desktop development with C++
  workload, WebView2, and the Tauri prerequisites. See
  [`docs/BUILD_WINDOWS.md`](docs/BUILD_WINDOWS.md).
- Linux: the GTK/WebKit/AppIndicator packages listed in
  [`.github/workflows/ci.yml`](.github/workflows/ci.yml), plus `grim`,
  `gnome-screenshot`, `wf-recorder` and FFmpeg for the optional capture paths.

## Local setup

```bash
pnpm install
pnpm prepare:ocr
pnpm mac:drag-bridge   # macOS only
pnpm tauri dev
```

`pnpm prepare:ocr` refreshes the tracked Tesseract WASM and language assets
under `public/tesseract/`. They are committed so a clean checkout can build and
run OCR offline. Do not delete or replace them with remote downloads.

## Required checks

Run the checks from the repository root unless a command says otherwise:

```bash
pnpm check
pnpm test
pnpm build

cd src-tauri
cargo fmt --check
cargo test --lib
cargo clippy --all-targets -- -D warnings
cd ..

git diff --check
```

The two network-dependent Rust tests are ignored by default:

- `share::tests::two_copies_on_one_machine_discover_each_other` binds the shared
  discovery port.
- `update::tests::the_configured_feed_parses_and_downloads` reaches the update
  feed.

Run the first explicitly when working on discovery, and the second only with a
feed you are willing to download from:

```bash
cd src-tauri
cargo test share:: -- --ignored --nocapture
SHIFT_UPDATE_URL="<manifest-url>" cargo test --lib update:: -- --ignored
```

## What CI covers

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) defines macOS, Windows and
Linux jobs. They run type checks, TypeScript tests, Rust formatting and Rust
unit tests; macOS also runs Clippy and a universal app build. Nothing in this
repository proves that the current hosting forge has a runner configured.
Link an actual run before describing CI as passing.

## Change rules

- Keep the source file untouched. Every operation writes a new result through
  the collision-safe naming helpers.
- Where the interface maps a backend failure, use a stable code such as
  `errTextTooLarge` or `share-timeout`; user wording belongs in `src/i18n.ts`.
  Add both English and Simplified Chinese. Raw third-party diagnostics may pass
  through unchanged.
- Use `messageFrom()` for unknown errors and `tLive()` for listeners that are
  registered once and must follow later language changes.
- Preserve protocol compatibility. LAN beacons and transfer headers may gain
  defaulted fields, but existing field meanings must not change. Update
  manifests are `schema: 1`; reject unsupported schemas instead of guessing.
- Treat downloaded installers and LAN input as untrusted. Do not interpolate
  paths into shell or batch text, and do not trust a declared file size without
  enforcing it while copying.
- Resource limits belong at the point where bytes are read or decoded, not only
  in the UI. The editor currently caps text at 8 MiB, image files at 64 MiB and
  decoded images at 40 megapixels; document any new unbounded path or add a
  limit.
- Do not edit generated output in `dist/`, `src-tauri/target/`, `artifacts/`, or
  generated icons. Change the source and the build script that produces them.

## Documentation changes

User-visible behaviour changes require matching updates in both
[`README.md`](README.md) and [`README.zh-CN.md`](README.zh-CN.md). Architecture
changes belong in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md); observable
behaviour and limits belong in [`docs/SPECIFICATION.md`](docs/SPECIFICATION.md);
trust-boundary changes belong in [`docs/SECURITY.md`](docs/SECURITY.md).

If a change alters discovery, conversion dependencies, update handoff or macOS
packaging, append the reason, evidence and any commit reference to
`docs/ENGINEERING-NOTES.md` rather than leaving the next contributor to
rediscover it.

## Manual checks

Unit tests cannot prove gesture handling, OS permissions, installer behaviour or
cross-machine discovery. Before a release, complete the relevant matrix:

- macOS: first launch, Screen Recording and Files & Folders permissions, area
  and full-screen capture, editor save and copy, drag-to-ring, LAN receive.
- Windows: installer, firewall prompt, drag-to-ring threshold, WebView2 file
  drag paths, recording with FFmpeg, LAN receive.
- Linux: build package, `grim` / `gnome-screenshot`, Wayland `wf-recorder`,
  X11 FFmpeg fallback, Poppler-dependent paths.
- Updates: install the previous release and run the in-app handoff once on each
  shipped platform. On Windows this requires real Windows hardware.

Record which checks were run and which were not. Do not replace an unavailable
hardware check with a compile result.

## Release work

The release process, signing key handling and channel verification are in
[`docs/RELEASE.md`](docs/RELEASE.md). A release key must never enter the
repository, logs or issue attachments. Use `scripts/release-key.py` with the key
outside the working tree.

Do not commit generated artifacts or publish a `releases` branch unless the
task explicitly asks for it.

