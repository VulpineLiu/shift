# Shift for iOS and Android

Planning document. It answers one question: which parts of Shift's desktop design survive
translation to a phone, and how the two ends share ground. Decisions are recorded as
decisions, with the reason, so a later reader knows what was considered and rejected.

## What Shift actually is

Strip the desktop chrome and Shift is four things:

1. A **gesture** that opens a capability menu attached to a file, not to a window.
2. A **catalogue** of what each file type can become, matched to what is really installed
   to do the work.
3. **Local processing** — convert, compress, redact, OCR, extract — with no upload.
4. A **hand-off** path: the result lands next to the source, in a history library, or on
   another machine on the LAN.

All four translate. The desktop UI does not: a 760 px radial ring around a cursor has no
mobile equivalent, and neither does "hold Shift while dragging in Finder".

## The one feature worth copying

**Share-drag is the feature.** Everything else on desktop is either table stakes
(convert, crop, compress) or platform-specific (menu bar, global hotkeys). The thing users
remember is that a capability menu appears *while you are already moving a file*, so the
action is part of the gesture instead of a place you navigate to.

iOS and Android both have exactly one gesture with that shape, and it is the one users
already perform every day:

| Desktop | iOS | Android |
| --- | --- | --- |
| Shift-drag out of the Finder row, release over a wedge | **Share sheet**, including the drag-and-drop share from Files/Photos/Safari where the picker slides out from under the finger | **Share sheet**, same shape, from any app |
| Radial ring of wedges | The share sheet's own app list, plus a Share Extension whose UI we own | Share sheet targets, plus an Action/Share Activity we own |
| Ring reorder in Settings | Share sheet "Edit Actions" ordering, mirrored in our Settings | Same, mirrored |

So the mobile pitch is not "Shift in your pocket", it is: **release a share over a Shift
action**. Concretely: long-press an image in Files → Share → Shift → *HEIC → PNG, then
redact*, and the file comes back where it went out from. Same mental model, native
gesture, no invented interaction.

Two more mobile entry points carry the same idea cheaply:

- **Quick Action / home-screen shortcuts** (long-press the app icon): "Capture + convert",
  "Scan text", "Compress". The iOS Action Button and Android's app shortcuts give the
  "one gesture away" promise better than any menu.
- **Share Extension from other apps** is the only sanctioned way in on both platforms;
  neither lets a third party overlay an arbitrary app's window the way the macOS drag
  bridge does. Do not plan a mobile equivalent of the global drag bridge — it does not
  exist, and an app that behaves like a system overlay gets rejected or uninstalled
  within a day.

## Ecosystem surfaces

"Hook into the OS ecosystem" on mobile means a short list of sanctioned doors. Each one is
an entry point with a different activation rule, and each costs one well-scoped UI. Nothing
here needs an account, a server, or a background service that fights the OS.

| Surface | iOS | Android | What it buys |
| --- | --- | --- | --- |
| Files in, other apps out | Share Extension (`com.apple.share-services`, activation rule on images/PDF/office types) | `ACTION_SEND` / `ACTION_SEND_MULTIPLE` intent filter | The core loop: any app's Share button reaches Shift |
| Files as a place | File Provider extension so Shift's history library appears in the Files app sidebar | `DocumentsProvider` exposing the same library | Shift's outputs are reachable *and* openable from outside, not trapped in a sandbox |
| Open a type | `CFBundleDocumentTypes` + UTType declarations (`public.image`, `com.adobe.pdf`, `org.openxmlformats...`) | `ACTION_VIEW` filters per MIME type | "Open with Shift" from mail, chat, and browser downloads |
| Write a type back | `ACTION_CREATE_DOCUMENT` equivalent: iOS document-based launch + "Save on iPhone" | `ACTION_CREATE_DOCUMENT` / `ACTION_GET_CONTENT` response | Shift can be the *saving* app, so it can hand a converted file back to whatever asked |
| One-tap entry | Home-screen Quick Actions, Action Button, App Intents (iOS 16+) so Shortcuts can call "Convert", "Redact", "Scan text" | Launcher long-press shortcuts, Quick Settings tile, Assistant/`app_functions.xml` | The "one gesture away" promise without a global hotkey, and other apps can compose Shift |
| Hand-off out | `NSItemProvider` with a `fileURL`, plus AirDrop, Save to Photos, Copy | `ClipData` `FILE_URI` through a `FileProvider` grant, temp-read permission | The result returns where it came from, which is the desktop behaviour that matters |
| Widgets | WidgetKit: one widget for "last capture → redact/share", one for history | Glance/App widget + notification with a Copy/Send action | Value without opening the app |
| Cross-machine | `NSLocalNetworkUsageDescription` + `_shift._tcp` Bonjour browse/register | `ACCESS_NETWORK_STATE`, `CHANGE_WIFI_MULTICAST_STATE` lock while discovering, NSD | The LAN Send path, described in the next section |

