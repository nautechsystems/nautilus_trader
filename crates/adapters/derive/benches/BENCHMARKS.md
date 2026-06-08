# Derive Adapter Benchmarks

Numbers measured 2026-05-30 on AMD Ryzen Threadripper 9980X under
rustc 1.95.0, `bench-lto` profile (release opts + `lto = "fat"` +
`codegen-units = 1`, `debug = full`). The CPU governor is pinned to
`performance` and ASLR is disabled via `setarch -R`. The suite is run once to
warm caches and settle clocks, then measured; a cold first run inflates every
row by roughly 10%.

Refresh on substantive perf change or before release; bump the date.
Absolute numbers vary by machine; only same-machine deltas are meaningful.

## How to reproduce

```bash
sudo cpupower frequency-set -g performance
setarch -R cargo bench -p nautilus-derive --profile bench-lto \
    --bench data --bench exec --bench micros --bench signing  # warm-up
setarch -R cargo bench -p nautilus-derive --profile bench-lto \
    --bench data --bench exec --bench micros --bench signing  # measured
sudo cpupower frequency-set -g powersave  # restore default
```

For policy and the general noise-reduction recipe see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

## Inbound pipeline (`data.rs`)

Raw WS frame bytes -> Nautilus domain type. Covers frame decode (single-pass
into a typed frame) + channel decode (raw payload bytes -> typed struct) +
parse + Nautilus type construction. No I/O, no async runtime, no channel. The
`bars` row is the REST OHLCV path (Derive has no WS candle channel): it decodes
the candle record and builds a `Bar`.

Rows ordered from the most fundamental market-data stream (book deltas) down
through the ticker-derived streams (quotes/mark/index/funding/bars), then the
options-specific greeks stream last.

| Bench                            | Median  | Throughput |
|----------------------------------|---------|------------|
| `inbound_pipeline/book_deltas`   | 473 ns  | 2.12 M/s   |
| `inbound_pipeline/quotes`        | 1.64 µs | 610 k/s    |
| `inbound_pipeline/trades`        | 742 ns  | 1.35 M/s   |
| `inbound_pipeline/mark_price`    | 1.59 µs | 631 k/s    |
| `inbound_pipeline/index_price`   | 1.59 µs | 631 k/s    |
| `inbound_pipeline/funding_rate`  | 1.57 µs | 639 k/s    |
| `inbound_pipeline/bars`          | 670 ns  | 1.49 M/s   |
| `inbound_pipeline/option_greeks` | 2.35 µs | 426 k/s    |

## Execution pipeline (`exec.rs`)

Strategy command (`OrderAny` / cancel) -> wire bytes ready to send.
`submit_limit`, `submit_market`, and `modify` cover the signed `private/order`
and `private/replace` path (normalize + ABI encode + EIP-712 sign + JSON
serialize). `cancel` covers the unsigned `private/cancel` path (build +
serialize). Derive supports only Limit and Market orders, so there is no
stop-order row.

| Bench                         | Median  | Throughput |
|-------------------------------|---------|------------|
| `exec_pipeline/submit_limit`  | 42.1 µs | 23.8 k/s   |
| `exec_pipeline/submit_market` | 42.1 µs | 23.7 k/s   |
| `exec_pipeline/modify`        | 42.1 µs | 23.7 k/s   |
| `exec_pipeline/cancel`        | 45.8 ns | 21.8 M/s   |

## Signing (`signing.rs`)

`sign_trade_action` is the EIP-712 order signature (ABI encode + keccak +
secp256k1) the order-submit path pays per order. `rest_auth_headers` is the
EIP-191 timestamp signature the HTTP read path pays per request.
`signer_from_key` is the secp256k1 key expansion, paid once at client startup.

| Bench               | Median  |
|---------------------|---------|
| `sign_trade_action` | 42.0 µs |
| `rest_auth_headers` | 40.9 µs |
| `signer_from_key`   | 31.6 µs |
| `abi_encode_trade`  | 236 ns  |
| `nonce_next`        | 45.8 ns |

## Dispatch (`exec.rs`)

Venue WS payload (`DeriveOrdersSubscriptionData`, `DeriveTradesSubscriptionData`)
-> events emitted via `ExecutionEventEmitter`. Covers parse + dedup + identity
lookup + event construction through `dispatch_orders_payload` /
`dispatch_trades_payload`. `orders_untracked` forwards a raw status report (no
registered identity); `orders_tracked` and `trades_fill` resolve a registered
identity and emit `OrderAccepted` / `OrderFilled` events.

