# Lighter Adapter Benchmarks

Signing numbers measured 2026-08-19 on AMD Ryzen Threadripper 9980X under
rustc 1.97.1, `bench-lto` profile (release opts + `lto = "fat"` +
`codegen-units = 1`, `debug = full`). The CPU governor is pinned to
`performance` and ASLR is disabled via `setarch -R`. Each Rust case is the
median Criterion midpoint of two measured sessions after one warm-up run.

The Go comparison uses Go 1.26.6, `-cpu=1`, the same governor and ASLR
controls, and the median `ns/op` of two sessions.

Refresh on substantive perf change or before release; bump the date.
Absolute numbers vary by machine; only same-machine deltas are meaningful.

This host had other interactive sessions during the run. Repeat the
measurement on a quieter machine if you need a quieter noise floor; the
ratios below are the result to compare.

## How to reproduce

Apply the host controls once, run Rust then Go while they remain in force, and
restore the governor after both sides finish.

```bash
sudo cpupower frequency-set -g performance
```

`setarch "$(uname -m)" -R` disables ASLR for the timed process on both sides.
`taskset` is optional extra isolation and was not used for these numbers.

### Rust signing baseline

```bash
CARGO_BUILD_JOBS=16 setarch "$(uname -m)" -R cargo bench --locked \
    --profile bench-lto -p nautilus-lighter --bench signing_sign_verify
```

Run that cargo command three times. Discard the first as warm-up and report
the median of the two measured sessions.

### Go comparison

The crate does not ship Go sources. Recreate the two scratch modules outside
the repository, paste the listings below, then run them. A Go 1.23+ toolchain
is required; install it from <https://go.dev/dl/> if `go version` is missing.

The primitive suite pins `elliottech/poseidon_crypto` to
`fbd3713966eeeb9496166db9b599d4a3bb7b9e2b`, the same revision as the crate
fixture vectors. Inputs are byte-identical to
[`common/mod.rs`](common/mod.rs).

The SDK suite pins `elliottech/lighter-go` to
`cef81af980850607a66213fcca5f3e76ddebda7e`
(`v1.0.9-0.20260812092842-cef81af98085`). That module depends on
`poseidon_crypto` v0.0.15. `ConstructCreateOrderTx` validates, hashes, and
signs the same create-order fields as `create_order_tx()`. The official Go
client signs in process through `poseidon_crypto`. It does not load the
closed-source shared library used by `lighter-python`.

With the governor still on `performance`, create the scratch tree, paste each
file, tidy modules, and run each suite twice. Take the median `ns/op`.

```bash
mkdir -p /tmp/lighter-signing-go/primitives /tmp/lighter-signing-go/sdk
# paste go.mod and bench_test.go into each directory
cd /tmp/lighter-signing-go/primitives
go mod tidy
setarch "$(uname -m)" -R go test -bench=. -benchtime=3s -cpu=1

cd /tmp/lighter-signing-go/sdk
go mod tidy
setarch "$(uname -m)" -R go test -bench=. -benchtime=3s -cpu=1

sudo cpupower frequency-set -g powersave
```

#### Primitive suite

`/tmp/lighter-signing-go/primitives/go.mod`:

```text
module lighter-signing-primitives

go 1.23

require github.com/elliottech/poseidon_crypto v0.0.0-20260410093228-fbd3713966ee
```

`/tmp/lighter-signing-go/primitives/bench_test.go`:

