# OKX Adapter Benchmarks

Numbers measured 2026-09-25 on AMD Ryzen Threadripper 9980X under
rustc 1.98.1, `bench-lto` profile (release opts + `lto = "fat"` +
`codegen-units = 1`, `debug = full`), ASLR disabled via `setarch -R`,
pinned to one core (`taskset -c 60`) with the `performance` governor on
that core and its SMT sibling. The host ran other workloads during the run.

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
| ------------------------------- | ------- | ---------- |
| `inbound_pipeline/book_deltas`  | 3.34 µs | 300 k/s    |
| `inbound_pipeline/book_depth`   | 3.65 µs | 274 k/s    |
| `inbound_pipeline/quotes`       | 849 ns  | 1.18 M/s   |
| `inbound_pipeline/trades`       | 688 ns  | 1.45 M/s   |
| `inbound_pipeline/mark_price`   | 481 ns  | 2.08 M/s   |
| `inbound_pipeline/index_price`  | 687 ns  | 1.46 M/s   |
| `inbound_pipeline/funding_rate` | 680 ns  | 1.47 M/s   |
| `inbound_pipeline/bars`         | 690 ns  | 1.45 M/s   |
| `inbound_pipeline/order_event`  | 5.21 µs | 192 k/s    |
| `inbound_pipeline/order_fill`   | 5.34 µs | 187 k/s    |

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
| ---------------------------------- | ------- | ---------- |
| `exec_pipeline/submit_market`      | 155 ns  | 6.46 M/s   |
| `exec_pipeline/submit_limit`       | 188 ns  | 5.31 M/s   |
| `exec_pipeline/submit_stop_market` | 201 ns  | 4.98 M/s   |
| `exec_pipeline/submit_ws_limit`    | 213 ns  | 4.69 M/s   |
| `exec_pipeline/cancel`             | 78.4 ns | 12.8 M/s   |
| `exec_pipeline/modify`             | 123 ns  | 8.10 M/s   |

## HTTP signing (`signing.rs`)

HMAC-SHA256 over `(timestamp + method + path + body)`, base64-encoded.
Only the HTTP path signs; the WS exec path does not.

| Bench              | Median |
| ------------------ | ------ |
| `sign_get_no_body` | 303 ns |
| `sign_order`       | 382 ns |
| `sign_order_algo`  | 420 ns |

## Report dispatch (`exec.rs`)

Venue execution report (`FillReport`, `OrderStatusReport`) forwarded via
`ExecutionEventEmitter`. Measures the untracked report-fallback path
through `dispatch_execution_reports`: trade-id dedup, dispatch-state
bookkeeping, and `send_*_report`.

| Bench                      | Median | Throughput |
| -------------------------- | ------ | ---------- |
| `dispatch/fill`            | 161 ns | 6.23 M/s   |
| `dispatch/status_accepted` | 125 ns | 8.01 M/s   |
| `dispatch/status_canceled` | 110 ns | 9.08 M/s   |
| `dispatch/status_filled`   | 129 ns | 7.74 M/s   |

## WebSocket dispatch (`exec.rs`)

Decoded private-stream message dispatched through `dispatch_ws_message`,
the execution client's entry point for its private WebSocket.
`order_accepted` and `order_filled` cover the tracked-order path: identity
lookup, order-event parse, dedup bookkeeping, and event construction. A
fill that arrives without a prior live update also synthesizes
`OrderAccepted`, so `order_filled` emits two events. `account` covers an
account-channel update with three balances through `parse_account_state`.

| Bench                        | Median  | Throughput |
| ---------------------------- | ------- | ---------- |
| `dispatch_ws/order_accepted` | 941 ns  | 1.06 M/s   |
| `dispatch_ws/order_filled`   | 1.59 µs | 629 k/s    |
| `dispatch_ws/account`        | 6.98 µs | 143 k/s    |

## Component breakdown (`micros.rs`)

Diagnostic benches that decompose the pipeline numbers above. Use these to
localize where time goes when a pipeline bench regresses.

| Bench                        | Median  |
| ---------------------------- | ------- |
| `decode_only/trade`          | 532 ns  |
| `decode_only/book`           | 2.19 µs |
| `parse_only/trade`           | 50.5 ns |
| `parse_only/book_deltas`     | 555 ns  |
| `atom/decimal_from_str`      | 8.16 ns |
| `atom/price_from_decimal_dp` | 7.24 ns |
| `atom/price_combined`        | 14.0 ns |
| `atom/trade_id_new`          | 9.82 ns |
| `atom/uuid4_new`             | 13.0 ns |
| `atom/instrument_lookup`     | 2.01 ns |
| `atom/book_order_construct`  | 1.51 ns |
