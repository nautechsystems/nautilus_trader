# OKX Adapter Benchmarks

Numbers measured 2026-05-19 on AMD Ryzen Threadripper 9980X under
rustc 1.95.0, `bench-lto` profile (release opts + `lto = "fat"` +
`codegen-units = 1`, `debug = full`), ASLR disabled via `setarch -R`,
default CPU governor.

Refresh on substantive perf change or before release; bump the date.
Absolute numbers vary by machine; only same-machine deltas are meaningful.

## How to reproduce

```bash
sudo cpupower frequency-set -g performance
setarch -R cargo bench -p nautilus-okx --profile bench-lto \
    --bench data --bench exec --bench micros --bench signing
sudo cpupower frequency-set -g powersave  # restore default
```

For policy and the general noise-reduction recipe see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

## Inbound pipeline (`data.rs`)

Raw WS frame bytes -> Nautilus domain type. Covers decode + parse + cache
lookup + Nautilus type construction. No I/O, no async runtime, no channel.

Rows ordered from the most fundamental market-data stream (book deltas) down
through derived streams (mark/index/funding/bars), then the private user
streams (live order / fill) at the end.

| Bench                           | Median  | Throughput |
|---------------------------------|---------|------------|
| `inbound_pipeline/book_deltas`  | 4.01 µs | 250 k/s    |
| `inbound_pipeline/book_depth10` | 4.21 µs | 237 k/s    |
| `inbound_pipeline/quotes`       | 852 ns  | 1.17 M/s   |
| `inbound_pipeline/trades`       | 686 ns  | 1.46 M/s   |
| `inbound_pipeline/mark_price`   | 504 ns  | 1.99 M/s   |
| `inbound_pipeline/index_price`  | 692 ns  | 1.45 M/s   |
| `inbound_pipeline/funding_rate` | 672 ns  | 1.49 M/s   |
| `inbound_pipeline/bars`         | 669 ns  | 1.49 M/s   |
| `inbound_pipeline/order_event`  | 4.87 µs | 205 k/s    |
| `inbound_pipeline/order_fill`   | 4.93 µs | 203 k/s    |

## Execution pipeline (`exec.rs`)

Strategy command (place/cancel/modify) -> wire bytes ready to send. Each
iteration both constructs the request struct and serializes it to JSON, so
the numbers reflect build + serialize together. OKX uses WebSocket for
live order ops with no per-message signature (auth is established once at
login); the per-request HMAC cost incurred by the HTTP path (instrument
fetch, algo orders) is in `signing.rs` below.

`submit_market`, `submit_limit`, and `submit_stop_market` emit the HTTP
order / order-algo request bodies (`OKXPlaceOrderRequest`,
`OKXPlaceAlgoOrderRequest`). `submit_ws_limit`, `cancel`, and `modify`
emit the production WS payload (`OKXWsRequest<WsPostOrderParams>`,
`WsCancelOrderParams`, `WsAmendOrderParams`).

| Bench                              | Median  | Throughput |
|------------------------------------|---------|------------|
| `exec_pipeline/submit_market`      | 145 ns  | 6.90 M/s   |
| `exec_pipeline/submit_limit`       | 152 ns  | 6.58 M/s   |
| `exec_pipeline/submit_stop_market` | 167 ns  | 5.99 M/s   |
| `exec_pipeline/submit_ws_limit`    | 208 ns  | 4.81 M/s   |
| `exec_pipeline/cancel`             | 69.1 ns | 14.5 M/s   |
| `exec_pipeline/modify`             | 107 ns  | 9.35 M/s   |

## HTTP signing (`signing.rs`)

HMAC-SHA256 over `(timestamp + method + path + body)`, base64-encoded.
Only the HTTP path signs; the WS exec path does not.

| Bench              | Median |
|--------------------|--------|
| `sign_get_no_body` | 266 ns |
| `sign_order`       | 339 ns |
| `sign_order_algo`  | 394 ns |

## Dispatch (`exec.rs`)

Venue execution report (`FillReport`, `OrderStatusReport`) forwarded via
`ExecutionEventEmitter`. Measures the untracked report-fallback path
through `dispatch_execution_reports`: trade-id dedup, dispatch-state
bookkeeping, and `send_*_report`. The tracked-order path
(`dispatch_ws_message` -> `dispatch_parsed_order_event` ->
`OrderAccepted`/`OrderFilled` event construction) is `pub(crate)` and not
exercised here; numbers below therefore exclude the per-event construction
cost the tracked path adds.

| Bench                      | Median  | Throughput |
|----------------------------|---------|------------|
| `dispatch/fill`            | 16.7 µs | 59.9 k/s   |
| `dispatch/status_accepted` | 11.9 µs | 84.1 k/s   |
| `dispatch/status_canceled` | 11.7 µs | 85.2 k/s   |
| `dispatch/status_filled`   | 15.9 µs | 62.8 k/s   |

## Component breakdown (`micros.rs`)

Diagnostic benches that decompose the pipeline numbers above. Use these to
localise where time goes when a pipeline bench regresses.

| Bench                        | Median  |
|------------------------------|---------|
| `decode_only/trade`          | 614 ns  |
| `decode_only/book`           | 3.26 µs |
| `parse_only/trade`           | 48.3 ns |
| `parse_only/book_deltas`     | 535 ns  |
| `atom/decimal_from_str`      | 8.08 ns |
| `atom/price_from_decimal_dp` | 7.30 ns |
| `atom/price_combined`        | 13.8 ns |
| `atom/trade_id_new`          | 9.94 ns |
| `atom/uuid4_new`             | 65.3 ns |
| `atom/instrument_lookup`     | 1.82 ns |
| `atom/book_order_construct`  | 1.51 ns |

## Notes

- **Inbound is JSON-decode dominated.** `decode_only/book` accounts for
  roughly 80% of `inbound_pipeline/book_deltas` (3.26 µs of 4.01 µs), and
  `decode_only/trade` accounts for roughly 90% of `inbound_pipeline/trades`
  (614 ns of 686 ns). Parsing itself is around 500 ns for a 10-level book
  delta and well under 100 ns for a trade tick.
- **`OKXWsFrame` still buffers through `serde_json::Value` once before
  variant dispatch.** Field extraction now takes ownership via
  `Map::remove(...)` instead of `.cloned()`, which removed the deep-clone
  per field. The remaining inbound headroom is the Value buffer itself;
  replacing it with a `serde::de::Visitor` or `RawValue` peek is the next
  lever and would also let `OKXBookMsg` levels deserialize without the
  intermediate `Vec<Value>` allocation.
- **Exec is allocation-bound, not signature-bound.** Build + JSON
  serialize lands in ~70-200 ns depending on shape, and OKX does not
  sign per-message on the WS path. HMAC-SHA256 (`signing.rs`) is
  HTTP-only.
- **Dispatch runs against a fresh empty `WsDispatchState` each iteration.**
  The state is built in the `iter_batched` setup closure (which Criterion
  excludes from timing), so the measured time is the
  `dispatch_execution_reports` body only. Production state lives forever
  and accumulates dedup entries; the steady-state cost on a reused state
  with a dedup hit is well below 100 ns.
