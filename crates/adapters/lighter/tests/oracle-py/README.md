# Lighter signing tx oracle

Layer 2 fixture generator. Loads the closed-source signer the official
`lighter-python` SDK ships with, runs it against deterministic inputs for the
trading-critical L2 transaction types, and dumps the canonical `tx_info`,
`tx_hash`, signature bytes, and pubkey bytes as JSON. The fixture lands at
`crates/adapters/lighter/test_data/signing_tx_oracle.json` and is loaded by
`#[cfg(test)] mod tests` in `signing/tx/encode.rs` to assert byte-equivalence
with the official signer outputs.

This is the "Layer 2" gate from the signing plan: open-source Go reference
behaviour is verified by the field/curve/hash/Schnorr fixture vectors, and the
official compiled signer is checked here as an SDK oracle. Live round-trip
testing remains the final gate for what the venue's sequencer accepts.

The signature (`Sig`) is non-deterministic: the upstream signer draws a fresh
nonce `k` per call. The fixture pins the inputs and the resulting tx_hash for
deterministic byte-equality, plus the signature bytes for the verify-side
round trip.

## Upstream reference

- Reference: <https://github.com/elliottech/lighter-python>
- License: Apache-2.0 (SDK repository; compiled signer distributed as a binary)

This program is committed for reproducibility. It is not built into the crate
and is run by hand whenever the upstream signer is bumped or new tx types are
added.

## How to run

```bash
git clone --depth 1 https://github.com/elliottech/lighter-python.git /tmp/lighter-python
cd crates/adapters/lighter/tests/oracle-py
python3 generate_oracle.py \
    --signer /tmp/lighter-python/lighter/signers/lighter-signer-linux-amd64.so \
    --out ../../test_data/signing_tx_oracle.json \
    --auth-out ../../test_data/signing_auth_token_oracle.json
```

`--auth-out` is optional. When set, the script also drives the signer's
`CreateAuthToken` against fixed `(deadline, account_index, api_key_index)`
triples and writes the signed REST/WS auth tokens to the named JSON file.
The Rust side loads it from `signing/auth_token.rs` to verify oracle tokens
against the same recomputed digest.

The signer `.so` ships inside the `lighter-python` package; on macOS use
`lighter-signer-darwin-arm64.dylib`. The script seeds the signer with a fixed
private key and synthesises a fixed `ExpiredAt` so successive runs produce
identical files.

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
  field/curve/hash/Schnorr decoder agrees with what the closed signer emits.
- A round trip: signing the same hash under the same `sk` with our explicit-`k`
  variant produces a sig that our `verify` accepts. The signer's `Sig` and
  ours will not be byte-equal because `k` is sampled fresh, but both are
  valid signatures of the same hash.
