# Gauge event contract

Mutation responses use a stable `action` attribute. Indexers should select the
single Wasm event containing that attribute and then read the fields below.
Additional attributes may be added compatibly; existing names and meanings must
not change without an API-major release.

## Orchestrator

| Action | Stable fields |
|---|---|
| `create_gauge` | `sender`, `gauge_id`, `adapter` |
| `update_gauge` | `sender`, `gauge_id`, `epoch_size`, `min_percent_selected`, `max_options_selected`, `max_available_percentage` |
| `stop_gauge`, `resume_gauge` | `sender`, `gauge_id` |
| `add_option`, `remove_option` | `sender`, `gauge_id`, `option` |
| `place_vote` | `sender`, `gauge_id`, `option_count`, `voting_power` |
| `member_changed_hook` | `hook_caller`, `member_count`, `member` (repeated), `updated_votes` |
| `stake_change_hook` | `hook_caller`, `kind`, `voter`, `amount`, `updated_votes` |
| `nft_stake_change_hook` | `hook_caller`, `kind`, `voter`, `token_count`, `updated_votes`; NFT stake also has `token_id` |
| `reset_gauge` | `sender`, `gauge_id`, `processed`, `complete`, `next_reset` |
| `execute_tally` | `sender`, `gauge_id`, `next_epoch`, `selected_count`, `message_count` |
| `add_hook`, `remove_hook` | `sender`, `hook` |
| `vote_hook_succeeded`, `remove_failed_vote_hook` | `hook`, `reply_id` |
| `migrate` | `from_version`, `to_version`, `migrated_records` |

`min_percent_selected` and `max_available_percentage` are `none` when
disabled. `updated_votes` counts voter/gauge records changed, not voters or
options.

## Marketing adapter

| Action | Stable fields |
|---|---|
| `create_submission` | `sender`, `submission`, `bond_state`, `liabilities`; bonded rows also include `depositor`, `bond_denom`, `bond_amount` |
| `update_submission` | `sender`, `submission`, `bond_state` |
| `reject` | `sender`, `submission`, `kind`, `bond_state`, `bond_amount`, `liabilities`; bonded rows also include `bond_denom` |
| `return_deposits` | `sender`, `processed`, `complete`, `next_cursor`, `message_count`, `refunded_amount`, `liabilities` |
| `migrate` | `from_version`, `to_version`, `migrated_records`, `migrated_bonds`, `liabilities` |

Bond states are `none`, `active`, `refunded`, or `forfeited`. Amount fields are
base-unit integers. `next_cursor=none` means the refund scan is complete.

## Budget allocator

| Action | Stable fields |
|---|---|
| `add_option`, `remove_option` | `sender`, `option` |
| `update_budget` | `sender`, `denom`, `amount` |

Both ownable adapters emit `update_ownership` with stable fields `sender`,
`owner`, `pending_owner`, and `pending_expiry`. The last three retain the
canonical `cw-ownable` serialization; absent values are the string `none`.
