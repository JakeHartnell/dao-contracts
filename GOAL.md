# Production-ready DAO DAO gauges

## Goal

Ship the `gauge-orchestrator`, marketing `gauge-adapter`, and
`gauge-budget-allocator` as production CosmWasm contracts whose accounting is
correct under adversarial inputs, whose hooks cannot silently drift or block
staking, whose migrations and operational controls are safe, and whose release
artifacts have been independently reviewed and exercised on a representative
chain.

This plan is based on the code in
[PR #928](https://github.com/DA0-DA0/dao-contracts/pull/928) at `d2da47e60` and
an engineering review performed on 2026-07-13. It is a readiness plan, not a
completed security audit. Every finding below must be confirmed with a failing
regression test before or alongside its fix.

Production means all P0 and P1 work and every release gate in this document are
complete. P2 items may be deferred only by an explicit, documented maintainer
decision that names the owner, residual risk, and follow-up milestone.

## Current baseline

The PR already provides three contracts, checked-in JSON schemas, extensive
`cw-multi-test` coverage, READMEs, formatting/clippy cleanup, and reported
release Wasm builds. The public PR report currently shows high patch coverage,
but also reports uncovered changed lines and a project coverage decrease.

The test suite could not be independently rerun in the live review workspace
because an incomplete, ignored `contracts/voting/dao-voting-juno-staked`
directory is matched by the workspace glob, causing Cargo resolution to fail
before compilation. This appears to be local environmental residue rather than
a tracked PR defect. Reproduction from a clean clone and required green CI are
therefore explicit gates below.

## P0 — correctness and security blockers

No public testnet program or production deployment should proceed until every
item in this section is resolved.

### P0.1 Preserve tally invariants for every vote input

`PlaceVotes` sums the original vector but collapses it through a `HashMap` while
computing the diff. Duplicate options can therefore be accounted once in the
immediate update but stored multiple times and applied multiple times by later
power-change hooks. Zero-weight and unbounded vote entries also create ambiguous
state and gas risk.

- [ ] Reject duplicate option names, zero weights, empty option strings, and
  vote vectors above a documented maximum before reading or mutating tallies.
- [ ] Decide whether weights must sum to exactly `1.0` or may sum to less; make
  the API documentation, selection math, and tests agree.
- [ ] Use checked arithmetic throughout tally updates and return domain-specific
  errors instead of relying on release-mode overflow/underflow behavior.
- [ ] Add regression tests for duplicates in the first/middle/last positions,
  repeated vote replacement, clearing votes, and stake/unstake after each case.
- [ ] Add property tests that generate vote sequences and power changes and
  assert after every operation:
  - `TALLY[(gauge, option)]` equals the sum of live per-voter contributions;
  - `TOTAL_CAST[gauge]` equals the sum of all option tallies;
  - every tally has exactly one matching `OPTION_BY_POINTS` entry and no stale
    index entry exists;
  - no tally or total can underflow or overflow.

Relevant code:
[`contract.rs`](contracts/gauges/gauge/src/contract.rs) (`place_votes`) and
[`state.rs`](contracts/gauges/gauge/src/state.rs) (`update_tallies`).

### P0.2 Make option removal safe for existing votes

`RemoveOption` deletes its tally and sorted index but leaves that option in
voter records. A later vote replacement or voting-power hook then subtracts the
old contribution from a missing tally, which can underflow and cause the hook or
vote transaction to fail. Adapter-side rejection/removal also does not
automatically invalidate the orchestrator's copy of an option.

- [ ] Define one removal model: atomically purge/rewrite affected votes in
  bounded batches, retain a tombstone until all references expire, or make
  removal part of a resumable state machine.
- [ ] Block selection and execution of invalidated options as soon as removal
  starts, without leaving inconsistent totals or indices.
- [ ] Define and implement synchronization between adapter registry removal and
  orchestrator option state. Do not allow a rejected marketing submission to
  remain payable merely because the orchestrator has stale state.
- [ ] Add tests for removal with zero, one, and many active voters, followed by
  vote replacement, abstention, cw4 changes, cw20 stake/unstake, cw721
  stake/unstake, reset, and epoch execution.
- [ ] Prove the removal path makes bounded progress and publish a keeper/recovery
  procedure for interrupted batches.

Relevant code: `remove_option` in
[`contract.rs`](contracts/gauges/gauge/src/contract.rs) and `remove_tally` in
[`state.rs`](contracts/gauges/gauge/src/state.rs).

### P0.3 Process all power changes without silent drift

Each cw4, fungible-stake, and NFT-stake hook queries at most 100 gauge votes for
a voter and does not continue pagination. Voting in gauge 101 and beyond causes
those tallies to retain stale power.

- [ ] Choose and enforce a safe model:
  - impose a hard, checked maximum number of active gauge votes per voter that
    is within a measured hook gas budget; or
  - redesign hook work as a complete, resumable/paginated process without
    exposing staking operations to partial accounting.
- [ ] Never silently truncate state-changing iteration. Return an explicit
  error before an unsupported state can be created.
- [ ] Apply the same semantics to cw4, cw20/native/token-factory, and cw721
  power changes.
- [ ] Test the boundary immediately below, at, and above the chosen limit for
  every hook type, including stopped and resetting gauges.
- [ ] Measure worst-case hook gas on the target chain/VM and keep a documented
  safety margin below transaction gas limits.

Relevant code: hook handlers in
[`contract.rs`](contracts/gauges/gauge/src/contract.rs) and
`query_votes_by_voter` in
[`state.rs`](contracts/gauges/gauge/src/state.rs).

### P0.4 Make reset a correct, resumable state machine

The current reset accepts `batch_size = 0`, which can put a gauge into resetting
state without advancing it. More generally, reset rewrites processed sorted
keys to zero and starts each later batch from the beginning; when the number of
options is at least the batch size, a later call can repeatedly select the same
zeroed keys instead of reaching the remaining options.

- [ ] Reject zero and cap the maximum batch size.
- [ ] Persist a stable cursor/phase so every non-empty call either processes new
  work or completes the reset.
- [ ] Validate `reset_epoch` and all timestamp arithmetic, including zero,
  overflow, long downtime, and repeated keeper calls.
- [ ] Make reset completion idempotent and define whether late schedules catch
  up from the previous deadline or restart from the current block time.
- [ ] Test option counts `0`, `1`, `batch-1`, `batch`, `batch+1`, and multiple
  batches with equal and changing tally values.
- [ ] Assert after completion that totals and indices are consistent, old votes
  are expired as intended, voting/execution resume, and a second caller cannot
  corrupt progress.

Relevant code: `reset_gauge` in
[`contract.rs`](contracts/gauges/gauge/src/contract.rs) and `Gauge::is_resetting`
in [`state.rs`](contracts/gauges/gauge/src/state.rs).

### P0.5 Replace index-based vote-hook failure handling

Vote-hook reply IDs are mutable vector indices. If multiple hooks fail during a
single vote, removing the first shifts later indices; subsequent replies can
remove a healthy hook or fail out of bounds and revert the vote, contradicting
the documented non-fatal behavior.

- [ ] Use stable hook identifiers/addresses, or collect failures and remove them
  without observing a mutating index space.
- [ ] Namespace reply IDs so future reply-using features cannot collide.
- [ ] Bound the number of subscribers and measure worst-case vote gas.
- [ ] Test two or more simultaneous failures at the first, middle, and last
  positions and mixed success/failure patterns.
- [ ] Prove the vote always commits, successful subscribers remain registered,
  and only failing subscribers are removed.

Relevant code:
[`hooks.rs`](contracts/gauges/gauge/src/hooks.rs) and the `reply` entry point in
[`contract.rs`](contracts/gauges/gauge/src/contract.rs).

### P0.6 Implement explicit marketing-bond accounting

The marketing adapter infers one liability per registry row rather than storing
bond state. A same-sender overwrite can collect a second bond for one row;
`ReturnDeposits` includes the synthetic default submission, does not mark debts
paid or remove them, and can be called again; a later `Reject` can attempt
another payout. This can strand user deposits or pay liabilities repeatedly
from unrelated funds of the same denomination.

- [ ] Store the actual bond asset, amount, depositor, and lifecycle state for
  each bonded submission; synthetic/default entries must have no liability.
- [ ] Make metadata updates deposit-free or atomically refund/replace the old
  bond with an explicitly tested policy.
- [ ] Make refund, soft rejection, hard rejection, and bulk wind-down mutually
  exclusive, one-time state transitions with state updated before messages are
  dispatched.
- [ ] Maintain and query aggregate liabilities; assert contract escrow covers
  liabilities before and after every transition.
- [ ] Replace unbounded bulk refund with a cursor-based, idempotent batch flow
  that reports progress and can safely resume after interruption.
- [ ] Add native and cw20 tests for overwrite, repeated refund, refund then
  reject, reject then refund, default entry, partial batches, unexpected token
  transfers, insufficient escrow, and ownership changes mid-wind-down.

Relevant code: submission, rejection, and refund handlers in
[`contract.rs`](contracts/gauges/gauge-adapter/src/contract.rs).

### P0.7 Resolve allocation semantics and zero-denominator execution

`max_available_percentage` clamps selected powers, after which the orchestrator
renormalizes the selected set to 100%. That can redistribute the supposed
excess and allows a sole capped winner to receive 100%, contrary to the README's
claim that excess goes to no one. Very small caps can also round every selected
power to zero, leaving execution to construct ratios with a zero denominator.

- [ ] Specify, with worked examples, whether caps burn/unallocate excess,
  redistribute it, or cap only selection power.
- [ ] Make orchestrator and adapter APIs carry the information needed to enforce
  the chosen semantics; do not hide the denominator through accidental
  renormalization.
- [ ] Define behavior for no votes, no qualifying options, all capped values
  rounding to zero, and adapter returning no messages.
- [ ] Use checked ratio construction and return a domain error or documented
  no-op for a zero denominator.
- [ ] Add table-driven tests for one/many winners, partial turnout, caps above
  and below actual shares, minimum thresholds, integer dust, and tiny voting
  power/budgets.

Relevant code: `selected_set` and epoch `execute` in
[`contract.rs`](contracts/gauges/gauge/src/contract.rs).

## P1 — required hardening and release engineering

### P1.1 Bound every externally influenced loop and payload

- [ ] Set documented limits for gauges per orchestrator, options per gauge,
  votes per voter/gauge, hook subscribers, adapter messages per execution,
  option/title/name/URL byte lengths, and initial options.
- [ ] Paginate `AllOptions`, `AllSubmissions`, and `SubmissionsBySender`; add a
  sender index rather than scanning all submissions.
- [ ] Make gauge attachment/import resumable or enforce a proven-safe maximum;
  do not synchronously import an unbounded adapter response.
- [ ] Validate budget-allocator destinations as chain addresses at instantiate
  and add time, since they are later used in `BankMsg::Send`.
- [ ] Apply `nonpayable` checks to every orchestrator/allocator execute and every
  marketing-adapter endpoint that is not intentionally receiving the configured
  native bond; reject rather than trap accidental funds.
- [ ] Add worst-case WasmVM gas/response-size tests for every bound.

### P1.2 Finish migration design for all three contracts

- [ ] Define a supported source-version matrix and state compatibility policy
  for each contract.
- [ ] Marketing and budget adapters must use cw2 to verify contract identity and
  older version and then write the new version, or remove their migrate entry
  points until a real migration is available. A successful no-op that leaves
  the old cw2 version is not acceptable.
- [ ] Add the missing legacy marketing-adapter migration from the former
  `Config.admin` layout to `cw-ownable`, initializing ownership without losing
  config, submissions, balances, or bond liabilities.
- [ ] Test wrong contract name, same/newer version, every supported old version,
  ownership preservation, populated state, and interrupted resumable state.
- [ ] Emit `from_version`, `to_version`, and migrated-record counts.
- [ ] Reconcile package, artifact, and cw2 names (`gauge-orchestrator` vs
  `crates.io:gauge`; `gauge-adapter` vs
  `crates.io:marketing-gauge-adapter`) before the first release.

### P1.3 Freeze and verify the public API

- [ ] Extract the adapter protocol messages/responses into a small shared
  package. The budget allocator should not depend on the marketing contract to
  obtain a generic protocol, and copied orchestrator/adapter types must not
  drift.
- [ ] Decide whether arbitrary `CosmosMsg` output is intentional. Treat every
  attached adapter as having DAO execution authority; document that trust
  boundary and audit/allowlist policy. If narrower authority is intended,
  validate message types, destinations, denominations, and amounts.
- [ ] Return the documented `CreateGaugeReply` data and a stable `gauge_id`
  event attribute, or remove the unused reply contract and update clients.
- [ ] Add a typed config query exposing owner, DAO core, voting-power source,
  and hook caller so deployments can be verified.
- [ ] Define stopped-gauge semantics. Enforce them consistently in voting,
  power hooks, reset, and execution, and add a safe resume/recovery path or a
  documented migration procedure.
- [ ] Decide and test minimum-turnout/quorum behavior; document the accepted
  economic risk if one low-power voter is intentionally allowed to allocate an
  entire epoch budget.
- [ ] Version the API and publish a compatibility matrix for contracts, schemas,
  DAO core, voting modules, and supported chains.

### P1.4 Correct schemas and documentation

- [ ] Generate schema for every external entry point, including the marketing
  adapter's real `ExecuteMsg` (not `Empty`) and all `MigrateMsg` types.
- [ ] Add schema smoke tests that deserialize representative instantiate,
  execute, query, response, reply, and migrate payloads.
- [ ] Make docs match code for epoch comparison, stopping, resets, cap behavior,
  owner/DAO roles, adapter trust, deposits, and `CreateGauge` response data.
- [ ] Add architecture/threat-model documentation covering accounting
  invariants, hook failure behavior, malicious adapters/options, keeper
  liveness, gas exhaustion, escrow, and chain-specific address/denom behavior.
- [ ] Restore or add the correct Apache-2.0 attribution/NOTICE material for the
  WyndDAO-derived code. The current README claims gauge-local LICENSE/NOTICE
  files exist, but they are absent from this branch; complete a provenance and
  license review before distribution.

### P1.5 Raise the verification bar

- [ ] Reproduce from a clean clone with the pinned toolchain and `--locked`:
  formatting, clippy with warnings denied, all unit/multitests, all workspace
  tests, schema regeneration/diff, and release Wasm builds.
- [ ] Add the invariant/property tests described in P0 and mutation-test the
  critical accounting, authorization, reset, refund, and reply paths.
- [ ] Add actual end-to-end suites for cw4, cw20-staked, cw721-staked, and
  native/token-factory-staked voting modules. Exercise real hook registration
  and staking contracts rather than only direct synthetic hook messages.
- [ ] Cover all currently missed changed lines that encode state transitions or
  error handling. Coverage percentage alone is not the acceptance criterion.
- [ ] Run dependency/license policy checks in required CI. Modernize/pin CI
  actions and make audit failures visible and blocking for releases.
- [ ] Run `cosmwasm-check` (or the project-standard equivalent) on optimized
  artifacts and verify allowed exports, capabilities, size limits, and no debug
  entry points.
- [ ] Commission an independent CosmWasm security audit after fixes stabilize;
  resolve every critical/high finding and document accepted lower-severity
  risks.
- [ ] Run a final adversarial review focused on economic manipulation,
  permission boundaries, hook liveness, storage growth, and migrations.

## P2 — deployment, observability, and operations

### P2.1 Stable events and monitoring

- [ ] Define stable event attributes for every mutation. Include relevant gauge
  ID, actor, option, old/new configuration, epoch/next epoch, selected count,
  message count, bond asset/amount/state, and batch progress.
- [ ] Add indexer tests for gauge creation, vote, power update, reset, execution,
  hook removal, submission, rejection, refund, ownership, stop, and resume.
- [ ] Alert on missed epochs, failed executions, underfunded DAO budgets, hook
  unregistration/wiring drift, resets without progress, escrow shortfall, and
  migration/version mismatch.
- [ ] Provide queries that let operators reconcile on-chain tallies, indices,
  active votes, liabilities, and batch progress.

### P2.2 Chain-level validation

- [ ] Deploy optimized artifacts to a representative local chain and public
  testnet with actual DAO core and supported voting/hook contracts.
- [ ] Exercise cw4, cw20-staked, native/token-factory-staked, and cw721-staked
  flows: create, vote, change power, remove/invalidate option, execute epoch,
  fail adapter, reset in batches, stop/resume, transfer ownership, and migrate.
- [ ] Record transaction gas, Wasm sizes, response sizes, code IDs, addresses,
  checksums, chain/VM versions, and pass/fail evidence in a checked-in test
  report.
- [ ] Soak-test multiple epochs and concurrent keepers with conservative but
  realistic maximum state.

### P2.3 Reproducible release and controlled rollout

- [ ] Produce deterministic optimized Wasm and independently reproduce every
  checksum from the tagged source commit.
- [ ] Publish an artifact manifest mapping filename, package, cw2 identity,
  semantic version, source commit, optimizer image digest, checksum, schema,
  and audit report. Decide whether the three crates are published to crates.io.
- [ ] If crates are published, add them to the publish workflow in dependency
  order, complete missing package metadata, and verify `cargo package --locked`
  plus a build/test from each packaged tarball.
- [ ] Prepare instantiate/migrate proposal payloads, verified hook-registration
  steps, ownership model, DAO balance preflight, keeper configuration,
  monitoring dashboard, and rollback/incident proposals before deployment.
- [ ] Canary with one low-value gauge and conservative limits. Observe at least
  two complete epochs including a power change and reset before expanding
  scope.
- [ ] Use staged treasury limits and require an explicit governance decision to
  increase them after canary evidence is reviewed.

### P2.4 Runbooks

- [ ] Deployment and configuration verification.
- [ ] Epoch keeper behavior, retries, duplicate callers, and underfunding.
- [ ] Reset/removal/refund batch continuation and stuck-state recovery.
- [ ] Voting hook failure and safe re-registration.
- [ ] Pause/stop, resume, ownership recovery, emergency migration, and rollback.
- [ ] Escrow reconciliation and user support for submission bonds.
- [ ] Security disclosure, incident severity, communication, and postmortem.

## Required release gates

All boxes below are mandatory for the first production deployment.

- [ ] Every P0 item has a regression test that fails on the reviewed commit and
  passes with the fix.
- [ ] Every accounting, index, and escrow invariant passes property tests and a
  populated-state migration test.
- [ ] Worst-case measured gas for vote, every power hook, selection/execution,
  reset/removal, attachment, and refund stays within the documented target-chain
  budget with an agreed safety margin.
- [ ] Required clean-clone CI is green: locked tests, fmt, clippy, schema diff,
  optimized Wasm, artifact validation, dependency/security, and license checks.
- [ ] Checked-in schemas match every deployed binary's API.
- [ ] Independent audit is complete; no unresolved critical/high issues remain.
- [ ] Testnet report, artifact manifest/checksums, deployment payloads,
  dashboards/alerts, and incident runbooks have maintainer sign-off.
- [ ] Canary exit criteria are met for at least two epochs and governance has
  explicitly approved production limits and residual risks.

## Suggested delivery order

1. Freeze new features and write failing regressions for P0.1–P0.7.
2. Fix vote/tally/removal/reset accounting and establish property invariants.
3. Fix bond escrow/refunds and hook processing/replies.
4. Bound state and gas, then freeze the shared adapter API and migration model.
5. Correct schemas/docs/events and complete clean-clone CI plus artifact checks.
6. Perform chain-level tests and gas measurements.
7. Obtain independent audit, remediate findings, and rerun the full suite.
8. Produce release artifacts/runbooks, canary, observe, and only then expand.

## Explicit non-goals for the first release

- Vote-weight decay remains a separate feature unless governance adds it to the
  threat model and repeats the relevant audit/test gates.
- New adapter types are deferred until the shared adapter protocol and trust
  policy are frozen.
- Performance optimizations that change accounting semantics are deferred until
  the reference invariant suite exists.