Two rules keep this from sprawling. First, **every surface ships with a job list, not a
window**: the share extension opens, does the work, and returns to the caller; the widget
taps deep-link into the library. Second, **no surface may require a persistent background
service**. iOS will kill one and Android's battery optimisers will throttle it, and a Shift
that silently stopped receiving is worse than one that never claimed to.

### Making the two ends actually meet

Discovery is the part that breaks between ecosystems, so it gets explicit answers:

- **Same-LAN discovery.** Desktop already beacons UDP multicast to `239.255.77.88:47888`
  with broadcast and unicast fallbacks. Phones keep that as a fallback but lead with
  Bonjour/mDNS on iOS and NSD on Android, because both give a wake-up that survives
  doze and neither needs a multicast lock held hostage. The desktop registers `_shift._tcp`
  alongside the existing beacon, so a phone finds it by name instead of guessing.
- **First contact is a pairing code, not a prompt.** A phone joining a network with five
  desktops needs to pick one. The desktop shows a short code and the phone types it (or
  scans a QR encoding host, port, and code); the desktop then pins that phone's identity
  the way SSH does. This removes the "who is `Desktop-2.local`?" problem and gives the
  consent card something to vouch for.
- **Background receiving is asymmetric, and stays that way.** iOS gives a socket a few
  seconds of life, so a phone is a first-class sender and a *wake-to-receive* peer: a push
  or a tap opens the app, which then pulls the queued job. Desktop-to-phone therefore
  behaves like a pull: the desktop queues, the phone collects. Saying so beats shipping a
  receiver that drops files at random.
- **No cloud relay, still.** The pairing code is what makes a *user-run* hub workable if a
  remote case ever appears — one desktop as the hub, the phone dialling it — without
  inventing a hosted service, accounts, or a retention policy.

## Capability map

Legend: **Native** = OS API, do not reimplement. **Port** = existing Shift logic moves
with little change. **Substitute** = the desktop mechanism is unavailable, use the listed
replacement. **Skip** = no mobile answer that fits the product.