```go
package primitives

import (
        "testing"

        curve "github.com/elliottech/poseidon_crypto/curve/ecgfp5"
        g "github.com/elliottech/poseidon_crypto/field/goldilocks"
        gFp5 "github.com/elliottech/poseidon_crypto/field/goldilocks_quintic_extension"
        schnorr "github.com/elliottech/poseidon_crypto/signature/schnorr"
)

func fixedSk() curve.ECgFp5Scalar {
        bytes := []byte{
                0x0b, 0x8e, 0x0f, 0x63, 0xc2, 0x4d, 0x8b, 0xaa, 0xcd, 0x9d, 0x29, 0xad, 0x4e, 0x9a, 0x4b,
                0x73, 0xc4, 0xa8, 0xd2, 0xbb, 0x8b, 0x16, 0xdc, 0x4f, 0xa9, 0xd7, 0xc2, 0xe1, 0xd3, 0xa8,
                0xb1, 0xf0, 0xe8, 0xd3, 0xa4, 0xc5, 0xb6, 0xe7, 0xf0, 0x01,
        }
        return curve.ScalarElementFromLittleEndianBytes(bytes)
}

func fixedK() curve.ECgFp5Scalar {
        var bytes [40]byte
        bytes[0] = 0x42
        bytes[7] = 0x01
        bytes[16] = 0x91
        bytes[24] = 0x37
        return curve.ScalarElementFromLittleEndianBytes(bytes[:])
}

func fixedHashedMsg() gFp5.Element {
        return gFp5.Element{
                g.GoldilocksField(0x0123_4567_89AB_CDEF),
                g.GoldilocksField(0xFEDC_BA98_7654_3210),
                g.GoldilocksField(0x1111_2222_3333_4444),
                g.GoldilocksField(0x5555_6666_7777_8888),
                g.GoldilocksField(0x0000_0001_0000_0001),
        }
}

func fixedPk() gFp5.Element {
        return schnorr.SchnorrPkFromSk(fixedSk())
}

func fixedSignature() schnorr.Signature {
        return schnorr.SchnorrSignHashedMessage2(fixedHashedMsg(), fixedSk(), fixedK())
}

var (
        sinkSig  schnorr.Signature
        sinkBool bool
        sinkPk   gFp5.Element
)

func BenchmarkSchnorrSign(b *testing.B) {
        sk := fixedSk()
        k := fixedK()
        msg := fixedHashedMsg()
        b.ResetTimer()
        for i := 0; i < b.N; i++ {
                sinkSig = schnorr.SchnorrSignHashedMessage2(msg, sk, k)
        }
}

func BenchmarkSchnorrVerify(b *testing.B) {
        pk := fixedPk()
        msg := fixedHashedMsg()
        sig := fixedSignature()
        b.ResetTimer()
        for i := 0; i < b.N; i++ {
                sinkBool = schnorr.IsSchnorrSignatureValid(pk, msg, sig)
        }
}

func BenchmarkSchnorrPkFromSk(b *testing.B) {
        sk := fixedSk()
        b.ResetTimer()
        for i := 0; i < b.N; i++ {
                sinkPk = schnorr.SchnorrPkFromSk(sk)
        }
}
```

#### Official SDK suite

`/tmp/lighter-signing-go/sdk/go.mod`:

```text
module lighter-signing-sdk

go 1.23.0

require github.com/elliottech/lighter-go v1.0.9-0.20260812092842-cef81af98085
```

`/tmp/lighter-signing-go/sdk/bench_test.go`:

```go
package sdk

import (
        "testing"

        "github.com/elliottech/lighter-go/signer"
        "github.com/elliottech/lighter-go/types"
        "github.com/elliottech/lighter-go/types/txtypes"
)

const (
        chainID      uint32 = 304
        accountIndex int64  = 12345
        apiKeyIndex  uint8  = 5
        nonce        int64  = 42
        expiredAt    int64  = 1_777_809_907_000
)

func fixedSkBytes() []byte {
        return []byte{
                0x0b, 0x8e, 0x0f, 0x63, 0xc2, 0x4d, 0x8b, 0xaa, 0xcd, 0x9d, 0x29, 0xad, 0x4e, 0x9a, 0x4b,
                0x73, 0xc4, 0xa8, 0xd2, 0xbb, 0x8b, 0x16, 0xdc, 0x4f, 0xa9, 0xd7, 0xc2, 0xe1, 0xd3, 0xa8,
                0xb1, 0xf0, 0xe8, 0xd3, 0xa4, 0xc5, 0xb6, 0xe7, 0xf0, 0x01,
        }
}

func createOrderReq() *types.CreateOrderTxReq {
        return &types.CreateOrderTxReq{
                MarketIndex:      1,
                ClientOrderIndex: 7,
                BaseAmount:       1_000_000,
                Price:            25_000_000,
                IsAsk:            0,
                Type:             txtypes.LimitOrder,
                TimeInForce:      txtypes.ImmediateOrCancel,
                ReduceOnly:       0,
                TriggerPrice:     0,
                OrderExpiry:      txtypes.NilOrderExpiry,
        }
}

func transactOpts() *types.TransactOpts {
        account := accountIndex
        apiKey := apiKeyIndex
        n := nonce
        return &types.TransactOpts{
                FromAccountIndex: &account,
                ApiKeyIndex:      &apiKey,
                ExpiredAt:        expiredAt,
                Nonce:            &n,
        }
}

var (
        sinkTx   *txtypes.L2CreateOrderTxInfo
        sinkHash []byte
)

func BenchmarkConstructCreateOrder(b *testing.B) {
        key, err := signer.NewKeyManager(fixedSkBytes())
        if err != nil {
                b.Fatal(err)
        }
        tx := createOrderReq()
        ops := transactOpts()
        if _, err := types.ConstructCreateOrderTx(key, chainID, tx, ops); err != nil {
                b.Fatal(err)
        }
        b.ResetTimer()
        for i := 0; i < b.N; i++ {
                sinkTx, err = types.ConstructCreateOrderTx(key, chainID, tx, ops)
                if err != nil {
                        b.Fatal(err)
                }
        }
}

func BenchmarkCreateOrderHash(b *testing.B) {
        txInfo := types.ConvertCreateOrderTx(createOrderReq(), transactOpts())
        if err := txInfo.Validate(); err != nil {
                b.Fatal(err)
        }
        var err error
        b.ResetTimer()
        for i := 0; i < b.N; i++ {
                sinkHash, err = txInfo.Hash(chainID)
                if err != nil {
                        b.Fatal(err)
                }
        }
}
```

