# CoinAPI Adapter Benchmarks

Numbers measured 2026-09-29 on AMD Ryzen 9 7950X (Linux 6.8.0) under rustc 1.98.1, `bench-lto`
profile (release opts + `lto = "fat"` + `codegen-units = 1`, `debug = full`), ASLR disabled via
`setarch -R`, pinned to one core (`taskset -c 30`). The core ran the `powersave` governor, its SMT
sibling was not isolated, and the host ran other workloads during the run. Each median is the lower
of two full sessions, which agreed within 1%.

Refresh on substantive perf change or before release; bump the date.
Absolute numbers vary by machine; only same-machine deltas are meaningful.

## How to reproduce

The recorded run kept the `powersave` governor. For lower noise, set `performance` for the run and
restore the default afterward:

```bash
sudo cpupower frequency-set -g performance
setarch -R taskset -c 30 cargo bench -p nautilus-coinapi --profile bench-lto \
    --bench data --bench micros
sudo cpupower frequency-set -g powersave  # restore default
```

For policy and the general noise-reduction recipe see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

## Inbound pipeline (`data.rs`)

Raw WS text frame bytes -> Nautilus domain type. Covers `Frame::decode`, the instrument lookup by
`symbol_id`, and conversion. The `book` and `book_l3` rows also run `BookSyncTracker::apply` on a
synced book: sequence validation, L3 order tracking, and the emitted sequence offset. `book20`
includes the depth stream's emitted sequence. No I/O, no async runtime, no channel.

`book` streams the `ws_book.json` capture as the update after its own snapshot. `book_l3` uses the
four entries of `ws_book_l3.json` (`ADD`, `DELETE`, `UPDATE`, `MATCH`) against an inline snapshot
of the resting orders. Its `MATCH` carries an `id`, which the capture omits, and a partial size of
`0.5` instead of `4.5`, so it emits an `Update` rather than a `Delete`. `ohlcv` excludes the data
client's period-close cache.

| Bench                      | Median  | Throughput |
| -------------------------- | ------- | ---------- |
| `inbound_pipeline/book`    | 1.25 µs | 802 k/s    |
| `inbound_pipeline/book_l3` | 2.09 µs | 478 k/s    |
| `inbound_pipeline/book20`  | 1.06 µs | 947 k/s    |
| `inbound_pipeline/quote`   | 705 ns  | 1.42 M/s   |
| `inbound_pipeline/trade`   | 683 ns  | 1.46 M/s   |
| `inbound_pipeline/ohlcv`   | 1.12 µs | 890 k/s    |

## Component breakdown (`micros.rs`)

Decoding is 69-82% of each pipeline measured here, so these benches split it. `decode_only` runs
`Frame::decode`; `decode_payload` decodes the message struct alone. Their difference is the
first pass that reads `type`, which scans the whole frame because CoinAPI sends `type` last:
180-280 ns per message and 560 ns for `book_l3`. The `atom` rows price one exact decimal token, one
timestamp, and the instrument lookup, which clones the instrument.

| Bench                    | Median  |
| ------------------------ | ------- |
| `decode_only/trade`      | 557 ns  |
| `decode_only/quote`      | 569 ns  |
| `decode_only/ohlcv`      | 883 ns  |
| `decode_only/book20`     | 727 ns  |
| `decode_only/book_l3`    | 1.65 µs |
| `decode_payload/trade`   | 358 ns  |
| `decode_payload/quote`   | 387 ns  |
| `decode_payload/ohlcv`   | 603 ns  |
| `decode_payload/book20`  | 504 ns  |
| `decode_payload/book_l3` | 1.09 µs |
| `atom/decimal_json`      | 35.5 ns |
| `atom/timestamp`         | 29.0 ns |
| `atom/instrument_lookup` | 84.7 ns |

## Not benchmarked

- WebSocket routing in `ConnectionHandler::route` checks each data message against the
  subscriptions and formats its topic. It is private to the client, so no bench reaches it.
- REST parsing runs once per instrument load or history request, not per streamed message. The
  fuzz targets cover it.
