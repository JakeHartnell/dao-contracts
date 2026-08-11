# Gauge Orchestrator

A generic, stake-weighted preference signal that periodically translates
into on-chain action. The orchestrator hosts many gauges, each backed by a
pluggable [adapter](../gauge-adapter/README.md) that decides what the gauge
"means". Curve-inspired ([Curve gauges](https://resources.curve.fi/reward-gauges/gauge-weights));
see [`contracts/gauges/README.md`](../README.md) for the bigger picture.

## Lifecycle

1. **Attach.** The DAO calls `ExecuteMsg::CreateGauge` with a `GaugeConfig`
   that references an adapter address. The orchestrator queries the adapter's
   `AllOptions` to seed the option set, then opens it for voting.
2. **Vote.** Anyone with nonzero voting power calls `PlaceVotes` with a list
   of `(option, weight)` pairs whose weights sum to ≤ 1.0. The orchestrator
   walks each voter's previous vote and applies a tally diff in one pass.
3. **Update tallies on stake changes (hook mode).** The orchestrator is registered as a
   staking hook (cw4 `MemberChangedHook`, cw20 `StakeChangedHook`, cw721
   `NftStakeChangedHook`). When a voter's power changes, every gauge they
   voted on is updated automatically — the user does not have to re-vote.
4. **Execute.** Once `next_epoch < block.time`, anyone can call
   `ExecuteMsg::Execute { gauge }`. The orchestrator snapshots the current
   tally, computes the *selected set* (top-N by weight subject to
   `min_percent_selected` and `max_available_percentage`), queries the
   adapter's `SampleGaugeMsgs(selected)` for the `CosmosMsg`s to dispatch,
   and forwards them to the DAO core for execution via
   `ProposalExecuteHook`. `next_epoch` advances by `epoch_size`.
5. **Optional reset.** If the gauge was created with a `reset_epoch`, the
   option list can be wiped and refreshed from the adapter on a separate
   cadence — useful for periodically pruning stale options without
   restarting the gauge.

## Epoch-snapshot power mode

`InstantiateMsg.epoch_snapshot` selects an alternative, non-hook power model
for the entire orchestrator. In this mode `hook_caller` must be empty, every
gauge must define `snapshot_policy`, and periodic reset is disabled.

Anyone may call `OpenEpoch` once the gauge is due. Opening records the current
block height, queries a nonzero historical total at exactly that height, and
copies the adapter's bounded option set into epoch-scoped storage. Every
ballot then queries the voter's power at that same height. A voter's power is
fixed on first use for the epoch; revising or removing a ballot cannot change
the historical value. Full participating power is counted once for turnout,
while `total_cast` counts only the power actually allocated across options.

At `closes_at`, `Execute` compares participation to `min_turnout_bps` using
checked integer cross-multiplication. Failed turnout is a terminal,
no-distribution outcome and does not roll funds into another epoch. Successful
execution supplies the adapter with the snapshotted epoch budget, current DAO
balance, and denomination. Epochs expose bounded ballot/allocation/history
queries and retain their outcome while `CleanupEpoch` removes ballot and
option working state in batches of at most 100. Ballot-list pages return an
explicit `next_start_after` scan cursor because an abstaining voter removes its
active ballot but retains its stable receipt index.

The configured guardian is a stop-only safety authority. It may call
`StopGauge`, but only the owner may resume, change future-epoch policy, or
otherwise administer the gauge. All stake/member hooks and vote-hook
subscriber management are rejected in snapshot mode.

### Stopping and resuming

The owner can call `StopGauge` to freeze new votes, periodic reset work, and
epoch execution. Stake, NFT-stake, and membership hooks deliberately continue
to update the power behind existing votes while stopped. This keeps tallies
current and prevents a stopped gauge from blocking unrelated staking actions.
`ResumeGauge` is owner-only and re-enables the frozen operations without
discarding votes or changing the existing epoch schedule.

## Why one orchestrator for many gauges

Each staking hook adds a CosmWasm call to every staking action. With N
separate gauge contracts each hooked, you pay N × hook-overhead per
stake/unstake. With one orchestrator, the hook fires once and the
orchestrator iterates its tallied state for each registered gauge — far
cheaper.

## Configuration knobs

Per-gauge config lives on the `Gauge` struct in `state.rs`. Mutable via
`ExecuteMsg::UpdateGauge` (owner-gated):

| Field | Meaning |
|---|---|
| `epoch` | Seconds between executions. Minimum 60. |
| `min_percent_selected` | Optional floor: options below this fraction of total cast are not selected. |
| `max_options_selected` | Hard cap on the size of the selected set. |
| `max_available_percentage` | Optional ceiling: an option's effective weight is clamped to this fraction (excess goes to no one). |
| `reset` | Optional periodic option-list refresh. |

## Enforced resource limits

The contract rejects state or payloads above these limits before performing
accounting work:

| Resource | Maximum |
|---|---:|
| Gauges per orchestrator | 100 |
| Options per gauge, including initial adapter options | 100 |
| Weighted option entries in one vote | 100 |
| Active gauge vote records per voter | 100 |
| Vote-hook subscribers | 10 |
| Adapter messages returned for one execution | 100 |
| Members in one cw4 power-change hook | 100 |
| Token IDs in one NFT unstake hook | 100 |
| Reset batch size | 100 (minimum 1) |
| Title or option byte length | 128 |
| Rows returned by a paginated list query | 100 |

The member-hook limit and per-voter gauge limit compose: a maximum-size cw4
hook may update at most 10,000 stored gauge votes. Production approval still
requires target-chain worst-case gas measurements with a documented safety
margin; these structural bounds are not a substitute for that evidence.

## Adapter contract (`AdapterQueryMsg`)

Every adapter must answer:

| Query | Purpose |
|---|---|
| `AllOptions {}` | Initial option seed at gauge attachment. |
| `CheckOption { option }` | Validates user-proposed additions via `AddOption`. |
| `SampleGaugeMsgs { selected, epoch_budget, available_balance, denom }` | Returns `Vec<CosmosMsg>` for the orchestrator to dispatch. Snapshot mode supplies all three budget fields; hook mode leaves them unset for compatibility. |

See [`gauge-adapter/README.md`](../gauge-adapter/README.md) for a worked
example.

### Adapter trust boundary

An attached adapter is trusted with the DAO core's execution authority for
each epoch: `SampleGaugeMsgs` may return arbitrary `CosmosMsg` values, and the
orchestrator forwards them without restricting message type, destination,
denomination, or amount. DAO governance must therefore audit and explicitly
approve each adapter code ID and configuration. The orchestrator is not an
allowlist or spending-limit layer; deployments that require narrower authority
must enforce it in the adapter and DAO proposal policy.

### Turnout policy

Hook mode intentionally has no minimum turnout or quorum. Selection percentages
use only `TOTAL_CAST`, so deployments must accept its low-turnout risk.
Epoch-snapshot mode instead requires an explicit `min_turnout_bps` policy and
uses the historical total-power snapshot as its denominator. A partial ballot
still counts the voter's full fixed power toward turnout, while unallocated
weight remains unspent.

## Voting power edge cases

Vote weight × voting power is computed with truncating integer math. A user
with 1 unit of power who splits 50/50 across two options would have *both*
options counted as 0 — silently erasing their voice. The orchestrator
rejects such votes with `VoteWeightRoundsToZero` so the user can retry with
larger per-option weights or fewer options. Acquire more power to express
finer-grained preferences.

## Storage layout

All non-global state is indexed first by `GaugeId` (u64, auto-incremented)
and then by a secondary key (voter address for votes, option string for
tallies). This is what lets one orchestrator host many gauges efficiently —
`.prefix()` / `.sub_prefix()` queries scope to a single gauge without
scanning the rest.

Key collections (see `state.rs`):

- `GAUGES: Map<GaugeId, Gauge>` — per-gauge config + execution state.
- `TALLY: Map<(GaugeId, &str), u128>` — cumulative weighted power per option.
- `OPTION_BY_POINTS: Map<(GaugeId, u128, &str), u8>` — secondary index for
  top-N selection.
- `TOTAL_CAST: Map<GaugeId, u128>` — denominator for percent math.
- `votes()` — indexed map keyed `(voter, gauge_id) → Vote`.
- `EPOCHS`, `EPOCH_BALLOTS`, and `EPOCH_TALLY` — historical-height metadata,
  receipts, and allocations scoped by `(gauge_id, epoch_id)` in snapshot mode.

### Health and reconciliation

`QueryMsg::GaugeHealth { gauge }` performs a bounded consistency scan of the
gauge's option tallies and sorted selection index. Its `consistent` flag checks
that `TALLY` sums to `TOTAL_CAST`, every active tally has the expected sorted
index entry, tombstoned options have none, and no stale index entries remain.
The response also reports active/invalid/indexed counts, the first mismatch,
and any reset cursor. A tombstoned option's tally legitimately remains in
`TOTAL_CAST` while stored votes reference it; tombstoning removes only its
eligibility for selection. Even at zero tally the tombstone is retained until
bounded reset cleanup, because a zero-power voter may later stake again.

The scan is capped by the contract's option limit. `scan_complete=false` or
`consistent=false` requires operator investigation; use the paginated option
and vote queries to identify affected records rather than editing raw storage.

## Hooks the orchestrator must be registered against

`ExecuteMsg`:

- `StakeChangeHook` — cw20-staked, native-staked, token-factory-staked.
- `NftStakeChangeHook` — cw721-staked.
- `MemberChangedHook` — cw4 group changes.

The DAO's voting module (or its underlying staking contract) must add the
orchestrator address as a hook receiver; otherwise stake changes will not
flow into gauge tallies and the gauge will drift from reality.

## Hooks the orchestrator emits

The orchestrator broadcasts a `GaugeVoteHook` to every registered
subscriber on each `PlaceVotes` call. Subscribers receive the new vote
state — gauge id, voter, the new `Vec<Vote>` (empty on abstain), the
voter's `voting_power` at the snapshot, and `height`. Useful for
participation rewards (a sibling `dao-rewards-distributor` paying for
active gauge participation, off-chain analytics, notification routers,
etc.). The hook payload type is
[`hooks::GaugeVoteHookMsg`](src/hooks.rs); subscribers match on
`GaugeVoteHookExecuteMsg::GaugeVoteHook(..)`.

| ExecuteMsg | Auth | Notes |
|---|---|---|
| `AddHook { addr }` | owner | Add a subscriber. |
| `RemoveHook { addr }` | owner | Drop a subscriber. |

| QueryMsg | Returns | Notes |
|---|---|---|
| `GetHooks {}` | `GetHooksResponse { hooks: Vec<String> }` | List current subscribers. |

Subscriber failure is non-fatal to the voter: submessages use
`reply_on_error`, and each reply ID is mapped to the subscriber's stable
address before dispatch. The `reply` handler removes exactly that address,
so simultaneous failures cannot shift an index and remove a healthy hook.
The mapping is consumed on both success and failure replies, and a
misconfigured downstream contract is self-pruning for future votes.

## Migration compatibility

The cw2 identity is `crates.io:gauge` (the workspace package and artifact are
named `gauge-orchestrator`). The supported source-version matrix is exactly
`2.4.2` and `2.5.0`, the two historical workspace versions under which the
development gauge branch was built. Other older versions, same/newer versions,
and other contract identities are rejected. `MigrateMsg.gauge_config`
optionally updates explicitly named gauges' future epoch/reset deadlines while
preserving votes, tallies, options, hooks, and all other gauges. The response
reports `from_version`, `to_version`, and `migrated_records` for proposal and
indexer verification.
