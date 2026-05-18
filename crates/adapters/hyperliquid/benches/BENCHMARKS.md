# Hyperliquid Adapter Benchmarks

Numbers measured 2026-05-18 on AMD Ryzen Threadripper 9980X under
rustc 1.95.0, `bench-lto` profile (release opts + `lto = "fat"` +
`codegen-units = 1`, `debug = full`). The CPU governor is pinned to
`performance` and ASLR is disabled via `setarch -R` for the run.

Refresh on substantive perf change or before release; bump the date.
Absolute numbers vary by machine; only same-machine deltas are meaningful.

## How to reproduce

```bash
sudo cpupower frequency-set -g performance
setarch -R cargo bench -p nautilus-hyperliquid --profile bench-lto \
    --bench data --bench exec --bench micros
sudo cpupower frequency-set -g powersave  # restore default
```

For policy and the general noise-reduction recipe see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

## Inbound pipeline (`data.rs`)

Raw WS frame bytes -> Nautilus domain type. Covers decode + parse + cache
lookup + Nautilus type construction. No I/O, no async runtime, no channel.

Rows ordered from the most fundamental market-data stream (book deltas) down
through derived streams (mark/index/funding/bars), then the private user
streams (fills, order updates) at the end.

| Bench                           | Median  | Throughput |
|---------------------------------|---------|------------|
| `inbound_pipeline/book_deltas`  | 3.95 µs | 253 k/s    |
| `inbound_pipeline/book_depth10` | 3.98 µs | 251 k/s    |
| `inbound_pipeline/quotes`       | 557 ns  | 1.79 M/s   |
| `inbound_pipeline/trades`       | 618 ns  | 1.62 M/s   |
| `inbound_pipeline/mark_price`   | 886 ns  | 1.13 M/s   |
| `inbound_pipeline/index_price`  | 896 ns  | 1.12 M/s   |
| `inbound_pipeline/funding_rate` | 897 ns  | 1.11 M/s   |
| `inbound_pipeline/bars`         | 652 ns  | 1.53 M/s   |
| `inbound_pipeline/order_event`  | 830 ns  | 1.20 M/s   |
| `inbound_pipeline/order_fill`   | 1.11 µs | 899 k/s    |

## Execution pipeline (`exec.rs`)

Strategy command (`OrderAny` / cancel / modify) -> fully signed wire bytes
ready to POST. Covers normalize + serialize (msgpack) + EIP-712 sign.

| Bench                              | Median  | Throughput |
|------------------------------------|---------|------------|
| `exec_pipeline/submit_market`      | 42.2 µs | 23.7 k/s   |
| `exec_pipeline/submit_limit`       | 42.1 µs | 23.7 k/s   |
| `exec_pipeline/submit_stop_market` | 42.5 µs | 23.5 k/s   |
| `exec_pipeline/cancel`             | 47.9 µs | 20.9 k/s   |
| `exec_pipeline/modify`             | 42.2 µs | 23.7 k/s   |

## Dispatch (`exec.rs`)

Venue report (`FillReport`, `OrderStatusReport`) -> events emitted via
`ExecutionEventEmitter`. Covers dedup + identity lookup + event construction.

Note: these numbers include per-iteration `WsDispatchState` construction +
drop, which is a bench-only artifact. In production, state lives forever and
the dispatch-only cost is much smaller (see `atom/dispatch_fill_reused` in
the component breakdown below).

| Bench                      | Median  | Throughput |
|----------------------------|---------|------------|
| `dispatch/fill`            | 15.6 µs | 64.2 k/s   |
| `dispatch/status_accepted` | 11.1 µs | 90.5 k/s   |
| `dispatch/status_canceled` | 15.4 µs | 64.8 k/s   |
| `dispatch/status_modified` | 12.3 µs | 81.1 k/s   |

## Component breakdown (`micros.rs`)

Diagnostic benches that decompose the pipeline numbers above. Use these to
localise where time goes when a pipeline bench regresses.

| Bench                           | Median  |
|---------------------------------|---------|
| `decode_only/trade`             | 549 ns  |
| `decode_only/book`              | 3.25 µs |
| `parse_only/trade`              | 59.0 ns |
| `parse_only/book_deltas`        | 678 ns  |
| `atom/decimal_from_str`         | 7.04 ns |
| `atom/price_from_decimal_dp`    | 6.38 ns |
| `atom/price_combined`           | 12.1 ns |
| `atom/trade_id_new`             | 17.9 ns |
| `atom/uuid4_new`                | 58.7 ns |
| `atom/state_construct_primed`   | 7.39 µs |
| `atom/state_drop_primed`        | 1.48 µs |
| `atom/event_filled_construct`   | 151 ns  |
| `atom/event_accepted_construct` | 148 ns  |
| `atom/dispatch_fill_reused`     | 12.6 ns |

## Notes

- **Inbound is JSON-decode dominated.** `decode_only` accounts for roughly
  80-90% of the inbound pipeline cost across every message kind. Decimal,
  Price, Quantity, UUID4, and TradeId construction are all sub-100 ns and
  not meaningful in the absolute pipeline number.
- **Exec is signature-bound.** EIP-712 + keccak + secp256k1 dominates, and
  `lto = "fat"` collapses the per-variant differences so submit and modify
  converge at ~42 µs. Cancel sits at ~48 µs because the cancel action
  serialises a different msgpack shape. Optimisations that don't change
  the signing scheme won't move these numbers.
- **Dispatch in production is faster than the bench suggests.** The
  canonical bench rebuilds state per iteration; the steady-state cost on a
  reused state is ~13 ns on a dedup hit, and a first-time fill on a fresh
  state is ~7 µs (`dispatch/fill` minus `state_construct_primed` +
  `state_drop_primed`).
- **simd-json was piloted and reverted.** A `simd-json` feature flag plus
  decode helper was prototyped, run side-by-side against `serde_json`, and
  found to be 20-50% **slower** on hyperliquid payload sizes. The mutable-
  buffer requirement forces a per-call `to_vec()`, payloads are too small
  to amortise SIMD setup, and owned-`String` deserialization negates the
  borrow advantage. Re-evaluate only if payloads grow materially or a
  zero-copy borrowed-string path lands.
