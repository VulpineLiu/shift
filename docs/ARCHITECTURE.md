# Architecture

Shift is a single locally running desktop process. There is no application
backend, account service or cloud database. Rust owns native work, durable
state and process boundaries; React owns presentation and transient interaction
state.

This document describes the current shape. Behavioural requirements live in
[`SPECIFICATION.md`](SPECIFICATION.md), security boundaries in
[`SECURITY.md`](SECURITY.md), and historical failure constraints in
[`ENGINEERING-NOTES.md`](ENGINEERING-NOTES.md).

## Process and window model

The Tauri process hosts one webview asset bundle. `src/App.tsx` selects one of
eight view kinds from the `view` query parameter:

| View | Window label | Responsibility |
| --- | --- | --- |
| `main` | `main` | File picker, drop targets, recent library, settings, first-run checks |
| `radial` | `radial` | Transparent always-on-top format/tool ring |
| `capture` | `capture` | Frozen-screen overlay and area selection |
| `recorder` | `recorder` | Recording timer and stop control |
| `editor` | `editor` | Image annotation, video player, read-only text preview, OCR |
| `result-card` | `result-*` | Post-capture actions; one window per capture |
| `share-notice` | `share-*` | Incoming-transfer consent card |
| `pin` | `pin` | Always-on-top image reference |

Closing the main, editor, radial or pin window hides it rather than exiting the
app. Tray and global-shortcut actions can bring these surfaces back. A second
instance is routed to the existing copy by `tauri-plugin-single-instance`; only
one process owns the global shortcuts and, on Windows, the shared discovery
port.

## State ownership

Rust-owned runtime state is registered as managed Tauri state:

- `AppState`: capture session, recording child, editor session, radial drag
  context, native drag bridge process and capture-window restoration list.
- `ShareState`: local beacon, bounded peer table, pending consent offers,
  accepted batch windows and the discovery enabled flag.
- `HistoryState`: the in-memory recent-file list, persisted to
  `app_data_dir/history.json`.
- `UpdateState`: last update status and whether the last signature check
  passed.

Settings are persisted as one JSON read-modify-write document at
`app_config_dir/settings.json`. Writes use a process-wide mutex and an atomic
replace (temporary file then rename). History is a convenience cache: its write
is a single replace and a corrupt or missing file falls back to an empty list.

Frontend state is not authoritative for file output, history, downloads or
editing results. A renderer can be hidden and recreated; durable state remains
in Rust or on disk.

## Module boundaries

| Module | Owns |
| --- | --- |
| `files.rs` | Extension classification, previews, conversion/tool catalogue execution, collision-safe output names, editor data commands, external process helpers |
| `pdf.rs` | PDF merge/split/rotate/text/render/compress and PDF-to-DOCX packaging |
| `office.rs` | DOCX image-page extraction, PPTX text/tables and spreadsheet parsing |
| `markdown.rs` | Bundled Markdown-to-HTML fallback |
| `capture.rs` | Screen permission checks, monitor selection, screenshot capture, crop and result handoff |
| `recording.rs` | `screencapture`, FFmpeg and `wf-recorder` backends, stop/finalization |
| `drag_bridge.rs` | Native drag detection, ring mode and radial context |
| `editor.rs` | Editor session and pinned-image window state |
| `history.rs` | Recent-file merge, capacity, pinning, persistence and events |
| `settings.rs` | Settings schema, output directories, write checks, app language |
| `share.rs` | LAN discovery, peer table, SHSF transfer protocol and consent |
| `update.rs` | Manifest fetch/parse/signature, download, integrity gate and installer handoff |
| `result_card.rs` | Post-capture card lifecycle and auto-close |
| `runtime.rs` | External component detection and installation instructions |
| `process.rs` | Spawn helper that suppresses console windows on Windows |

`pdf.rs`, `office.rs` and `markdown.rs` deliberately have no Tauri window
management. They operate on paths and values so they can be tested without a
live app handle.

## Core data flows

### File action

1. A drag, picker or recent row chooses one action and one or more source paths.
2. `RadialMenu.tsx` calls `perform_file_action` with a generated `jobId`.
3. Rust runs `files::perform_action` off the UI thread.
4. Progress may arrive as `app://file-action-progress`; completion arrives as
   `app://file-action-finished` with either an `ActionOutcome` or an error code.
5. New paths are recorded in `history.json`, broadcast on `history://changed`
   and announced through `app://file-result`.
6. The UI reveals results only if the auto-reveal setting is enabled.

Source files are opened read-only. Every output goes through `unique_output` or
`unique_output_in`, which append ` 2`, ` 3`, ... instead of replacing an
existing file.

### Capture and recording

1. `start_capture` checks screen-capture permission and selects the monitor
   under the pointer.
2. The screen is captured to a temporary file, Shift's own windows are hidden,
   and the `capture` overlay is shown with the frozen frame.
3. A crop is written through the same collision-safe naming path as any other
   result. Full-screen capture skips the overlay.
4. A result card window is created; if it cannot be created, the editor opens
   instead so the capture still has an action surface.
