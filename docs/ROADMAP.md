# Shift Roadmap

Statuses describe this repository as it stands, not intentions. `Done` means the
feature exists and is covered by the test suite or a verified manual run; `Partial`
names what is still missing.

## M1: Foundation — done

- Tangerine radial menu and transition between format/tool rings.
- Global Shift-drag bridge on macOS.
- Area screenshot, area recording, local EN/ZH OCR.
- Image conversion and basic file operations.
- Shared English/Chinese UI and tray app.
- Launch at login and a master enable switch for the drag rings, both in Settings.

## M2: Capture parity — done

- Multi-display capture for the monitor under the pointer, plus per-display recording.
- Annotation canvas: crop, rectangle, arrow, text, highlight, black-bar redaction,
  pixelate/mosaic, blur, stamps (check / cross / star), numbering and gradient background.
- Pinned screenshot windows with copy and close controls.
- Recording controls that only offer what the platform honours: audio plus pause/resume
  on macOS, frame rate plus pause/resume on the FFmpeg backends. `recording_capabilities`
  is the single source of truth, so a switch the backend would ignore is never shown.
- Capture history library: Rust-owned list in the app data directory with search, pinning,
  per-row removal, clear, and drag-out of a row into Finder or another application. The drag
  payload (`text/uri-list` with a `file://` entry, plus the plain path and Shift's internal
  type) is unit-tested in `src/lib/dragOut.test.ts`; the release of that drag is the one part
  of M2 still needing a gesture-level check on each platform, and the README says so.
- Quick-access result card after capture, matching CleanShot's floating card, with
  Copy / Pin / Send / Reveal / Edit and a "card or editor" switch in Settings.

## M3: File workspace — done

- PDF, image, media, DOCX and Markdown paths are implemented; DOC/DOCX/ODT/RTF/HTML/TXT
  to PDF is implemented on macOS through the bundled AppKit renderer; PowerPoint/Excel
  basic text/table PDF export is implemented, with layout-accurate export next.
- Batch operations and drag-to-submenu interaction.
- Custom radial item ordering with per-file-type presets: the wedge catalogue lives in
  `src/lib/radialOrder.ts`, orders are stored per kind and ring in `settings.json`. The
  Settings tab that edited them was removed in 0.1.1, so orders now come from
  `settings.json` alone; `src/components/RadialOrderPanel.tsx` is kept for when an editor
  comes back.
- Undo history in the annotation canvas (annotations pop first, then image-level
  operations, matching the README table) and conflict-safe output naming: `unique_output`
  appends a numeric suffix instead of overwriting. There is no undo for a completed file
  conversion — the original file is never touched, so nothing needs undoing.

## M4: Cross-platform parity — partial

