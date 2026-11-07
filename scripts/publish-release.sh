#!/bin/bash
# Publish the built artifacts onto the git branch that IS the update channel.
#
#   scripts/publish-release.sh [--notes FILE] [--key FILE] [--dry-run]
#                              [--no-push] [--branch NAME]
#
# The two release installers in artifacts/ are copied onto an orphan `releases` branch together
# with `shift-update.json`, which names each file, its size and its SHA256. A running
# Shift reads that manifest from the forge's raw endpoint, downloads its own
# platform's artifact and refuses to launch it unless the recomputed hash matches, so
# this script is the only thing needed to ship a release: no release service, no
# object storage, no credentials beyond what `git push` already uses.
#
# With `--key` (or SHIFT_RELEASE_KEY) the manifest is also signed, and the base64
# ed25519 signature is pushed as `shift-update.sig`. A Shift built with the matching
# SHIFT_UPDATE_PUBKEY refuses a manifest that does not verify, so the channel proves
# authenticity and not just integrity. Without a key the publish still works and the
# app falls back to trusting the feed over HTTPS, and says so in the Updates panel.
#
# The branch is rebuilt as a fresh root commit on every run and force-pushed, so it
# always holds exactly one release instead of accumulating gigabytes of history.
set -euo pipefail
cd "$(dirname "$0")/.."

BRANCH="${SHIFT_RELEASE_BRANCH:-releases}"
MANIFEST="shift-update.json"
NOTES_FILE=""
KEY_FILE="${SHIFT_RELEASE_KEY:-}"
DRY_RUN=0
NO_PUSH=0

while [ $# -gt 0 ]; do
  case "$1" in
    --notes|--key|--branch)
      if [ "$#" -lt 2 ] || [ -z "${2:-}" ]; then
        echo "$1 needs a value" >&2
        exit 2
      fi
      case "$1" in
        --notes) NOTES_FILE="$2" ;;
        --key) KEY_FILE="$2" ;;
        --branch) BRANCH="$2" ;;
      esac
      shift 2
      ;;
    --dry-run) DRY_RUN=1; shift ;;
    --no-push) NO_PUSH=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[ -n "$BRANCH" ] || { echo "--branch cannot be empty" >&2; exit 2; }
[ -z "$NOTES_FILE" ] || [ -f "$NOTES_FILE" ] || {
  echo "notes file not found: $NOTES_FILE" >&2
  exit 2
}
[ -z "$KEY_FILE" ] || [ -f "$KEY_FILE" ] || {
  echo "release key not found: $KEY_FILE" >&2
  exit 2
}

VERSION=$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
PLAN=$(mktemp /tmp/shift-release-plan.XXXXXX)
INDEX_FILE=$(mktemp /tmp/shift-release-index.XXXXXX)
trap 'rm -f "$PLAN" "$INDEX_FILE"' EXIT

# The plan is: one JSON manifest document, then a NUL-separated list of the files
# that back it. Keeping the hashing in Python means the manifest and the pushed
# bytes cannot disagree.
python3 - "$VERSION" "$MANIFEST" "$NOTES_FILE" > "$PLAN" <<'PY'
import datetime, hashlib, json, pathlib, re, sys

version, manifest_name, notes_file = sys.argv[1:4]
artifacts = pathlib.Path("artifacts")
if not artifacts.is_dir():
    sys.exit("artifacts/ is missing; build a release first.")
notes = pathlib.Path(notes_file).read_text(encoding="utf-8").strip() if notes_file else ""

# The channel intentionally has exactly two installers. Versioned, exact names keep
# a stale DMG or an old architecture from slipping into a new release. Supporting
# files such as the checksums and readmes still travel with the branch.
PATTERNS = [
    ("macos", rf"^Shift-{re.escape(version)}-macOS-universal\.dmg$"),
    ("windows", rf"^Shift-Setup-{re.escape(version)}-win-x64\.exe$"),
]
candidates = sorted(p.name for p in artifacts.iterdir() if p.is_file())

def digest(name):
    path = artifacts / name
    return hashlib.sha256(path.read_bytes()).hexdigest(), path.stat().st_size