5. Recording is a separate child process. Stopping it finalizes the file,
   restores hidden windows and hands the recording to the editor/card flow.

### Editor

Images are read by Rust and returned as a data URL so the canvas can export
without cross-origin taint. Annotation happens in the renderer; saving returns
the composed PNG to Rust, which decodes it, checks limits and writes a new file.
Text files are read by Rust and shown in a read-only `<pre>`. Videos are loaded
through the asset protocol and played by the webview.

The editor does not write back to the source. `Copy` and `Save As` remain
separate operations.

### OCR

Image OCR runs in the renderer through Tesseract.js/WASM. The worker, core and
English/Simplified Chinese models are local files under `public/tesseract/`.
PDF OCR is a Rust-to-renderer loop: Rust renders each PDF page into a cache
folder, the renderer recognizes page images, then Rust writes the combined text
result and the cache folder is cleaned up.

No OCR path uploads a file or downloads a model at runtime.

### LAN sharing

One long-lived UDP discovery thread sends multicast, directed broadcast,
limited broadcast and a bounded unicast sweep. Peers are kept in a table capped
at 256 entries and expire after 15 seconds without a beacon. The table emits
`share://peers` only when its visible signature changes.

Transfers use a direct TCP connection to the port advertised in the beacon:

```text
"SHSF" | 4-byte big-endian header length | JSON header | 0/1 decision | body | ack
```

The receiver writes to a temporary `.shift-part` file and renames only after the
exact declared byte count has arrived. The sender copies exactly the header's
declared size, so a source file that grows cannot bleed bytes into the ack.
Inbound offers prompt once; later files in the same accepted batch use a
two-minute trust window.

The protocol has no authentication or encryption. Consent prevents automatic
writing, not sender impersonation. See [`SECURITY.md`](SECURITY.md).

### Updates

1. The manifest URL is derived at build time from the git remote, or overridden
   with `SHIFT_UPDATE_URL`.
2. The manifest is fetched, capped at 256 KiB, parsed as `schema: 1`, and each
   named artifact must be a safe basename with a nonzero size and SHA256.
3. If a public key is baked in, the detached `shift-update.sig` must verify over
   the manifest's exact bytes.
4. The artifact is downloaded to the temp update directory, then its exact size
   and SHA256 are checked.
5. Before installing, both checks run again; a failure deletes the artifact.
6. macOS and Windows hand the installer to a detached helper only after Shift
   exits. Paths are process arguments, never interpolated script text.

## Events

Events are grouped by owner and are not a replacement for durable state.

| Event | Producer | Purpose |
| --- | --- | --- |
| `app://error` | Rust | User-visible native failure |
| `app://file-action-progress` | `files.rs` | Job-specific progress |
| `app://file-action-finished` | `files.rs` | Success or error for one job |
| `app://file-result` | history/capture/share | New result is available |
| `capture://ready` | capture | Overlay session data |
| `editor://open` | editor | New editor session |
| `pin://show` | editor | Pinned image payload |
| `history://changed` | history | Shared recent list changed |
| `radial://*` | drag bridge | Drag context, mode, drop and dismissal |
| `recording://started` | recording | Recording session data |
| `share://peers` | share | Visible peer list changed |
| `share://transfer` | share | Sending/received/error transfer state |
| `update://progress` | update | Download state |
| `update://status` | update | Verified update or error state |

## Platform adapters

| Concern | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Drag detection | AppKit drag bridge sidecar | Win32 drag-state poll plus native drop | No global drag bridge; main-window picker only |
| Capture | `screencapture` | GDI `BitBlt` / `GetDIBits` | `grim`, fallback `gnome-screenshot` |
| Recording | `screencapture -v` | FFmpeg/X11 | `wf-recorder` on Wayland, FFmpeg on X11 |
| Document/PDF rendering | AppKit/PDFKit Swift sidecars | Poppler/Pandoc | Poppler/Pandoc |
| LAN discovery interface list | `getifaddrs` | `GetAdaptersAddresses` | `getifaddrs` |

All external processes go through `process::command`, which hides console
windows on Windows. A source-check test rejects bare `Command::new` in shipped
code.

## Invariants

- A conversion or tool never overwrites its source or an existing output.
- A visible wedge exists only because the format catalogue lists it and the
  backend has a handler.
- Failures the interface maps have stable backend codes; raw third-party
  diagnostics may pass through unchanged.
- A transfer or update is not considered complete until all declared bytes are
  present and any required hash matches.
- A helper process receives untrusted paths as arguments, never as executable
  script text.
- Resource limits are enforced before unbounded allocation where a path can
  receive large local input.

## Related documents

- [`SPECIFICATION.md`](SPECIFICATION.md): observable behaviour and acceptance.
- [`SECURITY.md`](SECURITY.md): threat model and residual risk.
- [`ENGINEERING-NOTES.md`](ENGINEERING-NOTES.md): regression constraints.
- [`RELEASE.md`](RELEASE.md): update channel and handoff.

