orc_config := env_var_or_default('CONFIG', '`pwd`/ci/configs/cosm-orc/ci.yaml')
test_addrs := env_var_or_default('TEST_ADDRS', `jq -r '.[].address' ci/configs/test_accounts.json | tr '\n' ' '`)
gas_limit := env_var_or_default('GAS_LIMIT', '10000000')

build:
	cargo build

test:
	cargo test

lint:
	cargo +nightly clippy --all-targets -- -D warnings

gen: build gen-schema

gen-schema:
	./scripts/schema.sh

integration-test: deploy-local workspace-optimize
	RUST_LOG=info CONFIG={{orc_config}} cargo integration-test

test-tube: download-deps workspace-optimize abc-test-tube-wasms
    cargo test --features "test-tube"

test-tube-dev: workspace-optimize
    cargo test --features "test-tube"

abc-test-tube-wasms:
    #!/bin/bash
    set -euo pipefail
    mkdir -p artifacts
    cargo_cmd=(cargo)
    if cargo +nightly-2024-01-08 --version >/dev/null 2>&1; then
        cargo_cmd=(cargo +nightly-2024-01-08)
    elif [[ -d "${CARGO_HOME%/.cargo}/.rustup" ]] && RUSTUP_HOME="${CARGO_HOME%/.cargo}/.rustup" cargo +1.78.0 --version >/dev/null 2>&1; then
        export RUSTUP_HOME="${CARGO_HOME%/.cargo}/.rustup"
        cargo_cmd=(cargo +1.78.0)
    fi

    # Build each contract separately so dependency feature unification cannot
    # enable the `library` feature on the artifact being stored in test-tube.
    RUSTFLAGS='-C link-arg=-s -C link-arg=--allow-undefined -C target-feature=-sign-ext,-bulk-memory' "${cargo_cmd[@]}" build -p cw-tokenfactory-issuer --release --target wasm32-unknown-unknown --no-default-features --features osmosis_tokenfactory
    cp target/wasm32-unknown-unknown/release/cw_tokenfactory_issuer.wasm artifacts/cw_tokenfactory_issuer-osmosis.wasm
    RUSTFLAGS='-C link-arg=-s -C link-arg=--allow-undefined -C target-feature=-sign-ext,-bulk-memory' "${cargo_cmd[@]}" build -p cw-abc --release --target wasm32-unknown-unknown --no-default-features --features osmosis_tokenfactory
    cp target/wasm32-unknown-unknown/release/cw_abc.wasm artifacts/cw_abc.wasm
    RUSTFLAGS='-C link-arg=-s -C link-arg=--allow-undefined -C target-feature=-sign-ext,-bulk-memory' "${cargo_cmd[@]}" build -p dao-abc-factory --release --target wasm32-unknown-unknown --no-default-features --features osmosis_tokenfactory
    cp target/wasm32-unknown-unknown/release/dao_abc_factory.wasm artifacts/dao_abc_factory.wasm
    RUSTFLAGS='-C link-arg=-s -C link-arg=--allow-undefined -C target-feature=-sign-ext,-bulk-memory' "${cargo_cmd[@]}" build -p dao-voting-token-staked --release --target wasm32-unknown-unknown --no-default-features --features osmosis_tokenfactory
    cp target/wasm32-unknown-unknown/release/dao_voting_token_staked.wasm artifacts/dao_voting_token_staked.wasm
    RUSTFLAGS='-C link-arg=-s -C link-arg=--allow-undefined -C target-feature=-sign-ext,-bulk-memory' "${cargo_cmd[@]}" build -p dao-proposal-single@2.8.0-alpha.2 --release --target wasm32-unknown-unknown
    cp target/wasm32-unknown-unknown/release/dao_proposal_single.wasm artifacts/dao_proposal_single.wasm
    RUSTFLAGS='-C link-arg=-s -C link-arg=--allow-undefined -C target-feature=-sign-ext,-bulk-memory' "${cargo_cmd[@]}" build -p dao-dao-core@2.8.0-alpha.2 --release --target wasm32-unknown-unknown
    cp target/wasm32-unknown-unknown/release/dao_dao_core.wasm artifacts/dao_dao_core.wasm

    # osmosis-test-tube's VM is older than current Rust/LLVM wasm defaults.
    # Lower sign-extension opcodes after build so local test-tube matches CI's
    # optimizer-compatible artifact shape.
    wasm-opt --enable-bulk-memory --signext-lowering artifacts/cw_tokenfactory_issuer-osmosis.wasm -o artifacts/cw_tokenfactory_issuer-osmosis.wasm
    wasm-opt --enable-bulk-memory --signext-lowering artifacts/cw_abc.wasm -o artifacts/cw_abc.wasm
    wasm-opt --enable-bulk-memory --signext-lowering artifacts/dao_abc_factory.wasm -o artifacts/dao_abc_factory.wasm
    wasm-opt --enable-bulk-memory --signext-lowering artifacts/dao_voting_token_staked.wasm -o artifacts/dao_voting_token_staked.wasm
    wasm-opt --enable-bulk-memory --signext-lowering artifacts/dao_proposal_single.wasm -o artifacts/dao_proposal_single.wasm
    wasm-opt --enable-bulk-memory --signext-lowering artifacts/dao_dao_core.wasm -o artifacts/dao_dao_core.wasm

