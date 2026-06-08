# Lighter signing fuzz targets

Run from the repo root to grind every target indefinitely (5 min slices,
bails on first crash):

```bash
crates/adapters/lighter/fuzz/grind.sh
```

Longer slices for deeper per-target coverage (10 min each):

```bash
crates/adapters/lighter/fuzz/grind.sh 600
```

Cycle only a subset by name substring (e.g. just the Pornin parity targets):

```bash
crates/adapters/lighter/fuzz/grind.sh 600 pornin
```

Crash artifacts land under `crates/adapters/lighter/fuzz/artifacts/<target>/`.
Ctrl-C between slices to stop. Future fuzz crates follow the same
`<crate>/fuzz/grind.sh` convention.

---

Coverage-guided fuzz targets for the cryptographic primitives, the L2 tx
hash pipeline, and the auth-token hash. Run any time the signing surface
changes; especially before relying on the signer against a live venue.

## Setup

Start from the repository root so the workspace-pinned tools are installed:

```bash
cargo install cargo-binstall --locked
make install-tools
rustup toolchain install nightly
```

`make install-tools` installs the `cargo-fuzz` version pinned in the root `Cargo.toml` under
`[workspace.metadata.tools]`. `cargo-fuzz` requires a nightly toolchain because the underlying
`libfuzzer-sys` runtime depends on unstable compiler flags. The fuzz crate is a standalone
workspace, so it does not affect the parent stable build.

## Targets

| Target                        | What it stresses                                                             |
|-------------------------------|------------------------------------------------------------------------------|
| `fuzz_verify`                 | `PublicKey::verify` against arbitrary `(pk, msg, sig)` byte triples.         |
| `fuzz_point_decode`           | `Point::decode` panic-freedom and decode/encode/decode round trip.           |
| `fuzz_signature_codec`        | `Signature::from_le_bytes_reduce` canonicality + encode-decode idempotence.  |
| `fuzz_hash_no_pad`            | `hash_n_to_m_no_pad` panic-freedom, output length, determinism.              |
| `fuzz_auth_message`           | `hash_auth_message` panic-freedom over arbitrary UTF-8.                      |
| `fuzz_compute_tx_hash`        | `compute_tx_hash` over arbitrary `CreateOrderTxInfo` body fields.            |
| `fuzz_scalar_mul_ct_diff`     | `scalar_mul_ct` vs `scalar_mul` differential on every `(scalar, base)` pair. |
| `fuzz_pornin_diff_decode`     | `Point::decode` vs Pornin's upstream Rust reference on every `Fp5`.          |
| `fuzz_pornin_diff_scalar_mul` | `Point::scalar_mul` vs Pornin's reference on every `(s, base)` pair.         |

## Running

From this directory (`crates/adapters/lighter/fuzz/`):

```bash
# List available targets
cargo +nightly fuzz list

# Run one target until interrupted (Ctrl-C) or a finding lands
cargo +nightly fuzz run fuzz_verify

# Bounded run for CI-style smoke checks
cargo +nightly fuzz run fuzz_verify -- -max_total_time=60

# Run with a corpus seed file
cargo +nightly fuzz run fuzz_verify path/to/seed
```

Findings drop into `fuzz/artifacts/<target>/`. Reproduce a crash with:

```bash
cargo +nightly fuzz run fuzz_verify fuzz/artifacts/fuzz_verify/crash-<hash>
```

Corpora accumulate under `fuzz/corpus/<target>/`. Both directories are
gitignored.

## Grind mode

For long idle sessions (e.g. an hour or overnight), `./grind.sh` round-robins
across every target with a per-slice budget. Spreads the wall time across the registered attack
surfaces instead of pegging one. Bails on the first crash and points at the artifact.

```bash
# 5-minute slice per target, cycle forever (default)
./grind.sh

# 10-minute slices, single target
./grind.sh 600 fuzz_verify
```

Each slice pegs one CPU core. Run only when the machine is otherwise idle.
Corpus persists between cycles, so longer wall time keeps growing coverage.

## Adding a new target

1. Drop a `.rs` file under `fuzz_targets/`.
2. Register it as a `[[bin]]` in `Cargo.toml` with `test = false`,
   `doc = false`, `bench = false`.
3. Use `libfuzzer_sys::fuzz_target!(|data: &[u8]| { ... })`. Slice the
   input to whatever layout the API under test expects; bail out early on
   short inputs.
4. Assert any deterministic invariant the API promises (panic-freedom is
   automatic; round trips, canonicality, and differentials must be
   asserted explicitly).
