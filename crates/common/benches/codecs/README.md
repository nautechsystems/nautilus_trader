# Native Codec Benchmark

This benchmark compares JSON and named-field MessagePack for complete native messages.
It checks field parity before measuring encode, decode, and normal MessageBus external publication and
republishing through a bounded in-memory test backing. It measures local costs; it does not measure a
network transport or a separate receiver process. Separate Redis transport and Python callback benchmarks
are described below.

From the repository root, build with the benchmark profile and the Python interpreter for your environment:

```bash
PYO3_PYTHON="$PWD/python/.venv/bin/python" CARGO_BUILD_JOBS=8 \
  cargo test --locked --profile bench-lto -p nautilus-common \
  --features sbe,high-precision --lib --no-run
```

Replace `<hash>` below with the hash in the emitted executable path. Run it directly to keep Cargo
outside the timed run:

```bash
NAUTILUS_BENCH_SAMPLES=2000 NAUTILUS_BENCH_TRIALS=3 \
  "target/bench-lto/deps/nautilus_common-<hash>" --exact \
  msgbus::external::codec::benchmarks::native_codec_measurements \
  --ignored --nocapture --test-threads=1
```

Use the controls in the [benchmarking guide](../../../../docs/developer_guide/benchmarking.md).
Record CPU model, kernel or OS, affinity, governor, ASLR, host load, toolchain, profile, and exact source
state. On Linux, prefix the run command with `setarch "$(uname -m)" -R` to disable process ASLR. Use results
from an uncontrolled workstation only for local comparisons.

The benchmark warms each case for 1,000 iterations and rotates format order across trials. Set
`NAUTILUS_BENCH_WARMUP` to change the warmup count. The coverage matrix below lists the fixture families.
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
to select a workload. Named data cases include `request_join`, `historical_trades`, `instrument_reply`,
`catalog_reply`, and `custom_batch`. Read exact case names from the output's `case` field.
Additional case families are:

| Family              | Case pattern                                                |
| ------------------- | ----------------------------------------------------------- |
| Requests            | `request_<Variant>_populated<bool>`                         |
| Subscriptions       | `subscribe_<kind>_populated<bool>`                          |
| Unsubscriptions     | `unsubscribe_<kind>_populated<bool>`                        |
| Responses           | `response_<kind>_mode<mode>_<count>`                        |
| Instrument variants | `response_instrument_<variant>_mode<mode>`                  |
| Market data         | `market_*`                                                  |
| Generic diagnostics | `generic_data_*`, `generic_report_order_with_fills_<count>` |

Generic diagnostics and `_concrete` cases require `NAUTILUS_BENCH_PROFILE` set to `codec_latency`,
`decode_latency`, `encode`, or `decode`. Other profiles omit these cases. Execution cases are listed below.

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

- Commands: `submit_order` (market), `submit_order_limit`, `submit_order_list`, `submit_order_algorithm`,
  `submit_order_list_algorithm`, `modify_order`,
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
  `execution_mass_status_<count>`, `portfolio_snapshot`, and `account_state`.
  `account_state_plain` omits the structured `info` map.

Append `_concrete` to a command case to compare JSON and MessagePack decoding with the concrete command
type already known. Every `TradingCommand` variant has a matching concrete case. Their bytes are
identical to the corresponding `TradingCommand` payloads. These diagnostic cases bypass variant dispatch;
they do not represent the current generic MessageBus ingress path.

## Coverage and measurement boundaries

`NAUTILUS_BENCH_PROFILE=codec_latency` measures encode and decode without local publication.
`NAUTILUS_BENCH_PROFILE=processor_latency` measures typed ingress through native decoding, JSON mapping,
the callback, and local publication. Each case checks complete serialized fields before timing.

