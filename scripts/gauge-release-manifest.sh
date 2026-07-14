#!/usr/bin/env bash
set -o errexit -o nounset -o pipefail

function usage() {
  echo "Usage: $0 ARTIFACT_DIR OUTPUT_JSON SOURCE_COMMIT OPTIMIZER_IMAGE AUDIT_REPORT CHAIN_REPORT"
}

if [ "$#" -ne 6 ]; then
  usage >&2
  exit 2
fi

ARTIFACT_DIR=$1
OUTPUT_JSON=$2
SOURCE_COMMIT=$3
OPTIMIZER_IMAGE=$4
AUDIT_REPORT=$5
CHAIN_REPORT=$6

for command in cargo jq sha256sum stat; do
  command -v "$command" >/dev/null || {
    echo "required command not found: $command" >&2
    exit 1
  }
done

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
EVIDENCE_VALIDATOR=$SCRIPT_DIR/validate-gauge-release-evidence.sh
if [ ! -x "$EVIDENCE_VALIDATOR" ]; then
  echo "release evidence validator is missing or not executable: $EVIDENCE_VALIDATOR" >&2
  exit 1
fi

if ! [[ "$SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
  echo "source commit must be a full lowercase 40-character Git SHA" >&2
  exit 1
fi
if ! [[ "$OPTIMIZER_IMAGE" =~ ^[^[:space:]@]+@sha256:[0-9a-f]{64}$ ]]; then
  echo "optimizer image must be pinned as repository@sha256:digest" >&2
  exit 1
fi
if [ "$(git rev-parse HEAD)" != "$SOURCE_COMMIT" ]; then
  echo "source commit does not match the checked-out HEAD" >&2
  exit 1
fi
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "release manifests may only be generated from an unmodified source tree" >&2
  exit 1
fi
ROOT=$(git rev-parse --show-toplevel)
ARTIFACT_DIR_ABS=$(cd "$ARTIFACT_DIR" && pwd)
case "$ARTIFACT_DIR_ABS" in
  "$ROOT"/*) ARTIFACT_DIR_REL=${ARTIFACT_DIR_ABS#"$ROOT"/} ;;
  *) ARTIFACT_DIR_REL='' ;;
esac
while IFS= read -r untracked; do
  if [ -z "$ARTIFACT_DIR_REL" ] || [[ "$untracked" != "$ARTIFACT_DIR_REL"/* ]]; then
    echo "untracked source file is not allowed in a release checkout: $untracked" >&2
    exit 1
  fi
done < <(git ls-files --others --exclude-standard)
for file in "$AUDIT_REPORT" "$CHAIN_REPORT"; do
  if [ ! -s "$file" ]; then
    echo "required signed-off evidence file is missing or empty: $file" >&2
    exit 1
  fi
  git ls-files --error-unmatch "$file" >/dev/null 2>&1 || {
    echo "evidence file must be tracked by the source commit: $file" >&2
    exit 1
  }
done
"$EVIDENCE_VALIDATOR" "$AUDIT_REPORT" "$CHAIN_REPORT" "$SOURCE_COMMIT"

METADATA=$(cargo metadata --locked --no-deps --format-version 1)

function package_version() {
  jq -er --arg package "$1" \
    '.packages[] | select(.name == $package) | .version' <<<"$METADATA"
}

function file_sha256() {
  sha256sum "$1" | awk '{print $1}'
}

function file_size() {
  stat -c '%s' "$1"
}

ARTIFACTS='[]'
while IFS='|' read -r package artifact cw2_identity schema license; do
  wasm="$ARTIFACT_DIR/$artifact"
  if [ ! -s "$wasm" ]; then
    echo "required Wasm artifact is missing or empty: $wasm" >&2
    exit 1
  fi
  if [ ! -s "$schema" ]; then
    echo "required schema is missing or empty: $schema" >&2
    exit 1
  fi

  chain_checksum=$(jq -er --arg package "$package" '
    [.deployments[].artifacts[] | select(.package == $package) | .checksum]
    | unique
    | if length == 1 then .[0] else error("deployment checksums disagree") end
  ' "$CHAIN_REPORT")
  chain_size=$(jq -er --arg package "$package" '
    [.deployments[].artifacts[] | select(.package == $package) | .wasm_size_bytes]
    | unique
    | if length == 1 then .[0] else error("deployment Wasm sizes disagree") end
  ' "$CHAIN_REPORT")
  if [ "$chain_checksum" != "$(file_sha256 "$wasm")" ]; then
    echo "chain report checksum does not match release artifact: $package" >&2
    exit 1
  fi
  if [ "$chain_size" != "$(file_size "$wasm")" ]; then
    echo "chain report Wasm size does not match release artifact: $package" >&2
    exit 1
  fi

  ARTIFACTS=$(jq -c \
    --arg package "$package" \
    --arg version "$(package_version "$package")" \
    --arg artifact "$artifact" \
    --arg cw2_identity "$cw2_identity" \
    --arg license "$license" \
    --arg wasm_sha256 "$(file_sha256 "$wasm")" \
    --argjson wasm_size "$(file_size "$wasm")" \
    --arg schema "$schema" \
    --arg schema_sha256 "$(file_sha256 "$schema")" \
    '. + [{
      package: $package,
      version: $version,
      artifact: $artifact,
      cw2_identity: $cw2_identity,
      license: $license,
      wasm_sha256: $wasm_sha256,
      wasm_size_bytes: $wasm_size,
      schema: $schema,
      schema_sha256: $schema_sha256
    }]' <<<"$ARTIFACTS")
done <<'EOF'
gauge-orchestrator|gauge_orchestrator.wasm|crates.io:gauge|contracts/gauges/gauge/schema/gauge-orchestrator.json|Apache-2.0
gauge-adapter|gauge_adapter.wasm|crates.io:marketing-gauge-adapter|contracts/gauges/gauge-adapter/schema/gauge-adapter.json|Apache-2.0
gauge-budget-allocator|gauge_budget_allocator.wasm|crates.io:gauge-budget-allocator|contracts/gauges/budget-allocator/schema/gauge-budget-allocator.json|BSD-3-Clause
EOF

mkdir -p "$(dirname "$OUTPUT_JSON")"
jq -n \
  --arg format_version "1" \
  --arg source_commit "$SOURCE_COMMIT" \
  --arg optimizer_image "$OPTIMIZER_IMAGE" \
  --arg rust_toolchain "1.81.0" \
  --arg cosmwasm_check "1.5.11" \
  --arg audit_report "$AUDIT_REPORT" \
  --arg audit_sha256 "$(file_sha256 "$AUDIT_REPORT")" \
  --arg chain_report "$CHAIN_REPORT" \
  --arg chain_sha256 "$(file_sha256 "$CHAIN_REPORT")" \
  --argjson artifacts "$ARTIFACTS" \
  '{
    format_version: $format_version,
    source_commit: $source_commit,
    optimizer_image: $optimizer_image,
    rust_toolchain: $rust_toolchain,
    cosmwasm_check: $cosmwasm_check,
    audit: {report: $audit_report, sha256: $audit_sha256},
    chain_test: {report: $chain_report, sha256: $chain_sha256},
    artifacts: $artifacts
  }' >"$OUTPUT_JSON"

jq -e '
  (.artifacts | length) == 3 and
  ([.artifacts[].package] | unique | length) == 3 and
  ([.artifacts[].artifact] | unique | length) == 3 and
  ([.artifacts[].cw2_identity] | unique | length) == 3
' "$OUTPUT_JSON" >/dev/null

echo "wrote $OUTPUT_JSON"
