# Polymarket Adapter Benchmarks

Numbers measured 2026-08-14 at `0ea286ec6d` on AMD Ryzen Threadripper 9980X under Linux
7.0.0-28-generic and rustc 1.97.1. The `bench-lto` profile uses release opts, `lto = "fat"`,
`codegen-units = 1`, and `debug = full`. The CPU governor was `performance`, and ASLR was disabled
for each benchmark process.

Refresh on substantive perf change or before release; bump the date.
Absolute numbers vary by machine; only same-machine deltas are meaningful.

## How to reproduce

```bash
sudo cpupower frequency-set -g performance
CARGO_BUILD_JOBS=16 setarch "$(uname -m)" -R \
    cargo bench -p nautilus-polymarket --profile bench-lto \
    --bench data --bench effective_deltas --bench exec --bench micros --bench signing
sudo cpupower frequency-set -g powersave  # restore default
```

For policy and the general noise-reduction recipe see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

## Price-change dispatch (`data.rs`)

Decoded six-change frame with alternating changes across two instruments -> two atomic
`OrderBookDeltas` batches. Covers the frame timestamp parse, instrument lookup, grouping, decimal
parse, and domain construction. It excludes JSON decode, subscription state, book application,
channel emission, and network I/O.

| Bench                               | Median | Throughput       |
| ----------------------------------- | ------ | ---------------- |
| `dispatch/price_change_interleaved` | 367 ns | 16.3 M changes/s |

## Inbound pipeline (`data.rs`)

Raw WS frame bytes (market channel) or REST row (user channel) -> Nautilus
domain type. Covers decode + parse + cache lookup + Nautilus type
construction. No I/O, no async runtime, no channel.

Rows ordered from the most fundamental market-data stream (book deltas)
down through the snapshot variant, the derived top-of-book quote
streams, trades, and finally the user-channel reports. `order_event`
and `order_fill` use the REST `GET /orders` and `GET /trades` parse
paths because the WS user-channel -> report conversion is private to
the dispatch loop; both paths share the string-decimal + status logic.

| Bench                                      | Median  | Throughput |
| ------------------------------------------ | ------- | ---------- |
| `inbound_pipeline/book_deltas`             | 471 ns  | 2.12 M/s   |
| `inbound_pipeline/book_snapshot`           | 1.36 µs | 734 k/s    |
| `inbound_pipeline/quote_from_snapshot`     | 1.01 µs | 985 k/s    |
| `inbound_pipeline/quote_from_price_change` | 532 ns  | 1.88 M/s   |
| `inbound_pipeline/trades`                  | 423 ns  | 2.37 M/s   |
| `inbound_pipeline/order_event`             | 601 ns  | 1.66 M/s   |
| `inbound_pipeline/order_fill`              | 1.22 µs | 818 k/s    |
| `inbound_pipeline/order_fill_maker`        | 1.15 µs | 867 k/s    |

## Effective delta processing (`effective_deltas.rs`)

Parsed `OrderBookDeltas` plus a populated L2 MBP book -> updated book and effective domain batch.
Criterion clones the seeded book outside the timed region. Snapshot depth is per side, so depth 100
contains 200 price levels.

| Bench                                                | Estimate |
| ---------------------------------------------------- | -------: |
| `effective_deltas/snapshot/unchanged/10`             | 2.64 µs  |
| `effective_deltas/snapshot/ten_percent_resized/10`   | 2.76 µs  |
| `effective_deltas/snapshot/ten_percent_replaced/10`  | 2.81 µs  |
| `effective_deltas/snapshot/unchanged/100`            | 32.6 µs  |
| `effective_deltas/snapshot/ten_percent_resized/100`  | 32.5 µs  |
| `effective_deltas/snapshot/ten_percent_replaced/100` | 33.0 µs  |

## Execution pipeline (`exec.rs`)

Resolved order inputs -> per-request JSON body + L2 HMAC-SHA256 signature.
Covers market-book crossing-price calculation, maker/taker amount math,
EIP-712 order signing (submits only), JSON body serialization, and the HMAC
body signature `auth_headers` attaches via `Credential::sign`. The market row
starts from decoded realistic CLOB book levels; remote fetch and JSON decode
are omitted. The fixed-cost work `auth_headers` does around the signature
(timestamp string format + the five `POLY_*` header entries) is also omitted;
it's constant overhead unrelated to the regressions these benches are meant
to catch. Polymarket has no in-place modify on the CLOB (cancel-replace is two
independent ops), so there is no `modify` row.

| Bench                                 | Median  | Throughput |
| ------------------------------------- | ------- | ---------- |
| `exec_pipeline/submit_limit`          | 48.3 µs | 20.7 k/s   |
| `exec_pipeline/submit_market`         | 49.0 µs | 20.4 k/s   |
| `exec_pipeline/submit_limit_neg_risk` | 48.8 µs | 20.5 k/s   |
| `exec_pipeline/cancel`                | 254 ns  | 3.93 M/s   |

## Crypto path (`signing.rs`)

Decomposes the exec-pipeline signature cost into its components and
covers the L2 HMAC path used by every authenticated REST call.

| Bench                  | Median  |
| ---------------------- | ------- |
| `sign_order`           | 47.4 µs |
| `sign_order_neg_risk`  | 47.2 µs |
| `sign_order_poly_1271` | 49.0 µs |
| `order_hash`           | 2.78 µs |
| `signer_construction`  | 34.2 µs |
| `sign_clob_auth`       | 81.6 µs |
| `hmac_l2_sign`         | 210 ns  |

