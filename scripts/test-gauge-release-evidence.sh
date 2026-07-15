#!/usr/bin/env bash
set -o errexit -o nounset -o pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
VALIDATOR="$ROOT/scripts/validate-gauge-release-evidence.sh"
COMMIT=0123456789abcdef0123456789abcdef01234567
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

jq -n --arg commit "$COMMIT" '{
  format_version: 1,
  kind: "gauge_security_audit_attestation",
  source_commit: $commit,
  independent_auditor: {name: "Independent Security", independent: true},
  report: {
    uri: "ipfs://audit-report",
    sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  },
  scope: {artifacts: [
    "gauge-orchestrator",
    "gauge-adapter",
    "gauge-budget-allocator"
  ]},
  findings: {
    unresolved_critical: 0,
    unresolved_high: 0,
    accepted_lower_severity: 1
  },
  signoff: {
    auditor: {name: "Auditor", date: "2026-07-14", approved: true},
    maintainer: {name: "Maintainer", date: "2026-07-14", approved: true}
  }
}' >"$TMP/audit.json"

jq -n --arg commit "$COMMIT" '
  def artifacts: [
    {package: "gauge-orchestrator", checksum: ("a" * 64), code_id: 1, address: "local1orchestrator", wasm_size_bytes: 1},
    {package: "gauge-adapter", checksum: ("b" * 64), code_id: 2, address: "local1adapter", wasm_size_bytes: 1},
    {package: "gauge-budget-allocator", checksum: ("c" * 64), code_id: 3, address: "local1allocator", wasm_size_bytes: 1}
  ];
  def scenarios: [
    "adapter_failure", "concurrent_keepers", "create", "cw20_power_change",
    "cw4_power_change", "cw721_power_change", "epoch_execute", "migrate",
    "native_or_tokenfactory_power_change", "option_invalidation",
    "ownership_transfer", "reset_batches", "stop_resume", "vote"
  ];
  def operations: [
    "attachment", "epoch_execution", "option_removal", "power_hook_cw20",
    "power_hook_cw4", "power_hook_cw721",
    "power_hook_native_or_tokenfactory", "refund_batch", "reset_batch",
    "selection_query", "vote"
  ];
  {
    format_version: 1,
    kind: "gauge_chain_test_report",
    source_commit: $commit,
    deployments: [
      {network_type: "local", chain_id: "local-1", wasmd_version: "v1", wasmvm_version: "v1", rpc_evidence_uri: "file://local", artifacts: artifacts},
      {network_type: "public_testnet", chain_id: "test-1", wasmd_version: "v1", wasmvm_version: "v1", rpc_evidence_uri: "https://example.test/evidence", artifacts: artifacts}
    ],
    scenarios: [["local", "public_testnet"][] as $network | scenarios[] | {
      name: ., passed: true, network_type: $network,
      evidence: ($network + ":tx:" + .)
    }],
    gas_measurements: [operations[] | {
      operation: ., network_type: "public_testnet",
      tx_hash_or_query_evidence: ("tx:" + .), gas_used: 70, gas_limit: 100,
      required_safety_margin_percent: 20, response_size_bytes: 100
    }],
    soak: {
      completed_epochs: 2,
      concurrent_keepers_exercised: true,
      conservative_max_state_exercised: true,
      evidence: "tx:soak"
    },
    signoff: {maintainer: {name: "Maintainer", date: "2026-07-14", approved: true}}
  }
' >"$TMP/chain.json"

"$VALIDATOR" "$TMP/audit.json" "$TMP/chain.json" "$COMMIT" >/dev/null

function must_reject() {
  local audit=$1
  local chain=$2
  if "$VALIDATOR" "$audit" "$chain" "$COMMIT" >/dev/null 2>&1; then
    echo "validator accepted invalid release evidence" >&2
    exit 1
  fi
}

jq '.findings.unresolved_high = 1' "$TMP/audit.json" >"$TMP/audit-high.json"
must_reject "$TMP/audit-high.json" "$TMP/chain.json"

jq 'del(.deployments[] | select(.network_type == "public_testnet"))' \
  "$TMP/chain.json" >"$TMP/chain-no-testnet.json"
must_reject "$TMP/audit.json" "$TMP/chain-no-testnet.json"

jq 'del(.scenarios[] | select(.network_type == "local" and .name == "vote"))' \
  "$TMP/chain.json" >"$TMP/chain-missing-local-flow.json"
must_reject "$TMP/audit.json" "$TMP/chain-missing-local-flow.json"

jq '(.deployments[] | select(.network_type == "public_testnet") |
  .artifacts[] | select(.package == "gauge-orchestrator") | .checksum) = ("d" * 64)' \
  "$TMP/chain.json" >"$TMP/chain-checksum-drift.json"
must_reject "$TMP/audit.json" "$TMP/chain-checksum-drift.json"

jq '.gas_measurements[0].required_safety_margin_percent = 40' \
  "$TMP/chain.json" >"$TMP/chain-low-margin.json"
must_reject "$TMP/audit.json" "$TMP/chain-low-margin.json"

jq '.source_commit = "ffffffffffffffffffffffffffffffffffffffff"' \
  "$TMP/chain.json" >"$TMP/chain-wrong-commit.json"
must_reject "$TMP/audit.json" "$TMP/chain-wrong-commit.json"

echo "gauge release evidence validator tests passed"
