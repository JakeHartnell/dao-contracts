# Gauges

A gauge is a stake-weighted preference signal that periodically translates into
on-chain action. Stakers continuously express how some pool of resources should
be allocated (reward emissions, validator delegations, marketing budget, etc).
The gauge orchestrates the voting, the adapter translates the result into
`CosmosMsg`s for the DAO to execute.

Inspired by the [Curve gauge system](https://resources.curve.fi/reward-gauges/gauge-weights).
Forked from the [Wynd DAO repo](https://github.com/wynddao/wynddao) (Apache-2.0;
the upstream license and attribution are preserved in [LICENSE](./LICENSE) and
[NOTICE](./NOTICE)) and modified to support any
DAO DAO voting module — cw4 membership, cw20-staked, cw721-staked, native- or
token-factory-staked.

## Two-contract design

```
                ┌────────────────────────┐         ┌──────────────────────┐
   staking      │                        │  query  │                      │
   hooks ─────▶│   gauge-orchestrator   │◀───────▶│    gauge-adapter     │
               │   (this folder/gauge)  │  msgs   │  (this folder/gauge- │
               │                        │         │   adapter, or your   │
               │                        │         │   own adapter)       │
               └────────────┬───────────┘         └──────────────────────┘
                            │
                            │ executes selected set
                            ▼
                  ┌──────────────────────┐
                  │     DAO DAO core     │
                  │   (proposal module)  │
                  └──────────────────────┘
```

- **`gauge` (gauge-orchestrator)** — generic vote-tally + epoch dispatcher.
  Holds one or many gauges, each with its own adapter. Doesn't know what an
  "option" *is* — just an opaque string that the adapter validates and
  interprets.
- **adapters** — pluggable. Each adapter defines what an option means (a
  project address, a validator address, an AMM pool…), how to validate
  user-submitted options, and how to translate a winning set into
  `CosmosMsg`s. This folder ships two:
  - **[`gauge-adapter`](./gauge-adapter/README.md)** — the *Marketing Gauge
    Adapter*: project registry, refundable bond, proportional reward
    dispatch (native or cw20).
  - **[`budget-allocator`](./budget-allocator/README.md)** — a minimal
    second example: admin-curated option list, no bond, native-token-only
    proportional payouts. Useful as a starting point for new adapters or
    as a treasury-allocator gauge in its own right.

Both contracts are wired into the DAO via the standard `dao-interface` module
plumbing — gauge-orchestrator is typically installed as a proposal module on
DAO DAO core, with the staking module's hooks routed to it.

See the individual contract READMEs for ExecuteMsg / QueryMsg semantics and
integration walk-throughs.

Production reviewers and operators should also read the
[architecture and threat model](./ARCHITECTURE.md) and the
[operations and recovery runbook](./OPERATIONS.md). Indexer fields are defined
in the [event contract](./EVENTS.md), and artifact and crates.io identity rules
are defined in the [release policy](./RELEASE.md). Production monitoring must
implement the [alert contract](./MONITORING.md) and retain its required canary
evidence. The current distinction between locally verified work, remaining
engineering checks, and genuinely external gates is maintained in the
[readiness evidence](./READINESS.md).

## Reference deployments

- [Curve](https://dao.curve.fi/gaugeweight) — the OG.
- [Wynd DAO](https://app.wynddao.com/gauges) — the immediate predecessor of this code.
