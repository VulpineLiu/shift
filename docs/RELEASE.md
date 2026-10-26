# Releases and updates

Shift's update channel is this repository. There is no update server, no object
storage, and no release service to configure: the installers and a small manifest are
pushed to a git branch, and a running copy reads that branch over HTTPS.

## The channel

An orphan branch named `releases` holds exactly one release:

```text
releases/
├── shift-update.json
├── shift-update.sig
├── Shift-macOS-universal.dmg
└── Shift-Setup-0.1.1-win-x64.exe
```

`shift-update.json` names the version and, per platform, the artifact file, its size in
bytes and its SHA256:

```json
{
  "schema": 1,
  "version": "0.1.1",
  "date": "2026-10-26",
  "notes": "",
  "artifacts": {
    "macos": { "file": "Shift-macOS-universal.dmg", "sha256": "57fc…", "size": 55479751 },
    "windows": { "file": "Shift-Setup-0.1.1-win-x64.exe", "sha256": "0255…", "size": 24545040 }
  }
}
```

## Supported release targets

**Shift ships two installers: macOS (universal) and Windows x64.** That is a scope decision,
not a code limit, and it decides what the channel may contain:

| Key | Shipped | Notes |
| --- | --- | --- |
| `macos` | yes | One universal bundle, Apple silicon and Intel. The DMG, because that is what the in-app updater mounts |
| — | yes | `Shift-macOS-universal.pkg`, also macOS universal: the file to hand a machine installing Shift for the first time. It carries no manifest key — see "macOS packaging" |
| `windows` | yes | x64, which is also what an ARM machine falls back to |
| `windows-arm64` | no | Buildable — `build-windows-release.sh --arch arm64` still works — just not published |
| `linux` | no | Build target, not a product; see below |

Publishing an ARM64 Windows build later needs no code change: the ladder already prefers
`windows-arm64` and falls back to `windows`, so adding the file to `artifacts/` and
re-publishing is enough.

Keys are platform, then platform and architecture where they differ. A build picks the
first key its own architecture matches and never crosses operating systems:

| Running on | Keys tried, in order |
| --- | --- |
| macOS (Apple silicon or Intel) | `macos` |
| Windows x64 | `windows` |
| Windows ARM64 | `windows-arm64`, then `windows` |
| Linux | `linux` |

The ARM64 fallback to `windows` is deliberate: Windows on ARM runs x64 binaries under
emulation, so a channel that only carries x64 keeps serving ARM machines exactly as it did
before the ARM build existed. `artifact_keys_for` and `select_artifact` in
`src-tauri/src/update.rs` hold that ladder and are tested directly, so the rule is fixed
by a test rather than by a comment.

The branch is rebuilt as a fresh root commit on every publish and force-pushed, so it
never accumulates gigabytes of installer history while `main` stays clean.

## macOS packaging

Shift builds are signed with a **self-signed** identity ("Shift Local Signing") and are
**not notarized** — notarization needs an Apple Developer ID account. Gatekeeper therefore
calls any Shift build an unidentified developer, and how annoying that is depends entirely
on the container:

| Container | What a fresh machine does |
| --- | --- |
| DMG, app dragged into /Applications | The app itself carries the quarantine flag from the browser, so **the first launch of Shift** is refused: *"无法打开，因为来自身份不明的开发者"*. The user approves in 系统设置 → 隐私与安全性 (允许 / 仍要打开), or right-clicks the app and chooses 打开. |
| `.pkg`, installed with Installer.app or `installer` | The approval moves to the package, once, before anything is installed (double-click reports it, right-click → 打开 passes it, `installer` from a terminal needs nothing). `installer` does not propagate quarantine onto what it installs, so **the installed app launches with no prompt at all** — verified: an installed copy carries only `com.apple.provenance`, no `com.apple.quarantine`. |

Both containers hold the same signed universal bundle — `pkgbuild` neither re-signs nor
strips the signature, and the payload passes `codesign --verify --deep --strict`.

So the channel publishes both, for different jobs:

