# Install and first run

Shift ships two installers: a macOS universal DMG and a Windows x64 EXE.
Linux builds from source but has no published installer. For what the app does
once it is running, see the [English README](../README.md) or the
[Chinese README](../README.zh-CN.md).

## Requirements

| Platform | Requirement |
| --- | --- |
| macOS | Apple silicon or Intel. The DMG is one universal bundle. |
| Windows | Windows 10 or 11, x64. ARM64 machines run the x64 build through the compatibility layer. |
| Windows | Microsoft Edge WebView2 Runtime. Windows 10/11 normally includes it; install it from Microsoft if the window opens blank. |

Installing and running Shift needs no administrator rights. Only installing an
optional runtime component from Settings can raise a UAC prompt.

## macOS

1. Download `Shift-<version>-macOS-universal.dmg` and double-click to mount it.
2. Drag `Shift.app` into `/Applications` (or any location you can write to).
3. The first launch is blocked as “unidentified developer” because builds are
   ad-hoc signed and not notarized. Cancel the dialog, then right-click
   `Shift.app` → **Open**, or allow it in **System Settings → Privacy &
   Security**.
4. Approve it once. Later launches open normally.

In-app updates download the same DMG directly and mount it themselves, so an
update never shows the Gatekeeper prompt.

### Permissions

- **Screen Recording** — screenshots and screen recording. Shift checks this
  before showing the capture overlay, so a denial produces an actionable message
  instead of a black screenshot.
- **Files & Folders** — the output folders you choose.
- **Microphone** — recording with audio.
- **Local Network** — Send to a device. If denied, Shift still broadcasts and
  answers, but never receives the beacons, so the device list stays empty.

Ad-hoc signing gives each rebuild a new code identity, and macOS ties these
grants to that identity. An upgrade may therefore ask again. A Developer ID
certificate and notarization would make the identity stable; that is future
work, not a setting you can change.

## Windows

1. Run `Shift-Setup-<version>-win-x64.exe`. It installs to
   `%LOCALAPPDATA%\Programs\Shift` and creates Start-menu and desktop shortcuts.
2. The build is unsigned. If SmartScreen says “Windows protected your PC”,
   choose **More info** → **Run anyway**.
3. On first launch, allow Shift on **private networks** if the firewall asks.
   If you declined earlier, enable it under **Windows Security → Firewall &
   network protection → Allow an app through firewall**. Without it, Send to a
   device cannot receive other machines' beacons.

## First run

Shift shows one checklist card:

- Screen Recording (macOS)
- whether the three output folders are writable
- whether this copy owns the global shortcuts
- which optional runtime components are missing

Each row says what it found and offers the fix. Dismissing the card changes
nothing. It appears once per install, including the first launch after an
upgrade; delete `setupReviewed` from `settings.json` to see it again.

Shift then lives in the menu bar (macOS) or notification area (Windows). No
window has to stay open — the rings work from anywhere.

## Optional components

Settings → **Runtime components** shows what Shift found on this machine, with
version and path. Missing tools can be installed from that panel:

| Component | Used for | macOS source | Windows source |
| --- | --- | --- | --- |
| FFmpeg | Video and audio conversion, Windows/Linux screen recording | Homebrew | WinGet `Gyan.FFmpeg` |
| Pandoc | Documents to PDF on Windows/Linux | Homebrew | WinGet `JohnMacFarlane.Pandoc` |
| Poppler | PDF rendering outside macOS | Homebrew | WinGet `oschwartz10612.Poppler` |
| ImageMagick | HEIC/AVIF decoding and HEIC encoding outside macOS | Homebrew | WinGet `ImageMagick.ImageMagick` |
| Typst | Document rendering | Homebrew | WinGet `Typst.Typst` |

Shift never downloads a runtime from a third-party site and never removes or
overwrites a tool you already have. A missing component disables only the
wedges that need it.

## Updates

Shift reads `shift-update.json` from the `releases` branch of the upstream
repository once a day, or when you press **Check** in Settings → **Software
updates**. A download is accepted only when its byte size and SHA256 match the
manifest, and both are checked again before the installer is launched.

The update source is derived from the git remote at build time. A build made
from a checkout with no remote reports “no feed”.

## Verify a download

```bash
# macOS
shasum -a 256 -c SHA256SUMS.txt
hdiutil verify Shift-<version>-macOS-universal.dmg
codesign -dv --verbose=2 /Applications/Shift.app 2>&1 | grep -E 'Signature|Identifier'
#   Signature=adhoc
#   Identifier=app.nex.shift
```

```powershell
# Windows
Get-FileHash .\Shift-Setup-<version>-win-x64.exe -Algorithm SHA256
```

`Signature=adhoc` is the intended form, not a broken signature.

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| macOS says the app is damaged or from an unidentified developer | The DMG is not notarized. Right-click → Open, or allow it in Privacy & Security. Do not run it from the archive preview window. |
| Screenshot is black, or capture never starts | Screen Recording is denied. Enable Shift under Privacy & Security → Screen Recording, then restart Shift. |
| Device list is empty | Local Network or firewall permission is missing, or the network isolates clients. Check the macOS Local Network pane, or the Windows private-network rule. |
| Windows window is blank | Install the WebView2 Runtime. |
| A wedge is missing | That conversion needs a tool that is not installed. Check Settings → Runtime components. |
| Video or audio conversion fails | FFmpeg is missing or lacks the needed encoder. Check the runtime panel. |
| Update says “no artifact for this platform” | Linux builds have no published artifact; on macOS and Windows, confirm the manifest carries your platform key. |
| Update downloads but does not install | Check that the app can write its own bundle location, then retry. The downloaded file is deleted if size or SHA256 differs from the manifest. |

## Uninstall

- macOS: quit Shift from the menu bar, then move `Shift.app` to the Trash.
- Windows: **Settings → Apps** → Shift → Uninstall.

Configuration and history live in the app data directory and are left in place;
delete them by hand if you want a clean state. Output files are never removed.
