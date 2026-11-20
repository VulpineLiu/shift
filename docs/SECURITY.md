# Security model

Shift is a local desktop utility that opens files the user chooses, invokes
native renderers and external tools, listens for local-network transfers, and
installs signed or hash-verified updates. This document states what is
protected, how, and what remains outside the current guarantee.

It is a design document, not a promise that every path has received an
independent audit.

## Assets

- The user's source files and output folders.
- The integrity of the installed application and its update channel.
- The user's consent before another machine writes a file.
- Local data such as `settings.json` and `history.json`.
- Release-signing and manifest-signing secrets.

## Threat model

Relevant adversaries include:

- a malicious or malformed image, document, PDF or media file;
- a machine on the same LAN that sends beacons, opens transfer sockets or
  impersonates another Shift;
- a compromised update feed, release branch or HTTP endpoint;
- an attacker who can modify files in the user's temp or config directories;
- a compromised renderer or injected content in the bundled webview.

The model does not assume the local operating-system account is already
compromised. An attacker with the user's account can generally read and modify
the same files Shift can.

## Local file access and webview

Shift's own React bundle calls Tauri commands with paths selected by the user or
returned by Shift. There is no user-authored HTML or remote page loaded into the
main window. External files are treated as data, not executable content.

The Tauri configuration currently sets `csp: null` and gives the asset protocol
scope `**`. That is intentionally broad because the editor and previews need
local files and the canvas needs same-origin image data. If renderer-content
injection is ever found, those settings increase the impact: the injected script
could call available commands or read local assets. Hardening this would require
a tested CSP/asset-scope reduction and a full editor, OCR-worker and clipboard
smoke test; until then, treat the webview bundle as part of the trusted computing
base.

The app does not ship a remote-content loader, analytics SDK or advertising
component.

## Untrusted files and resource exhaustion

File extensions decide the conversion catalogue, but not trust. Document and
media parsers receive attacker-controlled bytes.

Implemented bounds:

- editor text preview: 8 MiB before reading;
- editor image input: 64 MiB before reading, plus a 40-megapixel decode limit
  where the decoder can report dimensions before full decode;
- editor data URL: 128 MiB, with a second decoded-size and pixel check;
- pinned-image preview: 40 megapixels;
- update manifest: 256 KiB after download;
- update artifact: exact declared size and SHA256, rechecked before install;
- LAN peer table: 256 entries;
- LAN transfer: the receiver reads exactly the header-declared byte count and
  removes a partial temp file on failure;
- GIF conversion: time, frame-rate, width and byte ceiling;
- PDF page extraction: per-page extracted-text limit and bounded render edge.

Not yet uniformly bounded:

- Office ZIP entries and XML can still expand under decoder defaults; there is
  no one shared decompressed-size quota across DOCX/PPTX/XLSX paths.
- PDF rendering and compression rely partly on external renderers (PDFKit,
  Poppler, ImageMagick, FFmpeg) with their own limits.
- CSV/TSV and some Office paths read whole files or sheets into memory.
- LAN transfers have no maximum file size or free-space preflight. The receiver
  sees the declared size before accepting, but a malicious peer can still
  occupy disk after consent.
- A manifest download is size-checked after it has been written, not while it is
  streaming.

Do not open adversarial documents in production until these paths have explicit
quotas and tests. Report a reproducible resource-exhaustion case rather than
assuming the editor limits cover every converter.

## Update trust boundary

The update channel is a git branch containing `shift-update.json`, optional
`shift-update.sig`, and installer files. Builds derive the feed URL from the git
remote or from `SHIFT_UPDATE_URL`.

Controls:

- manifest `schema` must be exactly the supported version (currently 1);
- artifact filenames must be safe ASCII basenames;
- artifact size must be nonzero and SHA256 must be valid hex;
- the manifest is capped at 256 KiB;
- a build compiled with `SHIFT_UPDATE_PUBKEY` requires a detached ed25519
  signature over the manifest's exact bytes;
- the installer is checked for exact byte length and SHA256 after download;
- both checks run again immediately before installing;
- the installer must be a regular file, not a symlink;
- macOS and Windows handoff scripts receive paths as process arguments or
  environment variables, never interpolated shell/batch text;
- `Discard` deletes the downloaded artifact, not just the UI state.

Residual risk:

- An **unsigned** build trusts the HTTPS origin plus anyone able to write the
  release branch or its raw endpoint. SHA256 proves the file matches the
  manifest, not that the manifest is authentic.
- A signed build trusts the baked public key and the custody of its private key.
  Leaking the key allows publishing a malicious manifest and installer.
- The update artifacts are ad-hoc signed and not notarized on macOS. Gatekeeper
  cannot independently vouch for the publisher; the in-app hash/signature path
  is the relevant check.
- Windows installer signing is not currently performed.
- The download directory is under the system temp directory. Re-verification
  detects replacement before install, but local temp-directory manipulation is
  still a same-account threat.
- Windows runs the handoff through `cmd.exe`; argument passing is covered by
  source-level checks, but every legal character in a user-profile path still
  needs real Windows validation.

## LAN sharing

Discovery uses UDP multicast/broadcast/unicast on `239.255.77.88:47888`.
Transfers use a direct TCP connection advertised by the beacon.

The SHSF protocol is **not authenticated, not encrypted and not confidential**.
Anyone on the reachable network can:

- observe beacon metadata such as machine name and platform;
- send a forged offer with an arbitrary sender name and transfer id;
- attempt connections to the receiver's TCP port.

The consent card prevents an unsolicited transfer from being written
automatically. It does **not** prove that the named sender is the real owner of
that machine. Accept files only from people and networks you trust. The batch
window means later files from the same unauthenticated batch id are accepted for
120 seconds without another prompt.

Implemented limits:

- peer table capped at 256, with stale entries removed first;
- transfer header capped at 64 KiB;
- part files are removed after failure;
- sender copies exactly the declared size, so extra source bytes cannot be
  mistaken for the acknowledgement;
- receiver reports a transfer error after an accepted transfer fails.

No protocol-level authentication, encryption, rate limiting or maximum transfer
size is implemented. Firewall or OS network policy is the outer boundary.

## External processes and secrets

External tools are invoked with argument arrays through `process::command`,
which also hides console windows on Windows. Paths are not passed through a
shell except where a documented system command requires one; those call sites
must be reviewed individually.

The release private key (`.ed25519`) must remain outside the repository and
outside build artifacts. `SHIFT_RELEASE_KEY` is for the packaging host only;
`SHIFT_UPDATE_PUBKEY` is the public value intended to be baked into builds.
Never paste a private key into an issue, log or CI variable that is visible to
untrusted jobs.

## Platform permissions

- macOS Screen Recording enables capture.
- macOS Files & Folders enables selected output directories.
- macOS Microphone enables recording with audio.
- macOS Local Network enables multicast discovery.
- Windows Firewall must allow private-network traffic for discovery/receive.

Ad-hoc macOS signing can change the code identity on rebuild, causing
permissions to reset. A Developer ID identity would stabilize them but is not
currently part of the release pipeline.

## Reporting

Report a vulnerability privately through the repository host's available
private security channel. Include the affected version, platform, minimal
reproduction, expected/actual behavior and whether a file, LAN peer or update
feed is required. Do not attach a stolen signing key or a working exploit to a
public issue.

If no private channel is configured, contact the maintainers first and share
only enough detail to establish a private transfer method.

