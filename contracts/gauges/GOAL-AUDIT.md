# GOAL.md completion audit

This is the requirement-by-requirement evidence map for `GOAL.md` as of
2026-07-14. “Local” means the current repository can prove the requirement
without a chain deployment, independent reviewer, maintainer signature, or
governance action. It does not mean the contracts are approved for production.

## P0 blockers

| Requirement | Status | Authoritative evidence |
|---|---|---|
| P0.1 vote/tally invariants | Local implementation verified | `place_votes` validates the complete vector before writes; checked tally/index helpers and `randomized_vote_and_power_sequences_preserve_all_indices` cover the reference invariants and numeric boundaries. |
| P0.2 option removal | Local implementation verified | Tombstone accounting and adapter validity pull-checks are covered by zero/one/many-voter tests, real CW4/CW20/native/CW721 hook tests, replacement/abstention, interrupted reset cleanup, and post-removal epoch execution. `OPERATIONS.md` defines recovery. |
| P0.3 complete power changes | Local model verified; chain gas external | The 100-active-gauge invariant rejects unsupported state before mutation. Shared iterator and CW4/NFT payload tests cover 99/100/101; all hook families use the same accounting path. Target-VM gas remains a release gate. |
| P0.4 reset state machine | Local implementation verified | Persistent cursor, batch validation, checked schedule catch-up, idempotence, health checks, and 0/1/boundary/multi-batch tests are present. `OPERATIONS.md` defines continuation. |
| P0.5 hook reply stability | Local implementation verified; chain gas external | Namespaced address reply IDs, subscriber cap, first/middle/last mixed failure tests, and event assertions prove votes commit and only failing subscribers are removed. |
| P0.6 marketing bonds | Local implementation verified | Explicit bonds/liabilities, solvency checks, exclusive transitions, sender index, and resumable 50-row wind-down have native/CW20, surplus, overwrite, ownership-transfer, idempotence, rejection, and populated migration coverage. |
| P0.7 allocation semantics | Local implementation verified | Shared selected-allocation protocol burns excess. The real-stack matrix covers empty/partial/capped/threshold/tiny/no-message cases; allocator tests cover proportional integer dust. |

Critical mutation runs reported no surviving viable mutants across orchestrator
accounting/reset/removal/selection/hook/reply/migration, marketing
submission/rejection/refund/migration, and allocator authorization/execution.
This proves the present tests detect injected versions of the repaired faults.
It is not a literal replay of every new test against commit `d2da47e60`; retain
the PR’s red/green CI history as the historical failing-regression record.

## P1 hardening

| Requirement | Status | Authoritative evidence |
|---|---|---|
| P1.1 bounded work and payloads | Local bounds verified; target gas external | Constants, boundary tests, paginated indexed queries, bounded attachment, address validation, and `nonpayable` coverage implement every named bound. `ARCHITECTURE.md` publishes them. |
| P1.2 migrations | Local implementation verified | Orchestrator/marketing accept only `2.4.2` and `2.5.0`, validate identity/version, preserve populated state/ownership/cursors, and emit version/count fields. Allocator intentionally has no migrate export. |
| P1.3 public API | Local implementation verified | `gauge-interface`, typed config/health queries, create reply/event, stop/resume policy, arbitrary-adapter authority policy, no-quorum decision, and identity/compatibility tables are checked in. |
| P1.4 schemas/docs/licenses | Local implementation verified | All external schemas regenerate, smoke tests deserialize representative payloads, architecture/operations/events/release docs match policy, and package lists contain required LICENSE/NOTICE files. |
| P1.5 verification | Local automation verified; independent review external | Rust 1.81 locked tests/clippy/fmt, property/mutation tests, real hook stacks, coverage, pinned CI, `cargo-deny`, schema diff, extracted-tarball tests, optimized build, `cosmwasm-check`, 800,000-byte limits, and exact export allowlists are present. Independent audit/adversarial sign-off remains external. |

The gauge workflow now runs `cargo test --workspace --locked` and explicitly
installs libclang for `osmosis-test-tube`. `scripts/test-gauge-packages.sh`
packages, extracts, builds, and tests all four publishable crates. A clean CI
run remains evidence about the eventual committed SHA and cannot be created by
the dirty working tree itself.

## P2 and release gates

| Requirement | Status | Evidence or remaining authority |
|---|---|---|
| P2.1 events/monitoring/reconciliation | Local implementation verified | `EVENTS.md`, event/indexer tests, `MONITORING.md`, health/config/liability/pagination queries, and `OPERATIONS.md`. |
| P2.2 chain validation | External | Requires exact-artifact local-chain and public-testnet deployments, gas/response measurements, scenario evidence, and a two-epoch maximum-state soak. |
| P2.3 release/rollout | Local tooling verified; release actions external | Pinned optimizer, evidence/manifest/approval validators, publish order, package verification, proposal/runbook templates, and hash binding exist. Clean-tag independent reproduction, canary, signatures, and governance approval remain external. |
| P2.4 runbooks | Local documentation verified | `OPERATIONS.md`, `MONITORING.md`, `RELEASE.md`, and `EVIDENCE.md` cover every named operational procedure. |
| Independent audit | External | An independent CosmWasm auditor must sign an immutable report with no unresolved critical/high findings. |
| Maintainer and governance decisions | External | Validators require real named/date-bound sign-offs and reject placeholders. |

Accordingly, repository-local engineering is ready for review, but the
`GOAL.md` definition of production is not achieved until all external rows and
the clean committed-SHA CI gate pass.