For policy and the general noise-reduction recipe see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

## Signing (`signing_sign_verify.rs`)

Published L2 signing baseline. `PrivateKey::sign` and `sign_tx` use a fixed
nonce `k` so the timed region is hash and curve work, not RNG.

| Bench                                   | Median  | Throughput |
| --------------------------------------- | ------: | ---------: |
| `signing/PrivateKey::sign`              | 67.1 µs |   14.9 k/s |
| `signing/PublicKey::verify`             |  139 µs |   7.19 k/s |
| `signing/PrivateKey::public_key`        | 64.6 µs |   15.5 k/s |
| `signing/compute_tx_hash (CreateOrder)` | 1.99 µs |    503 k/s |
| `signing/compute_tx_hash (CancelOrder)` |  994 ns |   1.01 M/s |
| `signing/sign_tx (CreateOrder)`         | 67.8 µs |   14.7 k/s |
| `signing/sign_tx (CancelOrder)`         | 66.8 µs |   15.0 k/s |
| `signing/build_auth_token_at`           | 67.8 µs |   14.7 k/s |

## Comparison with the official Go stack

| Workload                  | Go                    | Rust (`bench-lto`) | Speedup |
| ------------------------- | --------------------: | -----------------: | ------: |
| Schnorr sign (fixed `k`)  |                247 µs |            67.1 µs |    3.7x |
| Schnorr verify            |                438 µs |             139 µs |    3.2x |
| Public-key derivation     |                241 µs |            64.6 µs |    3.7x |
| CreateOrder hash          |               4.37 µs |            1.99 µs |    2.2x |
| CreateOrder hash and sign | 283 µs (`lighter-go`) |            67.8 µs |    4.2x |

The first three rows are the same `poseidon_crypto` primitives as
`PrivateKey::sign`, `PublicKey::verify`, and `PrivateKey::public_key`, with
a fixed nonce on both sides.

CreateOrder hash is `L2CreateOrderTxInfo.Hash` versus `compute_tx_hash`.
Both use empty attributes, so only the body Poseidon2 preimage runs.

CreateOrder hash and sign is not the same function on both sides:

- Go runs `types.ConstructCreateOrderTx`: validate, hash, sample a fresh
  nonce, and sign.
- Rust runs `sign_tx` with the fixture nonce already in hand.

The extra Go work is real official-SDK cost. Use the fixed-`k` primitive
rows when the question is curve and hash implementation cost.

Verify is the most expensive primitive in both implementations because it
performs a double-base scalar multiplication plus a Poseidon2 hash. Sign
and public-key derivation each reduce to one constant-time scalar
multiplication. `sign_tx` is that sign plus a 2 µs create-order hash.

## Other suites

`data.rs`, `exec.rs`, and `micros.rs` cover the inbound pipeline, signed
wire assembly, and diagnostic breakdown. They were not remeasured in this
pass. `signing_field.rs` is wall-clock noise at the per-op level;
`signing_field_iai.rs` is the instruction-count gate for those primitives.

Quote `signing_sign_verify.rs` when publishing a signing number.
`micros.rs` repeats a few of the same calls next to decode and JSON render
so a pipeline regression can be localised.
