# Third-Party Licenses (Derive adapter)

This crate references third-party material for action-signing equivalence testing.

- **Derive.xyz - `v2-action-signing-python`**
  - Usage: The Rust EIP-712 signing pipeline under `src/signing/` is an original
    implementation of Derive's published self-custodial action-signing protocol.
    The upstream Python SDK serves as the behavioural reference for the signing
    pipeline and generates the oracle vectors recorded under
    `test_data/common/signing_trade_action_vectors.json` via
    `tests/oracle-py/generate_oracle.py`. The vectors are generated outputs for
    equivalence verification and record the pinned upstream revision in their
    metadata.
  - Pinned revision: `d1914d61985e33559244da242892c7255b6fd0ca` (version 0.0.13,
    committed 2025-08-21).
  - Attribution: Derive.xyz <joshua@derive.xyz>, 8baller <8baller@station.codes>
    (authors declared in the upstream `pyproject.toml`).
  - License: MIT (declared via the pyproject classifier; the upstream repository
    carries no LICENSE file at the pinned revision).
  - Source: <https://github.com/derivexyz/v2-action-signing-python>