| Runtime path                          | Fixtures                                                                               | Measurement and correctness check                                        | Optimization disposition                             |
| ------------------------------------- | -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ | ---------------------------------------------------- |
| Core named-field MessagePack          | Quote, trade, and bar singles; native messages                                         | Full-field parity in this corpus; `serialization_comparison` timing only | Borrowed strings and numeric parsing                 |
| Core UUID and trade identifiers       | UUIDs in execution/position events; `StackStr` through `TradeId`                       | Full-field parity and native allocation profiles                         | Retained string deserialization                      |
| Generic and concrete command dispatch | All nine commands; plain, nested, limit, and algorithmic submissions                   | Identical generic/concrete bytes; complete decoded fields                | Retained `Value` dispatch; shared parsing            |
| Execution events and state            | All 17 order events, four position events, reports, account, and portfolio state       | Codec, processor, and local publication where supported                  | Retained event layouts; shared parsing               |
| Mass execution status                 | Empty, 1, 32, and 256 reports                                                          | Codec and supported bus stages; complete report fields                   | Retained report layouts; shared parsing              |
| Data requests and subscriptions       | All 12 requests, 16 subscriptions, and 16 unsubscriptions; plain and nested options    | Full-field parity; option-chain strike range variants                    | Retained dispatch; shared identifiers and enums      |
| Native data responses                 | Every response variant, all 18 instrument variants, and absent/nested/empty Params     | Codec and supported bus stages; book reconstruction                      | Retained response layouts and book reconstruction    |
| Custom data responses                 | Scalar, batch, byte vector, `Bytes`, and text                                          | Full envelope, data type, identifier, metadata, and payload parity       | Borrowed response views; retained canonical envelope |
| Market data                           | Deltas, depth, quote, trade, bar, mark, index, funding, and greeks                     | Concrete codecs and generic `Data` diagnostics                           | Borrowed strings; numeric and bar-type parsing       |
| Event-store payload                   | Named-field order report with 0, 1, 32, and 256 fills                                  | Codec diagnostic; excludes storage, hashing, and indexing                | Retained named-field payload layout                  |
| Redis cache conversion                | Plain/nested account, instrument, custom data, and positions with 1, 32, and 256 fills | Separate payload benchmark with timestamp normalization                  | Retained `Value` timestamp normalization             |
| Python typed ingress                  | Supported typed-message fixtures                                                       | Native decode, JSON mapping, Python conversion, and callback             | Retained `Value` and Python object mapping           |
| Redis stream transport                | Supported external envelopes with preencoded payloads                                  | Separate round trip with complete received-envelope assertions           | Preencoded transport control                         |

Scalable response and generic market-data fixtures use sizes 1, 32, and 256. Scalar and empty
response cases keep their actual size under each size label. Compare their emitted wire sizes rather
than treating those labels as batch counts. Generic `Data` cases are dispatch diagnostics. Concrete
book delta and depth payloads omit the tag required by generic `Data` decoding; the benchmark asserts
that rejection and measures their supported concrete paths.

The event-store diagnostic uses the actual `order_report` and `fill_reports` field names. It does not
claim that this wrapper is an external bus message. Hyperliquid signing has a separate MessagePack
hashing contract. SBE and HyperSync are outside these measurements.

### Wire compatibility fixtures

Set `NAUTILUS_BENCH_FIXTURE_DIR` to export the payload, encoding, payload type, complete expected fields,
and the current decoder's canonical reencoding. Set `NAUTILUS_BENCH_REPLAY_DIR` on a later executable to
check those fields and exact reencoded bytes. Capture and replay under the same feature combination:
`serde_json/preserve_order` changes map ordering. Original incoming bytes can have a different key order
from canonical reencoding through `serde_json::Value`.

### Allocation counts

With heaptrack 1.5 and zstd installed, capture one operation and format per trace:

