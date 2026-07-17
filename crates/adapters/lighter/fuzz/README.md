# Lighter signing fuzz targets

Coverage-guided fuzz targets for the cryptographic primitives, L2 transaction hash pipeline, and
auth-token hash. Run them whenever the signing surface changes, especially before relying on the
signer against a live venue.

The ordinary targets are binary targets of `nautilus-lighter` behind its `fuzz` feature. The
workspace pins `libfuzzer-sys`, and `nautilus-live` owns the shared libFuzzer integration. The
Pornin differential targets remain in the `publish = false` package in this directory so the
published Lighter package does not depend on the git-pinned reference implementation.

## Setup

Start from the repository root:

```bash
cargo install cargo-binstall --locked
make install-tools
rustup toolchain install nightly
```

`make install-tools` installs the `cargo-fuzz` version pinned in the root `Cargo.toml` under
`[workspace.metadata.tools]`.

## Adapter targets

| Target                    | What it stresses                                                             |
|---------------------------|------------------------------------------------------------------------------|
| `fuzz_verify`             | `PublicKey::verify` against arbitrary `(pk, msg, sig)` byte triples.         |
| `fuzz_point_decode`       | `Point::decode` panic-freedom and decode/encode/decode round trip.           |
| `fuzz_signature_codec`    | `Signature::from_le_bytes_reduce` canonicality and codec idempotence.        |
| `fuzz_hash_no_pad`        | `hash_n_to_m_no_pad` panic-freedom, output length, and determinism.          |
| `fuzz_auth_message`       | `hash_auth_message` panic-freedom over arbitrary UTF-8.                      |
| `fuzz_compute_tx_hash`    | `compute_tx_hash` over arbitrary `CreateOrderTxInfo` body fields.            |
| `fuzz_scalar_mul_ct_diff` | `scalar_mul_ct` vs `scalar_mul` differential on every `(scalar, base)` pair. |

Direct adapter-target runs disable the workspace's fat-LTO release setting because it is
incompatible with sanitizer-coverage linking. The shared runner sets this environment override
automatically.

Run one adapter target:

```bash
CARGO_PROFILE_RELEASE_LTO=false cargo +nightly fuzz run fuzz_verify \
  --fuzz-dir crates/adapters/lighter \
  --features fuzz
```

Run a bounded adapter target:

```bash
CARGO_PROFILE_RELEASE_LTO=false cargo +nightly fuzz run fuzz_verify \
  --fuzz-dir crates/adapters/lighter \
  --features fuzz \
  -- \
  -max_total_time=60
```

Grind all adapter targets with 5-minute slices, or select a subset by name:

```bash
scripts/fuzz-adapter.sh lighter
scripts/fuzz-adapter.sh lighter 600
scripts/fuzz-adapter.sh lighter 600 scalar
```

Corpora accumulate under `crates/adapters/lighter/corpus/<target>/`. Crash artifacts land under
`crates/adapters/lighter/artifacts/<target>/`.

## Pornin differential targets

| Target                        | What it stresses                                                       |
|-------------------------------|------------------------------------------------------------------------|
| `fuzz_pornin_diff_decode`     | `Point::decode` vs Pornin's reference on every `Fp5`.                  |
| `fuzz_pornin_diff_scalar_mul` | `Point::scalar_mul` vs Pornin's reference on every `(s, base)` pair.   |
| `fuzz_pornin_diff_algebra`    | Field, scalar, and curve algebra vs Pornin's reference implementation. |

Run or grind the retained standalone targets:

```bash
cargo +nightly fuzz run fuzz_pornin_diff_decode \
  --fuzz-dir crates/adapters/lighter/fuzz
crates/adapters/lighter/fuzz/grind.sh
crates/adapters/lighter/fuzz/grind.sh 600 scalar
```

Their corpora and crash artifacts remain under `crates/adapters/lighter/fuzz/`.

## Adding a target

For an ordinary target:

1. Add a `.rs` file under `crates/adapters/lighter/fuzz_targets/`.
2. Register it as a `[[bin]]` in `crates/adapters/lighter/Cargo.toml` with
   `required-features = ["fuzz"]`, `test = false`, `doc = false`, and `bench = false`.
3. Import `nautilus_live::fuzz::fuzz_target` and assert the deterministic invariants promised by
   the API.

Add a target to the retained package only when it requires the git-pinned Pornin reference.
