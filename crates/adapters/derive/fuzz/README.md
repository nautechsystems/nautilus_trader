# Derive Fuzz Targets

Coverage-guided fuzz targets for Derive adapter internals. These targets stay below the live
WebSocket flow: they stress frame decoding, venue decimal normalization, trade-module ABI encoding,
EIP-712 hash assembly, and nonce sequencing.

Run them when the Derive wire models, parsers, signing payloads, or nonce manager change.

## Setup

Start from the repository root so the workspace-pinned tools are installed:

```bash
cargo install cargo-binstall --locked
make install-tools
rustup toolchain install nightly
```

`make install-tools` installs the `cargo-fuzz` version pinned in the root `Cargo.toml` under
`[workspace.metadata.tools]`. `cargo-fuzz` requires a nightly toolchain because `libfuzzer-sys` uses
unstable compiler flags. The fuzz crate is a standalone workspace so the parent stable build stays
unchanged.

## Targets

| Target                     | What it stresses                                                        |
|----------------------------|-------------------------------------------------------------------------|
| `fuzz_ws_decode`           | `DeriveWsFrame::parse` plus public/private subscription payload decode. |
| `fuzz_decimal_decode`      | Derive decimal normalization across strings, numbers, and nulls.        |
| `fuzz_trade_module_encode` | Trade module 1e18 scaling and seven-word ABI encoding.                  |
| `fuzz_action_hash`         | EIP-712 action hash and typed-data hash assembly.                       |
| `fuzz_nonce_sequence`      | Per-wallet/subaccount monotonic nonce allocation and refresh ordering.  |

## Running

From this directory (`crates/adapters/derive/fuzz/`):

```bash
cargo +nightly fuzz list
cargo +nightly fuzz run fuzz_ws_decode
cargo +nightly fuzz run fuzz_ws_decode -- -max_total_time=60
```

Run from the repo root to grind every target indefinitely with 5-minute slices:

```bash
crates/adapters/derive/fuzz/grind.sh
```

Use a longer slice or filter by target-name substring:

```bash
crates/adapters/derive/fuzz/grind.sh 600
crates/adapters/derive/fuzz/grind.sh 600 nonce
```

Crash artifacts land under `crates/adapters/derive/fuzz/artifacts/<target>/`. Corpora accumulate
under `crates/adapters/derive/fuzz/corpus/<target>/`. Both directories are gitignored.

## Seeds

The JSON target benefits from real venue payloads. Seed it with Derive fixtures when starting a new
corpus:

```bash
cargo +nightly fuzz run fuzz_ws_decode ../test_data/perps/ws_orderbook_eth.json
cargo +nightly fuzz run fuzz_ws_decode ../test_data/perps/ws_ticker_slim_eth.json
cargo +nightly fuzz run fuzz_ws_decode ../test_data/perps/ws_trade_eth.json
cargo +nightly fuzz run fuzz_ws_decode ../test_data/options/ws_ticker_slim_eth_call.json
```

The structured targets (`fuzz_trade_module_encode`, `fuzz_action_hash`, `fuzz_nonce_sequence`) unpack
bytes directly, so they get useful coverage without JSON seeds.

## Adding a target

1. Add a `.rs` file under `fuzz_targets/`.
2. Register it as a `[[bin]]` in `Cargo.toml` with `test = false`, `doc = false`, and
   `bench = false`.
3. Keep the harness below network/runtime code. Assert deterministic invariants when the API
   promises them; panic-freedom alone is the baseline.
