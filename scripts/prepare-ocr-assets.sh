#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
target="$root_dir/public/tesseract"
core_source="$root_dir/node_modules/.pnpm/tesseract.js-core@7.0.0/node_modules/tesseract.js-core"

mkdir -p "$target/lang" "$target/core"
cp "$root_dir/node_modules/tesseract.js/dist/worker.min.js" "$target/worker.min.js"
cp "$core_source"/tesseract-core*.wasm.js "$target/core/"
cp "$core_source"/tesseract-core*.wasm "$target/core/"
cp -L "$root_dir/node_modules/@tesseract.js-data/eng/4.0.0_best_int/eng.traineddata.gz" "$target/lang/"
cp -L "$root_dir/node_modules/@tesseract.js-data/chi_sim/4.0.0_best_int/chi_sim.traineddata.gz" "$target/lang/"
