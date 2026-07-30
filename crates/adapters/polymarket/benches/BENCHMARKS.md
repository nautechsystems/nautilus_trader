# Polymarket Adapter Benchmarks

Numbers measured 2026-07-31 on AMD Ryzen Threadripper 9980X under
rustc 1.97.1, `bench-lto` profile (release opts + `lto = "fat"` +
`codegen-units = 1`, `debug = full`). The CPU governor was `powersave`,
ASLR was enabled, and the shared host was not quiesced. Treat these as a
directional local baseline. Use the controlled recipe below before making a
performance claim.

Refresh on substantive perf change or before release; bump the date.
Absolute numbers vary by machine; only same‑machine deltas are meaningful.

## How to reproduce

```bash
sudo cpupower frequency-set -g performance
setarch -R cargo bench -p nautilus-polymarket --profile bench-lto \
    --bench data --bench exec --bench micros --bench signing
sudo cpupower frequency-set -g powersave  # restore default
```

For policy and the general noise‑reduction recipe see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

## Inbound pipeline (`data.rs`)

Raw WS frame bytes (market channel) or REST row (user channel) -> Nautilus
domain type. Covers decode + parse + cache lookup + Nautilus type
construction. No I/O, no async runtime, no channel.

Rows ordered from the most fundamental market‑data stream (book deltas)
down through the snapshot variant, the derived top‑of‑book quote
streams, trades, and finally the user‑channel reports. `order_event`
and `order_fill` use the REST `GET /orders` and `GET /trades` parse
paths because the WS user‑channel -> report conversion is private to
the dispatch loop; both paths share the string‑decimal + status logic.

| Bench                                      | Median  | Throughput |
| ------------------------------------------ | ------- | ---------- |
| `inbound_pipeline/book_deltas`             | 684 ns  | 1.46 M/s   |
| `inbound_pipeline/book_snapshot`           | 2.06 µs | 486 k/s    |
| `inbound_pipeline/quote_from_snapshot`     | 1.78 µs | 561 k/s    |
| `inbound_pipeline/quote_from_price_change` | 748 ns  | 1.34 M/s   |
| `inbound_pipeline/trades`                  | 581 ns  | 1.72 M/s   |
| `inbound_pipeline/order_event`             | 615 ns  | 1.63 M/s   |
| `inbound_pipeline/order_fill`              | 1.29 µs | 777 k/s    |
| `inbound_pipeline/order_fill_maker`        | 1.15 µs | 873 k/s    |

## Execution pipeline (`exec.rs`)

Resolved order inputs -> per‑request JSON body + L2 HMAC‑SHA256 signature.
Covers market‑book crossing‑price calculation, maker/taker amount math,
EIP‑712 order signing (submits only), JSON body serialization, and the HMAC
body signature `auth_headers` attaches via `Credential::sign`. The market row
starts from decoded realistic CLOB book levels; remote fetch and JSON decode
are omitted. The fixed‑cost work `auth_headers` does around the signature
(timestamp string format + the five `POLY_*` header entries) is also omitted;
it's constant overhead unrelated to the regressions these benches are meant
to catch. Polymarket has no in‑place modify on the CLOB (cancel‑replace is two
independent ops), so there is no `modify` row.

| Bench                                 | Median  | Throughput |
| ------------------------------------- | ------- | ---------- |
| `exec_pipeline/submit_limit`          | 48.9 µs | 20.5 k/s   |
| `exec_pipeline/submit_market`         | 49.1 µs | 20.4 k/s   |
| `exec_pipeline/submit_limit_neg_risk` | 48.3 µs | 20.7 k/s   |
| `exec_pipeline/cancel`                | 404 ns  | 2.48 M/s   |

## Crypto path (`signing.rs`)

Decomposes the exec‑pipeline signature cost into its components and
covers the L2 HMAC path used by every authenticated REST call.

| Bench                  | Median  |
| ---------------------- | ------- |
| `sign_order`           | 47.4 µs |
| `sign_order_neg_risk`  | 46.8 µs |
| `sign_order_poly_1271` | 48.3 µs |
| `order_hash`           | 2.62 µs |
| `signer_construction`  | 33.1 µs |
| `sign_clob_auth`       | 79.0 µs |
| `hmac_l2_sign`         | 335 ns  |

## Component breakdown (`micros.rs`)

