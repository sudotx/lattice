# pinocchio-vault

A minimal SOL vault program for Solana, written with [Pinocchio](https://github.com/anza-xyz/pinocchio) 0.11.
Each owner gets one vault PDA; they can deposit lamports and withdraw everything above the rent-exempt minimum.

Program ID: `G9S9ELuKARYok1H8QguVSZ5mu7VoT3FfRBqeFL3vK3uW` (devnet)

## Instructions

Vault PDA seeds: `["vault", owner]`. Instruction data starts with a 1-byte discriminator.

| # | Instruction | Data | Accounts |
|---|---|---|---|
| 0 | `Deposit` | `amount: u64` (LE, > 0) | `owner` (signer, writable), `vault` (writable), `system_program` |
| 1 | `Withdraw` | — | `owner` (signer, writable), `vault` (writable) |

The first deposit creates the vault, including when someone has already sent lamports to the PDA address.

## Prerequisites

- Agave 4.x toolchain (pinocchio 0.11 needs rustc >= 1.89). The Makefile points `SOLANA_BIN` at a local 4.x install; override it if yours lives elsewhere.
- Node + pnpm (IDL / client codegen)
- `make tools` installs `shank-cli` and the Codama deps

## Usage

```bash
make build                   # SBPFv3 build -> target/deploy/pinocchio_vault.so
make test                    # build + LiteSVM tests
make check                   # fmt, clippy, tests
make generate                # Shank IDL (idl/) + TS client (clients/js)
make deploy CLUSTER=devnet   # default CLUSTER is localhost (see `make localnet`)
make idl-upload CLUSTER=devnet
make                         # list all targets
```

Deploys need the program keypair at `keys/pinocchio_vault-keypair.json` (gitignored; `make check-id` verifies it matches the program ID). Mainnet deploys go through `make deploy-mainnet`, which asks for confirmation.