- **`Shift-macOS-universal.pkg`** — first install on any machine. Build it with
  `scripts/macos-pkg.sh <Shift.app> <out.pkg>`; it stages the bundle under a
  `/Applications` root and runs `pkgbuild`, keeping the app's own signature.
  The package itself is unsigned: `productsign` needs a certificate with the *Installer
  Signing* purpose and this build has only a code-signing identity. A browser-downloaded
  unsigned package is refused on a plain double-click, so the user approves it once —
  right-click → 打开, or 仍要打开 in 系统设置 → 隐私与安全性. What the package changes is
  *where* that single approval lands: on the download, before install, instead of on the
  app every time it is opened.
  `installer -pkg Shift-macOS-universal.pkg -target /` from a terminal needs no such
  permission at all.
- **`Shift-macOS-universal.dmg`** — the `macos` manifest key, because
  `update_install`'s handoff mounts the image with `hdiutil` and copies the bundle out of
  it. Pointing the key at a `.pkg` would make that mount fail, so the key stays a DMG
  until the updater learns to run `installer`. A DMG Shift downloads itself arrives without
  a quarantine flag (`curl` sets none, and the handoff strips it anyway), which is why
  in-app updates never trigger the prompt — it is only the browser download on a new
  machine that does.

Closing this for good means an Apple Developer ID: sign with `Developer ID Application`,
notarize with `notarytool`, and staple the ticket. Then both containers open on double-click
with no pane to visit, and Screen Recording approval stops resetting between builds because
the identity stops changing.

## Publishing

```bash
pnpm tauri build --target universal-apple-darwin --bundles app,dmg
scripts/macos-pkg.sh src-tauri/target/universal-apple-darwin/release/bundle/macos/Shift.app \
                     artifacts/Shift-macOS-universal.pkg   # the first-install container
printf 'Fixes the ring on external displays.\n' > /tmp/notes.md
scripts/publish-release.sh --notes /tmp/notes.md     # hash, commit, push
```

`scripts/publish-release.sh` hashes every file in `artifacts/`, builds the manifest,
writes the tree with git plumbing (`update-index` → `write-tree` → `commit-tree` →
`update-ref`, through a temporary `GIT_INDEX_FILE` so the working tree is untouched) and
force-pushes `releases`. The manifest and the pushed bytes cannot disagree, because the
same Python pass that walks the files produces both.

| Flag | Effect |
| --- | --- |
| `--notes FILE` | Release notes shown in the Updates panel |
| `--key FILE` | Sign the manifest with this ed25519 key (`SHIFT_RELEASE_KEY` also works) |
| `--dry-run` | Print the manifest and the tree that would be pushed; write nothing |
| `--no-push` | Commit locally on `releases` but leave the remote alone |
| `--branch NAME` | Publish to another channel instead of `releases` |

Recognised installers: `Shift-macOS-universal.dmg` (macOS), `Shift-Setup-<version>-win-x64.exe`
and `Shift-Setup-<version>-win-arm64.exe` (Windows, keys `windows` and `windows-arm64`),
`*.AppImage` / `*.deb` (Linux). Other files in `artifacts/` — the macOS `.pkg`, the portable
ZIPs, `SHA256SUMS.txt`, the `README-Windows-<arch>.txt` notes — travel with the branch and
are downloadable from it, but are not offered as installers by the manifest. The `.pkg`
carries no key on purpose: the `macos` key must stay a DMG so the in-app handoff can mount
it ("macOS packaging"). Dotfiles and an old manifest are skipped.

Both Windows architectures come from one command, which cross-compiles each target,
checks the PE machine field of the binary it just built (`0x8664` for x64, `0xAA64` for
ARM64) so a wrong `--target` cannot ship an x64 EXE inside an arm64 package, and gates the
installer to the matching machine:

```bash
scripts/build-windows-release.sh                    # both architectures
scripts/build-windows-release.sh --arch arm64       # one of them
scripts/build-windows-release.sh --skip-build       # repackage existing EXEs
```

## Signing the channel

Integrity says the download matches the manifest; authenticity says the manifest came
from you. `scripts/release-key.py` closes that gap with one ed25519 key.

```bash
scripts/release-key.py keygen ~/.config/shift-release/release.ed25519
# -> prints the hex public key to bake into the build

export SHIFT_RELEASE_KEY=~/.config/shift-release/release.ed25519
pnpm tauri build --target universal-apple-darwin --bundles app,dmg   # macOS
scripts/build-windows-release.sh          # both Windows architectures
scripts/publish-release.sh --key "$SHIFT_RELEASE_KEY"
```

