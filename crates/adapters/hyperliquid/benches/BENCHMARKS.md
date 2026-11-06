# Hyperliquid Adapter Benchmarks

Measured 2026-10-19 on AMD Ryzen Threadripper 9980X, Linux 7.0.0-34-generic,
and rustc 1.99.0 (b940084d7 2026-09-28). All four targets use default features, including
`high-precision`, and the `bench-lto` profile: release options, `lto = "fat"`, `codegen-units = 1`,
and full debug symbols.

Each benchmark process runs with the `performance` governor and ASLR disabled
for that process via `setarch -R`. SMT and CPU boost remain enabled. Normal desktop services remain
active. Published runs exclude concurrent builds, full test suites, and profiling. Runs that overlap a build
are discarded and repeated.

Each case uses 100 Criterion samples, a 3-second warmup, and a 5-second measurement period.
The tables show the current source's median of three session mean estimates. Target order rotates
across repeats. A comparison against base revision `758191655d` uses the same corrected harness and
identical invocation settings. Throughput is the reciprocal of the published latency.

The largest session spread outside fresh-state dispatch is 6.0% in `atom/state_drop_primed`,
where spread is `(maximum - minimum) / median` across the three means. Dispatch shows all three
session means because its variability makes small-regression comparisons unreliable.

Refresh on substantive performance changes or before release; update the date.
Absolute numbers vary with the machine, toolchain, and source revision. Measure code states
back-to-back under identical controls to attribute a change to a patch.

## How to reproduce

Build all targets before measurement:

```bash
CARGO_BUILD_JOBS=8 cargo bench --locked -p nautilus-hyperliquid --profile bench-lto \
    --bench data --bench exec --bench micros --bench signing --no-run
```

This host uses `powersave` before measurement. Record the original governors on another host and
restore those values afterward. Run three full sessions, use a separate `CRITERION_HOME` for each,
and rotate the target order between sessions:

```bash
sudo cpupower frequency-set -g performance
CRITERION_HOME="$PWD/target/criterion-hyperliquid-1" \
    CARGO_BUILD_JOBS=8 setarch "$(uname -m)" -R \
    cargo bench --locked -p nautilus-hyperliquid --profile bench-lto \
    --bench data --bench exec --bench micros --bench signing -- \
    --sample-size 100 --warm-up-time 3 --measurement-time 5 --noplot
sudo cpupower frequency-set -g powersave  # Restore this host's original governor
```

For policy and the general noise-reduction recipe, see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

## Inbound pipeline (`data.rs`)

Raw WS frame bytes -> Nautilus domain type. The cases call the production `decode_ws_message`
entry point and the parsing functions used by the WebSocket handler, including cache lookup and domain
construction. Each iteration also drops its decoded frame and output. No I/O, async runtime, or channel
is included.

The mark, index, and funding cases decode the same asset-context frame and call the same parser;
each passes a different member of the returned tuple to `black_box`.

Rows start with book deltas and depth, followed by quotes, trades, derived streams, and private user streams.

| Bench                           | Median  | Throughput |
| ------------------------------- | ------- | ---------- |
| `inbound_pipeline/book_deltas`  | 5.33 µs | 188 k/s    |
| `inbound_pipeline/book_depth`   | 5.58 µs | 179 k/s    |
| `inbound_pipeline/quotes`       | 841 ns  | 1.19 M/s   |
| `inbound_pipeline/trades`       | 945 ns  | 1.06 M/s   |
| `inbound_pipeline/mark_price`   | 1.37 µs | 728 k/s    |
| `inbound_pipeline/index_price`  | 1.38 µs | 725 k/s    |
| `inbound_pipeline/funding_rate` | 1.35 µs | 742 k/s    |
| `inbound_pipeline/bars`         | 921 ns  | 1.09 M/s   |
| `inbound_pipeline/order_event`  | 1.2 µs  | 832 k/s    |
| `inbound_pipeline/order_fill`   | 1.71 µs | 585 k/s    |

## Execution pipeline (`exec.rs`)

Strategy command (`OrderAny` / cancel / modify) -> MessagePack action bytes plus EIP-712 signing.
Covers normalization, action serialization, an action-byte clone, and signing. The harness discards
the signature and returns the action bytes; it excludes HTTP request-body construction and I/O.

| Bench                              | Median  | Throughput |
| ---------------------------------- | ------- | ---------- |
| `exec_pipeline/submit_market`      | 47.7 µs | 21 k/s     |
| `exec_pipeline/submit_limit`       | 47.6 µs | 21 k/s     |
| `exec_pipeline/submit_stop_market` | 48.1 µs | 20.8 k/s   |
| `exec_pipeline/cancel`             | 47.2 µs | 21.2 k/s   |
| `exec_pipeline/modify`             | 47.2 µs | 21.2 k/s   |