```bash
NAUTILUS_BENCH_PROFILE=encode NAUTILUS_BENCH_FORMAT=msgpack \
NAUTILUS_BENCH_SAMPLES=100 NAUTILUS_BENCH_TRIALS=1 \
  heaptrack --record-only -o /tmp/native-msgpack-encode \
  "target/bench-lto/deps/nautilus_common-<hash>" --exact \
  msgbus::external::codec::benchmarks::native_codec_measurements \
  --ignored --nocapture --test-threads=1 > /tmp/native-msgpack-encode.log 2>&1
python3 crates/common/benches/codecs/allocations.py \
  /tmp/native-msgpack-encode.zst /tmp/native-msgpack-encode.log
```

The reader attributes allocation events below the non-inlined codec boundary to each case. A one-byte
marker separates cases outside that boundary. Counts include reallocations; allocated bytes measure
requested allocation volume, not peak or retained memory. Profile separately from latency runs.
Use an allocating control case to confirm symbol matching before interpreting an all-zero profile.

### Money parsing

The `money_criterion` benchmark isolates `Money::from_str` for plain, underscored, and scientific
notation. Each input produces the same monetary value. Build it with the same precision feature as
the codec benchmarks:

```bash
PYO3_PYTHON="$PWD/python/.venv/bin/python" CARGO_BUILD_JOBS=8 \
  cargo bench --locked --profile bench-lto -p nautilus-model \
  --features high-precision --bench money_criterion --no-run
```

Run the emitted `target/bench-lto/deps/money_criterion-<hash>` executable with
`--bench 'Money::from_str'` to select all three forms. This measures parsing without a codec or bus.

### Redis cache payloads

Build the infrastructure test executable with matching precision:

```bash
PYO3_PYTHON="$PWD/python/.venv/bin/python" CARGO_BUILD_JOBS=8 \
  cargo test --locked --profile bench-lto -p nautilus-infrastructure \
  --features nautilus-common/sbe,nautilus-common/high-precision --lib --no-run
```

Run its ignored
`redis::queries::tests::redis_payload_measurements` test. It needs no Redis connection. The benchmark
uses the existing cache conversion through `Value`, including timestamp normalization, rather than
native bus serialization. The sample, trial, warmup, case, format, and encode/decode profile controls
have the same names as the native corpus.

### Python callback

Build the dedicated executable with the matching Python interpreter and library:

```bash
PYO3_PYTHON="$PWD/python/.venv/bin/python" CARGO_BUILD_JOBS=8 \
  cargo bench --locked --profile bench-lto -p nautilus-live \
  --features python,nautilus-common/sbe,nautilus-common/high-precision \
  --bench msgbus_python --no-run
```

Run the emitted `target/bench-lto/deps/msgbus_python-<hash>` executable with
`NAUTILUS_BENCH_REPLAY_DIR` pointing at fixtures captured with the same features. Set `LD_LIBRARY_PATH`
to the matching Python library directory when the loader needs it. The benchmark checks the complete
Python mapping through JSON before timing a no-op Python callback.
It measures decode, mapping, Python object construction, the callback, and destruction. It leaves
streaming types unset, so it excludes local republishing. Use the default Python allocator for latency;
`PYTHONMALLOC=malloc` permits separate allocation profiling and changes allocator behavior.

Set `NAUTILUS_BENCH_PROFILE` to any value to skip timing for allocation profiling. Use `call_processor`
as the measured boundary; `allocations.py` processes native codec traces only.

### Redis transport

Build `redis_msgbus` from `nautilus-infrastructure` with `bench-lto`. Setting
`NAUTILUS_BENCH_REDIS_PORT` selects actual stream delivery against Redis on `127.0.0.1`; without it,
the executable runs its existing stream-field construction benchmarks. Supply captured fixtures through
`NAUTILUS_BENCH_REPLAY_DIR`. Use a disposable Redis instance with persistence disabled.

The transport run creates a unique stream, waits for reader readiness, sends one message at a time,
asserts every received envelope field, and deletes its stream at completion. It uses immediate flush
and an approximate maximum stream length of 64. Results include local Redis and scheduling costs and
exclude payload encoding and decoding. They provide no remote-network latency claim. Retain actual
transport results separately from the bounded in-memory backing rows.