| Desktop capability | Mobile | Mechanism |
| --- | --- | --- |
| Radial ring | Substitute | Share-sheet extension with its own ring-style picker; the wedge catalogue is shared code, the geometry is a phone-sized arc |
| Ring catalogue + custom order | Port | `src/lib/radialOrder.ts` is pure TypeScript with no Tauri import; reuse it verbatim, persist orders in app group / `SharedPreferences` |
| Format catalogue (`conversionFormats`) | Port | Pure TS, same table; prune per platform at build time where a codec is absent |
| Image convert (PNG/JPG/WebP/TIFF/BMP/GIF) | Port | Keep the Rust `image` crate — it builds for `aarch64-apple-ios` and `aarch64-linux-android`; or substitute `ImageIO`/`ImageDecoder` |
| HEIC decode/encode | Native | `ImageIO` on iOS, HEIF support in Android's decoder (API 28+); desktop's `sips`/ImageMagick has no mobile role |
| Image pixel tools (rotate, flip, B&W, 50%, crop, compress) | Port | Same math. On desktop these now decode HEIC/AVIF through `open_for_pixels`, which shells out to `sips`/ImageMagick; that function is the one thing in the path to replace, and both mobile platforms decode those formats natively, so the mobile core needs an injected decoder rather than a subprocess |
| Video/audio convert, compress, 720p, mute, extract audio, mono, frame | Native | `AVFoundation` on iOS, `Media3`/`MediaCodec` on Android; `mobile-ffmpeg`-style bundled FFmpeg works but costs 15–25 MB and licence review for GPL builds — decide per platform, not globally |
| Video GIF | Native + port | Frame sampling natively, palette-quantise in Rust (desktop already caps GIF at 15 s / 12 fps / 480 px / 8 MB — keep those caps, phones care more) |
| PDF text, split, merge, rotate | Port | `lopdf` is pure Rust; or native `PDFKit`/`PdfRenderer` for page images |
| PDF → images | Native | `PDFKit` (iOS), `PdfRenderer` (Android) — replaces PDFKit-sidecar/Poppler |
| PDF → DOCX | Substitute | Native page render + the existing pure-Rust DOCX packager; the macOS AppKit renderer has no mobile twin |
| PPTX / XLSX / CSV → Markdown | Port | `calamine` + `quick-xml`, pure Rust, no platform dependency. Highest value-to-effort item on the list |
| Document → PDF | Substitute | iOS: `UIPrintRenderer`/`UIMarkupTextPrintFormatter`; Android: `PdfRenderer`/WebView print. Neither matches the AppKit renderer's fidelity; say so in the UI |
| Annotate: rectangle, arrow, text, highlight, number | Port | The canvas model is React/TS; a mobile build reuses it, though native `PencilKit` is better for finger/stylus |
| Annotate: redact, pixelate, blur, stamps | Port | Same drawing primitives; redaction stays "burned in", which is the whole point |
| Cutout (corner-sample keying) | Port | Works, but iOS `VNGeneratePersonSegmentationRequest` / Android segmenter are dramatically better — substitute where available |
| OCR EN + ZH | Native | **Drop Tesseract WASM on mobile.** `Vision` (`VNRecognizeTextRequest`, `.enUS`+`.zh-Hans`) and ML Kit Text Recognition v2 are faster, smaller and better on-device engines. Keep Tesseract only if one shared engine matters more than quality |
| Screen capture | Native | iOS `ReplayKit` (broadcast or one-shot, user-initiated); Android MediaProjection, plus the built-in screenshot + `MediaStore` observer |
| Screen recording | Native | `ReplayKit` / MediaProjection; frame-rate honesty (`capabilities()`) carries straight over — mobile has the same "backend ignores this" problem |
| Result card (Copy / Pin / Send / Reveal / Edit) | Substitute | No floating always-on-top windows. iOS: the Share-sheet extension's own completion UI + Live Activity where useful; Android: a floating service (easy) and a Quick Settings tile |
| History library | Port | `history.rs` logic (capacity, pinning, missing files) is portable; storage becomes the app group container / scoped storage. Search + pin behave identically |
| Drag a row out | Native | `NSItemProvider` with `fileURL` on iOS, `ClipData` `FILE_URI` + `FileProvider` on Android — same intent as `text/uri-list` with a `file://` entry |
| Pin to screen | Substitute | Android: floating overlay window. iOS: no API — pinned images become a Widget or nothing |
| Menu bar / tray | Substitute | iOS: Menu Bar Extra is not a thing; share sheet + Shortcuts actions. Android: Quick Settings tile, launcher icon long-press |
| Global hotkeys | Skip | Do not request Accessibility for a keyboard shortcut. Offer App Intents (iOS 16+) / Quick Settings tile + Assistant shortcuts |
| Launch at login | Skip | Meaningless on mobile |
| LAN Send (SHSF beacons + TCP) | Port | UDP multicast + direct TCP work on both platforms; needs `NSLocalNetworkUsageDescription` + Bonjour on iOS (the privacy prompt is unavoidable) and `ACCESS_NETWORK_STATE`/multicast lock on Android. Background reception is unreliable on iOS, so treat mobile as a first-class *sender* and a best-effort receiver |
| Update channel on git | Skip | Store review is the only permitted update path for executable code. Keep the SHA256 discipline for *content* (models, catalogues), not for the binary |
| Runtime components panel | Substitute | Nothing is installable. Show "detected codecs" read-only, or drop it; do not fake a desktop panel |
| i18n en/zh | Port | The two message tables are data; move them as-is |

