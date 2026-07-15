#!/usr/bin/env bash
set -o errexit -o nounset -o pipefail

function usage() {
  echo "Usage: $0 RELEASE_MANIFEST CANARY_REPORT GOVERNANCE_APPROVAL"
}

if [ "$#" -ne 3 ]; then
  usage >&2
  exit 2
fi

MANIFEST=$1
CANARY=$2
GOVERNANCE=$3

for command in jq sha256sum; do
  command -v "$command" >/dev/null || {
    echo "required command not found: $command" >&2
    exit 1
  }
done

for file in "$MANIFEST" "$CANARY" "$GOVERNANCE"; do
  if [ ! -s "$file" ] || ! jq -e . "$file" >/dev/null 2>&1; then
    echo "production approval input must be a nonempty JSON document: $file" >&2
    exit 1
  fi
done

MANIFEST_SHA=$(sha256sum "$MANIFEST" | awk '{print $1}')
CANARY_SHA=$(sha256sum "$CANARY" | awk '{print $1}')

if ! jq -e -s \
  --arg manifest_sha "$MANIFEST_SHA" \
  --arg canary_sha "$CANARY_SHA" '
  def nonempty: type == "string" and length > 0;
  def sha256: type == "string" and test("^[0-9a-f]{64}$");
  .[0] as $manifest |
  .[1] as $canary |
  .[2] as $governance |
  ($manifest.format_version == "1" or $manifest.format_version == 1) and
  ($manifest.source_commit | type == "string" and test("^[0-9a-f]{40}$")) and
  ($manifest.optimizer_image | type == "string" and test("^[^[:space:]@]+@sha256:[0-9a-f]{64}$")) and
  $manifest.rust_toolchain == "1.81.0" and
  $manifest.cosmwasm_check == "1.5.11" and
  ($manifest.audit.report | nonempty) and
  ($manifest.audit.sha256 | sha256) and
  ($manifest.chain_test.report | nonempty) and
  ($manifest.chain_test.sha256 | sha256) and
  ($manifest.artifacts | length) == 3 and
  ([$manifest.artifacts[].package] | sort) == [
    "gauge-adapter",
    "gauge-budget-allocator",
    "gauge-orchestrator"
  ] and
  ([$manifest.artifacts[].cw2_identity] | sort) == [
    "crates.io:gauge",
    "crates.io:gauge-budget-allocator",
    "crates.io:marketing-gauge-adapter"
  ] and
  all($manifest.artifacts[];
    (.wasm_sha256 | sha256) and
    (.wasm_size_bytes | type == "number" and . > 0) and
    (.schema | nonempty) and
    (.schema_sha256 | sha256)
  ) and
  $canary.format_version == 1 and
  $canary.kind == "gauge_canary_report" and
  $canary.manifest_sha256 == $manifest_sha and
  $canary.source_commit == $manifest.source_commit and
  $canary.network_type == "public_testnet" and
  ($canary.chain_id | nonempty) and
  ($canary.gauge_id | type == "number" and . >= 0 and floor == .) and
  $canary.risk_tier == "low" and
  ($canary.start_height | type == "number" and . > 0) and
  ($canary.end_height | type == "number") and
  $canary.end_height > $canary.start_height and
  ($canary.started_at | nonempty) and
  ($canary.completed_at | nonempty) and
  ($canary.epochs | length) >= 2 and
  all($canary.epochs[];
    .executed == true and
    (.height | type == "number" and . > 0) and
    (.tx_hash | nonempty)
  ) and
  $canary.power_change.observed == true and
  ($canary.power_change.tx_hash | nonempty) and
  $canary.power_change.health_consistent_after == true and
  ($canary.reset.call_count | type == "number" and . >= 2 and floor == .) and
  $canary.reset.complete == true and
  ($canary.reset.evidence | nonempty) and
  all($canary.health_checks[];
    .scan_complete == true and .consistent == true and .mismatch_count == 0
  ) and
  ($canary.health_checks | length) >= 2 and
  $canary.alerts.unresolved_critical == 0 and
  $canary.alerts.unaccepted_warning == 0 and
  ($canary.dashboard.uri | nonempty) and
  ($canary.dashboard.sha256 | sha256) and
  $canary.proposed_production_limits.staged_treasury_limits == true and
  ($canary.proposed_production_limits.max_epoch_budget.amount | type == "string" and test("^[0-9]+$") and . != "0") and
  ($canary.proposed_production_limits.max_epoch_budget.denom | nonempty) and
  ($canary.signoff.maintainer.name | nonempty) and
  ($canary.signoff.maintainer.date | nonempty) and
  $canary.signoff.maintainer.approved == true and
  $governance.format_version == 1 and
  $governance.kind == "gauge_governance_approval" and
  $governance.manifest_sha256 == $manifest_sha and
  $governance.canary_report_sha256 == $canary_sha and
  $governance.source_commit == $manifest.source_commit and
  $governance.canary_chain_id == $canary.chain_id and
  ($governance.production_chain_id | nonempty) and
  ($governance.proposal_id | nonempty) and
  ($governance.execution_tx_hash | nonempty) and
  ($governance.passed_height | type == "number" and . > 0) and
  $governance.approved == true and
  $governance.production_limits == $canary.proposed_production_limits and
  $governance.production_limits.staged_treasury_limits == true and
  ($governance.residual_risks | type == "array") and
  ($governance.signoff.name | nonempty) and
  ($governance.signoff.date | nonempty) and
  $governance.signoff.approved == true
' "$MANIFEST" "$CANARY" "$GOVERNANCE" >/dev/null; then
  echo "production approval is missing canary exit evidence, exact staged limits, or governance authorization" >&2
  exit 1
fi

echo "gauge production approval is valid"
