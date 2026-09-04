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
unstable compiler flags. The targets are binary targets of `nautilus-derive` behind its `fuzz`
feature. The workspace pins `libfuzzer-sys`, and `nautilus-live` owns the shared libFuzzer
integration.

## Targets

| Target                     | What it stresses                                                        |
| -------------------------- | ----------------------------------------------------------------------- |
| `fuzz-ws-decode`           | `DeriveWsFrame::parse` plus public/private subscription payload decode. |
| `fuzz-decimal-decode`      | Derive decimal normalization across strings, numbers, and nulls.        |
| `fuzz-trade-module-encode` | Trade module 1e18 scaling and seven-word ABI encoding.                  |
| `fuzz-action-hash`         | EIP-712 action hash and typed-data hash assembly.                       |
| `fuzz-nonce-sequence`      | Shared wallet/subaccount nonce uniqueness, ordering, and exhaustion.    |

## Running

Direct runs disable the workspace's fat-LTO release setting because it is incompatible with
sanitizer-coverage linking. The shared runner sets this environment override automatically.

From the repository root:

```bash
cargo +nightly fuzz list --fuzz-dir crates/adapters/derive
CARGO_PROFILE_RELEASE_LTO=false cargo +nightly fuzz run fuzz-ws-decode \
  --fuzz-dir crates/adapters/derive \
  --features fuzz
CARGO_PROFILE_RELEASE_LTO=false cargo +nightly fuzz run fuzz-ws-decode \
  --fuzz-dir crates/adapters/derive \
  --features fuzz \
  -- \
  -max_total_time=60
```

Grind every target indefinitely with 5-minute slices:

```bash
scripts/fuzz-adapter.sh derive
```

Use a longer slice or filter by target-name substring:

```bash
scripts/fuzz-adapter.sh derive 600
scripts/fuzz-adapter.sh derive 600 nonce
```

Crash artifacts land under `crates/adapters/derive/artifacts/<target>/`. Corpora accumulate under
`crates/adapters/derive/corpus/<target>/`. Both directories are gitignored.

## Seeds

The JSON target benefits from real venue payloads. Seed it with Derive fixtures when starting a new
corpus:

```bash
CARGO_PROFILE_RELEASE_LTO=false cargo +nightly fuzz run fuzz-ws-decode \
  crates/adapters/derive/test_data/perps/ws_orderbook_eth.json \
  --fuzz-dir crates/adapters/derive \
  --features fuzz
```

The structured targets (`fuzz-trade-module-encode`, `fuzz-action-hash`, `fuzz-nonce-sequence`) unpack
bytes directly, so they get useful coverage without JSON seeds.

## Adding a target

1. Add a `.rs` file under `crates/adapters/derive/fuzz/fuzz_targets/`.
2. Register it as a `[[bin]]` in `crates/adapters/derive/Cargo.toml` with
   `required-features = ["fuzz"]`, `test = false`, `doc = false`, and `bench = false`.
3. Import `nautilus_live::fuzz::fuzz_target`.
4. Keep the harness below network/runtime code. Assert deterministic invariants when the API
   promises them; panic-freedom alone is the baseline.