integration-test-dev test_name="":
	SKIP_CONTRACT_STORE=true RUST_LOG=info CONFIG='{{`pwd`}}/ci/configs/cosm-orc/local.yaml' cargo integration-test {{test_name}}

bootstrap-dev: deploy-local workspace-optimize
	RUST_LOG=info CONFIG={{orc_config}} cargo run bootstrap-env

deploy-local: download-deps
	docker kill cosmwasm || true
	docker volume rm -f junod_data
	docker run --rm -d --name cosmwasm \
		-e PASSWORD=xxxxxxxxx \
		-e STAKE_TOKEN=ujunox \
		-e GAS_LIMIT={{gas_limit}} \
		-e MAX_BYTES=22020096 \
		-e UNSAFE_CORS=true \
		-p 1317:1317 \
		-p 26656:26656 \
		-p 26657:26657 \
		-p 9090:9090 \
		--mount type=volume,source=junod_data,target=/root \
		ghcr.io/cosmoscontracts/juno:v15.0.0 /opt/setup_and_run.sh {{test_addrs}}

download-deps:
	mkdir -p artifacts target
	# Pinned versions: CosmWasm/cw-plus and cw-nfts float `latest` on each
	# release. Recent cw-plus tags ship wasm built against newer cosmwasm-vm
	# features (specifically the Wasm sign-extension proposal — opcode 0xC0 =
	# 192) that the integration-test chain (juno v15.0.0 → wasmd v0.31 →
	# cosmwasm-vm 1.2) can't deserialize. Pin to v1.1.2 / v0.18.0 which match
	# the workspace's `cw20 = "1.1"` and `cw721 = "0.18"` deps and are
	# known-compatible with the test chain.
	wget https://github.com/CosmWasm/cw-plus/releases/download/v1.1.2/cw20_base.wasm -O artifacts/cw20_base.wasm
	wget https://github.com/CosmWasm/cw-plus/releases/download/v1.1.2/cw4_group.wasm -O artifacts/cw4_group.wasm
	wget https://github.com/CosmWasm/cw-nfts/releases/download/v0.18.0/cw721_base.wasm -O artifacts/cw721_base.wasm

workspace-optimize:
    #!/bin/bash
    if [[ $(uname -m) == 'arm64' ]] || [[ $(uname -m) == 'aarch64' ]]; then docker run --rm -v "$(pwd)":/code \
            --mount type=volume,source="$(basename "$(pwd)")_cache",target=/target \
            --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
            --platform linux/arm64 \
            cosmwasm/optimizer-arm64:0.17.0; \
    elif [[ $(uname -m) == 'x86_64' ]]; then docker run --rm -v "$(pwd)":/code \
            --mount type=volume,source="$(basename "$(pwd)")_cache",target=/target \
            --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
            --platform linux/amd64 \
            cosmwasm/optimizer:0.17.0; fi