## Component breakdown (`micros.rs`)

Diagnostic benches that decompose the pipeline numbers above. Use these
to localise where time goes when a pipeline bench regresses.

| Bench                             | Median  |
| --------------------------------- | ------- |
| `decode_only/trade`               | 274 ns  |
| `decode_only/book`                | 902 ns  |
| `decode_only/price_change`        | 415 ns  |
| `decode_only/user_order`          | 978 ns  |
| `decode_only/user_order_captured` | 952 ns  |
| `decode_only/user_order_dispatch` | 1.05 µs |
| `decode_only/user_trade`          | 639 ns  |
| `decode_only/user_batch`          | 2.15 µs |
| `parse_only/trade`                | 160 ns  |
| `parse_only/book_snapshot`        | 385 ns  |
| `parse_only/book_deltas`          | 40.4 ns |
| `atom/decimal_from_str`           | 7.70 ns |
| `atom/price_from_decimal_dp`      | 11.7 ns |
| `atom/quantity_from_decimal_dp`   | 8.22 ns |
| `atom/price_combined`             | 18.7 ns |
| `atom/compute_commission`         | 119 ns  |
| `atom/adjust_market_buy_amount`   | 209 ns  |
| `atom/trade_id_determine`         | 108 ns  |
| `atom/uuid4_new`                  | 14.5 ns |
| `atom/event_filled_construct`     | 19.0 ns |
| `atom/event_accepted_construct`   | 15.5 ns |

## Notes

- **Inbound decode avoids Serde's tagged content buffer.** Field order varies:
  the market fixtures and synthetic user fixtures put `event_type` first,
  while the captured FOK order puts it last. The production parser decodes
  tag-first messages in one pass and uses a tag scan plus direct typed decode
  for reordered single messages. The LTO market pipelines improve by 26% to
  36%. Against same-session baselines, the tag-first user order and trade
  fixtures improve by about 39% to 40%; the captured tag-last order improves
  by 9.5%, and its handler dispatch path improves by 16.7%. The user batch
  row uses tag-last elements and the generic derived batch parser. Decimal,
  Price, Quantity, UUID, and TradeId construction remain small in the absolute
  pipeline numbers.
- **String -> Price / Quantity is Decimal-direct.** `parse_price` and
  `parse_quantity` in `websocket::parse` route through `Decimal::from_str`
  then `Price::from_decimal_dp` (matches hyperliquid). All Decimal-typed
  REST fields (`PolymarketOpenOrder`, `PolymarketTradeReport`,
  `PolymarketMakerOrder`) and the WS user-channel string fields skip the
  intermediate `f64` parse entirely. The combined string-to-Price path is
  about 18.7 ns and avoids float-rounding risk.
- **Fee-bearing fills are now measured.** `order_fill` uses a non-zero taker
  rate and exponent, so it includes the current fee curve. `compute_commission`
  is about 119 ns. `order_fill_maker` covers one maker leg and its composite
  trade ID, but not the private WS dispatch tracker and emitter work.
- **Exec submits are EIP-712-bound.** `sign_order` is about 47 µs and dominates
  every `exec_pipeline/submit_*` row; LTO collapses the per-shape differences
  so limit, market, and neg-risk converge near 48 to 49 µs. The market row also
  includes the decoded-book price walk and fee-aware BUY sizing. The remaining
  work is maker/taker amount math, builder state, JSON body serialization, and
  the L2 HMAC step. Optimizations that do not change the EIP-712 + keccak +
  secp256k1 path won't move these numbers.
- **POLY_1271 has its own signing baseline.** Its extended signature encoding
  adds little beside the shared secp256k1 cost.
- **`cancel` is HMAC-bound.** REST cancels do not need an EIP-712 signature,
  so the client-side cost is the JSON body serialization plus the L2
  HMAC-SHA256 signature `auth_headers` attaches via `Credential::sign`.
  `Credential` initializes the HMAC key once and streams the four message
  segments without allocating a combined string. The network round trip still
  dominates wall time in production.
- **`sign_clob_auth` carries hidden signer construction.** The function
  builds a fresh `PrivateKeySigner` from the hex key on every call
  (~34 µs of overhead, exactly the `signer_construction` cost) before
  signing. This path is cold (only used by the CLOB `/auth/api-key`
  and `/auth/derive-api-key` flows at credential bootstrap), so the
  overhead is not a production hotspot. If `sign_clob_auth` ever ends
  up on a hot path, accept a pre-constructed signer instead.
- **`trade_id_determine` (108 ns)** is the FNV-1a hash over
  `(asset_id, side, price, size, timestamp)` used to make trade IDs
  deterministic across reconnects.
- **Interleaved price-change dispatch avoids per-change clones.** Against the optimization parent
  `c6bb45e0a7`, the same six-change fixture and controls improve from 638 ns and 9.40 M changes/s to
  367 ns and 16.3 M changes/s. This is 42.5% lower latency and 73.9% higher throughput. The result
  covers grouping and parsing, not end-to-end adapter or network latency. Parent session estimates
  span 597 to 641 ns; optimized session estimates span 365 to 369 ns.
- **Actual user WS dispatch remains a profiling boundary.** The price-change row mirrors the
  production grouping and parsing work but does not call the crate-private router, retained state,
  book application, or emitter. The suite measures user-message decoding and both public report
  builders separately.
