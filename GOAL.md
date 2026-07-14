# Production readiness goal: Augmented Bonding Curves

## Objective

Ship a production release of `cw-abc`, `cw-curves`, and `dao-abc-factory` that is safe to deploy through DAO DAO on explicitly supported CosmWasm chains.

Production-ready means the economic and state-machine invariants are defined and tested, all work is bounded by chain gas limits, privileged and dependency trust is explicit, artifacts are reproducible, and an independent auditor has approved the exact release commit and Wasm checksums.

This plan covers PR [#926](https://github.com/DA0-DA0/dao-contracts/pull/926) at `32a0f26ea`. The checked-in May 9 internal review is useful input, but is not production sign-off: material vesting, refund, Power, and Sigmoid code landed after its review scope, and it excluded important issuer and integration surfaces.

## Release gates

No mainnet deployment until every P0 item is complete. P1 items are required before a general production launch. P2 items may run in parallel, but documentation and economic sign-off must be complete before a public launch.

### P0 — Correctness and security blockers

- [ ] **Redesign vesting so transfers cannot bypass it.** The current check is attached to `HATCHERS[info.sender]`, while the tokenfactory supply token is transferable. A hatcher can transfer locked tokens to a fresh address and sell them; conversely, later Open-phase tokens held by a hatcher can be incorrectly constrained.
  - Choose and document an asset-level design: vesting escrow/claim token, transfer-restricted hatch receipt converted on vesting, issuer send hook, or transfer-aware locked-lot accounting.
  - Prove with multi-test and chain-level tests that direct, partial, multi-hop, split, merge, and mixed vested/unvested transfers cannot make locked value sellable.
  - Prove that legitimately unlocked and Open-purchased tokens remain sellable.
  - Add randomized transfer/buy/sell sequence invariants.

- [ ] **Make every lifecycle transition bounded and practically O(1).** Hatch-to-Open currently rewrites every hatcher, and `AbortHatch` folds every hatcher. `contribution_limits.min` can be zero or dust, so either transition can exceed block gas and become permanently unreachable.
  - Store a single Open/vesting start timestamp instead of stamping each account.
  - Maintain total contributed incrementally instead of scanning the map on abort.
  - Cap all remaining vectors and per-call DAO queries using measured chain gas limits.
  - Demonstrate transition gas is independent of population with at least 10,000 simulated hatchers and remains below each supported chain's budget with a documented safety margin.

- [ ] **Validate all curve configurations before storage and eliminate contract-reachable traps.** Add a fallible `CurveType::validate` used by instantiate, update, and queries/calculations as appropriate.
  - Bound decimals, scale, slopes, coefficients, exponent numerator/denominator, `num + den`, midpoint, steepness, amplitude, and supported supply/reserve domains.
  - Replace assertions, unchecked arithmetic, and silent non-convergence with typed errors.
  - Fuzz every numeric boundary, including zero, one, maxima, decimal extremes, and near-`Uint128`/`u32` limits; no valid execute or query may panic or trap.

- [ ] **Remove Sigmoid from the production API or complete a dedicated quantitative review and audit.** Its numerical reserve and inverse use different integration precision, documented error is large enough to be economically relevant, and non-convergence currently returns an estimate.
  - If retained, establish justified full-domain error bounds, monotonicity, deterministic native/Wasm parity, bounded gas, and no profitable fee-free round trip or transaction-splitting strategy beyond an explicit rounding budget.
  - Non-convergence must return an error.
  - Apply the same domain/error analysis to Power; feature-gate any curve not approved for production.

- [ ] **Pin trusted implementations.** Caller authentication does not make caller-supplied `cw-abc` and issuer code IDs trustworthy.
  - Configure the production factory with governance-approved code IDs and checksums for both ABC and token issuer implementations.
  - Reject unapproved implementations and test with interface-compatible malicious contracts.
  - Expose and emit the selected versions, code IDs, and checksums in factory records/events.

- [ ] **Choose and implement an upgrade strategy.** ABC and issuer instances currently have `admin: None`, making the ABC migrate entrypoint unusable; the factory has no migrate entrypoint.
  - Record an ADR choosing immutable releases or DAO/timelock-controlled migration.
  - If immutable, remove misleading migration surfaces and document defect response, user exit, and replacement/fork procedures.
  - If migratable, use a documented DAO/timelock admin, exact source-version guards, real state transforms, and tests from every supported release.
  - Rehearse the chosen upgrade or replacement procedure on a representative testnet.

- [ ] **Define and prove refund solvency.** Decide whether a failed hatch promises gross-contribution refunds. Today hatch fees may be forwarded or withdrawn before abort, so `reserve + funding` may be less than gross contributions.
  - If full refunds are promised, escrow all contribution-derived funds until Hatch succeeds and prohibit forwarding/withdrawal beforehand.
  - Define transferred/lost hatch-token behavior, claim duration, abandonment, finalization, rounding dust, and last-claim policy.
  - Maintain claimed/refunded aggregates and queries; prove `sum(claims) <= snapshot`, conservation of escrow, and the maximum residual dust.
  - Test multiple hatchers, donations, forwarding modes, withdrawals, double claims, zero claims, transferred tokens, and abandoned claims.

- [ ] **Freeze the final scope and commission an independent external audit.** Scope must include `cw-abc`, every production-enabled `cw-curves` curve, `dao-abc-factory`, `cw-tokenfactory-issuer`, tokenfactory adapters, DAO core/voting callbacks, migrations/admin design, and relevant dependencies.
  - Include economic invariants, rounding/precision, transfer/vesting bypass, malicious dependency contracts, refund solvency, front-running/slippage, and gas/DoS analysis.
  - Resolve every Critical and High; document lower-severity dispositions; obtain auditor verification of fixes.
  - Publish a report tied to the final commit, schemas, optimizer/toolchain, and Wasm checksums.

### P0 — Required verification infrastructure

- [ ] **Add state-machine property and fuzz testing.** Generate sequences across instantiate, buy, sell, transfer, pause, phase changes, abort/refund, close, curve/config updates, ownership, and unauthorized calls.
  - Assert reserve/supply/bank-balance conservation, monotonicity, quote/execute agreement, fee/funding accounting, max-supply bounds, legal transitions, no unauthorized mutation, and no profitable rounding loop beyond a documented bound.
  - Cover all production curves, fee/limit boundaries, decimal extremes, maximum values, and fixed plus scheduled multi-seed runs.
  - Persist minimized failures as regressions and maintain a bounded nightly fuzz corpus/job.

- [ ] **Add normal-PR `cw-multi-test` security flows.** Exercise actual issuer messages, replies, and balances rather than directly seeding storage.
  - Cover vesting and transfer bypasses, factory reverse handshake and malicious implementations, abort/refund/claim, rounding/dust, forwarding modes, DAO-priority allowlists, deadline boundaries, pause behavior, and ownership handoff failures.

- [ ] **Test and build the complete feature matrix.** For each supported Osmosis, generic CosmWasm, and Thorchain tokenfactory backend:
  - Enforce exactly one backend at compile time with a clear error.
  - Run check, unit/multi-test, optimized Wasm build, schema validation, and backend-appropriate integration tests.
  - Publish an explicit backend × chain × curve × critical-path support matrix. Mark combinations unsupported until proven.

- [ ] **Expand real-chain/test-tube coverage.** Exercise every production-enabled curve and the complete Hatch → Open → buy/sell/close and Hatch → Refunding → claim paths, factory/DAO creation, governance interaction, issuer failure, replies, and any supported migration.

### P1 — Protocol and API hardening

- [ ] Commit a lifecycle/state-action matrix for `Hatch`, `Open`, `Closed`, `Refunding`, and `Paused`, and mirror it with table-driven tests. Define whether pause freezes everyone or allows only a narrow emergency action set; owners should not implicitly retain arbitrary buy/sell/withdraw access during an emergency pause.
- [ ] Decide whether `initial_raise.max` is a cap or only a transition trigger. If a cap, reject or partially fill overshoots and prove `reserve <= max` at fee-rounding boundaries.
- [ ] Bound DAO allowlist entries and smart queries per transaction, or replace sequential DAO queries with proof-based membership. Benchmark worst-case failing queries.
- [ ] Decide which economic terms become immutable after the first contribution. Publish a privilege matrix for pause, withdraw, forwarding, fees, phase config, curve, max supply, allowlist, close, and ownership. Production ownership must be DAO/timelock controlled and deployment verification must enforce it.
- [ ] Make factory deployment records authoritative: DAO, voting module, ABC, issuer, denom, backend, code IDs/checksums, version, height, and creation transaction. Define whether a DAO may create multiple curves and expose stable pagination.
- [ ] Stabilize schemas and events. Emit old/new phase on transitions and old/new values for privileged changes; provide a consolidated status/config query; add schema and event snapshots; define semantic-versioning rules.

### P1 — CI, artifacts, gas, and operations

- [ ] Make unit, multi-test, feature-matrix, real-chain integration, schema, optimized artifact, audit, and coverage jobs required PR checks with timeouts and concurrency cancellation.
- [ ] Add coverage floor and changed-lines coverage for the three new crates and issuer integration. Pin CI actions and installed tools by immutable version/SHA.
- [ ] Establish per-chain gas baselines for instantiate, quote, buy, sell, every phase transition, abort/claim, allowlist worst case, privileged updates, factory creation, and each curve at small and maximum supported inputs. Upload PR reports and fail absolute or regression thresholds.
- [ ] Produce backend-qualified, reproducible optimized Wasm artifacts. Rebuild twice, compare checksums (or document unavoidable nondeterminism), run `cosmwasm-check`, enforce size ceilings, and publish schemas, hashes, toolchain/optimizer details, SBOM, licenses, and a source/build manifest.
- [ ] Create deployment and incident runbooks covering supported chains, checksum/code-ID verification, ownership/admin validation, smoke buys/sells, pause, stalled/failed hatch, reserve/bank divergence, issuer or DAO-query failure, governance compromise, tokenfactory/chain upgrades, and upgrade/replacement procedures.
- [ ] Add monitoring for phase/pause/admin/config changes, withdrawals/forwarding, refund activation and claims, DAO query failures, stalled hatch, and internal reserve versus actual bank balance.
- [ ] Complete a testnet launch rehearsal and incident drill; retain deployment manifests, transaction evidence, gas reports, and remediation notes.

### P2 — Economic review and user-facing readiness

- [ ] Obtain independent token-engineering sign-off on curve parameters, fees, raise bounds, vesting, max supply, refund/shutdown behavior, liquidity/run scenarios, whale/Sybil participation, front-running/slippage, reserve shocks, rounding extraction, and governance changes.
- [ ] Check the approved launch configuration into a reviewed, immutable deployment manifest.
- [ ] Replace experimental/stale documentation with schema-valid examples and durable descriptions of every enabled curve, units/decimals, rounding/error bounds, fees, phases, abort/refund, vesting/transfer rules, privileges, supported backends/chains, and safe parameter ranges.
- [ ] Ensure frontends display phase, fees, price impact/slippage protection, vesting/transfer restrictions, refund terms, owner powers, code version, and checksum before users transact.

## Definition of done

The release is complete only when:

1. All P0 and P1 checkboxes above are complete with linked tests, ADRs, reports, and runbooks.
2. CI is green for every advertised backend and chain path, with no stale schemas or unqualified artifacts.
3. The independent audit is closed against the frozen release commit and exact artifact checksums.
4. A non-alpha release manifest maps source commit → schemas → Wasm checksums → deployed chain code IDs.
5. A testnet rehearsal demonstrates creation through DAO DAO, hatch success and failure paths, governance controls, monitoring, and the selected incident/upgrade strategy.
6. The maintainers record an explicit go/no-go approval for each initial mainnet deployment configuration.

## Current evidence

- `cargo +nightly-2024-01-08 test -p cw-curves --locked`: **33 passed** on 2026-07-14.
- Existing strengths include seeded differential curve tests, 30 audit-regression unit tests, generated schemas, and Osmosis test-tube flows.
- These do not close the blockers above: the current vesting bypass, unbounded transitions, unvalidated curve domain, incomplete backend/on-chain matrix, and unaudited post-review code remain production blockers.
