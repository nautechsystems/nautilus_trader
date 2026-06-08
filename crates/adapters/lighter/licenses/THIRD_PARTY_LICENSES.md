# Third-Party Licenses (Lighter adapter)

This crate references third-party material for the cryptographic primitives
and oracle fixtures Lighter requires for L2 transaction signing.

- **Thomas Pornin – `ecgfp5` reference Rust implementation**
  - Usage: The Rust implementation under `src/signing/field/` and
    `src/signing/curve/` is an original Rust implementation of the Goldilocks
    field `Fp = 2^64 - 2^32 + 1`, the quintic extension `GF(p^5)`, and the
    `ecgfp5` elliptic curve. It follows Pornin's published paper
    (*EcGFp5: a Specialized Elliptic Curve*, IACR ePrint 2022/274) and uses
    the author's reference Rust code as a reading reference. Any
    constants and test vectors copied from upstream are pinned to a specific
    upstream revision and reproduced under `test_data/` for equivalence
    verification. The reference crate itself is also consumed
    as a `#[cfg(test)]` dev-dependency (zero transitive deps; commit-pinned
    via the `rev` field in `Cargo.toml`) by the differential proptest at
    `src/signing/pornin_diff.rs` and the fuzz targets at
    `fuzz/fuzz_targets/fuzz_pornin_diff_*.rs`, which assert byte-equality
    of every public algebra operation against the reference on each
    random sample. The dev-dep is never linked into the production binary.
  - Attribution: Copyright (c) 2022 Thomas Pornin.
  - License: MIT License.
  - Source: <https://github.com/pornin/ecgfp5>
  - Full text: `MIT-pornin-ecgfp5.txt`

- **`elliottech/poseidon_crypto` contributors**
  - Usage: The Rust implementation under `src/signing/hash/` and
    `src/signing/schnorr/` is an original Rust implementation of Poseidon2
    hashing and the Schnorr binding Lighter applies on top of `ecgfp5`. It is
    written from public specifications, with `poseidon_crypto` used as the
    behavioural reference for Lighter's specific parameter sets (round
    constants, MDS matrices). Test vectors reproduced verbatim under
    `test_data/signing_field_goldilocks_vectors.json`,
    `test_data/signing_field_quintic_vectors.json`,
    `test_data/signing_curve_ecgfp5_vectors.json`,
    `test_data/signing_hash_poseidon2_vectors.json`, and
    `test_data/signing_schnorr_vectors.json` record the pinned upstream
    revision in their metadata.
  - Pinned revision: `fbd3713966eeeb9496166db9b599d4a3bb7b9e2b` (committed
    2026-04-10).
  - Attribution: contributors to `elliottech/poseidon_crypto`.
  - License: Apache License 2.0.
  - Source: <https://github.com/elliottech/poseidon_crypto>
  - Full text: `Apache-2.0-poseidon-crypto.txt`

- **`elliottech/lighter-python` SDK contributors**
  - Usage: The script under `tests/oracle-py/` loads the compiled signer
    distributed with the official Python SDK to generate deterministic
    transaction and auth-token oracle fixtures under `test_data/`. The compiled
    signer is not vendored in this repository and is not linked into the crate;
    only the generated fixture outputs are committed for byte-equivalence tests.
  - Attribution: contributors to `elliottech/lighter-python`.
  - License: Apache License 2.0 for the public SDK repository.
  - Source: <https://github.com/elliottech/lighter-python>
  - Full text: `Apache-2.0-poseidon-crypto.txt` (shared Apache-2.0 text).