- Windows global drag bridge using the Win32/OLE drag state is implemented.
- Linux capture: `grim` for stills on both Wayland and X11, and `wf-recorder` for area
  recording under Wayland (ffmpeg's `x11grab` has no root window to grab there), with the
  X11 ffmpeg path unchanged.
- Windows capture is native already: `capture_windows_screen` in `src-tauri/src/capture.rs`
  grabs the monitor under the pointer with GDI `BitBlt` + `GetDIBits`, pinning the worker
  thread to per-monitor-v2 DPI awareness so a mixed-DPI or multi-monitor desktop cannot
  shift the image out from under the overlay. PowerShell is nowhere in the capture path; it
  is used only as the HTTP client for the update download.
- Windows ARM64: `scripts/build-windows-release.sh` builds both architectures by
  default, checks each binary's PE machine field (`0x8664` / `0xAA64`) before packaging
  it, and gates each NSIS installer to the matching machine. The update manifest gained a
  `windows-arm64` key; an ARM build tries it first and falls back to `windows`, which
  Windows on ARM runs under emulation, so a channel that predates the ARM build still
  serves it. Verified by cross-building, packaging and publishing all of that from macOS;
  running the installer on real Windows-on-ARM hardware has not happened. Since 0.1.1 the
  shipped targets are macOS and Windows x64 only, so ARM64 stays a buildable architecture
  (`--arch arm64` still works, `windows-arm64` still falls back to `windows`) that nothing
  is published for.
- Linux is supported as a build and run target but is **not a shipped product**: no deb or
  AppImage is published, so the update channel has no `linux` key and the platform table
  reports "no artifact" rather than a failure. `scripts/linux/Dockerfile` and the `linux` CI
  job stay as the recipe for anyone who wants a package; nothing in the app blocks on them.

- HEIC/HEIF/AVIF inputs no longer stop the pixel tools. The pixel tools used to call the
  Rust decoder directly, so a phone screenshot could be converted but not rotated,
  halved, compressed or cleaned. They now share the decode path the convert wedge already
  used (`open_for_pixels` in `src-tauri/src/files.rs`), which falls back to the system
  codec, and the result is written as PNG because nothing in this path can write HEIC.
  Covered by `pixel_tools_read_system_only_image_formats`, which builds a real HEIC with
  `sips` and skips only when the machine cannot write one. Shipped: 0.1.1 is the first
  published build carrying it; the 0.1.0 installers were built before it.

## M5: Distribution — partial

- Update channel on git is implemented: `scripts/publish-release.sh` copies the built
  installers plus `shift-update.json` (version, size, SHA256 per platform) onto an orphan
  `releases` branch, and Shift reads that manifest from the forge's raw endpoint, downloads
  its own artifact, and refuses to install it unless the recomputed SHA256 matches. See
  `docs/RELEASE.md`.
- Daily startup probe with a manual "check now", a progress readout, and a tray row that
  jumps to Settings → Updates.
- Installing happens inside the app: after the SHA256 gate, macOS replaces its own bundle
  from the downloaded disk image and Windows runs the same NSIS installer with `/S`, each
  from a detached helper that waits for this process to exit and then opens the new copy.
  macOS was exercised end to end — a 0.1.1 bundle swapped by the published 0.1.2 image,
  `codesign --verify --deep --strict` clean afterwards — including the failure path, where
  a busy image left the old copy running instead of leaving Shift closed. The Windows
  script is cross-built and its `${Silent}` gate compiles, but the restart itself needs
  real hardware. See "Applying an update" in `docs/RELEASE.md`.
- An update no longer costs the user the Recent list. That library lives in the app data
  directory rather than in the bundle, and since the move out of the webview's own storage
  it had no bridge from the old store: `history_import` adopts those rows on the first
  launch after an upgrade, re-deriving each entry's kind from the file rather than trusting
  the older store's guess, then the legacy key is dropped so a later upgrade cannot
  resurrect rows the user has since removed.
- Signed and notarized macOS package: pending. Builds are **ad-hoc** signed, deliberately:
  signing with a locally created certificate made every other machine refuse the app as
  *damaged / malware* with no override, because that certificate cannot chain to a trusted
  root anywhere else, while an ad-hoc binary gets the softer "unidentified developer" box
  the user can approve. The channel ships one macOS container, the DMG: it was verified on a
  fresh machine (drag to /Applications, one approval on first launch), so the `.pkg` that
  once accompanied it was dropped rather than kept as a second thing to maintain. The `macos`
  manifest key has to stay a DMG anyway, because the in-app handoff mounts it with `hdiutil`;
  see "macOS packaging" in `docs/RELEASE.md`. A Developer ID plus notarization removes the
  prompt entirely and would also stop Screen Recording approval from resetting between
  builds.
- Signed Windows installer: pending.
- Manifest signing is done. `scripts/release-key.py` mints the release key,
  `publish-release.sh --key` publishes a detached `shift-update.sig` over the manifest's
  exact bytes, and a build compiled with the key refuses a manifest that does not verify.
  Every release artifact on the channel is built with the key baked in —
  `build-windows-release.sh` derives `SHIFT_UPDATE_PUBKEY` from `SHIFT_RELEASE_KEY`, so the
  installer and the signature cannot drift apart. Verified against the published feed both
  ways: the real key passes, a valid but different key fails, and a key that is not a curve
  point is an error rather than a silent fall-back to "unsigned". What remains is custody of
  the key itself (it lives outside the repository, and losing it means shipping a new build
  with a new one) plus the code-signing items above.
- Guided first-run checks are done: one card, four probes (Screen Recording, writable
  output folders, ownership of the global shortcuts, missing optional tools), each with the
  fix next to the finding, and dismissal changes no setting. It reuses the same commands
  Settings uses — `screen_capture_access_status`, `check_output_access`, `hotkey_status`,
  `machine_info` — so the card cannot disagree with the panels, and it reads
  `setup_reviewed` from `settings.json` so a second copy on the same account stays quiet.
  Rendered and checked in a browser for all three row states; a real first launch on each
  platform is still worth watching.
- The Linux install step no longer ends in "Automatic updates are not wired up on this
  platform", which was a dead end for anyone who builds their own package: it now sets the
  executable bit and starts a downloaded AppImage, and hands a `.deb` to the desktop's
  package handler. That serves a self-built bundle, not this channel — Linux publishes
  nothing, so a stock Linux build still stops at "no artifact for this platform". No host
  here compiles the branch, so it was type-checked by temporarily disabling the other two
  arms; it has never run on a Linux desktop.
- The published channel is re-verified by hand against the live `releases` branch with
  every release: the 0.1.2 manifest's signature checks out against the baked key and both
  artifacts (`macos`, `windows`) match their published size and SHA256 after being
  downloaded back. The `windows-arm64` and `linux` keys are absent because only those two
  platforms are shipped. See "State of the published channel" in `docs/RELEASE.md`.

## M6: Next desktop work

Restricted to work this repository can do something about; hardware QA is last because no
commit closes it. Each item states the check that closes it. Linux packaging is deliberately
absent — the platform is supported, not shipped.

1. **A real CI run.** The forge (Gitea) returns 404 for `/actions/runs`, so `ci.yml` is
   never executed. Either mirror to GitHub or install Forgejo Actions. Close: one green run
   per platform whose log is linked from the release notes.
2. **Persistent job queue with cancel and retry.** `perform_file_action` already emits
   `app://file-action-progress` with real page counts, but a batch dies with the window and
   an error aborts the rest of the batch. Close: a queued batch survives an app restart,
   one file's failure reports and continues, and each row can be cancelled.
3. **A metadata reader.** The wedge is now labelled **Strip metadata** (*移除元数据*), which
   is what it does, so the label no longer promises a lookup; the wedge id stays `metadata`
   and `LABEL_KEYS` does the rename, so orders saved by earlier builds still resolve. What
   is still missing is the reader itself. Close: a read-only panel with dimensions, codec,
   colour space and the EXIF/XMP a file actually carries, reachable from the same ring.
4. **AVIF export.** `sips` on this macOS reports `public.avif` as readable but not
   writable, so this needs an encoder rather than a flag; the `image` crate has no AVIF
   encoder. Close: an AVIF target in the image ring with the matrix test updated, or an
   explicit README statement that AVIF stays read-only.
5. **Drag-out gesture QA on real Windows hardware.** These are
   the claims in the README that rest on cross-compilation rather than execution, and no
   code change closes them.

## Mobile

`docs/MOBILE.md` is the plan; this is the short version of what it concluded.

- The one desktop idea worth copying is not the ring, it is that a capability menu appears
  while a file is already moving. On both phones that gesture already exists and users do it
  daily: the share sheet. So the mobile product is "release a share over a Shift action",
  not "Shift in your pocket".
- Portable, and now measured: `pdf.rs` and `office.rs` have no Tauri reference at all,
  `files.rs` needs its command wrappers peeled off, and the conversion dependency closure
  (161 crates) needs no C toolchain on iOS or Android. That is most of the product value
  with no platform API in it.
- Substituted rather than ported: HEIC and media codecs, OCR (`Vision` / ML Kit over the
  Tesseract WASM), capture (ReplayKit / MediaProjection), and the result card, which has no
  always-on-top window to live in. Skipped: global hotkeys, launch-at-login, the update
  channel (the store is the only permitted update path), and the runtime-components panel.
- Ecosystem surfaces are enumerated per platform in that document — share extension and
  `ACTION_SEND`, File Provider and `DocumentsProvider` so the history library appears in
  Files, document-type declarations for open-with and create-document, App Intents and
  launcher shortcuts, widgets, and the local-network permissions the LAN path needs.
- Cross-machine work is the reason a phone app exists at all, and it needs one protocol
  change first: SHSF has no version byte today. Pairing by short code, mDNS/NSD discovery
  beside the existing beacon, and a pull model for desktop→phone because iOS will not let a
  backgrounded socket listen. Still no cloud relay.
- Step zero is executable today and needs no phone: extract `shift-core` on desktop and keep
  `cargo test --lib` green while the pure functions move out from under the command layer.
