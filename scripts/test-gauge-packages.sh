#!/usr/bin/env bash
set -o errexit -o nounset -o pipefail

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$WORK_DIR/target"}

METADATA="$WORK_DIR/metadata.json"
PATCH_CONFIG="$WORK_DIR/patches.toml"
cargo metadata --locked --no-deps --format-version 1 >"$METADATA"

{
  echo '[patch.crates-io]'
  jq -r '
    .packages[]
    | select(.source == null)
    | "\(.name) = { path = \(.manifest_path | sub("/Cargo.toml$"; "") | @json) }"
  ' "$METADATA"
} >"$PATCH_CONFIG"

for package in \
  gauge-interface \
  gauge-orchestrator \
  gauge-adapter \
  gauge-budget-allocator
do
  package_dir="$WORK_DIR/$package"
  output_dir="$package_dir/output"
  extracted_dir="$package_dir/extracted"
  mkdir -p "$output_dir" "$extracted_dir"

  cargo package --locked --allow-dirty --no-verify \
    --config "$PATCH_CONFIG" \
    --target-dir "$output_dir" \
    -p "$package"

  crate=$(find "$output_dir/package" -maxdepth 1 -type f \
    -name "$package-*.crate" -print -quit)
  if [ -z "$crate" ]; then
    echo "missing packaged tarball for $package" >&2
    exit 1
  fi

  tar -xzf "$crate" -C "$extracted_dir"
  source_dir=$(find "$extracted_dir" -mindepth 1 -maxdepth 1 -type d \
    -name "$package-*" -print -quit)
  if [ -z "$source_dir" ]; then
    echo "missing extracted source directory for $package" >&2
    exit 1
  fi

  mkdir -p "$source_dir/.cargo"
  # Contract packages include a legacy `.cargo/config`; Cargo gives it
  # precedence over `config.toml`, so replace it in the disposable extraction.
  cp "$PATCH_CONFIG" "$source_dir/.cargo/config"
  (
    cd "$source_dir"
    cargo test --all-targets
  )
done

echo 'gauge packaged tarballs build and test successfully'
