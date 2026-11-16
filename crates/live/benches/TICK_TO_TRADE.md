# Tick-to-Trade Benchmarks

Compare quote- and trade-triggered order latency with risk enabled or bypassed. Both suites use real
`LiveNode` engines and equivalent Rust and Python strategies. Bybit connects real adapter clients to
a local mock venue without venue credentials.

Benchmarks: [engine](tick_to_trade.rs) and
[Bybit](../../adapters/bybit/benches/tick_to_trade.rs).
Both use dedicated Cargo bench targets with `test = false`, excluding them from normal integration
test runs. The launcher selects `--bench tick_to_trade` explicitly.

## Run

Run the full matrix from the repository root:

```bash
CARGO_BUILD_JOBS=1 VIRTUAL_ENV= uv run --project python --no-sync python \
  scripts/benchmark-tick-to-trade.py --stress-count 3
```

The launcher defaults to two runtime workers, one build job, serial process-isolated tests, and no
retries. It pins `PYO3_PYTHON` and the shared-library path to its interpreter.

Python cases require Python 3.13 or later and record the linked version. All Bybit cases require
the `python` feature.

- **Profiles**: Use `--profile bench` for faster investigation or `--profile nextest` for correctness
  checks. The first LTO build needs substantial time and memory.
- **Replay**: Use `--binaries-metadata <path>` to reuse a Nextest build without compiling. Preserve
  binaries, hashes, build revision, and toolchain: revision and compiler queries run at measurement time.
  Pass `--profile` to label reused binaries; otherwise their profile is recorded as `unknown`.
- **Quick check**: Prefix with `NAUTILUS_TTT_SAMPLES=8 NAUTILUS_TTT_WARMUP=2` and omit `--stress-count 3`.
  Orders must pass validation before timings count.
- **Diagnostics**: Set `NAUTILUS_TTT_DIAGNOSTIC=1` for stage timings. Run separately from headline cases:
  the message-bus tap adds timestamp and storage work to the measured path.