Give the release build `SHIFT_UPDATE_PUBKEY` (the hex key) and it can verify the feed; give
it `SHIFT_RELEASE_KEY` (the private key) and `build-windows-release.sh` derives the public
key from it, so a cross-build can never be packaged with a key that does not match the
signature `publish-release.sh --key` produces. Without either, the script prints
`no release key: this build will report the feed as unsigned` rather than letting it pass
for a quieter log.

The signature is **detached**: `shift-update.sig` holds base64 over the manifest's exact
bytes, so verification needs no JSON canonicalisation rules and the manifest's formatting
stays free to change. It is fetched from the directory that already holds the manifest, so
a staging feed needs only its own manifest plus signature — a fork never configures a
second URL.

| Build | Feed | Result |
| --- | --- | --- |
| Key baked | signature verifies | update offered, panel says the signature was verified |
| Key baked | signature missing or wrong | check fails with that reason; nothing is offered |
| No key | any | integrity gate only; panel says this build cannot verify a signature |

The key is the one secret in this design. Losing it means publishing a new app build with
a new baked key; leaking it means an attacker can publish anything to the channel, exactly
as leaking push access to `releases` already does today. Keep it outside the repository —
`~/.config/shift-release/` is where the local publishing machine keeps it.

## Where a build looks for updates

`src-tauri/build.rs` derives the feed from the `origin` remote at build time:

| Remote | Feed URL |
| --- | --- |
| Gitea / Forgejo | `https://host/owner/repo/raw/branch/releases/shift-update.json` |
| GitHub | `https://github.com/owner/repo/raw/releases/shift-update.json` |
| GitLab | `https://gitlab.example/owner/repo/-/raw/releases/shift-update.json` |

Two overrides exist, and neither needs a rebuild of the frontend:

- `SHIFT_UPDATE_PUBKEY` at build time bakes in the release key; anything that is not 32
  bytes of hex fails the build rather than silently producing an updater that trusts the
  feed anyway. A key that parses but is not a curve point is a runtime error, never a
  quiet downgrade to "no key".
- `SHIFT_UPDATE_URL` at build time bakes a different feed into the binary.
- `SHIFT_UPDATE_URL` in the environment of a running copy wins over the baked value,
  which is how a fork or a staging branch is tested.

A source tree with no remote builds an updater that reports "no feed" rather than
guessing at one.

## What the app does with it

1. Once a day, at startup (and on demand from Settings → **Updates**), the manifest is
   fetched with the system HTTP client — `/usr/bin/curl` on macOS and Linux,
   `Invoke-WebRequest` on Windows. The day of the last probe is stored in `settings.json`,
   so restarts cannot spend the day's check twice.
2. Versions are compared with semver rules, prereleases included: `0.2.0-alpha` sorts
   below `0.2.0`, so a test build does not offer to downgrade itself.
3. Download writes into a temp folder, then two gates run before anything can be
   installed: the byte length must equal the manifest's `size`, and the recomputed SHA256
   must equal the manifest's `sha256`. A mismatch deletes the file and reports the error,
   so a truncated or swapped installer never reaches the installer step.
4. **Install & relaunch** applies the verified artifact and restarts Shift on it, as
   described in the next section; **Discard** removes the download. When an update is
   available the tray menu gains a row that opens Settings on the Updates tab.

Manifest `schema` is checked, and a newer schema than this build understands is refused
instead of being parsed optimistically. Unknown platform keys are ignored, so the
manifest can name a platform an older app has never heard of. The Updates panel always
states which of the two properties the current feed satisfies, so an unsigned channel
never looks signed.

## Applying an update

Neither operating system lets a running program replace its own executable, so the work
belongs to a helper that outlives the app. `update_install` writes a script into the
download folder, starts it detached, and exits about half a second later; the script waits
for that pid to disappear and only then touches the installed copy.

| Platform | What the helper does |
| --- | --- |
| macOS | Mounts the downloaded `.dmg`, `ditto`s the new bundle next to the old one, swaps them with two `mv` calls, removes the quarantine flag, detaches the image, and `open`s the result |
| Windows | Waits for `shift.exe` to leave the process table, runs the installer with `/S`, then starts the `shift.exe` the installer wrote |
| Linux, and a macOS binary run outside a bundle | Nothing is replaced; the artifact is opened instead, and the panel says so |

