# Reconciliation Latency Benchmark

The ignored `reconciliation_latency` integration tests measure market-data delivery while a real
`LiveNode` reconciles deterministic venue reports. Each case runs in a separate process and checks
the resulting order and position state before printing measurements. This guide describes the method;
each run produces its own measurements.

## Run

The tests use in-memory reports: no venue connection or credentials are required. Simulation builds
exclude these wall-clock tests.

From the repository root:

```bash
CARGO_BUILD_JOBS=8 NEXTEST_TEST_THREADS=1 cargo nextest run \
  --cargo-profile bench -p nautilus-live --test integration \
  --run-ignored ignored-only -E 'test(reconciliation_latency)' \
  --stress-count 3 --success-output immediate --failure-output immediate
```

Use `bench` for local investigation. Follow the repository's
[benchmarking policy](../../../BENCHMARKING.md) for published absolute figures and controlled
comparisons.

- Record the source revision and patch, compiler, CPU, profile, governor, background load, and complete output.
- Run serially without concurrent compilation or other benchmark loads.
- Compare repeated results and their spread; do not compare only the best runs.

The test lives under
[`tests/integration/node/serial_tests/reconciliation_bench.rs`](../tests/integration/node/serial_tests/reconciliation_bench.rs)
to reuse the integration suite's deterministic execution client and exact-state fixtures.

## Workload

### Traffic and measurement window

An independent OS thread schedules 10,000 trade ticks per second using fixed deadlines. Late
producers catch up rather than dropping deadlines. A typed trade subscription records timestamps
after the real data-engine dispatch. Every tick must arrive in order, and the final count must
match exactly.

Each case follows this sequence:

1. Discard 1,000 warm-up ticks. Hold the first order or position report until the final warm-up callback.
1. Verify that no report has returned, then release it and measure the next 30,000 ticks
   (three seconds of scheduled traffic).
1. Check the recovered state in the final measured callback. Recovery outside the measured window cannot pass.

Every recovery case checks exact prices, quantities, commissions, and trade identities, plus the
order count and open position count and quantity.

### Cases

| Case                       | Cached state                       | Reconciliation workload                          |
| -------------------------- | ---------------------------------- | ------------------------------------------------ |
| Baseline                   | One accepted order.                | All recurring reconciliation intervals disabled. |
| Steady                     | One accepted order.                | Matching open-order reports.                     |
| Baseline many              | 1,024 accepted orders.             | All recurring reconciliation intervals disabled. |
| Steady many                | 1,024 accepted orders.             | Matching open-order reports.                     |
| Cancel many                | 1,024 accepted orders.             | One burst canceling all orders.                  |
| Targeted                   | One accepted order.                | 1, 10, 64, 256, or 1,024 missing fills.          |
| Position                   | One accepted order.                | The same missing-fill sizes, capped per cycle.   |
| Position history           | 1,024 existing fills on one order. | Repeated history plus one missing fill.          |
| Warm baseline and recovery | One existing fill on one order.    | Baseline, or 1 / 1,024 missing fills.            |
| Stall control              | One accepted order.                | A deliberate 5 ms data-callback stall.           |

### Reconciliation settings

The benchmark configures these settings for its test workload.

- **Check cadence**: open-order or position checks request a 50 ms interval. Maintenance scheduling
  determines the actual cadence; query counts expose it.
- **Disabled checks**: startup reconciliation and in-flight checks are disabled.
- **Targeted queries**: the limit is one order, with zero inter-query delay.
- **Position recovery**: the production 64-fill dispatch cap applies. The fake client returns the full
  fill history on each query.

### Initialization and exclusions

Cases without existing fills measure the first execution recovery in a process. Warm cases preload
one fill before starting the node, excluding that initialization from measurement. All cases bypass
logging and omit user strategies, Python callbacks, external persistence, and real network I/O.

## Measurements

Each distribution reports its sample count, p50, p95, p99, p99.9, and maximum in microseconds using
the nearest-rank percentile definition.

| Metric                             | Meaning                                                         |
| ---------------------------------- | --------------------------------------------------------------- |
| `delivery`                         | Timestamp immediately before send to the trade callback.        |
| `scheduled`                        | Intended producer deadline to the trade callback.               |
| `producer`                         | Intended deadline to the timestamp immediately before send.     |
| `core_poll`                        | Wall-clock duration of each measured `LiveNode::run` poll.      |
| `core_poll_busy_us`                | Sum of measured poll durations.                                 |
| `maintenance_us`                   | Runner maintenance-counter delta between measurement bounds.    |
| `maintenance_between_ticks_max_us` | Largest maintenance-counter increase between consecutive ticks. |
| Query counts                       | Completed scenario's fill, bulk-order, and position requests.   |

### Delivery latency

`delivery` includes channel send, queuing, data-engine processing, and subscription dispatch. It
excludes the producer's scheduling delay, which `scheduled` includes. The stall control requires
a tick sent during the deliberate pause to incur at least 4 ms of delivery delay. An unrelated
latency spike cannot satisfy this check.

### Core poll timing

`core_poll` includes report-future polling, synchronous reconciliation, dispatch, and callbacks.
It excludes runtime scheduling and the driver future between node polls. A poll can process several
loop iterations. Polls are selected by the received tick count at poll entry, so a poll can cross
a measurement boundary and include the final state assertions. Its sample population differs from
the per-tick distributions.

### Maintenance timing

Maintenance counters exclude some report-future work and include unrelated maintenance. Their
largest per-tick increase can combine several handlers; it is not a single-handler duration.
Both maintenance and poll timings include any OS descheduling inside the measured interval and
must not be labeled CPU time. Report CPU profiling separately when available.

## Interpretation limits

### Venue behavior

The synthetic workload isolates core scheduling and recovery costs. It does not establish venue
latency, production percentiles, or a latency guarantee.

In-memory responses can keep higher-priority report branches ready across successive cycles.
Real network waits can allow market data to run between responses. The benchmark measures recovery
with promptly available reports, not a particular venue's cadence.

### Bursts and percentiles

Large position recoveries span multiple cycles, so queued-data delay can exceed any single poll.

One recovery burst within the three-second traffic window can affect the maximum without affecting
p99; inspect all metrics and workload sizes.

Ticks delayed by the same burst are correlated. A high percentile can describe that burst rather
than independent recovery events. Three repetitions provide three recovery observations, not 90,000
independent observations of recovery cost. These results do not establish a statistical regression threshold.

### Noise and unmeasured costs

The callback samples runner metrics and stores timings. This instrumentation adds overhead in every
case. Background load, OS scheduling, power management, and first-use initialization also affect results.

- Preserve baseline and producer distributions beside recovery figures; do not attribute every outlier
  to reconciliation.
- Compare small recovery cases with baseline spread from the same run. If the distributions overlap,
  do not attribute a small difference to reconciliation.
- Warm recovery is sampled only at 1 and 1,024 missing fills. Intermediate warm costs remain unmeasured.
