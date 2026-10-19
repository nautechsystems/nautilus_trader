# CoinAPI Fuzz Targets

Coverage-guided fuzz targets for the CoinAPI adapter's untrusted input boundaries: WebSocket frame
decode and conversion, book sequencing and L3 order tracking, and REST symbol, history, and order
book responses. The targets run below the network and async runtime, so they need no
`COINAPI_API_KEY` or network access.

Run them when the CoinAPI wire models, parsers, symbol mapping, or book synchronization change.

## Setup

Start from the repository root so the workspace-pinned tools are installed:

```bash
cargo install cargo-binstall --locked
make install-tools
rustup toolchain install nightly
```

`make install-tools` installs the `cargo-fuzz` version pinned in the root `Cargo.toml` under
`[workspace.metadata.tools]`. `cargo-fuzz` requires a nightly toolchain because `libfuzzer-sys` uses
unstable compiler flags. The targets are binary targets of `nautilus-coinapi` behind its `fuzz`
feature. The workspace pins `libfuzzer-sys`, and `nautilus-live` owns the shared libFuzzer
integration.

## Targets

| Target              | What it stresses                                                               |
| ------------------- | ------------------------------------------------------------------------------ |
| `fuzz-ws-decode`    | `Frame::decode` and conversion of trades, quotes, OHLCV, books, and depth.     |
| `fuzz-book-sync`    | `BookSyncTracker` over generated `book` and `book_l3` message sequences.       |
| `fuzz-rest-symbols` | Symbol list decode and `parse_instruments`.                                    |
| `fuzz-rest-history` | Quote, trade, and OHLCV history and order book snapshot decode and conversion. |

Each target asserts invariants beyond panic freedom:

- `fuzz-ws-decode` and `fuzz-rest-history`: every decoded decimal written as a plain JSON number,
  quoted or not, equals its token exactly, checked against a canonical digit comparison that shares
  no code with the decoder. Every converted trade, quote, and bar keeps its source prices, sizes,
  and timestamps without rounding, except for OHLCV volume. Accepted volume is rounded half to even
  at the instrument's size precision, and its adjustment stays within both `1e-15` times the source
  volume and `1e-6` times one unit at that precision.
- `fuzz-ws-decode`: book deltas and depth follow their levels in order. A snapshot batch starts with
  a clear and carries the snapshot flag on every delta, and only the last delta ends the batch.
- `fuzz-rest-history`: an order book snapshot whose levels all have positive sizes reports the best
  bid and ask of its listed levels.
- `fuzz-book-sync`: emitted sequences strictly rise across snapshots, reconnects, and
  resubscriptions, and keep a constant offset from the venue sequence within a snapshot episode.
  An update is accepted only when it continues the last accepted sequence, and never after a
  rejected message until a snapshot is accepted. A reference model of the resting L3 orders accepts
  an L3 snapshot or contiguous update exactly when the tracker does, matches every accepted batch,
  and never holds a non-positive remaining size.
- `fuzz-rest-symbols`: every symbol of a supported type yields exactly one instrument or error.
  Parsed instrument IDs are unique and map back to the listed exchange symbol, and increments and
  multipliers equal the listed values.

## Running

Direct runs disable the workspace's fat-LTO release setting because it is incompatible with
sanitizer-coverage linking. The shared runner sets this environment override automatically.

From the repository root:

```bash
cargo +nightly fuzz list --fuzz-dir crates/adapters/coinapi
CARGO_PROFILE_RELEASE_LTO=false cargo +nightly fuzz run fuzz-ws-decode \
  --fuzz-dir crates/adapters/coinapi \
  --features fuzz \
  -- \
  -max_total_time=60
```

Grind every target indefinitely with 5-minute slices, or use a longer slice and filter by
target-name substring:

```bash
scripts/fuzz-adapter.sh coinapi
scripts/fuzz-adapter.sh coinapi 600 rest
```

Crash artifacts land under `crates/adapters/coinapi/artifacts/<target>/`. Corpora accumulate under
`crates/adapters/coinapi/corpus/<target>/`. Both directories are gitignored.

## Seeds

The JSON targets start from the captured fixtures in `test_data/`. Pass the target's corpus
directory first so libFuzzer writes new inputs there, and `test_data/` after it as a read-only seed
directory:

```bash
mkdir -p crates/adapters/coinapi/corpus/fuzz-ws-decode
CARGO_PROFILE_RELEASE_LTO=false cargo +nightly fuzz run fuzz-ws-decode \
  crates/adapters/coinapi/corpus/fuzz-ws-decode \
  crates/adapters/coinapi/test_data \
  --fuzz-dir crates/adapters/coinapi \
  --features fuzz
```

Use the same form for `fuzz-rest-symbols` and `fuzz-rest-history`. `fuzz-book-sync` unpacks bytes
into message sequences directly, so it gets useful coverage without JSON seeds.

## Adding a target

1. Add a `.rs` file under `crates/adapters/coinapi/fuzz/fuzz_targets/`.
1. Register it as a `[[bin]]` in `crates/adapters/coinapi/Cargo.toml` with
   `required-features = ["fuzz"]`, `test = false`, `doc = false`, and `bench = false`.
1. Import `nautilus_live::fuzz::fuzz_target`, and share instruments and checks through
   `fuzz/common/mod.rs`.
1. Keep the harness below network and runtime code. Assert deterministic invariants when the API
   promises them; panic freedom alone is the baseline.
