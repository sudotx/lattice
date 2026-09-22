# pinocchio-vault — run `make` for the target list.
#
# Override any variable on the command line, e.g.
#   make deploy CLUSTER=devnet WALLET=~/.config/solana/devnet.json

# pinocchio 0.11 needs rustc >= 1.89, which ships with Agave 4.x platform-tools.
# The active `solana` release (2.3.x) is too old to build this program.
SOLANA_BIN      ?= $(HOME)/.local/share/solana/install/releases/stable-515465dd278366f3785dc75e5cce3165edbe316f/solana-release/bin
CARGO_BUILD_SBF := $(SOLANA_BIN)/cargo-build-sbf
SOLANA          := $(SOLANA_BIN)/solana
SOLANA_KEYGEN   := $(SOLANA_BIN)/solana-keygen
TEST_VALIDATOR  := $(SOLANA_BIN)/solana-test-validator
# SBPFv3 is live on devnet + mainnet; v0-v2 deploys are disabled on a 4.x localnet
# (SIMD-0500) and will be on the real clusters once that feature activates.
SBF_ARCH        ?= v3

PROGRAM_ID      := G9S9ELuKARYok1H8QguVSZ5mu7VoT3FfRBqeFL3vK3uW
PROGRAM_SO      := target/deploy/pinocchio_vault.so
# Kept outside target/ so `cargo clean` can't delete the program's upgrade identity.
PROGRAM_KEYPAIR ?= keys/pinocchio_vault-keypair.json
WALLET          ?= $(HOME)/.config/solana/id.json
CLUSTER         ?= localhost

IDL_DIR         := idl
IDL             := $(IDL_DIR)/pinocchio_vault.json
CODAMA_IDL      := $(IDL_DIR)/pinocchio_vault.codama.json

RPC_localhost    := http://127.0.0.1:8899
RPC_devnet       := https://api.devnet.solana.com
RPC_mainnet-beta := https://api.mainnet-beta.solana.com
# `:=` so a shell-exported RPC_URL (other chains) can't leak in; override with
# `make ... CLUSTER_RPC=https://...` if you need a private endpoint.
CLUSTER_RPC      := $(RPC_$(CLUSTER))

.DEFAULT_GOAL := help
.PHONY: help tools build test fmt lint check idl idl-upload idl-fetch client generate \
        localnet deploy deploy-mainnet program-id check-id show clean

help: ## List targets
	@grep -hE '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

tools: ## Install shank-cli and the Codama codegen deps
	cargo install shank-cli --version 0.4.9 --locked
	pnpm install

# ---- build & test -----------------------------------------------------------

build: ## Build the SBF program -> target/deploy/pinocchio_vault.so
	$(CARGO_BUILD_SBF) --arch $(SBF_ARCH)

test: build ## Build, then run the LiteSVM integration tests
	cargo test

fmt: ## Format Rust sources
	cargo fmt

lint: ## Check formatting and run clippy
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

check: lint test ## Everything CI would run

# ---- codegen ----------------------------------------------------------------

idl: ## Generate the Shank IDL + its Codama form -> idl/
	shank idl --crate-root . --out-dir $(IDL_DIR) --program-id $(PROGRAM_ID)
	pnpm exec codama convert $(IDL) $(CODAMA_IDL)

idl-upload: idl check-id ## Publish the Codama IDL on-chain (Program Metadata) to CLUSTER
	@if [ "$(CLUSTER)" = "mainnet-beta" ]; then echo "refusing mainnet; run the command by hand"; exit 1; fi
	npx -y @solana-program/program-metadata@0.10.0 write idl $(PROGRAM_ID) $(CODAMA_IDL) \
		--keypair $(WALLET) \
		--rpc $(CLUSTER_RPC)

idl-fetch: ## Fetch the on-chain IDL from CLUSTER
	npx -y @solana-program/program-metadata@0.10.0 fetch idl $(PROGRAM_ID) --rpc $(CLUSTER_RPC)

client: idl ## Generate the TS client (@solana/kit) from the IDL -> clients/js
	pnpm exec codama run js

generate: client ## Alias: IDL + TS client

# ---- deploy -----------------------------------------------------------------

localnet: ## Start a local validator (foreground, fresh ledger)
	$(TEST_VALIDATOR) --reset

check-id: ## Fail unless PROGRAM_KEYPAIR matches the declared program ID
	@test -f $(PROGRAM_KEYPAIR) || { echo "missing $(PROGRAM_KEYPAIR)"; exit 1; }
	@actual=$$($(SOLANA_KEYGEN) pubkey $(PROGRAM_KEYPAIR)); \
	if [ "$$actual" != "$(PROGRAM_ID)" ]; then \
		echo "keypair $(PROGRAM_KEYPAIR) is $$actual, but declare_id! is $(PROGRAM_ID)"; exit 1; \
	fi
	@echo "program id ok: $(PROGRAM_ID)"

deploy: build check-id ## Deploy to CLUSTER (localhost | devnet), default localhost
	@if [ "$(CLUSTER)" = "mainnet-beta" ]; then echo "use 'make deploy-mainnet'"; exit 1; fi
	$(SOLANA) program deploy $(PROGRAM_SO) \
		--program-id $(PROGRAM_KEYPAIR) \
		--keypair $(WALLET) \
		--url $(CLUSTER)

deploy-mainnet: build check-id ## Deploy to mainnet-beta (asks for confirmation)
	@echo "Deploying $(PROGRAM_ID) to MAINNET with wallet $(WALLET)"
	@$(SOLANA) balance --keypair $(WALLET) --url mainnet-beta
	@read -p "Type 'mainnet' to continue: " ans && [ "$$ans" = "mainnet" ]
	$(SOLANA) program deploy $(PROGRAM_SO) \
		--program-id $(PROGRAM_KEYPAIR) \
		--keypair $(WALLET) \
		--url mainnet-beta

show: ## Show the deployed program on CLUSTER
	$(SOLANA) program show $(PROGRAM_ID) --url $(CLUSTER)

program-id: ## Print the declared program ID
	@echo $(PROGRAM_ID)

clean: ## Remove build output and generated code (keeps keys/)
	cargo clean
	rm -rf $(IDL_DIR) clients/js/src/generated
