#!/usr/bin/env bash
set -o errexit -o nounset -o pipefail

function usage() {
  echo "Usage: $0 AUDIT_ATTESTATION_JSON CHAIN_REPORT_JSON SOURCE_COMMIT"
}

if [ "$#" -ne 3 ]; then
  usage >&2
  exit 2
fi

AUDIT_REPORT=$1
CHAIN_REPORT=$2
SOURCE_COMMIT=$3

command -v jq >/dev/null || {
  echo "required command not found: jq" >&2
  exit 1
}

if ! [[ "$SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
  echo "source commit must be a full lowercase 40-character Git SHA" >&2
  exit 1
fi

for file in "$AUDIT_REPORT" "$CHAIN_REPORT"; do
  if [ ! -s "$file" ] || ! jq -e . "$file" >/dev/null 2>&1; then
    echo "release evidence must be a nonempty JSON document: $file" >&2
    exit 1
  fi
done

if ! jq -e --arg commit "$SOURCE_COMMIT" '
  def nonempty: type == "string" and length > 0;
  def sha256: type == "string" and test("^[0-9a-f]{64}$");
  .format_version == 1 and
  .kind == "gauge_security_audit_attestation" and
  .source_commit == $commit and
  .independent_auditor.independent == true and
  (.independent_auditor.name | nonempty) and
  (.report.uri | nonempty) and
  (.report.sha256 | sha256) and
  (.scope.artifacts | sort) == [
    "gauge-adapter",
    "gauge-budget-allocator",
    "gauge-orchestrator"
  ] and
  .findings.unresolved_critical == 0 and
  .findings.unresolved_high == 0 and
  .findings.accepted_lower_severity >= 0 and
  (.signoff.auditor.name | nonempty) and
  (.signoff.auditor.date | nonempty) and
  .signoff.auditor.approved == true and
  (.signoff.maintainer.name | nonempty) and
  (.signoff.maintainer.date | nonempty) and
  .signoff.maintainer.approved == true
' "$AUDIT_REPORT" >/dev/null; then
  echo "audit attestation is incomplete, out of scope, unsigned, or has unresolved critical/high findings" >&2
  exit 1
fi

if ! jq -e --arg commit "$SOURCE_COMMIT" '
  def nonempty: type == "string" and length > 0;
  def sha256: type == "string" and test("^[0-9a-f]{64}$");
  def expected_artifacts: [
    "gauge-adapter",
    "gauge-budget-allocator",
    "gauge-orchestrator"
  ];
  def expected_scenarios: [
    "adapter_failure",
    "concurrent_keepers",
    "create",
    "cw20_power_change",
    "cw4_power_change",
    "cw721_power_change",
    "epoch_execute",
    "migrate",
    "native_or_tokenfactory_power_change",
    "option_invalidation",
    "ownership_transfer",
    "reset_batches",
    "stop_resume",
    "vote"
  ];
  def expected_gas: [
    "attachment",
    "epoch_execution",
    "option_removal",
    "power_hook_cw20",
    "power_hook_cw4",
    "power_hook_cw721",
    "power_hook_native_or_tokenfactory",
    "refund_batch",
    "reset_batch",
    "selection_query",
    "vote"
  ];
  def valid_deployment:
    (.chain_id | nonempty) and
    (.wasmd_version | nonempty) and
    (.wasmvm_version | nonempty) and
    (.rpc_evidence_uri | nonempty) and
    (.artifacts | length) == 3 and
    ([.artifacts[].package] | sort) == expected_artifacts and
    all(.artifacts[];
      (.checksum | sha256) and
      (.code_id | type == "number" and . > 0 and floor == .) and
      (.address | nonempty) and
      (.wasm_size_bytes | type == "number" and . > 0 and floor == .)
    );
  .format_version == 1 and
  .kind == "gauge_chain_test_report" and
  .source_commit == $commit and
  ([.deployments[].network_type] | unique | sort) == ["local", "public_testnet"] and
  all(.deployments[]; valid_deployment) and
  ([
    expected_artifacts[] as $package |
    ([.deployments[].artifacts[] |
      select(.package == $package) |
      [.checksum, .wasm_size_bytes]
    ] | unique | length == 1)
  ] | all) and
  ([.scenarios[].name] | unique | sort) == expected_scenarios and
  all(.scenarios[];
    .passed == true and
    (.network_type == "local" or .network_type == "public_testnet") and
    (.evidence | nonempty)
  ) and
  ([
    expected_scenarios[] as $scenario |
    ["local", "public_testnet"][] as $network |
    any(.scenarios[];
      .name == $scenario and .network_type == $network and .passed == true
    )
  ] | all) and
  ([.gas_measurements[].operation] | unique | sort) == expected_gas and
  all(.gas_measurements[];
    . as $measurement |
    ($measurement.network_type == "local" or $measurement.network_type == "public_testnet") and
    ($measurement.tx_hash_or_query_evidence | nonempty) and
    ($measurement.gas_used | type == "number" and . > 0) and
    ($measurement.gas_limit | type == "number") and
    ($measurement.gas_limit > $measurement.gas_used) and
    ($measurement.required_safety_margin_percent | type == "number" and . > 0) and
    ((($measurement.gas_limit - $measurement.gas_used) * 100 / $measurement.gas_limit) >= $measurement.required_safety_margin_percent) and
    ($measurement.response_size_bytes | type == "number" and . >= 0)
  ) and
  .soak.completed_epochs >= 2 and
  .soak.concurrent_keepers_exercised == true and
  .soak.conservative_max_state_exercised == true and
  (.soak.evidence | nonempty) and
  (.signoff.maintainer.name | nonempty) and
  (.signoff.maintainer.date | nonempty) and
  .signoff.maintainer.approved == true
' "$CHAIN_REPORT" >/dev/null; then
  echo "chain report is incomplete, out of scope, below its gas margin, or unsigned" >&2
  exit 1
fi

echo "gauge release evidence is valid"