## Architecture for two ecosystems

The claim that the core is portable was checked rather than assumed, against this
repository as it stands:

- `cargo metadata --no-deps` reports one workspace package, `shift`. Nothing is separable
  yet, so step zero is a real refactor, not a packaging detail.
- `src-tauri/src/pdf.rs` and `src-tauri/src/office.rs` contain **zero** references to
  `tauri::` or `AppHandle`. Their functions already take `&Path` and return
  `Result<PathBuf, String>`, which is exactly the shape a C header wraps. They move
  essentially unchanged.
- `src-tauri/src/files.rs` is 2,761 lines with 11 `#[tauri::command]` wrappers, and 28 of
  its lines mention `tauri::` or `AppHandle`. The wrappers are thin — the workers beneath
  them already take `&Path` and an `output_dir: Option<&Path>` — but the settings lookups
  inside the command layer (`files_output_dir`, `auto_reveal`) have to become parameters,
  and the `history::record` call each command makes after a write becomes a callback.
- The dependency closure of `image`, `lopdf`, `printpdf`, `calamine`, `quick-xml`, `zip`,
  `sha2`, `ed25519-dalek`, `base64`, `serde` and `serde_json` is 161 crates, and not one of
  them needs a C compiler: the only system-flavoured members are `libc`, `windows-sys`,
  `adler2` and `miniz_oxide`, all of which build for iOS and Android. There is no AV1 or
  HEVC encoder in the tree, which is why HEIC and video stay native on mobile.

So the core is genuinely the leverage: `image`, `lopdf`, `printpdf`, `calamine`,
`quick-xml`, `zip`, `sha2` cross-compile to `aarch64-apple-ios`, `aarch64-apple-ios-sim`
and `aarch64-linux-android`. Put the conversion pipeline, the wedge catalogue, the PDF and
Office readers, and the SHSF protocol in a Rust static library and let both platforms call
it; that is where most of the value is, and it is the part with no platform API in it.

```text
shared/
├─ shift-core (Rust staticlib + C header)
│   ├─ convert / compress / pixel tools / pdf / office  ← from src-tauri/src/files.rs, pdf.rs, office.rs
│   ├─ catalogue            ← from src/lib/radialOrder.ts + conversionFormats.ts (generated or mirrored)
│   └─ shsf                 ← from src-tauri/src/share.rs
├─ ios/       Swift UI + Share Extension + App Intents, links shift-core
└─ android/   Kotlin UI + Share Activity + Tile, JNI to shift-core
```

Step zero, before any UI: split the workspace into `shift-core` (the pure functions above,
plus a decoder trait standing in for `open_for_pixels`), a `shift-ffi` staticlib over it,
and the existing Tauri app consuming `shift-core`. The desktop then exercises the same code
the phones ship, which is the only thing that keeps three platforms from drifting. That
split is verifiable on desktop alone — `cargo test --lib` keeps passing while the pure
functions move out from under the command layer.

Two consequences worth deciding early:

- **The wedge catalogue should stop being TypeScript-only.** It is currently the source of
  truth for the desktop ring. Either generate the mobile lists from it, or move it into
  `shift-core` and have the desktop read it. Do not maintain three hand-written copies.
- **Tauri does not run on iOS/Android usefully here.** Tauri v2 can target mobile, but the
  product is a share-sheet extension, which is a small native UI, not a webview app. Plan
  a native shell around the Rust core; keep the desktop's React UI on desktop.

## What mobile must send to desktop

Cross-ecosystem hand-off is the feature that justifies a mobile app at all. The current
`SHSF` frame is: four magic bytes `SHSF`, a big-endian `u32` header length, a JSON header
(`fileName`, `size`, `peerName`, `transferId`, `batchId`, `batchIndex`, `batchTotal`), the
raw file bytes, then a one-byte accept decision. Notably **there is no version field** —
compatibility today comes from `#[serde(default)]` letting a newer reader accept an older
header. That is enough for two desktop revisions and not enough once a phone app is out in
the wild and cannot be force-updated.

