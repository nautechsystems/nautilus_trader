# Native Codec Benchmark

This benchmark compares JSON and named-field MessagePack for complete native messages.
It checks field parity before measuring encode, decode, and normal MessageBus external publication and
republishing through a bounded in-memory test backing. It measures local costs; it does not measure a
network transport or a separate receiver process.

From the repository root, build with the benchmark profile and the Python interpreter for your environment:

```bash
PYO3_PYTHON="$PWD/python/.venv/bin/python" CARGO_BUILD_JOBS=8 \
  cargo test --locked --profile bench-lto -p nautilus-common \
  --features sbe,high-precision --lib --no-run
```

Replace `<hash>` below with the hash in the emitted executable path. Run it directly to keep Cargo
outside the timed run:

```bash
NAUTILUS_BENCH_SAMPLES=100000 NAUTILUS_BENCH_TRIALS=3 \
  "target/bench-lto/deps/nautilus_common-<hash>" --exact \
  msgbus::external::codec::benchmarks::native_codec_measurements \
  --ignored --nocapture --test-threads=1
```

Use the controls in the [benchmarking guide](../../../../docs/developer_guide/benchmarking.md).
Record CPU model, kernel or OS, affinity, governor, ASLR, host load, toolchain, profile, and exact source
state. On Linux, prefix the run command with `setarch "$(uname -m)" -R` to disable process ASLR. Use results
from an uncontrolled workstation only for local comparisons.

The benchmark warms each case for 1,000 iterations and rotates format order across trials. Cases cover
submit, modify, and cancel commands; account queries; accepted, updated, and filled orders; order status
reports; join requests; historical trade replies; instrument and catalog replies; custom batches; and account state.
The output consists of JSON lines with wire size, sample count, mean, p50, p95, p99, p99.9, and maximum
latency. Compare all trials and retain their variation when reporting results.

| Operation                              | Measured work                          | Queue policy                                                  |
| -------------------------------------- | -------------------------------------- | ------------------------------------------------------------- |
| `encode` / `decode`                    | Native codec call                      | None                                                          |
| `codec_and_local_publish`              | Encode, decode, and local callback     | None                                                          |
| `codec_and_local_publish_instrumented` | Same path with encode/decode fractions | None                                                          |
| `steady`                               | External egress through republish      | Drain after each publication                                  |
| `burst`                                | Same path in bursts of 256             | Capacity 128; drain one when full, drain all after each burst |
| `overload`                             | Same path in bursts of 256             | Capacity 128; drop when full, drain after each burst          |

Delivery rows report delivered and dropped counts, backpressure events, peak queue size, and delivered
throughput. Their latency includes queue residence from the start of publication through the receiver
callback. The test backing implements the stated queue policies; these are not transport guarantees.
Delivery rows use `bytes: 0`; read the payload wire size from the corresponding `encode` row. Allocation
costs are included in latency. Collect allocation counts separately with an allocation profiler.

Set `NAUTILUS_BENCH_CASE` to one case name and `NAUTILUS_BENCH_FORMAT` to `json` or `msgpack`
to select a workload. Data cases are `request_join`, `historical_trades`, `instrument_reply`,
`catalog_reply`, and `custom_batch`. Execution cases are listed below.

For allocation profiles, set `NAUTILUS_BENCH_PROFILE` to `encode` or `decode`. This mode skips timing and
delivery measurements and emits a `profile_calls` row for each selected case.
The non-inlined functions of those names delimit measured calls; warmup calls bypass those boundaries.
All fixtures use warm interned identifiers. Measure cold-process setup separately when that cost matters.

## Execution decoding

Set `NAUTILUS_BENCH_PROFILE=decode_latency` to time only deserialization into an owned native message.
Encoding, fixture setup, and destruction of decoded values remain outside the timed region. The output
includes mean, percentiles, and wire size. Each format checks complete field parity before timing and
warms both codecs for 1,000 calls. Timestamps use nanosecond values with the same digit width as current
Unix timestamps; identifiers are warm and interned. SBE has no execution-message codec in this project.

```bash
NAUTILUS_BENCH_PROFILE=decode_latency NAUTILUS_BENCH_CASE=order_filled \
NAUTILUS_BENCH_SAMPLES=100000 NAUTILUS_BENCH_TRIALS=3 \
  "target/bench-lto/deps/nautilus_common-<hash>" --exact \
  msgbus::external::codec::benchmarks::native_codec_measurements \
  --ignored --nocapture --test-threads=1
```

Execution cases cover:

- Commands: `submit_order` (market), `submit_order_limit`, `submit_order_list`, `modify_order`,
  `modify_orders`, `cancel_order`, `cancel_orders`, `cancel_all_orders`, `query_order`, and `query_account`.
  List, batch-modify, and batch-cancel commands contain two orders inside one message.
  The parameter-free cases `submit_order_plain`, `submit_order_limit_plain`, `modify_order_plain`, and
  `cancel_order_plain` model ordinary messages. The corresponding cases without `_plain` include
  scalar or nested parameters; nested cases use the shared numeric-boundary and structured-value fixture.
- Order events: `order_initialized`, `order_denied`, `order_emulated`, `order_released`, `order_submitted`,
  `order_accepted`, `order_rejected`, `order_canceled`, `order_expired`, `order_triggered`,
  `order_pending_update`, `order_pending_cancel`, `order_modify_rejected`, `order_cancel_rejected`,
  `order_updated`, `order_filled`, and `order_fill_voided`.
- Position events: `position_opened`, `position_changed`, `position_closed`, and `position_adjusted`.
- Reports and state: `order_status_report`, `fill_report`, `position_status_report`, `generate_mass_status`,
  and `account_state`. `account_state_plain` omits the structured `info` map.

Append `_concrete` to `submit_order`, `cancel_order`, `modify_order`, or one of the four `_plain` command cases
to compare JSON and MessagePack decoding with the concrete command type already known. Their bytes are
identical to the corresponding `TradingCommand` payloads. These diagnostic cases bypass variant dispatch;
they do not represent the current generic MessageBus ingress path.