For authoritative baselines, follow the [measurement controls](../../../BENCHMARKING.md#measurement-requirements)
and keep other builds and timing runs off the host. Record CPU, OS, power settings, background load,
compiler, profile, and command with the output.

For the benchmark harness's regression checks, run from the repository root:

```bash
CARGO_BUILD_JOBS=1 VIRTUAL_ENV= uv run --project python --no-sync python \
  scripts/benchmark-tick-to-trade.py --profile nextest --run-ignored default
```

Use `--run-ignored all` to include these checks with the timing cases.

## Matrix and workload

### Case matrix

Each suite has 16 cases:

- **Language**: Rust or Python, including Python's strategy wrapper, callbacks, order factory, and
  submission bindings. Rust drives both strategy paths; Python host-loop overhead is excluded.
- **Trigger**: One quote or trade per order. Quotes use the bid; trades use the trade price.
- **Risk**: Enabled or `bypass=true`, which skips pre-trade checks and submission throttling but retains
  risk-engine routing and queueing.
- **Traffic**: One event in flight or fixed-rate ticks from an independent OS thread.

### Orders and validation

Defaults: 200 warm-up orders discarded, then 2,000 measured. Each order buys `0.010` units with GTC
time in force, an integer price cycling through `[3000, 3009]`, and a unique caller-supplied client
order ID. Automatic ID generation is excluded. Quote bid and ask sizes differ.

After the outbound timing boundary, the mock client or venue accepts and cancels each order.
Validation requires exact fields, counts, and final canceled status. Rejections, missing orders,
changed prices, or duplicates observed during collection fail the run. Late extra Bybit frames and
detached socket-task failures after collection are outside this validation.

### Traffic and timing

Single-event traffic waits for cancellation before the next tick. Fixed-rate traffic defaults to
1,000 ticks/s without waiting for orders. It retains late deadlines and skips no ticks, exposing
producer and queueing delays. The producer sleeps until each deadline, so OS wake-up lateness affects
both `producer_ns` and `scheduled_ns`; inspect it separately. Terminal orders stay cached, so comparisons
must match sample and warm-up counts.

### Engine and venue settings

Risk uses default checks with a 1,000,000 orders/s submit limit to measure successful throttling,
excluding the default 100 orders/s admission policy. Startup reconciliation and recurring in-flight,
open-order, and position checks are disabled. Logging is bypassed; no external persistence is configured.

The mock Bybit venue advertises a 1,000,000 orders/s account quota in normal response headers. The
adapter applies it and retains its 3,000 messages/s IP limit. This measures processing below those
limits, excluding the default venue admission rate.

The engine suite seeds an ETH perpetual instrument and USDT margin balance; Bybit loads a BTC perpetual
instrument and account responses from checked-in HTTP fixtures. Compare enabled/bypass variants within
each suite. Subtracting suite totals does not isolate adapter cost.

## Boundaries and metrics

| Suite            | Start                                            | End                                       |
| ---------------- | ------------------------------------------------ | ----------------------------------------- |
| `engine`         | Before enqueueing a decoded tick to the runner.  | Entry to the mock client's submit method. |
| `bybit_loopback` | Before the mock venue sends a market-data frame. | Mock venue receives the order frame.      |

The engine path includes runner queueing, data-cache updates, subscription dispatch, strategy and
order creation, risk processing, and execution dispatch. Bybit adds market-data transport and decoding,
adapter scheduling, request serialization, and outbound transport over established plain loopback
WebSockets. TLS, connection setup, external network delay, exchange processing, and physical NIC egress
are excluded.

The Bybit mock venue runs on its own thread and runtime, keeping receipt timestamps independent of
post-submit work on the node thread. Receipt timestamps precede payload parsing and validation.
Collector checks on the node thread and acceptance/cancellation feedback still add load to both traffic modes.

Distributions report per-event p50, p95, p99, p99.9, and maximum in nanoseconds using nearest-rank percentiles:

| Metric         | Interval                                        |
| -------------- | ----------------------------------------------- |
| `latency_ns`   | Actual source send to outbound receipt.         |
| `scheduled_ns` | Intended producer deadline to outbound receipt. |
| `producer_ns`  | Intended producer deadline to actual send.      |

Diagnostic cases also report consecutive intervals:

| Metric                     | Interval                                             |
| -------------------------- | ---------------------------------------------------- |
| `ingress_ns`               | Source send to data-engine subscription fanout.      |
| `strategy_ns`              | Subscription fanout to risk command enqueue.         |
| `risk_queue_ns`            | Risk command enqueue to risk-engine entry.           |
| `risk_ns`                  | Risk-engine entry to execution command enqueue.      |
| `execution_queue_ns`       | Execution command enqueue to execution-engine entry. |
| `execution_and_adapter_ns` | Execution-engine entry to the outbound receipt.      |

Fanout is timestamped before the strategy callback. Intervals include measurement overhead and OS
descheduling: they measure elapsed time, not CPU time. They sum per event; their percentiles do not.
Compare full enabled/bypass distributions and variation between runs. Subtracting p99 values does not
isolate risk processing; use diagnostic `risk_ns` to locate it.

## Output and controls

Each completed case prints a summary and writes JSON to `target/tick-to-trade/`, named by suite, case, process ID,
and timestamp. Artifacts include raw per-event samples, distributions, revision, tracked diff, benchmark
sources, executable path, runtime compiler version, selected build profile, kernel release, available CPU
information, linked Python version, and fixed-point precision. Failed metadata queries print diagnostics
and record `null`; the Git diff covers tracked files only.

Keep the build command with the artifacts: an executable path neither identifies its profile nor
preserves its contents. For a custom directory shared by both suites, use an absolute `NAUTILUS_TTT_OUTPUT`.

| Variable                  | Default                 | Purpose                             |
| ------------------------- | ----------------------- | ----------------------------------- |
| `NAUTILUS_TTT_SAMPLES`    | `2000`                  | Measured orders per case.           |
| `NAUTILUS_TTT_WARMUP`     | `200`                   | Orders excluded from summaries.     |
| `NAUTILUS_TTT_RATE`       | `1000`                  | Fixed-rate ticks per second.        |
| `NAUTILUS_TTT_DIAGNOSTIC` | Unset                   | Any set value enables stage timing. |
| `NAUTILUS_TTT_OUTPUT`     | `target/tick-to-trade/` | Artifact directory.                 |

Total orders and rate must each be in `[1, 1000000]` and fit the 120-second case deadline, which bounds
stalled workloads. Use more samples for tail estimates: 2,000 events leave at most two above p99.9.
Keep every repeated run, including outliers, and label shared-host results as local evidence.
