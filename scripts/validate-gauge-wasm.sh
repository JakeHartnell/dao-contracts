#!/usr/bin/env bash
set -o errexit -o nounset -o pipefail

ARTIFACT_DIR=${1:?usage: validate-gauge-wasm.sh ARTIFACT_DIR}
MAX_WASM_SIZE_BYTES=800000

function validate_artifact() {
  local artifact=$1
  local expected_exports=$2
  local path="$ARTIFACT_DIR/$artifact"

  if [ ! -f "$path" ]; then
    echo "missing Wasm artifact: $path" >&2
    return 1
  fi

  local size
  size=$(wc -c <"$path")
  if [ "$size" -gt "$MAX_WASM_SIZE_BYTES" ]; then
    echo "$artifact is $size bytes; limit is $MAX_WASM_SIZE_BYTES" >&2
    return 1
  fi

  local actual_exports
  actual_exports=$(node -e '
    const fs = require("fs");
    const module = new WebAssembly.Module(fs.readFileSync(process.argv[1]));
    process.stdout.write(JSON.stringify(
      WebAssembly.Module.exports(module).map(({ name }) => name).sort()
    ));
  ' "$path")

  if [ "$actual_exports" != "$expected_exports" ]; then
    echo "unexpected exports for $artifact" >&2
    echo "expected: $expected_exports" >&2
    echo "actual:   $actual_exports" >&2
    return 1
  fi
}

COMMON='["__data_end","__heap_base","allocate","deallocate","execute","instantiate","interface_version_8","memory"'
CAPABILITIES=',"query","requires_cosmwasm_1_1","requires_cosmwasm_1_2","requires_iterator","requires_stargate"]'

validate_artifact \
  gauge_orchestrator.wasm \
  "$COMMON,\"migrate\",\"query\",\"reply\",\"requires_cosmwasm_1_1\",\"requires_cosmwasm_1_2\",\"requires_iterator\",\"requires_stargate\"]"
validate_artifact \
  gauge_adapter.wasm \
  "$COMMON,\"migrate\"$CAPABILITIES"
validate_artifact \
  gauge_budget_allocator.wasm \
  "$COMMON$CAPABILITIES"

echo "gauge Wasm sizes and exports are valid (limit: $MAX_WASM_SIZE_BYTES bytes)"
