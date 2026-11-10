# Kalshi Fuzz Targets

Coverage-guided fuzz targets for Kalshi adapter boundaries. These targets stay
below live network I/O. They stress frame decoding, fixed-point and decimal
normalization, book generation and gap handling, V2 order construction and
request signatures, execution-report rejection, and per-subscription sequence
advancement.

Run them when Kalshi wire models, parsers, book synchronization, signing, or
order and fill normalization change.

## Setup

Start from the repository root so the workspace-pinned tools are installed:

```bash
cargo install cargo-binstall --locked
make install-tools
rustup toolchain install nightly
```

`make install-tools` installs the `cargo-fuzz` version pinned in the root
`Cargo.toml` under `[workspace.metadata.tools]`. `cargo-fuzz` requires a nightly
toolchain because `libfuzzer-sys` uses unstable compiler flags. The targets are
binary targets of `nautilus-kalshi` behind its `fuzz` feature. The workspace
pins `libfuzzer-sys`, and `nautilus-live` owns the shared libFuzzer integration.
The adapter does not depend on `libfuzzer-sys` directly.

## Targets

| Target              | What it stresses                                                               |
| ------------------- | ------------------------------------------------------------------------------ |
| `fuzz-ws-decode`    | Frame decode, quote and trade domain bounds, and ticker identity round trips.  |
| `fuzz-fixed-point`  | Exact fixed-point and decimal-number round trips, scale, and bounded encoding. |
| `fuzz-book`         | Book generation, gap rejection, generation reset, and uncrossed emitted books. |
| `fuzz-order-wire`   | V2 limit construction, canonical body bytes, and RSA-PSS and Ed25519 verify.   |
| `fuzz-exec-report`  | Order and fill report rejection, complementary prices, and nonnegative fees.   |
| `fuzz-sid-sequence` | Stale rejection, contiguous acceptance, and gap detection for one SID.         |

A rejected payload does not produce a quote, trade, book batch, order request, or
fill report. The same invalid input is rejected on a second call.

## Running

Direct runs disable the workspace's fat-LTO release setting because it is
incompatible with sanitizer-coverage linking. The shared runner sets this
environment override automatically.

From the repository root:

```bash
cargo +nightly fuzz list --fuzz-dir crates/adapters/kalshi
CARGO_PROFILE_RELEASE_LTO=false cargo +nightly fuzz run fuzz-ws-decode \
  --fuzz-dir crates/adapters/kalshi \
  --features fuzz \
  -- \
  -max_total_time=60
```

Grind every target indefinitely with 5-minute slices:

```bash
scripts/fuzz-adapter.sh kalshi
```

Use a longer slice or filter by target-name substring:

```bash
scripts/fuzz-adapter.sh kalshi 600 book
```

Crash artifacts land under `crates/adapters/kalshi/artifacts/<target>/`. Corpora
accumulate under `crates/adapters/kalshi/corpus/<target>/`. Both directories are
gitignored.

## Seeds

`fuzz-ws-decode` reads the input as frame bytes. When those bytes are UTF-8, it
also wraps that text as the `msg` of a ticker, trade, or book frame. Seed one
complete frame, or one inner ticker or trade object, of at most 4,096 bytes.
Copy the `trade` or `ticker` object from `market_data.json`, not the wrapper.
Each value in `websocket.json` is an escaped frame string. Decode one value and
seed those bytes. The file is 7,563 bytes, so copying it truncates the input
before decode.

Seed `fuzz-fixed-point` with unquoted text such as `2.75`, or one
`{"value": ...}` object, of at most 4,096 bytes. A quoted string does not
exercise the parser.

`fuzz-book`, `fuzz-exec-report`, `fuzz-order-wire`, and `fuzz-sid-sequence` unpack
the input as bytes. They load permitted fixtures with `include_str!`, so copying
those JSON files into the corpus does not seed them.

## Adding a target

1. Add a `.rs` file under `crates/adapters/kalshi/fuzz/fuzz_targets/`.
1. Register it as a `[[bin]]` in `crates/adapters/kalshi/Cargo.toml` with
   `required-features = ["fuzz"]`, `test = false`, `doc = false`, and `bench = false`.
1. Import `nautilus_live::fuzz::fuzz_target`.
1. Keep the harness below network and runtime I/O. Assert deterministic
   invariants when the API promises them. Panic freedom alone is the baseline.
