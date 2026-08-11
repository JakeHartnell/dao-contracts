#!/usr/bin/env bash
set -o errexit -o nounset -o pipefail

ROOT=$(git rev-parse --show-toplevel)
OUTPUT_DIR=${1:-"$ROOT/artifacts/gauges"}
OPTIMIZER_IMAGE='cosmwasm/optimizer@sha256:7e0b9229c1a4118d0c9a2af2e7f5d95a91f264c26a2ce5681c779926e74d7f85'

if [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=all)" ]; then
  if [ "${GAUGE_ALLOW_DIRTY_BUILD:-0}" != "1" ]; then
    echo "refusing release build from a dirty worktree" >&2
    exit 1
  fi
  echo "warning: dirty candidate build; no release manifest may be generated" >&2
fi

mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR=$(cd "$OUTPUT_DIR" && pwd)
rm -f \
  "$OUTPUT_DIR/gauge_orchestrator.wasm" \
  "$OUTPUT_DIR/gauge_adapter.wasm" \
  "$OUTPUT_DIR/gauge_budget_allocator.wasm" \
  "$OUTPUT_DIR/checksums.txt" \
  "$OUTPUT_DIR/sizes.txt" \
  "$OUTPUT_DIR/build-tools.txt" \
  "$OUTPUT_DIR/build-provenance.txt"

CONTAINER="gauge-optimizer-$$"
function cleanup() {
  docker rm --force "$CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker create \
  --name "$CONTAINER" \
  --entrypoint sh \
  "$OPTIMIZER_IMAGE" \
  -lc '
    set -o errexit -o nounset -o pipefail
    export PATH="/usr/local/cargo/bin:$PATH"
    rustup toolchain install 1.81.0 --profile minimal --target wasm32-unknown-unknown
    mkdir -p /out
    rustc +1.81.0 --version > /out/build-tools.txt
    cargo +1.81.0 --version >> /out/build-tools.txt
    wasm-opt --version >> /out/build-tools.txt
    cd /code
    export CARGO_TARGET_DIR=/target
    export RUSTFLAGS="-C link-arg=-s -C link-arg=--allow-undefined"
    cargo +1.81.0 build --locked --release --lib \
      --target wasm32-unknown-unknown \
      -p gauge-orchestrator \
      -p gauge-adapter \
      -p gauge-budget-allocator
    for artifact in \
      gauge_orchestrator.wasm \
      gauge_adapter.wasm \
      gauge_budget_allocator.wasm
    do
      wasm-opt -Os "/target/wasm32-unknown-unknown/release/$artifact" \
        -o "/out/$artifact"
    done
  ' >/dev/null

tar \
  --exclude=.git \
  --exclude=target \
  --exclude=artifacts \
  --exclude='*.wasm' \
  -C "$ROOT" -cf - . | docker cp - "$CONTAINER:/code"
docker start --attach "$CONTAINER"
docker cp "$CONTAINER:/out/." "$OUTPUT_DIR"
cleanup
trap - EXIT

(
  cd "$OUTPUT_DIR"
  sha256sum \
    gauge_orchestrator.wasm \
    gauge_adapter.wasm \
    gauge_budget_allocator.wasm >checksums.txt
  wc -c \
    gauge_orchestrator.wasm \
    gauge_adapter.wasm \
    gauge_budget_allocator.wasm >sizes.txt
)

{
  echo "source_commit=$(git -C "$ROOT" rev-parse HEAD)"
  echo "optimizer_image=$OPTIMIZER_IMAGE"
  echo "rust_toolchain=1.81.0"
  echo "cosmwasm_check=1.5.11"
  echo "dirty_worktree=${GAUGE_ALLOW_DIRTY_BUILD:-0}"
} >"$OUTPUT_DIR/build-provenance.txt"

if [ "${GAUGE_ALLOW_DIRTY_BUILD:-0}" = "1" ]; then
  printf '%s\n' 'NOT FOR RELEASE: built from a dirty worktree' >"$OUTPUT_DIR/BUILD_NOT_FOR_RELEASE"
else
  rm -f "$OUTPUT_DIR/BUILD_NOT_FOR_RELEASE"
fi

echo "wrote optimized gauge artifacts to $OUTPUT_DIR"