## Signing (`signing.rs`)

Direct signer and serialization benches isolate L1 signing from order normalization and POST body
construction. `msgpack_serialize_action` produces named MessagePack bytes with `rmp_serde::to_vec_named`.
`json_serialize_action` builds a `serde_json::Value` with `serde_json::to_value`; it does not encode JSON text.

| Bench                       | Median  |
| --------------------------- | ------- |
| `sign_l1_action`            | 48.1 µs |
| `sign_l1_action_with_vault` | 48 µs   |
| `signer_construction`       | 35.6 µs |
| `msgpack_serialize_action`  | 186 ns  |
| `json_serialize_action`     | 512 ns  |

## Dispatch (`exec.rs`)

Venue report (`FillReport`, `OrderStatusReport`) -> events emitted via
`ExecutionEventEmitter`. Covers deduplication, context lookup, and event construction.

Criterion's `iter_batched` excludes state construction and receiver draining from the timed region.
The routine consumes and drops each fresh `WsDispatchState` inside the timed region, so these rows
include state destruction. Production dispatch reuses state.

These rows vary substantially across sessions. All three session means appear below in session order;
do not use these results to judge small regressions.

| Bench                      | Median  | Throughput | Session means (µs)  |
| -------------------------- | ------- | ---------- | ------------------- |
| `dispatch/fill`            | 1.3 µs  | 767 k/s    | 2.57, 1.28, 1.3     |
| `dispatch/status_accepted` | 701 ns  | 1.43 M/s   | 1.02, 0.674, 0.701  |
| `dispatch/status_canceled` | 1.13 µs | 882 k/s    | 0.967, 1.13, 1.21   |
| `dispatch/status_modified` | 882 ns  | 1.13 M/s   | 0.882, 0.854, 0.913 |

## Component breakdown (`micros.rs`)

Diagnostic benches that decompose the pipeline numbers above. Use these to
localize where time goes when a pipeline bench regresses.

`state_construct_primed` includes construction and destruction; `state_drop_primed` times destruction
with construction outside the timed region. `dispatch_fill_reused` repeatedly dispatches the same
fill against one state, so its measured steady state is duplicate-fill rejection.

| Bench                             | Median  |
| --------------------------------- | ------- |
| `decode_only/trade`               | 870 ns  |
| `decode_only/book`                | 4.62 µs |
| `parse_only/trade`                | 49 ns   |
| `parse_only/book_deltas`          | 476 ns  |
| `atom/decimal_from_str`           | 7.84 ns |
| `atom/price_from_decimal_dp`      | 7.15 ns |
| `atom/price_combined`             | 14.1 ns |
| `atom/trade_id_new`               | 20.2 ns |
| `atom/cloid_from_client_order_id` | 232 ns  |
| `atom/cloid_to_hex`               | 11.4 ns |
| `atom/uuid4_new`                  | 14.2 ns |
| `atom/data_converter_ws_snapshot` | 430 ns  |
| `atom/state_construct_primed`     | 1.41 µs |
| `atom/state_drop_primed`          | 254 ns  |
| `atom/event_filled_construct`     | 156 ns  |
| `atom/event_accepted_construct`   | 143 ns  |
| `atom/dispatch_fill_reused`       | 25.8 ns |

## Notes

- **Inbound JSON decoding dominates the measured trade and book cases.** The decode-only latency is
  about 92% of the trade pipeline latency and 87% of the book-delta pipeline latency.
  These separate component runs indicate where time goes; they do not decompose every message kind.
  The measured Decimal, Price, UUID4, and TradeId construction cases each take less than 100 ns.
- **Execution spends most of its time signing.** Compare the direct L1 signing rows with the execution
  pipeline rows. Normalization and MessagePack encoding add relatively little for these single-order actions.
- **Reused-state dispatch measures deduplication.** A duplicate-fill result does not establish fresh-fill
  dispatch latency. Do not subtract independently measured state costs from the pipeline rows to infer it.
- **A historical simd-json pilot was reverted.** A `simd-json` feature flag and decoder were prototyped
  and measured side-by-side against `serde_json`. The prototype was [20%, 50%] slower on Hyperliquid
  payload sizes. The mutable-buffer requirement forces a per-call `to_vec()`, payloads are too small to amortize
  SIMD setup, and owned-`String` deserialization negates the borrowing advantage. This refresh does not
  repeat that experiment. Re-evaluate if payloads grow materially or a zero-copy borrowed-string path lands.
