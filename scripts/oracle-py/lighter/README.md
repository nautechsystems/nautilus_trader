# Lighter signing tx oracle

Layer 2 fixture generator. Loads the compiled signer released by `lighter-go`,
runs it against fixed inputs for trading-critical L2 transaction types, and dumps the canonical `tx_info`,
`tx_hash`, and signature bytes as JSON. The fixture lands at
`crates/adapters/lighter/test_data/signing_tx_oracle.json` and is loaded by
`#[cfg(test)] mod tests` in `signing/tx/encode.rs` to assert byte-equivalence
with the official signer outputs. The optional grouped fixture covers native OTO, OCO, and OTOCO.

This is the "Layer 2" gate from the signing plan: open-source Go reference
behavior is verified by the field/curve/hash/Schnorr fixture vectors, and the
official compiled signer is checked here as an SDK oracle. Live round-trip
testing remains the final gate for what the venue's sequencer accepts.

The signature (`Sig`) is non-deterministic: the upstream signer draws a fresh
nonce `k` per call. The fixture pins the inputs and the resulting tx_hash for
deterministic byte-equality, plus the signature bytes for the verify-side
round trip.

## Upstream reference

- Reference: [lighter-go v1.0.12](https://github.com/elliottech/lighter-go/releases/tag/v1.0.12),
  revision `324c93b067f24ede24918f3ac10663605d5f11c3`.
- License: Apache-2.0 (Go repository; compiled signer distributed as a binary).

The transaction and grouped-order fixtures cover market IDs `40000`, `4294967296`, and
`281474976710655` across create, cancel, modify, grouped orders, and leverage updates.
The v1.0.12 signer's upper market-ID bound is `281474976710655` (`2^48 - 1`); larger IDs are excluded
because its validation rejects them. The sentinel ID `255` is also invalid. See the pinned
[market-ID bounds](https://github.com/elliottech/lighter-go/blob/324c93b067f24ede24918f3ac10663605d5f11c3/types/txtypes/constants.go#L170-L173).

This program is committed for reproducibility. It is not built into the crate
and is run by hand whenever the upstream signer is bumped or new tx types are
added.

## How to run

```bash
curl -fL https://github.com/elliottech/lighter-go/releases/download/v1.0.12/lighter-signer-linux-amd64.so \
    -o /tmp/lighter-signer-linux-amd64.so
cd scripts/oracle-py/lighter
python3 generate_oracle.py \
    --signer /tmp/lighter-signer-linux-amd64.so \
    --out ../../../crates/adapters/lighter/test_data/signing_tx_oracle.json \
    --grouped-out ../../../crates/adapters/lighter/test_data/signing_grouped_tx_oracle.json
```

The revision above identifies the release tag. The Linux AMD64 release asset used for these fixtures
has SHA-256 `7f107a0e2240aa40c9f0ae8aba396380fe62892fe199dd3691e3103fc9a6fd8e`.

`--grouped-out` optionally writes the grouped fixture to the supplied path, covering OTO, OCO, and
OTOCO with empty and populated transaction attributes. For every fixture, use the signer at the
pinned `UPSTREAM_REVISION` in `generate_oracle.py`. The auth-token and self-trade fixtures remain
unchanged until regenerated with their optional flags.

`--self-trade-out` optionally writes every permitted self-trade behavior/equality pair, with and
without zero-fee integrator attribution. Default-valued modes do not contribute to the attribute hash.

`--auth-out` is optional. When set, the script also drives the signer's
`CreateAuthToken` against fixed `(deadline, account_index, api_key_index)`
triples and writes the signed REST/WS auth tokens to the named JSON file.
The Rust side loads it from `signing/auth_token.rs` to verify oracle tokens
against the same recomputed digest.

Download the release asset for your platform; on Apple Silicon macOS use
`lighter-signer-darwin-arm64.dylib`. The script uses a fixed private key and records the
signer's wall-clock `ExpiredAt`. Regeneration changes expiry, hashes, and random signatures;
the committed fixtures provide fixed inputs for deterministic Rust comparisons.

## Vector shape

Each entry has the form:

```json
{
  "kind":          "create_order",
  "chain_id":      300,
  "sk":            "<hex 40 bytes>",
  "account_index": 12345,
  "api_key_index": 5,
  "expired_at":    1777804107504,
  "nonce":         42,
  "fields":        {"market_index": 0, "...": "..."},
  "tx_type":       14,
  "tx_info":       "<json string emitted by the signer>",
  "tx_hash":       "<hex 40 bytes; the signed message hash>",
  "sig":           "<hex 80 bytes; s_le || e_le>"
}
```

The Rust tests assert:

- `tx_hash` byte-equality vs what our `signing/tx/encode.rs` produces from the
  same fields. Hash assembly is deterministic.
- `pk.verify(tx_hash, sig)` accepts the upstream signature, gating that our
  field/curve/hash/Schnorr decoder agrees with what the compiled signer emits.
- A round trip: signing the same hash under the same `sk` with our explicit-`k`
  variant produces a sig that our `verify` accepts. The signer's `Sig` and
  ours will not be byte-equal because `k` is sampled fresh, but both are
  valid signatures of the same hash.