Both scripts are written so a failure cannot end with Shift simply closed: every macOS
failure path still `open`s the bundle it had, and the swap is `move aside, move in`, so a
copy that fails halfway leaves the previous version where it was. Before the handoff
starts, macOS writes a probe file next to the bundle — a root-owned copy in
`/Applications` is reported as "open the downloaded installer yourself" rather than
quietly quitting on an update that could not be applied.

The Windows installer is the same one a user runs by hand, so the update path cannot drift
from the manual one. Because the silent install comes from Shift itself, `${Silent}` skips
the interactive "Shift is running, close it?" box: the app already told the user it is
about to restart, and a box nobody can answer would abort the update.

This is not a delta update and not a signature check on the running binary: the same
SHA256 gate that guards a manual install is the only integrity gate, and the manifest
signature rules in "Signing the channel" apply unchanged.

Replacing the bundle gives it a new code-signing hash, and macOS keys permissions such as
Screen Recording to that hash. A release signed with the same identity as the copy it
replaces normally keeps its approvals; a locally signed build, where every build has its
own identity, can ask again after an update. That is the same consequence a manual
reinstall has, and it is why the notarized-build item in the roadmap is worth closing.

## Trust boundary

Two properties, two mechanisms:

- **Integrity of the installer** — byte length plus recomputed SHA256 against the
  manifest. Always enforced, in every build.
- **Authenticity of the manifest** — a detached ed25519 signature verified against a baked
  public key. Enforced in every build that was compiled with `SHIFT_UPDATE_PUBKEY`.

A build without a key falls back to the weaker trust it had before signing existed: the
HTTPS connection to the forge plus whoever can push to `releases`. That is the only
remaining gap for such a build, and the panel says so out loud. Developer ID signing plus
notarization for the macOS package — which macOS requires for downloaded apps regardless —
is still open and tracked in `docs/ROADMAP.md` under M5.

```bash
scripts/release-key.py verify ~/.config/shift-release/release.pub \
  manifest.json manifest.sig        # check a channel by hand
```

## Verifying a channel by hand

```bash
SHIFT_UPDATE_URL="https://host/owner/repo/raw/branch/releases/shift-update.json" \
  cargo test --lib update:: -- --ignored      # from src-tauri/
```

```bash
cargo test --lib update:: -- --ignored          # unsigned build: integrity only
SHIFT_UPDATE_PUBKEY=<hex> cargo test --lib update:: -- --ignored
```

The test fetches the real feed, checks the manifest signature when the build carries a
key, then downloads the artifact for this platform and architecture and checks size and
hash against what is on disk. A baked key that is valid but not the publisher's fails the
test, which is the proof that the check is real and not a pass-through.

## State of the published channel

Checked against the live `releases` branch of this repository:

Version **0.1.3**, checked by downloading each artifact back from the raw endpoint rather
than trusting the local copy:

| Key | Artifact | Verified from the remote |
| --- | --- | --- |
| `macos` | `Shift-macOS-universal.dmg` (55,528,662 B) | bytes downloaded, SHA256 matches the manifest |
| `windows` | `Shift-Setup-0.1.3-win-x64.exe` (24,517,835 B) | bytes downloaded, SHA256 matches the manifest |
| `windows-arm64` | not published | by choice — see "Supported release targets" |
| `linux` | not published | by choice: Linux is a build target here, not a shipped product |

`Shift-macOS-universal.pkg` sits on the same branch with no manifest key, for first installs
("macOS packaging"); it was hashed the same way and downloaded back to check.

`shift-update.sig` verifies over the manifest's exact bytes against the key baked into both
artifacts. The channel's home is derived from `origin` at build time, and this forge is
Gitea, so the raw form is `/raw/branch/releases/`.

Both artifacts carry the key and the feed URL, which is what makes an in-app upgrade
possible at all: a build whose binary predates those constants reports "this build has no
feed" and can never see the channel, however new it is.

The forge runs **no Actions**, so nothing on this list was built by CI: every artifact was
cross-compiled on one macOS host and pushed with `scripts/publish-release.sh`. Treat "pass
CI" in the contributing guide as a definition to run locally, not a gate that already ran.

A Linux release would slot in under the `linux` key with no code change — `artifact_keys_for`
already answers `linux` for that host — so publishing one is a build task, not a feature.
