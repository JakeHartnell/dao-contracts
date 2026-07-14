# Marketing Gauge Adapter

An example adapter for the [gauge orchestrator](../gauge/README.md). It
implements a project-registry-with-bond pattern: projects apply by posting
a refundable deposit, the gauge's voters allocate the reward proportionally
to applicants, and the orchestrator dispatches the payouts at each epoch.

See [`contracts/gauges/README.md`](../README.md) for the broader two-contract
design (orchestrator + adapter); this README focuses on the adapter's
specific semantics. Other adapters can plug into the same orchestrator by
implementing the three `AdapterQueryMsg` variants.

## Lifecycle

1. **Instantiate.** The DAO uploads this contract and instantiates with:
   - `owner` — the only address allowed to call `ReturnDeposits` /
     `Reject`. Managed via standard [`cw_ownable`](https://crates.io/crates/cw-ownable)
     two-step transfer (`UpdateOwnership`) and `RenounceOwnership`.
   - `required_deposit` — optional native or cw20 bond per submission.
   - `community_pool` — refund target for unbid funds (gets a default
     "Unimpressed" submission so unused weight isn't lost).
   - `reward` — total payout pool (native or cw20) for the gauge's epoch.

2. **Submit.** Projects call `ExecuteMsg::CreateSubmission { name, url,
   address }`. If `required_deposit` is native they attach the deposit as
   `funds`; if it's a cw20 they `Send` the cw20 to this contract with an
   inner `ReceiveMsg::CreateSubmission`. Submissions are keyed by
   `address` (the *recipient* of any future reward, distinct from the
   submitter). Re-submitting from the same sender overwrites; submitting
   to an address already claimed by a different sender fails with
   `UnauthorizedSubmission`.

3. **Vote.** Once the gauge orchestrator points at this adapter, voters
   weight the submitted addresses. The orchestrator queries `AllOptions`
   for the option set and `CheckOption` for user-added options.

4. **Execute.** At epoch close, the orchestrator queries
   `SampleGaugeMsgs { selected }` (where `selected` is `Vec<(address,
   Decimal)>` with weights summing to ≤1.0). The adapter returns
   `Vec<CosmosMsg>` — one transfer per recipient, native or cw20 depending
   on how `reward` was configured.

5. **Refund.** The owner calls `ReturnDeposits {}` repeatedly to
   refund posted bonds in bounded batches. Progress is persisted, calls are
   idempotent after completion, and new submissions are paused mid-refund.

## ExecuteMsg

| Variant | Auth | Notes |
|---|---|---|
| `CreateSubmission { name, url, address }` | anyone | Native-deposit path. Funds must match `required_deposit` exactly (or be empty if no deposit required). |
| `Receive(Cw20ReceiveMsg)` | the configured cw20 | cw20-deposit path. Sender of the cw20 `Send` becomes the submission's sender. |
| `Reject { submission, soft }` | `owner` | Remove `submission` from the registry. With `soft = true`, refund the bond to the original sender (good-faith reject). With `soft = false`, forfeit the bond to the community pool (spam / malicious). No-op on the bond side if no `required_deposit` is configured. Cannot target the default community-pool submission. |
| `ReturnDeposits {}` | `owner` | Starts a one-way wind-down and refunds at most 50 posted bonds per call. Use repeatedly until `complete=true`; later calls remain complete no-ops and new submissions stay closed. |
| `UpdateOwnership(action)` | `owner` / pending owner | Standard [`cw_ownable`](https://crates.io/crates/cw-ownable) two-step transfer / renounce flow. |

## QueryMsg (`AdapterQueryMsg`)

| Variant | Response | Purpose |
|---|---|---|
| `Config {}` | `Config` | Inspect the deployed parameters. |
| `AllOptions {}` | `AllOptionsResponse` | Used by the orchestrator on gauge attach. |
| `CheckOption { option }` | `CheckOptionResponse { valid: bool }` | Used by the orchestrator when a voter calls `AddOption`. |
| `SampleGaugeMsgs { selected }` | `SampleGaugeMsgsResponse { execute: Vec<CosmosMsg> }` | Translates a selected set into payout messages. |
| `Submission { address }` | `SubmissionResponse` | Read a single submission. |
| `AllSubmissions { start_after, limit }` | `AllSubmissionsResponse` | Paginated submissions, capped at 100 rows. |
| `SubmissionsBySender { sender, start_after, limit }` | `AllSubmissionsResponse` | Paginated sender-index lookup, capped at 100 rows. |
| `Ownership {}` | `cw_ownable::Ownership<Addr>` | Current owner + any pending transfer. |
| `Liabilities {}` | `LiabilitiesResponse` | Active bond total, escrow balance, refund cursor, and persistent `refunds_complete` state. |

## Errors

| Variant | Trigger |
|---|---|
| `Ownership(OwnershipError)` | Caller is not the owner (or `cw-ownable` transfer-flow misuse). |
| `UnauthorizedSubmission` | Submission to a recipient claimed by a different sender. |
| `InvalidDepositType` | Sent the wrong denom / wrong cw20. |
| `InvalidDepositAmount { correct_amount }` | Sent the right denom but wrong amount. |
| `NoDepositToRefund` | `ReturnDeposits` called on a deposit-less adapter. |
| `SubmissionNotFound(addr)` | `Reject` targeted a submission that isn't in the registry. |
| `CannotRejectDefault` | `Reject` targeted the default community-pool submission. |
| `PaymentError` | Missing funds when a deposit is required. |

## Migration compatibility

The cw2 identity is `crates.io:marketing-gauge-adapter` (the workspace package
and Wasm artifact are named `gauge-adapter`). The supported source-version
matrix is exactly `2.4.2` and `2.5.0`, the historical workspace versions of
the development gauge branch. Other older versions, same/newer versions, and
other contract identities are rejected. Both historical state layouts are
supported:

- legacy `Config.admin` is moved into `cw-ownable` without changing the owner;
- modern `cw-ownable` state, including a pending or renounced owner, is left
  intact.

For pre-bond-accounting state, every live non-default submission is assigned
the configured bond and depositor, sender indices are rebuilt, and aggregate
liabilities are recomputed. Migration aborts before writing if the contract's
native/cw20 escrow cannot cover those liabilities. The synthetic community-pool
entry never receives a bond.

## Writing a different adapter

Any contract that answers `AllOptions`, `CheckOption`, and
`SampleGaugeMsgs` can plug into the orchestrator. Common patterns:

- **Validator-delegation adapter.** Options = validator operator
  addresses; `SampleGaugeMsgs` emits `MsgDelegate` / `MsgRedelegate` for
  the DAO's staking position.
- **AMM-incentive adapter.** Options = pool IDs; `SampleGaugeMsgs` emits
  the AMM-specific incentive-funding message proportional to weights.
- **Budget-allocation adapter.** Options = bank addresses;
  `SampleGaugeMsgs` emits `BankMsg::Send` proportional to weights.

The marketing adapter shipped here is one shape (registry + bond +
proportional payout). The orchestrator is agnostic to the choice.
