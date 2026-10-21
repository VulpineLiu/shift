# Shift Roadmap

## M1: Foundation

- Tangerine radial menu and transition between format/tool rings.
- Global Shift-drag bridge on macOS.
- Area screenshot, area recording, local EN/ZH OCR.
- Image conversion and basic file operations.
- Shared English/Chinese UI and tray app.
- Launch at login and a master enable switch for the drag rings, both in Settings.

## M2: Capture parity

- Multi-display capture for the monitor under the pointer is implemented; per-display recording QA is ongoing.
- Annotation canvas: crop, rectangle, arrow, text, highlight, black-bar redaction, pixelate/mosaic, numbering and gradient background are implemented; blur and stamps are next.
- Pinned screenshot windows with copy and close controls are implemented.
- Recording controls: resolution, frame rate, audio source, pause/resume.
- Capture history library with search, pinning and drag-out.
- Quick-access overlay after capture, matching CleanShot's floating result card.

## M3: File workspace

- PDF, image, media, DOCX and Markdown paths are implemented; DOC/DOCX/ODT/RTF/HTML/TXT to PDF is implemented on macOS through the bundled AppKit renderer; PowerPoint/Excel basic text/table PDF export is implemented, with layout-accurate export next.
- Batch operations and drag-to-submenu interaction.
- Custom radial item ordering and per-file-type presets.
- Undo history and conflict-safe output naming.

## M4: Cross-platform parity

- Windows global drag bridge using the Win32/OLE drag state is implemented.
- Windows native capture backend without PowerShell where practical.
- Linux Wayland and X11 capture backends.
- Universal macOS build is implemented; Windows ARM64 build is pending.

## M5: Distribution

- Signed and notarized macOS package.
- Signed Windows installer.
- Automatic updates with signed manifests.
- Permission onboarding and diagnostics screen.