Diagnostic benches that decompose the pipeline numbers above. Use these
to localise where time goes when a pipeline bench regresses.

| Bench                           | Median  |
| ------------------------------- | ------- |
| `decode_only/trade`             | 424 ns  |
| `decode_only/book`              | 1.62 µs |
| `decode_only/price_change`      | 657 ns  |
| `decode_only/user_order`        | 969 ns  |
| `decode_only/user_trade`        | 1.07 µs |
| `parse_only/trade`              | 154 ns  |
| `parse_only/book_snapshot`      | 373 ns  |
| `parse_only/book_deltas`        | 51.8 ns |
| `atom/decimal_from_str`         | 7.39 ns |
| `atom/price_from_decimal_dp`    | 11.5 ns |
| `atom/quantity_from_decimal_dp` | 7.99 ns |
| `atom/price_combined`           | 18.0 ns |
| `atom/compute_commission`       | 147 ns  |
| `atom/adjust_market_buy_amount` | 206 ns  |
| `atom/trade_id_determine`       | 107 ns  |
| `atom/uuid4_new`                | 13.9 ns |
| `atom/event_filled_construct`   | 19.3 ns |
| `atom/event_accepted_construct` | 15.2 ns |

## Notes

- **Inbound is JSON‑decode dominated.** Decode accounts for most of the book,
  price‑change, and trade pipelines. The new user‑channel rows also put raw
  order and trade decoding near 1 µs before dispatch, tracking, metadata, and
  event construction begin. Decimal, Price, Quantity, UUID, and TradeId
  construction are not meaningful in the absolute pipeline numbers.
- **String -> Price / Quantity is Decimal‑direct.** `parse_price` and
  `parse_quantity` in `websocket::parse` route through `Decimal::from_str`
  then `Price::from_decimal_dp` (matches hyperliquid). All Decimal‑typed
  REST fields (`PolymarketOpenOrder`, `PolymarketTradeReport`,
  `PolymarketMakerOrder`) and the WS user‑channel string fields skip the
  intermediate `f64` parse entirely. The combined string‑to‑Price path is
  about 18 ns and avoids float‑rounding risk.
- **Fee‑bearing fills are now measured.** `order_fill` uses a non‑zero taker
  rate and exponent, so it includes the current fee curve. `compute_commission`
  is about 147 ns. `order_fill_maker` covers one maker leg and its composite
  trade ID, but not the private WS dispatch tracker and emitter work.
- **Exec submits are EIP‑712‑bound.** `sign_order` is about 47 µs and dominates
  every `exec_pipeline/submit_*` row; LTO collapses the per‑shape differences
  so limit, market, and neg‑risk converge near 48 to 49 µs. The market row also
  includes the decoded‑book price walk and fee‑aware BUY sizing. The remaining
  work is maker/taker amount math, builder state, JSON body serialization, and
  the L2 HMAC step. Optimizations that do not change the EIP‑712 + keccak +
  secp256k1 path won't move these numbers.
- **POLY_1271 has its own signing baseline.** Its extended signature encoding
  adds little beside the shared secp256k1 cost.
- **`cancel` is HMAC‑bound.** REST cancels do not need an EIP‑712 signature,
  so the client‑side cost is the JSON body serialization plus the L2
  HMAC‑SHA256 signature `auth_headers` attaches via `Credential::sign`. The
  HMAC alone is most of this row; the network round trip dominates wall time in
  production.
- **`sign_clob_auth` carries hidden signer construction.** The function
  builds a fresh `PrivateKeySigner` from the hex key on every call
  (~33 µs of overhead, exactly the `signer_construction` cost) before
  signing. This path is cold (only used by the CLOB `/auth/api-key`
  and `/auth/derive-api-key` flows at credential bootstrap), so the
  overhead is not a production hotspot. If `sign_clob_auth` ever ends
  up on a hot path, accept a pre‑constructed signer instead.
- **`trade_id_determine` (107 ns)** is the FNV‑1a hash over
  `(asset_id, side, price, size, timestamp)` used to make trade IDs
  deterministic across reconnects.
- **Actual user WS dispatch remains a profiling boundary.** The production
  router and its retained trackers are crate‑private, so the canonical external
  Criterion targets do not call them. The suite now measures user‑message
  decoding and both public report builders. A dispatch benchmark should reuse a
  real production boundary rather than add a benchmark‑only interface.
