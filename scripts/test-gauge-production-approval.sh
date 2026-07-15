#!/usr/bin/env bash
set -o errexit -o nounset -o pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
VALIDATOR="$ROOT/scripts/validate-gauge-production-approval.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
COMMIT=0123456789abcdef0123456789abcdef01234567

jq -n --arg commit "$COMMIT" '{
  format_version: "1", source_commit: $commit,
  optimizer_image: ("cosmwasm/optimizer@sha256:" + ("d" * 64)),
  rust_toolchain: "1.81.0", cosmwasm_check: "1.5.11",
  audit: {report: "audit.json", sha256: ("e" * 64)},
  chain_test: {report: "chain.json", sha256: ("f" * 64)},
  artifacts: [
    {package: "gauge-orchestrator", artifact: "gauge_orchestrator.wasm", cw2_identity: "crates.io:gauge"},
    {package: "gauge-adapter", artifact: "gauge_adapter.wasm", cw2_identity: "crates.io:marketing-gauge-adapter"},
    {package: "gauge-budget-allocator", artifact: "gauge_budget_allocator.wasm", cw2_identity: "crates.io:gauge-budget-allocator"}
  ] | map(. + {
    wasm_sha256: ("a" * 64), wasm_size_bytes: 1,
    schema: (.package + ".json"), schema_sha256: ("b" * 64)
  })
}' >"$TMP/manifest.json"
MANIFEST_SHA=$(sha256sum "$TMP/manifest.json" | awk '{print $1}')

jq -n --arg commit "$COMMIT" --arg manifest "$MANIFEST_SHA" '{
  format_version: 1, kind: "gauge_canary_report",
  manifest_sha256: $manifest, source_commit: $commit,
  network_type: "public_testnet", chain_id: "test-1", gauge_id: 7,
  risk_tier: "low", start_height: 100, end_height: 200,
  started_at: "2026-07-14T00:00:00Z", completed_at: "2026-07-16T00:00:00Z",
  epochs: [
    {executed: true, height: 140, tx_hash: "tx-epoch-1"},
    {executed: true, height: 180, tx_hash: "tx-epoch-2"}
  ],
  power_change: {observed: true, tx_hash: "tx-power", health_consistent_after: true},
  reset: {call_count: 2, complete: true, evidence: "tx-reset"},
  health_checks: [
    {scan_complete: true, consistent: true, mismatch_count: 0},
    {scan_complete: true, consistent: true, mismatch_count: 0}
  ],
  alerts: {unresolved_critical: 0, unaccepted_warning: 0},
  dashboard: {uri: "ipfs://dashboard", sha256: ("b" * 64)},
  proposed_production_limits: {
    staged_treasury_limits: true,
    max_epoch_budget: {denom: "utest", amount: "1000"}
  },
  signoff: {maintainer: {name: "Maintainer", date: "2026-07-14", approved: true}}
}' >"$TMP/canary.json"
CANARY_SHA=$(sha256sum "$TMP/canary.json" | awk '{print $1}')

jq -n --arg commit "$COMMIT" --arg manifest "$MANIFEST_SHA" \
  --arg canary "$CANARY_SHA" '{
  format_version: 1, kind: "gauge_governance_approval",
  manifest_sha256: $manifest, canary_report_sha256: $canary,
  source_commit: $commit, canary_chain_id: "test-1",
  production_chain_id: "production-1", proposal_id: "42",
  execution_tx_hash: "tx-governance", passed_height: 1000, approved: true,
  production_limits: {
    staged_treasury_limits: true,
    max_epoch_budget: {denom: "utest", amount: "1000"}
  },
  residual_risks: ["low turnout capture"],
  signoff: {name: "Governance executor", date: "2026-07-14", approved: true}
}' >"$TMP/governance.json"

"$VALIDATOR" "$TMP/manifest.json" "$TMP/canary.json" "$TMP/governance.json" >/dev/null

function must_reject() {
  if "$VALIDATOR" "$1" "$2" "$3" >/dev/null 2>&1; then
    echo "validator accepted invalid production approval" >&2
    exit 1
  fi
}

function governance_for_canary() {
  local canary=$1
  local output=$2
  local sha
  sha=$(sha256sum "$canary" | awk '{print $1}')
  jq --arg sha "$sha" '.canary_report_sha256 = $sha' \
    "$TMP/governance.json" >"$output"
}

jq '.epochs = [.epochs[0]]' "$TMP/canary.json" >"$TMP/one-epoch.json"
governance_for_canary "$TMP/one-epoch.json" "$TMP/one-epoch-governance.json"
must_reject "$TMP/manifest.json" "$TMP/one-epoch.json" "$TMP/one-epoch-governance.json"

jq '.alerts.unresolved_critical = 1' "$TMP/canary.json" >"$TMP/critical-alert.json"
governance_for_canary "$TMP/critical-alert.json" "$TMP/critical-alert-governance.json"
must_reject "$TMP/manifest.json" "$TMP/critical-alert.json" "$TMP/critical-alert-governance.json"

jq '.production_limits.max_epoch_budget.amount = "2000"' \
  "$TMP/governance.json" >"$TMP/expanded-limits.json"
must_reject "$TMP/manifest.json" "$TMP/canary.json" "$TMP/expanded-limits.json"

jq '.canary_report_sha256 = ("f" * 64)' \
  "$TMP/governance.json" >"$TMP/wrong-canary.json"
must_reject "$TMP/manifest.json" "$TMP/canary.json" "$TMP/wrong-canary.json"

jq '.reset.call_count = 1' "$TMP/canary.json" >"$TMP/single-reset.json"
governance_for_canary "$TMP/single-reset.json" "$TMP/single-reset-governance.json"
must_reject "$TMP/manifest.json" "$TMP/single-reset.json" "$TMP/single-reset-governance.json"

echo "gauge production approval validator tests passed"
