# Polymarket Adapter Benchmarks

Numbers measured 2026-05-19 on AMD Ryzen Threadripper 9980X under
rustc 1.95.0, `bench-lto` profile (release opts + `lto = "fat"` +
`codegen-units = 1`, `debug = full`). ASLR is disabled via `setarch -R`
for the run. The CPU governor was `powersave` (not `performance`) for
this capture; absolute numbers will improve under `performance`, but
same-machine deltas remain meaningful.

Refresh on substantive perf change or before release; bump the date.
Absolute numbers vary by machine; only same-machine deltas are meaningful.

## How to reproduce

```bash
sudo cpupower frequency-set -g performance
setarch -R cargo bench -p nautilus-polymarket --profile bench-lto \
    --bench data --bench exec --bench micros --bench signing
sudo cpupower frequency-set -g powersave  # restore default
```

For policy and the general noise-reduction recipe see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

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
|--------------------------------------------|---------|------------|
| `inbound_pipeline/book_deltas`             | 643 ns  | 1.56 M/s   |
| `inbound_pipeline/book_snapshot`           | 1.90 µs | 528 k/s    |
| `inbound_pipeline/quote_from_snapshot`     | 1.60 µs | 625 k/s    |
| `inbound_pipeline/quote_from_price_change` | 672 ns  | 1.49 M/s   |
| `inbound_pipeline/trades`                  | 537 ns  | 1.86 M/s   |
| `inbound_pipeline/order_event`             | 603 ns  | 1.66 M/s   |
| `inbound_pipeline/order_fill`              | 1.05 µs | 949 k/s    |

## Execution pipeline (`exec.rs`)

Strategy intent -> per-request JSON body + L2 HMAC-SHA256 signature.
Covers maker/taker amount math, EIP-712 order signing (submits only),
JSON body serialization, and the HMAC body signature `auth_headers`
attaches via `Credential::sign`. The fixed-cost work `auth_headers`
does around the signature (timestamp string format + the five `POLY_*`
header entries) is omitted; it's constant overhead unrelated to the
regressions these benches are meant to catch. Polymarket has no
in-place modify on the CLOB (cancel-replace is two independent ops),
so there is no `modify` row.

| Bench                                 | Median  | Throughput |
|---------------------------------------|---------|------------|
| `exec_pipeline/submit_limit`          | 49.0 µs | 20.4 k/s   |
| `exec_pipeline/submit_market`         | 46.5 µs | 21.5 k/s   |
| `exec_pipeline/submit_limit_neg_risk` | 47.3 µs | 21.2 k/s   |
| `exec_pipeline/cancel`                | 399 ns  | 2.51 M/s   |

## Crypto path (`signing.rs`)

Decomposes the exec-pipeline signature cost into its components and
covers the L2 HMAC path used by every authenticated REST call.

| Bench                       | Median  |
|-----------------------------|---------|
| `sign_order`                | 44.3 µs |
| `sign_order_neg_risk`       | 44.0 µs |
| `order_hash`                | 2.60 µs |
| `signer_construction`       | 31.5 µs |
| `sign_clob_auth`            | 77.6 µs |
| `hmac_l2_sign`              | 332 ns  |

## Component breakdown (`micros.rs`)

Diagnostic benches that decompose the pipeline numbers above. Use these
to localise where time goes when a pipeline bench regresses.

| Bench                            | Median  |
|----------------------------------|---------|
| `decode_only/trade`              | 384 ns  |
| `decode_only/book`               | 1.56 µs |
| `parse_only/trade`               | 150 ns  |
| `parse_only/book_snapshot`       | 350 ns  |
| `atom/decimal_from_str`          | 6.94 ns |
| `atom/price_from_decimal_dp`     | 10.7 ns |
| `atom/quantity_from_decimal_dp`  | 7.41 ns |
| `atom/price_combined`            | 16.8 ns |
| `atom/trade_id_determine`        | 99.5 ns |
| `atom/uuid4_new`                 | 59.9 ns |
| `atom/event_filled_construct`    | 64.4 ns |
| `atom/event_accepted_construct`  | 60.4 ns |

## Notes

- **Inbound is JSON-decode dominated.** `decode_only/book` (1.56 µs)
  accounts for ~82% of the `book_snapshot` pipeline (1.90 µs);
  `decode_only/trade` (384 ns) is ~71% of the `trades` pipeline (537 ns).
  Decimal / Price / Quantity / UUID / TradeId construction are all sub-
  µs and not meaningful in the absolute pipeline number.
- **String -> Price / Quantity is Decimal-direct.** `parse_price` and
  `parse_quantity` in `websocket::parse` route through `Decimal::from_str`
  then `Price::from_decimal_dp` (matches hyperliquid). All Decimal-typed
  REST fields (`PolymarketOpenOrder`, `PolymarketTradeReport`,
  `PolymarketMakerOrder`) and the WS user-channel string fields skip the
  intermediate `f64` parse entirely. This removed the
  `Decimal -> String -> f64 -> Price` round-trips that dominated the
  earlier `order_event` row, dropping it from ~742 ns to ~603 ns
  (~19%), and dropped `parse_only/book_snapshot` from ~422 ns to
  ~350 ns (~17%). The change also eliminates a class of float-rounding
  risk on the string-to-fixed-point boundary.
- **Exec submits are EIP-712-bound.** `sign_order` is 44 µs and dominates
  every `exec_pipeline/submit_*` row; LTO collapses the per-shape
  differences so limit, market, and neg-risk converge near 47-49 µs.
  The remaining ~3-5 µs is maker/taker amount math, builder state, JSON
  body serialise, and the L2 HMAC step (~332 ns, lost in the noise next
  to ECDSA). Optimisations that don't change the EIP-712 + keccak +
  secp256k1 path won't move these numbers.
- **`cancel` is HMAC-bound (~399 ns).** REST cancels don't need an
  EIP-712 signature, so the whole client-side cost is the JSON body
  serialise plus the L2 HMAC-SHA256 signature `auth_headers` attaches
  via `Credential::sign`. The HMAC alone (`hmac_l2_sign`, 332 ns) is
  ~80% of this row; the network round-trip dominates wall time in
  production.
- **`sign_clob_auth` carries hidden signer construction.** The function
  builds a fresh `PrivateKeySigner` from the hex key on every call
  (~31 µs of overhead, exactly the `signer_construction` cost) before
  signing. This path is cold (only used by the CLOB `/auth/api-key`
  and `/auth/derive-api-key` flows at credential bootstrap), so the
  overhead is not a production hotspot. If `sign_clob_auth` ever ends
  up on a hot path, accept a pre-constructed signer instead.
- **`trade_id_determine` (102 ns)** is the FNV-1a hash over
  `(asset_id, side, price, size, timestamp)` used to make trade IDs
  deterministic across reconnects. Lower numbers are not desirable
  here; the cost reflects the security property.
- **Maker fills don't show up in `order_fill`.** The REST trade-report
  fixture exercises the taker fast path; the maker fan-out
  (`build_maker_fill_report` per maker order in `maker_orders`) is one
  small allocation per leg and is not separately benched. Add a
  `maker_fan_out` row if you need to track it.