| Bench                       | Median  | Throughput |
|-----------------------------|---------|------------|
| `dispatch/orders_untracked` | 8.53 µs | 117 k/s    |
| `dispatch/orders_tracked`   | 9.01 µs | 111 k/s    |
| `dispatch/trades_fill`      | 8.45 µs | 118 k/s    |

## Component breakdown (`micros.rs`)

Diagnostic benches that decompose the pipeline numbers above. Use these to
localise where time goes when a pipeline bench regresses. `decode_only` is the
raw-bytes -> typed-message cost; `parse_only` is the typed-message -> Nautilus
domain cost; the two sum to the matching inbound number. `order_report` and
`fill_report` decompose the inbound execution path that `dispatch` runs
end-to-end.

| Bench                         | Median  |
|-------------------------------|---------|
| `decode_only/orderbook`       | 423 ns  |
| `decode_only/ticker`          | 1.56 µs |
| `parse_only/orderbook_deltas` | 49.9 ns |
| `parse_only/trade`            | 32.2 ns |
| `parse_only/ticker_quote`     | 36.6 ns |
| `parse_only/order_report`     | 90.7 ns |
| `parse_only/fill_report`      | 109 ns  |
| `atom/decimal_from_str`       | 6.97 ns |
| `atom/price_from_decimal_dp`  | 6.54 ns |
| `atom/price_combined`         | 12.3 ns |
| `atom/trade_id_new`           | 8.89 ns |
| `atom/uuid4_new`              | 12.9 ns |
| `atom/state_construct_primed` | 4.11 µs |
| `atom/state_drop_primed`      | 1.13 µs |
| `atom/dedup_trade_hit`        | 11.6 ns |

## Notes

- **Inbound decode avoids the `Value` intermediate.** The frame parses in a
  single pass into a typed struct, capturing `params.data` as a
  `serde_json::value::RawValue` (the raw payload bytes); each channel parser
  then decodes those bytes straight into its typed struct. Nothing
  materialises the frame, or the large `data` subtree, into a `serde_json::Value`
  tree. This roughly halved every inbound row versus the prior `Value`-based
  decode (e.g. `decode_only/ticker` 3.11 µs -> 1.56 µs, `book_deltas`
  1.06 µs -> 0.47 µs).
- **Inbound is still decode-dominated.** `decode_only` accounts for ~90% of
  `book_deltas` (423 ns of 473 ns) and ~95% of `quotes` (1.56 µs of 1.64 µs).
  Parse itself is sub-50 ns for a book delta and under 40 ns for a quote/trade;
  Decimal, Price, UUID4, and TradeId construction are all sub-15 ns.
- **The four ticker-derived rows share one decode in production.** `quotes`,
  `mark_price`, `index_price`, and `funding_rate` each measure a standalone
  `DeriveTickerMsg` decode (~1.56 µs) plus a sub-40 ns parse, so they all land
  at ~1.6 µs. The live data client decodes a ticker frame once and derives all
  four from that single message; summing the four rows overcounts. The lever
  for all of them is the ticker decode, not the per-stream parse.
- **`option_greeks` is the heaviest inbound row** (2.35 µs) because the option
  slim ticker carries the `option_pricing` block (delta/gamma/vega/theta/rho,
  IVs, forward) on top of the shared ticker fields.
- **Exec is signature-bound.** `sign_trade_action` (EIP-712: ABI encode +
  keccak + secp256k1) is 42.0 µs and dominates `submit_limit`/`submit_market`
  and `modify` (all ~42.1 µs); ABI encode (236 ns) and JSON serialize are noise
  next to it. `cancel` is unsigned and lands at 46 ns. Optimisations that don't
  change the signing scheme won't move the signed rows. `rest_auth_headers`
  (EIP-191) costs ~41 µs because it is the same secp256k1 sign.
- **`signer_from_key` is amortised.** The 31.6 µs secp256k1 key expansion runs
  once when the execution client constructs its signer, not per order.
- **Dispatch runs against a fresh `WsDispatchState` each iteration.** The state
  is rebuilt in the `iter_batched` setup closure (excluded from timing), so the
  measured time is parse + dedup + identity lookup + `ExecutionEventEmitter`
  send. The channel send adds variance; these rows are noisier than the inbound
  and exec groups. Production state lives forever, so the steady-state dedup hit
  is ~12 ns (`atom/dedup_trade_hit`) rather than the per-iteration construct +
  drop (`atom/state_construct_primed` + `atom/state_drop_primed`).
