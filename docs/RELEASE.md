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
| `macos` | yes | One universal bundle, Apple silicon and Intel, shipped as a DMG. Verified on a fresh machine by dragging it to /Applications — see "macOS packaging" |
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

Shift builds are **ad-hoc signed** (`signingIdentity: "-"` in `tauri.conf.json`) and are
**not notarized** — notarization needs an Apple Developer ID account, which this project
does not have. Gatekeeper therefore cannot vouch for any Shift build, and what a fresh
machine says depends on how the binary is signed:

| Signing | What a fresh machine says |
| --- | --- |
| Ad-hoc (`Signature=adhoc`) — **current** | "cannot be opened because it is from an unidentified developer", and the box offers a way through: Cancel, then right-click → 打开, or 允许 in 系统设置 → 隐私与安全性 |
| Self-signed certificate | "cannot be opened because it is damaged" / reported as malware, with **only 移至废纸篓** — no override, no pane entry to approve |
| Developer ID + notarized | Opens on double-click, no prompt |

The middle row is what a release shipped with, and it is a trap because it looks healthy
locally: `codesign` reports `valid on disk` and `satisfies its Designated Requirement`, and
`spctl` accepts it on the build host. The certificate ("Shift Local Signing") is self-issued,
`CA:FALSE`, and present in only that one Mac's keychain, so on any other machine it cannot
chain to a trusted root — and an unchainable signature on a quarantined download is a hard
refusal rather than a prompt.

One correction worth keeping, because the obvious story is wrong: every DMG this channel has
ever published carries that same `Authority=Shift Local Signing`, including the earliest 0.1.0
ones, because Tauri picks a keychain identity itself when `signingIdentity` is unset. So
"older builds were ad-hoc" is **not** why one machine accepted them — a machine that had been
shown the app before may already trust the certificate, and Gatekeeper's own heuristics have
changed across macOS versions. What is verified is the direction of the fix: an ad-hoc binary
is reported as an unidentified developer with a way through, which is the behaviour this
project wants, and no release is worse off for losing a certificate no other Mac can check.
Reusing that identity again would mean installing it **and trusting it** on every machine,
which is not something to ask of a user.

So: ad-hoc, and one container. The channel ships **`Shift-macOS-universal.dmg`** and nothing
else on macOS — the earlier `.pkg` companion was dropped after the DMG was verified to work
on a fresh machine, and one file per platform is less to keep straight.

| Situation | What happens |
| --- | --- |
| A machine installing Shift for the first time | Double-click the DMG, drag `Shift.app` to /Applications. The app keeps the browser's quarantine flag, so the **first launch** needs the one approval above: right-click → 打开, or 允许 in 系统设置 → 隐私与安全性. Every launch after that is normal. |
| An installed Shift updating itself | `update_install`'s handoff downloads the DMG, mounts it with `hdiutil`, copies the bundle out and drops the quarantine flag. Shift's own downloader sets no quarantine flag (`curl` sets none, and the handoff strips it anyway), so **in-app updates never prompt**. |

The `macos` manifest key is a DMG for the same reason: the in-app handoff mounts the image,
so pointing the key at a package would break every in-app update. That was the one real
constraint the container had to satisfy, and a DMG satisfies it.

`scripts/macos-pkg.sh` — which staged the bundle under an `/Applications` root and ran
`pkgbuild` — is kept in the tree but is no longer part of a release: it built a container
with no manifest key, and the DMG covers the first install anyway.

Closing this for good means an Apple Developer ID: `Developer ID Application` +
`notarytool` + `stapler staple`. Then the DMG opens on double-click with no pane to visit,
and the privacy grants keyed to that identity — Screen Recording, and Local Network, which
is what device discovery needs — stop resetting between builds. That reset is not
theoretical: a 0.1.4 user found the device list empty after upgrading, with nothing in the
UI to say why.

## Publishing

```bash
pnpm tauri build --target universal-apple-darwin --bundles app,dmg
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
`*.AppImage` / `*.deb` (Linux). Other files in `artifacts/` — the portable ZIPs,
`SHA256SUMS.txt`, the `README-Windows-<arch>.txt` notes and `README-macOS-universal.txt`,
the packaging note explaining the ad-hoc signature and how to verify a download — travel
with the branch and are downloadable from it, but are not offered as installers by the
manifest. Dotfiles and an old manifest are skipped.

`README-macOS-universal.txt` is tracked in the repository as well as pushed to the channel,
so the scheme is documented where a contributor reads it and not only next to a build
product. Every claim in it was checked against the shipped artifacts: the bundle inside the
published DMG is `Signature=adhoc`, and `shasum -a 256 -c SHA256SUMS.txt` passes on the
published set.

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

Version **0.1.6**, checked by downloading each artifact back from the raw endpoint rather
than trusting the local copy:

| Key | Artifact | Verified from the remote |
| --- | --- | --- |
| `macos` | `Shift-macOS-universal.dmg` (55,539,236 B) | bytes downloaded, SHA256 matches the manifest |
| `windows` | `Shift-Setup-0.1.6-win-x64.exe` (24,494,638 B) | bytes downloaded, SHA256 matches the manifest |
| `windows-arm64` | not published | by choice — see "Supported release targets" |
| `linux` | not published | by choice: Linux is a build target here, not a shipped product |

The retired `Shift-macOS-universal.pkg` left the branch with this publish: the branch is
rebuilt from `artifacts/` on every run, so dropping it from the build was enough. macOS now
ships the one container ("macOS packaging").

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
