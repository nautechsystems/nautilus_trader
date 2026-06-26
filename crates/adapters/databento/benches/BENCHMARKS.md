# Databento Adapter Benchmarks

Numbers measured 2026-06-26 on AMD Ryzen Threadripper 9980X under
rustc 1.96.0, `bench-lto` profile (release opts + `lto = "fat"` +
`codegen-units = 1`, `debug = full`). ASLR is disabled via `setarch -R`
for the run. The CPU governor was `powersave` for this capture; absolute
numbers will improve under `performance`, but same-machine deltas remain
meaningful.

Refresh on substantive perf change or before release; bump the date.
Absolute numbers vary by machine; only same-machine deltas are meaningful.

## How to reproduce

```bash
sudo cpupower frequency-set -g performance
setarch -R cargo bench -p nautilus-databento --profile bench-lto \
    --bench data --bench micros
sudo cpupower frequency-set -g powersave
```

For policy and the general noise-reduction recipe see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

## DBN stream decode (`data.rs`)

Fixture file -> Databento typed record. Covers file open, zstd setup/decode,
and DBN decode. Stops before Nautilus instrument lookup, precision resolution,
and domain type construction.

| Bench                        | Median  | Throughput |
|------------------------------|---------|------------|
| `dbn_stream_decode/mbo`      | 2.94 µs | 681 k/s    |
| `dbn_stream_decode/mbp1`     | 2.91 µs | 686 k/s    |
| `dbn_stream_decode/mbp10`    | 4.05 µs | 494 k/s    |
| `dbn_stream_decode/trades`   | 2.59 µs | 772 k/s    |
| `dbn_stream_decode/ohlcv_1s` | 3.10 µs | 645 k/s    |
| `dbn_stream_decode/status`   | 2.98 µs | 1.34 M/s   |

## Historical loader (`data.rs`)

Fixture file -> Nautilus domain value through the public
`DatabentoDataLoader` API. Covers file open, zstd + DBN decode, instrument
resolution when needed, price precision resolution, Nautilus type construction,
and collection into the public return shape. No async runtime and no channel.

The benches use the same compressed fixtures as the Databento tests and seed
`ESM4.GLBX` with price precision `2`.

| Bench                           | Median   | Throughput |
|---------------------------------|----------|------------|
| `historical_loader/mbo_deltas`  | 3.39 µs  | 590 k/s    |
| `historical_loader/mbp1_quotes` | 3.21 µs  | 623 k/s    |
| `historical_loader/mbp10_depth` | 5.21 µs  | 384 k/s    |
| `historical_loader/bbo_quotes`  | 4.95 µs  | 808 k/s    |
| `historical_loader/cmbp_quotes` | 3.26 µs  | 614 k/s    |
| `historical_loader/cbbo_quotes` | 3.01 µs  | 665 k/s    |
| `historical_loader/tbbo_trades` | 4.44 µs  | 450 k/s    |
| `historical_loader/trades`      | 2.86 µs  | 698 k/s    |
| `historical_loader/bars`        | 2.89 µs  | 691 k/s    |
| `historical_loader/status`      | 2.93 µs  | 1.37 M/s   |
| `historical_loader/imbalance`   | 12.17 µs | 164 k/s    |
| `historical_loader/statistics`  | 2.94 µs  | 681 k/s    |

## Large MBO fixture diagnostics (`data.rs`)

The larger MBO diagnostics use
`tests/test_data/databento/esh4-glbx-mdp3-20231225.mbo.dbn.zst`, a committed
997 KB DBN fixture with 68,792 raw MBO records and 65,819 decoded deltas. They
exercise sustained decode and loader behavior without depending on local-only
data files.

| Bench                           | Median  | Throughput |
|---------------------------------|---------|------------|
| `large_mbo/dbn_stream_decode`   | 2.99 ms | 23.0 M/s   |
| `large_mbo/loader_collect`      | 6.12 ms | 10.8 M/s   |
| `large_mbo/loader_stream_count` | 5.89 ms | 11.2 M/s   |

## Component breakdown (`micros.rs`)

Diagnostic benches that decompose the pipeline numbers above. Use these to
localise where time goes when a loader bench regresses.

`record_decode` measures already-decoded Databento records converted into
Nautilus domain values.

| Bench                         | Median  |
|-------------------------------|---------|
| `record_decode/mbo_delta`     | 14.4 ns |
| `record_decode/mbo_trade`     | 27.3 ns |
| `record_decode/trade`         | 34.4 ns |
| `record_decode/mbp1_quote`    | 43.0 ns |
| `record_decode/mbp1_trade`    | 56.3 ns |
| `record_decode/mbp10_depth`   | 212 ns  |
| `record_decode/bbo_quote`     | 28.0 ns |
| `record_decode/cmbp_quote`    | 43.2 ns |
| `record_decode/cmbp_trade`    | 99.7 ns |
| `record_decode/tbbo`          | 52.8 ns |
| `record_decode/ohlcv`         | 19.0 ns |
| `record_decode/status`        | 12.4 ns |
| `record_decode/imbalance`     | 17.0 ns |
| `record_decode/statistics`    | 6.00 ns |

`record_dispatch` measures the generic `RecordRef` branch chain used by the
loader and live feed handler.

| Bench                         | Median  |
|-------------------------------|---------|
| `record_dispatch/trade`       | 41.3 ns |
| `record_dispatch/mbp10_depth` | 245 ns  |
| `record_dispatch/ohlcv`       | 31.6 ns |

`atom` isolates primitive price, quantity, precision, record-header, and
trade-ID construction costs.

| Bench                         | Median  |
|-------------------------------|---------|
| `atom/decode_price_or_undef`  | 420 ps  |
| `atom/decode_price_increment` | 7.58 ns |
| `atom/decode_quantity`        | 6.49 ns |
| `atom/precision_from_raw`     | 1.25 ns |
| `atom/trade_id_from_sequence` | 13.3 ns |
| `atom/record_header_ref`      | 200 ps  |

## Notes

- File-backed benches include open and zstd setup costs because those costs are
  part of historical loader usage. The fixtures are tiny, so these rows are
  regression baselines for the public loader API rather than sustained
  streaming throughput claims.
- Direct record decode is not the historical-loader bottleneck for most
  schemas. File open, zstd setup/decode, DBN stream iteration, and collection
  dominate the µs-level loader rows.
- MBP10 direct decode is the largest pure Nautilus conversion row because it
  constructs 10 bid orders, 10 ask orders, and both count arrays.
- CMBP trade rows include deterministic trade-ID derivation because CMBP/TCBBO
  schemas do not publish native trade IDs. The derivation hashes the instrument
  id, timestamps, price, size, and side without allocating an intermediate
  `InstrumentId` string, then formats the hash through a fixed hex buffer.
- `historical_loader/imbalance` is materially slower than its direct decode
  row. If imbalance ingestion matters for a production workload, profile the
  stream path before changing domain construction.
