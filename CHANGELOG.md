# Changelog

All notable changes to Shift are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Shift is at `0.x`,
so a minor bump may still change behaviour.

Installers and the update manifest live on the `releases` branch of the upstream
repository, not in the source tree. See [`docs/INSTALL.md`](docs/INSTALL.md)
for installation and [`docs/RELEASE.md`](docs/RELEASE.md) for the channel
itself.

## 0.1.14 — 2026-11-20

### Fixed

- **In-app updates on Windows now actually run.** The handoff script receives
  the process id, installer and log paths in its own body, and `cmd.exe` gets a
  single quoted script path. Profiles whose folder names contain spaces no
  longer lose the script's quotes and silently skip the install.
- The app icon is the orange ground with the white mark, matching the identity
  the product has shipped with, rather than a re-tinted variant.

### Changed

- Relicensed from MIT to GPL-3.0. Copyright is now held by mula.liu.

## 0.1.13 — 2026-11-19

### Added

- **Release banner in the main window.** A new version shows as
  “Shift *x.y.z* is available” with a button that opens Settings → Software
  updates. The notice is stored on disk, so it survives the launch that
  installs the update instead of disappearing with the process.

### Changed

- One icon mark drawn twice: a tinted version for the shell and tray, and the
  full-colour version for the dock.

## 0.1.12 — 2026-11-19

### Fixed

- **One-click component install works on Windows.** Missing Pandoc, FFmpeg,
  Poppler, ImageMagick or Typst can be installed from Settings → Runtime
  components and are re-detected when the install finishes.
- A pinned image has a working enlarge control again.
- Favourites are visible without hovering, and thumbnails are only decoded for
  rows that are actually on screen.
- The tray uses a black/white system mark instead of the colour wheel.

### Changed

- Sampling stops when no window is looking at the history, cutting idle cost.
  The measured A/B is recorded in `docs/ENGINEERING-NOTES.md`.

## 0.1.11 — 2026-11-06

### Fixed

- **Dropping onto a button now finishes the same way on Windows.** A row dragged
  from the Recent list, and a file dragged from Explorer, spend the button they
  land on instead of one path falling back to a different action.
- A button that cannot serve the dragged file is disabled for the gesture, and
  the prohibition is carried by the cursor instead of a second icon drawn on the
  button.
- The drag-end fallback no longer consumes the drop it was waiting for.

### Changed

- The OCR button is spent instead of explaining a refusal, and the app uses one
  voice across its panels.

## 0.1.10 — 2026-11-01

### Changed

- The drop gesture is quieter: only the three drop targets respond while a drag
  is in flight, and the panel around them stays still.
- OCR only accepts images; a non-image drop is refused during the gesture
  rather than opening an empty ring.

## 0.1.9 — 2026-11-01

### Added

- **Drop a file onto a button.** Convert files, File tools and OCR are drop
  targets in the main window, and the button you release on decides the action.

### Fixed

- The document renderer reports the bundled companion binary on macOS instead
  of a path that only exists on the build machine.

## 0.1.8 — 2026-10-27

### Added

- The release public key is baked into macOS packages, so a signed manifest can
  be verified by the build that ships it.
- `pnpm mac:release` produces the versioned DMG and its checksums in one step.

### Changed

- macOS ships a DMG only. The `.pkg` published for 0.1.7 is retired; the DMG is
  both the first-install path and the in-app update payload.

## 0.1.7 — 2026-10-26

### Fixed

- **Device discovery survives networks that drop both multicast and broadcast.**
  Instances fall back to a unicast scan of the local subnet, and every ordinary
  beacon is answered with a unicast reply so a device no longer appears once and
  then expires.
- The device list keeps an entry for 15 seconds, which clears stale peers after
  a network change while leaving room for older builds to answer.

## 0.1.6 — 2026-10-26

### Fixed

- Peers are discovered on Wi-Fi networks that filter multicast: each instance
  also sends a directed broadcast to every enabled IPv4 subnet.

## 0.1.5 — 2026-10-26

### Fixed

- Device discovery survives an upgrade, and the app declares the macOS local
  network usage description so the permission prompt names the feature.

## 0.1.4 — 2026-10-26

### Fixed

- **Figures survive every conversion.** Images referenced by relative paths are
  kept when a DOCX is printed to PDF and when a Markdown document is converted.
- Install and discard read as one decision in the update panel.

### Changed

- macOS builds are ad-hoc signed so a fresh machine can open them, and the DMG
  replaces the `.pkg` as the shipped artifact.

## 0.1.3 — 2026-10-26

### Added

- **The Recent list survives an upgrade.** History moved from Webview storage
  into `history.json` in the app data directory, and the first launch after the
  upgrade adopts records left in the old store.

## 0.1.2 — 2026-10-26

### Added

- **Install an update in place.** macOS replaces its own `.app` from the
  downloaded DMG, and Windows runs the installer after the process exits; both
  relaunch the new version.
- The update tab leads with the host, architecture, current version and feed it
  actually reads.

## 0.1.1 — 2026-10-26

First tagged release.

### Added

- **Git-backed update channel.** A `releases` branch carries the manifest and
  installers; the app checks it daily and verifies size and SHA256 before
  handing a download to the installer.
- macOS universal and Windows x64 installers, with a Linux source-build target
  that is not a shipped product.
- Settings and UI trims, plus the documentation set this file belongs to.