Before the first mobile release, freeze the protocol:

```text
"SHSF" | version:u8 = 1 | headerLen:u32 | header:JSON | payload | decision:u8
```

Plus, in the header: `platform`, `appVersion`, `kind` ("file" | "job"), and for jobs
`action` + `args`. Everything else the mobile app needs is a job it can describe:

1. **Phone → desktop, direct.** LAN transfer, receiver stores to its output folder, both
   histories update. This is Send with a phone as a peer. Small change: the peer beacon
   needs a `kind` so a phone does not look like a desktop, and the consent card needs to
   say "iPhone sent this".
2. **Phone → desktop as a job.** A phone encodes 4K video or a 900-page PDF poorly. Send
   the file plus a requested action, the desktop runs it in the existing job pipeline
   (`perform_file_action` already reports progress and writes conflict-safe names), and the
   result comes back. The desktop queue already exists; the phone needs a second, smaller
   queue that only shows status.
3. **Desktop → phone.** Symmetric; a phone receiving a file is one `URLSession`/`OkHttp`
   upload to the desktop's existing receive port.

Deliberately **not** part of the plan: a cloud relay. Shift's whole claim is that nothing
leaves the machine, and a relay would need authentication, retention limits, and a privacy
policy for a feature that works fine over the LAN. If a remote case ever demands it, the
right shape is user-run (one desktop is the hub, the phone dials it with a pairing code),
not a hosted service.

The privacy posture carries over unchanged: no telemetry, no analytics SDK, no crash
reporter that uploads file content. On iOS that also means the local-network permission
string and a Bonjour service type in `Info.plist` — write them before the first submission,
because the denial is silent and the feature looks broken.

## Build order

Each step ships something usable on its own; none depends on a feature Shift lacks today.

0. **Extract `shift-core` on desktop.** Move the pure conversion, PDF and Office code out
   from under the Tauri command layer and add the `staticlib` target. Ships nothing new to
   users, and it is the step that can be finished and verified without an Apple developer
   account or an Android SDK: `cargo test --lib` keeps passing the whole way, and the tests
   that follow the functions prove the boundary is real.
1. **iOS share-sheet converter for images.** `shift-core` + the ported catalogue + a
   ring-style picker. No networking, no capture, no accounts. Proves the gesture.
2. **Android share-sheet converter.** Same core, same catalogue; the second platform is
   the test that the abstraction is real rather than iOS-shaped.
3. **PDF and Office text extraction.** Pure-Rust ports, no platform APIs, high perceived
   value ("turn this deck into Markdown on my phone").
4. **Native OCR (Vision / ML Kit) + annotation.** Redact first — it is the mobile-native
   killer use (screenshot → redact → share, all on device).
5. **LAN peer + phone→desktop jobs.** Add the version byte and the `kind` field to SHSF
   here, in the same release as the first phone that speaks it.
6. **Capture on mobile, last.** ReplayKit/MediaProjection is real work, mobile screenshots
   are already excellent, and the desktop capture loop is the least portable thing in the
   product. Do it when users ask for it rather than for parity.

## Open decisions

| Question | Leaning | Why |
| --- | --- | --- |
| Tauri mobile or native shell | Native shell + Rust core | Share extensions are small native UIs; a webview adds size and latency to the one interaction that must feel instant |
| One FFmpeg binary per platform | iOS: no, Android: measure | `AVFoundation` covers most mobile media needs at zero licence risk; Android's codecs are as capable but the surface is fragmented |
| Shared OCR engine or native | Native | Vision and ML Kit beat Tesseract WASM on speed and Chinese accuracy; a shared engine buys nothing users can see |
| Catalogue home | `shift-core` | Desktop can read from it; phones and desktop cannot drift apart |
| Priced app to fund it | Undecided | The desktop app is MIT and free; a paid mobile app is defensible but changes the project's centre of gravity |
