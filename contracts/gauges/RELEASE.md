# Gauge release policy

The four gauge crates are published to crates.io in this order:

1. `gauge-interface`;
2. `gauge-orchestrator`;
3. `gauge-adapter`;
4. `gauge-budget-allocator`.

The shared protocol must finish crates.io index propagation before dependent
contract packages can be verified. `scripts/publish.sh` places it in the
workspace package phase and the contracts near the end of the contract phase,
after multiple existing propagation waits. Contract publication uses
`cargo hack publish --no-dev-deps` because workspace dev-dependency cycles
otherwise prevent packaging.

`scripts/test-gauge-packages.sh` creates each normalized crate tarball with the
pinned workspace lockfile, extracts it, and builds/tests that extracted source.
Until the `2.8.0-alpha.2` dependency set is present on crates.io, the test uses
Cargo patches to corresponding workspace packages; the crate under test is
always the extracted tarball. The shared interface directly constrains crypto
dependencies that otherwise resolve to edition-2024 releases incompatible with
the Rust 1.81 release MSRV when no workspace lockfile is present.

Crate publication is distinct from the production Wasm release. A production
tag must additionally provide optimizer-built Wasm, `cosmwasm-check` results,
checksums, schemas, cw2 identities, source commit, optimizer image digest,
audit report, and the chain-test evidence required by `GOAL.md`. A locally
compiled or crates.io-published crate is not deployment approval.

All release Wasm is capped at 800,000 bytes after the linker removes
non-executable name/debug metadata (`-C link-arg=-s`). The release validator also checks
the complete export allowlist: only orchestrator exports `reply`; orchestrator
and marketing export `migrate`; allocator intentionally exports neither. This
rejects accidental debug or entry-point exports in addition to
`cosmwasm-check` capability validation.

## Toolchain compatibility

Gauge CI pins Rust 1.81.0 for both host tests and Wasm builds and validates the
result with `cosmwasm-check` 1.5.11. Rust 1.82 enabled Wasm reference types by
default, which CosmWasm/wasmvm 1.5 rejects; merely passing `cargo build` is not
evidence that an artifact can be stored on chain. The test-only `proptest` and
`tempfile` lockfile versions are pinned to releases that Cargo 1.81 can parse.

The CI Wasm files are unoptimized validation artifacts. Production artifacts
must be built with `scripts/build-gauge-release.sh`. The script pins optimizer
digest
`sha256:7e0b9229c1a4118d0c9a2af2e7f5d95a91f264c26a2ce5681c779926e74d7f85`
and explicitly installs Rust 1.81.0 because the mutable `0.17.0` tag now ships
a newer default Rust. It builds from an isolated source snapshot, records tool
provenance, and marks any explicitly allowed dirty candidate as not releasable.
The manual `Gauge optimized release candidate` workflow runs this clean path
and independently applies the pinned validator.

After the independent audit attestation and chain report are checked in and
signed off, generate the release manifest with:

```sh
scripts/gauge-release-manifest.sh \
  artifacts/gauges artifacts/gauges/manifest.json \
  "$(git rev-parse HEAD)" \
  cosmwasm/optimizer@sha256:7e0b9229c1a4118d0c9a2af2e7f5d95a91f264c26a2ce5681c779926e74d7f85 \
  path/to/audit-report path/to/chain-test-report
```

Manifest creation rejects a dirty checkout, a mismatched source commit,
mutable image tags, missing/untracked evidence, missing Wasm or schemas, and
duplicate artifact identities. Evidence inputs must be JSON documents accepted
by `scripts/validate-gauge-release-evidence.sh`; a placeholder, prose report,
or merely nonempty file cannot satisfy a release gate. The audit attestation
binds the independent report hash and proves no critical/high issue remains.
The chain report proves local and public-testnet deployments, every required
scenario, operation-specific gas/response measurements with the declared
safety margin, a two-epoch maximum-state soak, and maintainer sign-off. The
manifest binds every artifact, schema, and evidence document by SHA-256.

A valid manifest authorizes a canary only. It does not authorize production.
After the two-epoch canary, validate its report and the subsequent governance
decision with `scripts/validate-gauge-production-approval.sh`. Production
automation must require that command to succeed against the exact manifest it
will deploy; see [`EVIDENCE.md`](./EVIDENCE.md).

Production dependency policy is enforced by `cargo-deny` against the Wasm
target and excludes development dependencies plus legacy migration/test roots.
New RustSec advisories, unapproved licenses, wildcard requirements, unknown
registries, and unknown git sources fail CI. `RUSTSEC-2024-0388` is explicitly
recorded because `derivative` is a build-time proc macro inherited from
CosmWasm 1.5, has no maintained compatible replacement, and is not linked into
the contract Wasm. Revisit that exception when upgrading the CosmWasm major
version.

## Integration evidence

The orchestrator multitest suite includes actual native, CW20, and CW721 voting
stacks. The native/token-factory-denom path uses
`dao-voting-token-staked`; the CW20 path uses `cw20-base`, `cw20-stake`, and
`dao-voting-cw20-staked`; the NFT path uses `cw721-base` and
`dao-voting-cw721-staked`. Each registers the orchestrator on the real hook
source and proves stake and unstake transactions update an existing gauge vote
through emitted hooks. These multitests are not equivalent to target-chain gas
and compatibility evidence and must not be reported as chain tests.

## Identity and artifact mapping

| Crate / artifact | cw2 identity | License |
|---|---|---|
| `gauge-orchestrator` / `gauge_orchestrator.wasm` | `crates.io:gauge` | Apache-2.0 |
| `gauge-adapter` / `gauge_adapter.wasm` | `crates.io:marketing-gauge-adapter` | Apache-2.0 |
| `gauge-budget-allocator` / `gauge_budget_allocator.wasm` | `crates.io:gauge-budget-allocator` | BSD-3-Clause |
| `gauge-interface` (library only) | n/a | Apache-2.0 |

The historical cw2 names are intentionally retained for safe migration. Their
mapping to current crate/artifact names is part of the compatibility contract
and must appear in every release manifest.