manifest = {
    "schema": 1,
    "version": version,
    "date": datetime.date.today().isoformat(),
    "notes": notes,
    "artifacts": {},
}
chosen = []
for platform, pattern in PATTERNS:
    match = next((name for name in candidates if re.search(pattern, name)), None)
    if not match:
        sys.exit(
            f"missing installer for {platform} at version {version}; expected "
            f"Shift-{version}-macOS-universal.dmg and "
            f"Shift-Setup-{version}-win-x64.exe"
        )
    hash_value, size = digest(match)
    manifest["artifacts"][platform] = {"file": match, "sha256": hash_value, "size": size}
    chosen.append(match)

unexpected = [
    name
    for name in candidates
    if re.search(r"\.(dmg|exe|msi|pkg|zip|AppImage|deb)$", name, re.IGNORECASE)
    and name not in chosen
]
if unexpected:
    sys.exit(
        "only the two versioned installers may be published; remove: "
        + ", ".join(unexpected)
    )

payload = json.dumps(manifest, indent=2, ensure_ascii=False) + "\n"
sys.stdout.write(payload)
sys.stdout.write("\0")
for name in candidates:
    # The manifest and its signature are regenerated each run; dotfiles are not
    # build products.
    if name in {manifest_name, "shift-update.sig"} or name.startswith("."):
        continue
    hash_value, size = digest(name)
    sys.stdout.write(f"{name}\t{hash_value}\t{size}\0")
PY

MANIFEST_JSON=$(python3 -c "import sys;print(sys.stdin.read().split('\0')[0],end='')" < "$PLAN")
echo "== manifest for $VERSION =="
printf '%s\n' "$MANIFEST_JSON"

MANIFEST_PATH=$(mktemp /tmp/shift-manifest.XXXXXX)
SIGNATURE_PATH=$(mktemp /tmp/shift-sig.XXXXXX)
trap 'rm -f "$PLAN" "$INDEX_FILE" "$MANIFEST_PATH" "$SIGNATURE_PATH"' EXIT
printf '%s\n' "$MANIFEST_JSON" > "$MANIFEST_PATH"

# The signature covers the manifest file's own bytes, which are the bytes a client
# fetches, so verification needs no JSON canonicalisation rules.
if [ -n "$KEY_FILE" ]; then
  python3 scripts/release-key.py sign "$KEY_FILE" "$MANIFEST_PATH" > "$SIGNATURE_PATH"
  echo "== manifest signed with $KEY_FILE =="
fi

echo "== building the $BRANCH tree =="
export GIT_INDEX_FILE=$INDEX_FILE
git read-tree --empty
git update-index --add --cacheinfo "100644,$(git hash-object -w "$MANIFEST_PATH"),$MANIFEST"
if [ -s "$SIGNATURE_PATH" ]; then
  git update-index --add --cacheinfo "100644,$(git hash-object -w "$SIGNATURE_PATH"),shift-update.sig"
fi
while IFS=$'\t' read -r -d '' name hash_value size; do
  [ -n "$name" ] || continue
  git update-index --add --cacheinfo "100644,$(git hash-object -w "artifacts/$name"),$name"
  printf '  %-38s %10s B  %s…\n' "$name" "$size" "${hash_value:0:16}"
done < <(python3 -c "
import sys
chunks = sys.stdin.read().split('\0')
for entry in chunks[1:]:
    if entry.strip():
        sys.stdout.write(entry + '\0')
" < "$PLAN")
TREE=$(git write-tree)
COMMIT=$(git commit-tree "$TREE" -m "release $VERSION")
unset GIT_INDEX_FILE

if [ "$DRY_RUN" = 1 ]; then
  echo "== dry run: $COMMIT would become $BRANCH =="
  git ls-tree -rl "$COMMIT" | while read -r mode type hash size name; do
    printf '  %12s B  %s\n' "$size" "$name"
  done
  exit 0
fi

if git rev-parse --verify -q "refs/heads/$BRANCH" >/dev/null; then
  echo "== replacing $(git rev-parse --short "$BRANCH") =="
fi
git update-ref "refs/heads/$BRANCH" "$COMMIT"
echo "committed $VERSION as $(git rev-parse --short "$BRANCH")"

if [ "$NO_PUSH" = 1 ]; then
  echo "--no-push: committed locally on $BRANCH"
  exit 0
fi

echo "== pushing $BRANCH =="
git push --force origin "$BRANCH"
echo "published $VERSION"
