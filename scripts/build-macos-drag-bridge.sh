#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
host_triple="$(rustc -vV | awk '/^host:/ { print $2 }')"
out_dir="$root_dir/src-tauri/binaries"
out_file="$out_dir/drag-bridge-$host_triple"
pdf_out_file="$out_dir/pdf-render-$host_triple"
doc_out_file="$out_dir/doc-render-$host_triple"
tmp_dir="$(mktemp -d)"
trap 'find "$tmp_dir" -type f -delete; rmdir "$tmp_dir"' EXIT

mkdir -p "$out_dir"

for arch in arm64 x86_64; do
  swiftc \
    -O \
    -target "$arch-apple-macosx13.0" \
    -framework AppKit \
    "$root_dir/native/macos/drag-bridge.swift" \
    -o "$tmp_dir/drag-bridge-$arch"
  swiftc \
    -O \
    -target "$arch-apple-macosx13.0" \
    -framework AppKit \
    -framework PDFKit \
    "$root_dir/native/macos/pdf-render.swift" \
    -o "$tmp_dir/pdf-render-$arch"
  swiftc \
    -O \
    -target "$arch-apple-macosx13.0" \
    -framework AppKit \
    "$root_dir/native/macos/doc-render.swift" \
    -o "$tmp_dir/doc-render-$arch"
done

lipo -create \
  "$tmp_dir/drag-bridge-arm64" \
  "$tmp_dir/drag-bridge-x86_64" \
  -output "$out_dir/drag-bridge-universal-apple-darwin"
lipo -create \
  "$tmp_dir/pdf-render-arm64" \
  "$tmp_dir/pdf-render-x86_64" \
  -output "$out_dir/pdf-render-universal-apple-darwin"
lipo -create \
  "$tmp_dir/doc-render-arm64" \
  "$tmp_dir/doc-render-x86_64" \
  -output "$out_dir/doc-render-universal-apple-darwin"

cp "$tmp_dir/drag-bridge-arm64" "$out_dir/drag-bridge-aarch64-apple-darwin"
cp "$tmp_dir/pdf-render-arm64" "$out_dir/pdf-render-aarch64-apple-darwin"
cp "$tmp_dir/doc-render-arm64" "$out_dir/doc-render-aarch64-apple-darwin"
cp "$tmp_dir/drag-bridge-x86_64" "$out_dir/drag-bridge-x86_64-apple-darwin"
cp "$tmp_dir/pdf-render-x86_64" "$out_dir/pdf-render-x86_64-apple-darwin"
cp "$tmp_dir/doc-render-x86_64" "$out_dir/doc-render-x86_64-apple-darwin"
if [[ "$host_triple" == "aarch64-apple-darwin" ]]; then
  cp "$tmp_dir/drag-bridge-arm64" "$out_file"
  cp "$tmp_dir/pdf-render-arm64" "$pdf_out_file"
  cp "$tmp_dir/doc-render-arm64" "$doc_out_file"
else
  cp "$tmp_dir/drag-bridge-x86_64" "$out_file"
  cp "$tmp_dir/pdf-render-x86_64" "$pdf_out_file"
  cp "$tmp_dir/doc-render-x86_64" "$doc_out_file"
fi
chmod +x "$out_file" "$pdf_out_file" \
  "$doc_out_file" \
  "$out_dir/drag-bridge-aarch64-apple-darwin" \
  "$out_dir/pdf-render-aarch64-apple-darwin" \
  "$out_dir/doc-render-aarch64-apple-darwin" \
  "$out_dir/drag-bridge-x86_64-apple-darwin" \
  "$out_dir/pdf-render-x86_64-apple-darwin" \
  "$out_dir/doc-render-x86_64-apple-darwin" \
  "$out_dir/drag-bridge-universal-apple-darwin" \
  "$out_dir/pdf-render-universal-apple-darwin" \
  "$out_dir/doc-render-universal-apple-darwin"

echo "Built $out_file"
echo "Built $pdf_out_file"
echo "Built $doc_out_file"
echo "Built universal macOS sidecars"
