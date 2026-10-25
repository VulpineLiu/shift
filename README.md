# Shift

**A radial file workspace for macOS and Windows. Drag a file, release over a wedge, and the conversion happens on your machine.**

**Language:** English · [简体中文](README.zh-CN.md)

Shift puts file conversion, screenshot, screen recording, annotation, and bilingual OCR behind one radial menu that appears wherever you drop a file. No uploads, no account, no cloud round-trip — every byte is processed locally.

Built on Tauri 2, Rust, React and TypeScript. Native capture and drag behaviour live behind per-platform adapters, so the radial menu, editor, and format catalogue are identical everywhere.

---

- [What it does](#what-it-does)
- [Quick start](#quick-start)
- [Supported files](#supported-files)
- [Conversions](#conversions)
- [Tools](#tools)
- [Compression, naming, cancellation](#compression-naming-cancellation)
- [Capture & annotation](#capture--annotation)
- [OCR](#ocr)
- [Send to a device](#send-to-a-device)
- [Shortcuts](#shortcuts)
- [Tray menu](#tray-menu)
- [Output locations](#output-locations)
- [External dependencies](#external-dependencies)
- [Platform status](#platform-status)
- [Build from source](#build-from-source)
- [Architecture](#architecture)
- [Repository layout](#repository-layout)
- [Privacy](#privacy)
- [Known limitations](#known-limitations)
- [FAQ](#faq)
- [Contributing](#contributing)
- [License](#license)

---

## What it does

**The radial menu is the whole interface.** Hold `Shift` and drag a file in Finder (or `Shift + Alt` on Windows) and a ring opens under the cursor. Release over a format to convert, or add `Option`/`Alt` to switch to the tool ring. The main window accepts drops too, and its `Convert files` / `File tools` buttons open the same rings for files you pick yourself.

**Everything is type-aware.** The ring only offers what the selected file can actually become, and never repeats the source format — an image can target PNG/JPG/WEBP/HEIC/TIFF/BMP/GIF/PDF except its current format, a PPTX offers PDF and Markdown, and a `.key` or `.pages` file offers no conversion wedges at all instead of failing later. The centre shows a glass preview with the file's dimensions and size; a multi-file selection renders a stacked icon deck.

**Batch by default.** Select ten files, drag, and one gesture produces ten outputs. Results land next to each source with a descriptive suffix (`report (compressed).pdf`, `photo (rotated).png`) and never overwrite — a numeric suffix is appended on collision. Settings can collect every result into one folder instead.

**Capture is built in.** Frozen-screen area capture, full-screen capture, and area recording with a floating timer, all aimed at the monitor under the pointer rather than always the primary display. Screenshots open straight into the annotation canvas and can be pinned as always-on-top reference windows.

**OCR runs on-device.** English and Simplified Chinese, via a bundled Tesseract WASM engine with the traineddata shipped inside the app. No network access at any point in the OCR path.

**Files can hop to another machine.** Every running Shift announces itself on the local network, so the recent list can hand a file to another computer the way AirDrop does — tap a device name, watch the progress bar, and the file lands in Shift's output folder on the other side.

## Quick start

Shift lives in the menu bar (macOS) and the notification area (Windows), and it also has a normal window. There is no required window to keep open — the rings work from anywhere.

**Convert a file (Shift).**

1. Find the file in Finder or File Explorer.
2. Press and hold on it and start dragging, keeping the mouse button down.
3. While dragging, hold `Shift`. The ring appears around the pointer.
4. Drag toward the format you want — the wedge fills in and grows, and its name shows in the centre — then release. You only have to head in its direction; precision is not required.
5. The status pill reports `Saved N files.` and the result sits next to the original.

> 💡 You can press `Shift` after the drag begins. Once the ring is up you may let go of `Shift`; it stays until you release the mouse.
> 💡 Changed your mind? Release back near the centre (inside the 92 px inner radius) and nothing happens.
> 💡 Near a screen edge the ring slides back on screen; the whole 760 px ring is clamped inside the monitor under the pointer.

**Use a tool (`Option + Shift`, or `Alt + Shift` on Windows).** Same gesture, and the wedges become tools for that file type instead of formats. While the ring is open, keep `Shift` held and press or release `Option`/`Alt` to flip between the format ring and the tool ring.

**Prefer not to drag?** In the main window, **Convert files** and **File tools** open a picker and then show the same ring for what you chose; **OCR** picks an image and reads it. Dropping files straight onto the main window opens the action ring directly. Drop them **onto a button** instead and that button lights up and runs its own action: **Convert files** opens the format ring, **File tools** the tool ring, **OCR** reads the image.

**Several files at once.** Multi-select with `⌘`/`Ctrl`-click, then drag. Every file is processed. With 2+ images or 2+ PDFs selected, a **Merge** wedge appears that combines them into a single PDF.

**First run on macOS.** Grant Screen Recording for screenshots and recording, and Files & Folders access for the output locations you choose. Capture checks permission before showing the overlay, so a denial produces an actionable error rather than a black screenshot.

## Supported files

Recognition is by extension, against four tables in `src-tauri/src/files.rs`. The `kind` drives the centre icon and which wedges appear. Bold extensions are the ones that actually convert; the rest are recognised and get Open, Reveal, or an export-first message.

| Kind | Extensions |
| --- | --- |
| `image` | **`png` `jpg` `jpeg` `webp` `tiff` `tif` `bmp` `gif` `heic` `heif` `avif`** |
| `video` | **`mp4` `mov` `m4v` `avi` `mkv` `webm`** |
| `audio` | **`mp3` `wav` `m4a` `aiff` `aif` `caf` `flac` `ogg`** |
| `pdf` | **`pdf`** |
| `document` | **`docx` `rtf` `odt` `txt` `md` `html` `htm`** `doc` `pages` |
| `presentation` | **`pptx`** `ppt` `key` |
| `spreadsheet` | **`xlsx` `xls` `xlsb` `ods` `csv` `tsv`** `numbers` |
| `other` | anything unrecognised — Open and Reveal only |

Decoding and encoding are not symmetric, and the app does not pretend otherwise:

- **Image decode** is native (Rust `image`) for PNG, JPEG, WebP, TIFF, BMP and GIF. HEIC/HEIF/AVIF first decode through the system codec — `sips` on macOS, ImageMagick elsewhere — and can then convert to every offered image target, including WEBP, or to PDF. The pixel tools still need a natively readable input, so convert those formats first before rotate, flip, B&W, 50% or compress.
- **Image encode** for PNG, JPG, WebP, TIFF, BMP and GIF is native. HEIC export uses `sips` on macOS and requires ImageMagick on Windows/Linux. AVIF is accepted as an input but is not offered as an export target.
- **Media** decode and encode are whatever your FFmpeg build supports; the menu offers MP4, MOV, GIF and M4A for video, and M4A, WAV and MP3 for audio.
- **Office** files are parsed in pure Rust (`calamine` for spreadsheets, `zip` + `quick-xml` for PPTX), so no Office installation is needed.
- **Recognised but not converted:** `.doc`, `.pages`, `.ppt`, `.key` and `.numbers` get their proper icon plus Edit, Open and Reveal, but no format wedges. Export them to DOCX, PPTX or XLSX first.

## Conversions

The format ring for a given file type. Every target below is wired in `src-tauri/src/files.rs`; the source's own format is filtered out at runtime.

| Source | Ring targets (source format excluded) | Engine | Notes |
| --- | --- | --- | --- |
| Image | `PNG` `JPG` `WEBP` `TIFF` `BMP` `GIF` | Rust `image` | Lossless path when the codec allows it |
| Image | `HEIC` | `sips` / ImageMagick | macOS native; other OS need `magick` |
| Image | `PDF` | `printpdf` | One page per image, sized at 300 dpi |
| Video | `MP4` `MOV` | FFmpeg | `libx264`, falling back to `h264_videotoolbox` then `mpeg4` |
| Video | `GIF` | FFmpeg | First 15 s only, 12 fps, 480 px wide, 8 MB ceiling — capped so a long recording cannot produce a 100 MB GIF |
| Video | `M4A` | FFmpeg | Audio track only, 192k AAC |
| Audio | `M4A` `WAV` `MP3` | FFmpeg | MP3 falls back to `lame` if FFmpeg lacks `libmp3lame` |
| PDF | `TXT` `MD` | `lopdf` | Text-layer extraction; errors clearly when a PDF is scanned |
| PDF | `DOCX` | Page rendering + native DOCX packaging | PDFKit on macOS, Poppler on Windows/Linux; preserves images, diagrams and page layout without Microsoft Word |
| PDF | `RTF` | `lopdf` + Pandoc | Text layer becomes the document body; needs Pandoc |
| PDF | `HEIC` | PDFKit / Poppler + `sips` | Pages render as PNG then transcode; needs a system HEIC codec |
| PDF | `PNG` `JPG` `TIFF` | PDFKit / Poppler | One file per page into a `… pages` folder |
| Document `docx` `rtf` `odt` `txt` `md` `html` | `PDF` | AppKit renderer (macOS) · Pandoc elsewhere | macOS path needs no LaTeX or wkhtmltopdf |
| Document `docx` `rtf` `odt` `txt` `md` `html` | `DOCX` `RTF` `HTML` `TXT` `MD` | Pandoc | The source format is omitted; Markdown writes as GFM. HTML is a self-contained file with embedded images |
| Presentation `.pptx` | `MD` `PDF` | Pure Rust + Pandoc | Per-slide headings, bullet text and **tables as real Markdown tables**; PDF is a text summary, not a layout render |
| Spreadsheet | `MD` `PDF` | `calamine` / Rust | One Markdown table per sheet; `csv`/`tsv` parsed directly |

Anything not in this table is not offered — the ring is built from this catalogue at runtime, so a wedge you can click is a wedge that works. If a dependency is missing, the wedge names it instead of failing silently.

**Page progress follows one rule.** Conversions that can enumerate real pages report the same `Converting page N/M…` event. Image-only DOCX→PDF and image-bearing PDF→DOCX use that path. One-shot Word→PDF layout and text-only PDF→DOCX cannot expose a reliable page count, so they show `Processing…` rather than a fabricated percentage.

> Note: the conversion ring never repeats the source format. For example, a DOCX offers no DOCX target, avoiding a needless full rewrite that can lose layout.
> Scanned/image-only PDFs have no text layer, so `TXT`/`MD` return a clear error — use **OCR** on those, which renders each page first.

## Tools

The tool ring (`Option + Shift` while dragging). These are one-gesture operations, not conversions.

| Tool | Applies to | Output | What it does |
| --- | --- | --- | --- |
| Compress | image, video, audio, PDF | `.jpg`/`.png`, `.mp4`, `.m4a`, `.pdf` | Images re-encode at q68 (PNG kept when alpha); video at CRF 28; audio to 128k AAC; PDF images resampled to ≤1440 px at q60 JPEG, then objects pruned |
| Metadata | image, PDF | same type, ` (clean)` | Rewrites the image with no EXIF; strips the PDF `/Info` and `/Metadata` entries |
| Rotate | image, PDF | same type, ` (rotated)` | 90° clockwise, in place for the format |
| Flip | image | same type, ` (flipped)` | Horizontal mirror |
| B&W | image | same type, ` (bw)` | Grayscale conversion |
| 50% | image | same type, ` (50%)` | Half width and height, Lanczos3 |
| Crop | image | `.png`, ` (cropped)` | Radial crop opens the editor for an interactive selection |
| Cutout | image | — | Opens the editor, which keys out the sampled background colour |
| Merge | 2+ images | `.pdf`, ` (images)` | One page per image, in selection order |
| Merge | 2+ PDFs | `.pdf`, ` (merged)` | Concatenates in selection order |
| Split | PDF | `Page 001.pdf` … | One PDF per page in a `… pages` folder |
| Extract text | PDF | `.txt` | Text layer only; use OCR for scans |
| 720p | video | `.mp4`, ` (720p)` | Scales to 720p height, keeps aspect |
| Mute | video | same type, ` (muted)` | Drops the audio track, video stream copied without re-encode |
| Frame | video | `.png`, ` (frame)` | First frame as a still |
| Extract audio | video | `.m4a` | 192k AAC |
| Mono | audio | `.m4a`, ` (mono)` | Downmix to single-channel 128k AAC |
| Plain | document | `.txt` | Plain-text dump via Pandoc |
| OCR | image, PDF | `.txt`, ` (OCR)` | See [OCR](#ocr) |
| Edit / Annotate / Redact / Add BG | image | — | Opens the editor with the matching tool preselected |
| Open · Reveal | any file | — | Hands off to the system default app or file manager |

## Compression, naming, cancellation

**What Compress actually does.**

| Input | Behaviour |
| --- | --- |
| Image | Re-encoded at quality 68. Images with transparency are written as PNG, everything else as JPG |
| PDF | Embedded images above 1440 px are resampled to q60 JPEG, then objects are pruned, renumbered and saved compressed. If the result is not smaller, nothing is written |
| Video | Re-encoded at CRF 28 / 2M target, AAC 96k audio, into MP4 |
| Audio | To 128k AAC M4A |

**Output naming — nothing is ever overwritten.** Results land next to the source with a descriptive suffix, and a collision appends ` 2`, ` 3`, … rather than replacing anything.

```text
report (compressed).pdf      photo (rotated).png     slide (bw).png
report (clean).pdf           photo (50%).png         report (merged).pdf
```

PDF→image and PDF split write into a new `Name pages` folder containing `Page 001.png`, `Page 002.png`, … A second run makes `Name pages 2`, also never overwriting.

**Cancel a gesture.** Release near the centre of the ring, or outside the ring entirely, and no action runs.

## Capture & annotation

**Capture.** Area capture paints a frozen still so the pixels you select are the pixels you get; full-screen capture skips the overlay entirely. Recordings save as MOV on macOS via `screencapture -v` and MP4 elsewhere via FFmpeg, with a floating timer and stop control. Linux capture prefers `grim` and falls back to `gnome-screenshot`.

**Annotation.** The editor canvas works at native resolution and writes a new PNG, so the source file is never modified.

| Tool | Behaviour |
| --- | --- |
| Rectangle, Arrow, Highlight, Text | Freehand placement, colour and weight per annotation |
| Redact | Solid black bar, burnt into the output — not an overlay |
| Pixelate | Real mosaic sampling of the region underneath, not a translucent fill |
| Number | Auto-incrementing callout badges |
| Crop | Interactive crop with an explicit apply step |
| Add BG | Rounded-corner padding with a gradient mat, for social and deck shots |
| Cutout | Samples the four corners, averages the background, keys it out to transparency |
| Undo | Pops annotations first, then image-level operations |
| Save copy / Copy | New file with a suffix, or straight to the clipboard |
| Pin | Floats the image as an always-on-top window with copy and close |

**Editor.** Images open the annotator; video opens a player; `txt` `md` `csv` `tsv` `html` `htm` open as editable text. Every editor host has Copy, Save As, Open, Reveal in Folder, OCR and Copy Text.

## OCR

Tesseract.js with the LSTM core, running as WASM inside the app. `eng` and `chi_sim` traineddata are copied into `public/tesseract` at build time by `pnpm prepare:ocr`, so recognition works with no network and no download on first run.

- **Images** — one pass, live progress, text goes to the editor or the clipboard.
- **Scanned PDFs** — pages are rendered to images first, then OCR'd sequentially with per-page progress, and the results are written to a ` (OCR)` text file with `--- Page N ---` separators.
- **Text PDFs** — use Extract Text instead; it reads the embedded layer directly and is far faster.

## Send to a device

Shift keeps a live list of the other Shift copies on your network and uses it for an AirDrop-style transfer. Hover any row in **Recent** and press the send button, or open a file's **Tools** ring and release over **Send** — either way a picker opens with one card per device, and clicking a card starts the transfer. The Tools ring offers Send for every file type, and selecting several files sends them as one batch.

Nothing lands without consent: the receiving machine shows a small card naming the sender and the file, with **Accept** and **Decline** and a countdown. Declining, closing the card, or letting it expire stops the transfer before a single file byte moves. An accepted batch is asked once, then the remaining files of that batch follow on their own. Accepted files are written to Shift's output folder, appear in that machine's Recent list, and a note names the sender. Names that already exist get ` 2`, ` 3` appended, and a partial transfer never leaves a half-written file behind.

How it finds peers, with no server and no configuration:

- Every copy sends a small UDP beacon every two seconds to multicast group `239.255.77.88:47888`, plus a legacy broadcast copy for networks that drop multicast. Beacons carry the machine name, OS, and the ephemeral TCP port that will receive files.
- A beacon that has not been repeated for nine seconds drops the device from the list, so switching networks clears stale entries by itself.
- File bytes move over a direct TCP connection: a `SHSF` handshake, a JSON header, the payload, then a one-byte acknowledgement, so a sender only reports success once the receiver has the whole file on disk.

Only the local network is involved — nothing reaches the internet. The receiver listens on all interfaces, so a macOS firewall prompt appears the first time Shift shares; allowing it enables both directions.

## Shortcuts

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| Capture area | `⌥⇧2` | `Alt+Shift+2` |
| Record area | `⌥⇧3` | `Alt+Shift+3` |
| Capture full screen | `⌥⇧4` | `Alt+Shift+4` |
| Format ring while dragging | hold `⇧` | hold `Shift` |
| Tool ring while dragging | `⌥⇧` | `Alt+Shift` |

The three capture shortcuts are registered globally. The drag rings are modifier-driven: begin dragging with the modifier already held and the ring appears under the cursor without Shift needing focus.

## Tray menu

The menu bar (macOS) and notification area (Windows) icon exposes the actions you would otherwise reach through the window:

| Item | Shortcut | Meaning |
| --- | --- | --- |
| Open Shift | — | Show the main window |
| Capture area | `Alt+Shift+2` | Start an area screenshot |
| Capture full screen | `Alt+Shift+4` | Grab the monitor under the pointer |
| Record area | `Alt+Shift+3` | Start, or stop, an area recording |
| OCR an image… | — | Pick an image and read its text locally |
| Quit Shift | `Cmd/Ctrl+Q` | Exit; an in-flight recording is finalised first |

Tray labels follow the selected app language; native macOS permission prompts remain in English.

Settings → **Behaviour** holds two switches: **Drag rings** (master enable for the Shift-drag gesture) and **Launch at login**.

## Output locations

| Result | Default |
| --- | --- |
| Screenshots | `~/Pictures/Shift` |
| Recordings | `~/Movies/Shift` |
| File actions | beside the source file |

Each is overridable in Settings. File actions offer a *collect everything in one folder* mode in place of beside-the-source. The Settings panel lists every resolved path, checks that it is actually writable, reveals it in the file manager, and deep-links to the macOS Screen Recording and Files & Folders privacy panes when access is missing.

## External dependencies

Core conversion, radial menu, capture, and OCR work with nothing installed. These extend coverage:

| Tool | Needed for | macOS | Windows | Linux |
| --- | --- | --- | --- | --- |
| FFmpeg | media convert/compress, 720p, mute, frame, mono; recording off-macOS | optional | **required** for recording | **required** |
| Pandoc | document ↔ DOCX/RTF/HTML/TXT/MD (ODT input only) | optional | needed | needed |
| Typst | Pandoc document-to-PDF engine | not needed (AppKit) | needed | needed |
| Poppler `pdftoppm` | PDF → images, PDF → Word, scanned-PDF OCR | not needed (PDFKit) | needed | needed |
| ImageMagick `magick` | HEIC/HEIF/AVIF input plus HEIC output | not needed (`sips`) | needed | needed |
| LAME | MP3 encode when FFmpeg lacks `libmp3lame` | rare | rare | rare |

Settings -> **Runtime components** shows the tools found on the current machine,
their versions and paths. Missing Pandoc, Typst, FFmpeg, Poppler and ImageMagick can be
installed from the same panel through Homebrew on macOS or WinGet on Windows,
then detected again automatically. Tools are also found on `PATH` and their
standard install locations at call time, so a missing dependency degrades one
wedge rather than the app.

## Platform status

| Capability | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Radial menu, editor, tray, i18n | Yes | Yes | Yes |
| Drag-to-ring | AppKit global drag bridge | Win32 drag-state poll + native drop | none yet — main-window picker only |
| Area / full-screen capture | `screencapture` | PowerShell | `grim` / `gnome-screenshot` |
| Monitor under cursor | Yes | Yes | `grim` output selection |
| Area recording | `screencapture -v` | FFmpeg | planned |
| Image convert and edit | Yes | Yes | Yes |
| HEIC / AVIF encode | `sips` | ImageMagick | ImageMagick |
| PDF text, merge, split, rotate, compress | Yes | Yes | Yes |
| PDF → images | PDFKit sidecar (+ `sips` for HEIC) | Poppler (+ ImageMagick for HEIC) | Poppler (+ ImageMagick for HEIC) |
| PDF → Word | Built-in page renderer | Poppler required | Poppler required |
| Scanned-PDF OCR | Yes | Poppler required | Poppler required |
| Document → PDF | bundled AppKit renderer | Pandoc + a PDF engine | Pandoc + a PDF engine |
| PPTX / XLSX / ODS → Markdown | Yes (pure Rust) | Yes | Yes |
| Local EN + ZH OCR | Yes | Yes | Yes |

## Build from source

Requires Node.js 20+, pnpm 8+, Rust 1.90+, and macOS 13+ with Xcode Command Line Tools for the native macOS sidecars.

```bash
pnpm install
pnpm mac:drag-bridge   # macOS only — AppKit drag, PDFKit and document renderers
pnpm tauri dev
```

`pnpm mac:drag-bridge` compiles `native/macos/*.swift` into per-architecture and universal sidecars under `src-tauri/binaries`. Windows and Linux skip it entirely.

```bash
pnpm check          # TypeScript, no emit
pnpm test           # conversion matrix and drag-target tests
pnpm build          # tsc + vite
cargo test --lib    # Rust unit tests, from src-tauri/
pnpm tauri build    # release bundle
```

Universal macOS build:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm mac:drag-bridge
PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" \
  RUSTUP_TOOLCHAIN=stable \
  pnpm tauri build --target universal-apple-darwin --bundles app,dmg
```

The result is one universal Mach-O including universal drag, PDF-render and doc-render sidecars. To produce the Windows installer and portable ZIP from macOS, run `scripts/build-windows-release.sh`; full details are in `docs/BUILD_WINDOWS.md`.

**Permissions (macOS).** Screen Recording for capture, Files & Folders for the output folders you choose, Microphone when recording with audio. Shift checks Screen Recording before opening the overlay, so a denial surfaces an actionable error rather than a black screenshot. Dev builds signed with a local certificate keep those grants stable across rebuilds; distribution needs Developer ID plus notarization.

## Architecture

```text
React UI                     one webview, six views
  ├─ main        drop target, shortcuts, recent outputs, language, settings
  ├─ radial      transparent always-on-top ring, hit-tested by polar sector
  ├─ capture     frozen still + area selection
  ├─ recorder    floating timer and stop control
  ├─ editor      annotation canvas, text, video, OCR
  └─ pin         always-on-top image reference

Tauri command layer          src-tauri/src/
  ├─ files.rs      inspect, convert, compress, metadata, rotate, crop, media
  ├─ pdf.rs        merge, split, rotate, text, render, image→PDF, compress
  ├─ office.rs     PPTX and spreadsheet → Markdown, pure Rust
  ├─ capture.rs    full-screen capture, region crop, editor handoff
  ├─ recording.rs  screencapture / FFmpeg, with interrupt-based finalization
  ├─ drag_bridge.rs  global drag detection, ring mode, radial positioning
  ├─ settings.rs   output directories, write checks, privacy deep links
  ├─ share.rs      LAN beacons, peer list, AirDrop-style file transfer
  └─ editor.rs     editor and pin window state

Platform adapters
  ├─ macOS    AppKit drag bridge · PDFKit renderer · AppKit doc renderer · screencapture
  ├─ Windows  Win32 drag state  · PowerShell capture · FFmpeg · Poppler
  └─ Linux    grim / gnome-screenshot · FFmpeg · Poppler

OCR: Tesseract.js WASM + bundled eng/chi_sim, in-process, offline
```

The radial hit-test is pure geometry — one polar calculation shared by native pointer events, webview hover, and the Windows drop payload — so ring behaviour does not drift between platforms.

## Repository layout

```text
src/                 React UI, i18n (en/zh), Tauri API bindings
src/lib/             pure helpers and Node tests, including the conversion matrix
src-tauri/src/       Rust command layer and platform adapters
native/macos/        Swift sidecars: drag bridge, PDF renderer, doc renderer,
                     compiled into src-tauri/binaries/ by pnpm mac:drag-bridge
scripts/            OCR staging, macOS sidecars, Windows installer and release builds
public/tesseract/    WASM core + traineddata, generated, not committed
assets/              app icon source
src-tauri/icons/     generated app icons per platform
docs/                Windows build guide, roadmap
.github/workflows/   macOS and Windows CI
FileFlipper/         separate companion project, its own repository
```

## Privacy

Shift makes no internet requests. Files are read, converted, and written locally; OCR runs in-process against bundled models; recent outputs are a local list. Nothing is uploaded, and nothing is collected.

The one exception is opt-in by usage: **Send to a device** talks to your local network only — UDP beacons on `239.255.77.88:47888` plus a direct TCP connection between the two machines, carrying the machine name so each side can label the other. It is inert until you open that picker or another Shift sends you a file.

## Known limitations

- Document → PDF on Windows/Linux still needs Pandoc plus a working PDF engine; the bundled AppKit renderer is macOS-only.
- PPTX and spreadsheet → PDF is a text and table summary, not a layout-accurate render.
- `.doc`, `.pages`, `.ppt`, `.key` and `.numbers` are recognised but not parsed — export to DOCX/PPTX/XLSX first.
- HEIC/HEIF/AVIF convert through the system decoder, but the pixel tools need a PNG/JPEG/WebP/TIFF/BMP/GIF input; AVIF is not yet an export target.
- PDF compression resamples embedded images to at most 1440 px as q60 JPEG. It deliberately skips anything it cannot prove it can round-trip — masks, indexed or CMYK colour, JPX/JBIG2/CCITT — so exotic scans may still resist shrinking.
- Windows recording and media conversion require `ffmpeg` on `PATH`.
- LAN sharing needs a network that allows peer traffic. Multicast is skipped by some guest and corporate Wi-Fi; those networks still work over the broadcast and unicast fallbacks, but two copies cannot meet across a router that isolates clients.
- On Windows only one Shift copy per machine can hold the discovery port, so extra instances do not appear as peers.
- Windows paths build and pass CI, but gesture-level QA needs real Windows hardware.
- Signing identity changes on macOS may require re-approving Screen Recording and Accessibility.

Roadmap: `docs/ROADMAP.md`.

## FAQ

**❓ I hold Shift while dragging and no ring appears.**
Start the drag first — mouse button held, file moving past the drag threshold — and hold `Shift` during it. Pressing `Shift` with a file merely selected does nothing. On Windows the ring needs `Shift` held at the moment the drag crosses the system threshold; add `Alt` at that moment for tools.

**❓ It says this file type has no formats.**
Unrecognised extensions get Open and Reveal only, and legacy Apple/Office formats (`.doc`, `.pages`, `.ppt`, `.key`, `.numbers`) intentionally offer no conversion. Export them to DOCX, PPTX or XLSX first.

**❓ Can I make it run at login, or toggle it off?**
Yes, both, in Settings → **Behaviour**. **Drag rings** is the master switch for the Shift-drag gesture: off means no ring ever appears, while the tray, window and shortcuts keep working. **Launch at login** registers a LaunchAgent on macOS, a `HKCU\...\Run` entry on Windows, or an autostart `.desktop` file on Linux, and is reversible from the same toggle.

The login item is registered for the current user only and points at the executable inside the app bundle, so macOS keeps the same Screen Recording identity across launches. Move the app and re-toggle it once.

**❓ Are my files uploaded?**
No. There is no upload path in the code and no network call in the conversion or OCR paths; the Tesseract models ship inside the app. Nothing is collected, and there is no account.

**❓ Where did my output go?**
Next to the source file by default, with a suffix — check the recent list in the main window first, since clicking an entry opens the result. Settings lists every resolved output path and can reveal it.

**❓ Which languages are supported?**
English and Simplified Chinese, switchable in the main window's Settings. On first launch it follows the system language. The tray menu follows the selected language; native macOS permission prompts stay in English.

**❓ Can I edit a screenshot without saving it?**
Yes — annotation works on a canvas copy and only writes on **Save copy** or **Copy**. The source file is never modified. Use **Pin** to keep it floating on screen as a reference instead.

**❓ How do I uninstall?**
Quit from the tray and delete the app. Nothing is written outside your output folders and the app's own config/cache directories.

## Contributing

Adding a format is one entry in `conversionFormats` in `src/lib/conversionFormats.ts`, its matrix test, and its handling in `perform_action` in `src-tauri/src/files.rs`; adding a tool is one entry in `toolWedges` plus one match arm. Run `pnpm check`, `pnpm test`, `pnpm build` and `cargo test --lib` before pushing. CI builds debug bundles on macOS and Windows.

**Forks and derivative works are welcome.** You may use, modify, redistribute, and build on this code commercially, provided you keep the copyright notice and the LICENSE file.

## License

Released under the **MIT License** — see [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

The app builds against Tauri (MIT / Apache-2.0), React (MIT), Lucide (ISC), and Tesseract.js with its English and Simplified Chinese models (Apache-2.0).
