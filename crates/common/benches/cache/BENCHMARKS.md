# Exchange-Rate Lookup Benchmarks

This baseline records native lookup time and temporary allocations for `Cache::try_get_xrate`.
At 20 populated currency pairs, a lookup makes 43 allocations with quotes and 48 with bar fallback.
All measured lookup allocations are freed. Requested allocation sizes do not measure resident memory.

## Measurement context

Measurements on 2026-09-16 use `crates/common/examples/cache_xrate_profile.rs` and the shared
fixtures in this directory. The source is `3f1ec2c16dc87af5524f03498c04582a7d2c7e4b` with the
allocation-free pair parsing and reusable pair-key buffer included with this report.
Refresh this baseline after changing the workloads, exchange-rate implementation, compiler,
features, allocator, or measurement host.

- AMD Ryzen Threadripper 9980X, Linux 7.0.0-31-generic, rustc 1.98.1 (48a229cea 2026-09-01).
- `bench-lto`: optimized, fat LTO, one code generation unit, full debug information;
  crate default features, standard precision (`high-precision` disabled).
- Four Cargo build jobs; execution pinned to logical CPU 63, process ASLR disabled, CPU 63 governor
  temporarily `performance`, then restored to `powersave`. SMT and boost remain enabled.
- Shared host: other builds may run, the SMT sibling is not reserved, and timing is local evidence.
- Heaptrack 1.5.0, interpreted format v3, standard system allocator; no custom Rust allocator.
- Native timing: three fresh processes per case, each with one first lookup followed by
  30 batches of 1,000 lookups. Other executable measurements run between these processes.
- Allocation profiling: one fresh profiled process per measured case, with one first lookup
  followed by 30 batches of 100 lookups. Allocation counts have no independent replication.

Measured example executable SHA-256:
`d481fd3ea67cf33545af560d2445ee37eb5bdcacdd76e010d2a729168b39e61c`.

## Workloads

Each venue has 5 or 20 distinct base currencies quoted against USD. Every profile requests
AUD to USD at Mid, whose exact rate is `0.80005`. All bars have 25 step sizes per side, so a
fully populated pair has 50 bar types. Each cache uses its default capacities.

- `quotes`: every pair has a quote and no bars.
- `bars`: every pair has Bid and Ask bars and no quote.
- `mixed`: every pair has bars; alternating pairs also have quotes, including AUD. Missing quotes
  force the lazy bar-table build. Separate Criterion cases query EUR, which uses the fallback.
- `scan`: only AUD has a quote and bars. The first missing quote triggers one scan
  of AUD's 50 bar types per lookup; the remaining instruments miss that built table. This models
  scan-and-miss fallback work while the final AUD lookup succeeds.
- Four-venue cases repeat the mixed population on each venue and query only SIM0.

## Native lookup time

The example's batch timer includes lookup, result checking, and loop overhead. Setup, currency
initialization, JSON output, and teardown are outside the timer. Every lookup checks its exact result.
The table reports the median of each process's 30 per-lookup batch means, then the median across
processes. `us` denotes microseconds. The interval contains the three process medians; it is not a
single-call latency percentile or a Criterion confidence interval. The example emits raw batches
as JSON. Heaptrack timings do not serve as native baselines.

| Mode   | Pairs/venue | Venues | Native us | Run interval us  |
| ------ | ----------- | ------ | --------- | ---------------- |
| quotes | 5           | 1      | 1.047     | [1.046, 1.057]   |
| quotes | 20          | 1      | 4.636     | [4.622, 4.675]   |
| bars   | 5           | 1      | 2.565     | [2.541, 2.567]   |
| bars   | 20          | 1      | 11.607    | [11.480, 11.625] |
| mixed  | 5           | 1      | 2.588     | [2.582, 2.633]   |
| mixed  | 20          | 1      | 11.694    | [11.653, 11.695] |
| scan   | 5           | 1      | 0.580     | [0.567, 0.586]   |
| scan   | 20          | 1      | 0.666     | [0.649, 0.667]   |
| mixed  | 5           | 4      | 3.401     | [3.358, 3.412]   |
| mixed  | 20          | 4      | 16.400    | [16.301, 16.406] |

## Lookup allocations

Allocation calls include reallocation events. Bytes are requested allocation sizes, not allocator
usable sizes or RSS. Lookup peak is the maximum sum of live requested bytes allocated below the
steady-lookup frame, replayed in event order. It excludes pre-existing cache data and setup and is
not the sum of independent stack peaks. Zero steady-lookup bytes remain at process exit in every run.
Only the workloads below have allocation measurements for this executable.

| Mode   | Pairs/venue | Venues | Allocs/call | Bytes/call | Lookup peak bytes |
| ------ | ----------- | ------ | ----------- | ---------- | ----------------- |
| quotes | 5           | 1      | 18          | 2372       | 1384              |
| quotes | 20          | 1      | 43          | 10660      | 5392              |
| bars   | 20          | 1      | 48          | 15824      | 5896              |
| mixed  | 20          | 1      | 48          | 15824      | 5896              |
| scan   | 20          | 1      | 10          | 984        | 572               |
| mixed  | 20          | 4      | 48          | 15824      | 5896              |

The four-venue mixed case has the same allocation metrics as single-venue mixed, while its native
lookup time is higher. The reusable pair-key buffer remains live across instrument-loop iterations
and map growth, so its lifetime contributes to temporary peak heap.

## First use and setup

The runner initializes the 20 base currencies and USD before constructing the fixture. In each
profiled case, that stage makes 99 allocations requesting 15,260 bytes, peaks at 11,496 live
requested bytes, and retains 8,112 bytes. `ustr::total_allocated()` increases by 4,960 bytes at
currency initialization; that measures interned-string usage, not all registry allocations or
reserved interner capacity. The interner itself is initialized before this stage, so its initial
infrastructure belongs to `other` in the event report.

Fixture construction interns the remaining identifiers. Both the first lookup and steady state
add zero interned-string bytes in every measured case. First-lookup allocation counts, bytes, and
peak equal one steady-state lookup. These are fresh-process first calls after populated-cache
setup, not a test that clears the global interner or invents currencies during lookup.

| Mode   | Pairs/venue | Venues | Setup peak bytes | Process peak bytes |
| ------ | ----------- | ------ | ---------------- | ------------------ |
| quotes | 5           | 1      | 4808200          | 17515346           |
| quotes | 20          | 1      | 19232344         | 31938789           |
| bars   | 20          | 1      | 1600245560       | 1612952003         |
| mixed  | 20          | 1      | 1609846488       | 1622552932         |
| scan   | 20          | 1      | 80997484         | 93703927           |
| mixed  | 20          | 4      | 6439351272       | 6452057716         |

These large requested setup sizes include cache deque capacities for each quote/bar series; they
are not resident-memory measurements. Process peak includes interner/runtime initialization and the
populated cache. The setup column includes only allocations made under the setup frame. Neither is
an estimate of temporary lookup heap. Peak RSS from Heaptrack includes profiling overhead and is
not used here.

## Reproduce

Build and check semantics from the repository root:

```bash
CARGO_BUILD_JOBS=4 cargo test --locked -p nautilus-common --lib xrate -- --test-threads=4
CARGO_BUILD_JOBS=4 cargo bench --locked -p nautilus-common --bench cache_xrate -- --test
CARGO_BUILD_JOBS=4 cargo build --locked --profile bench-lto -p nautilus-common --example cache_xrate_profile
python/.venv/bin/pytest -q --noconftest python/tests/unit/test_profile_xrate_heap.py
```

For native Criterion measurements, use the existing `cache_xrate` target and the controls in the
[benchmarking guide](../../../../docs/developer_guide/benchmarking.md#measure-criterion-for-publication).
The mixed group covers 5/20 pairs, 1/25 steps, and 1/4 venues. Its pre-timing checks verify exact
forward and inverse Bid, Ask, and Mid rates, including quote precedence over disagreeing bar sides.
The focused cache tests cover missing sides, venue isolation, newest timestamps, and bar-type ties.

Install Heaptrack 1.5.0 and `zstd`. On Linux, select an available logical CPU and apply the
measurement controls above. For the recorded host, set CPU 63's governor to `performance` with
`sudo cpupower -c 63 frequency-set -g performance` and restore its prior value after the run.
Arguments are mode, pairs per venue, bar steps per side, venues, and lookups per batch.
Run each native table row three times in fresh processes with 1,000 lookups per batch; collect
one Heaptrack trace per allocation table row with 100 lookups per batch. The commands below
show the four-venue mixed workload:

```bash
profile_bin=target/bench-lto/examples/cache_xrate_profile
taskset -c 63 setarch "$(uname -m)" -R "$profile_bin" mixed 20 25 4 1000
heap_out=$(mktemp -d)
taskset -c 63 setarch "$(uname -m)" -R heaptrack --record-only -o "$heap_out/xrate" \
    "$profile_bin" mixed 20 25 4 100
zstd -dc "$heap_out/xrate.zst" | python3 scripts/profile-xrate-heap.py
heaptrack_print "$heap_out/xrate.zst" --filter-bt-function steady_lookup -p 0 -T 0
```

Divide `steady_lookup.allocations` and `steady_lookup.allocated_bytes` by the JSON `lookups` count;
leave `peak_bytes` unnormalized. Function-level groups include the first lookup, so divide their
allocation counts and allocated bytes by `lookups + 1` (3,001 for the profiled command above).
Keep raw streams outside the repository until analysis is complete. The parser accepts
completed, interpreted 1.5.0/v3 traces from this single-process runner, not raw traces or arbitrary
Heaptrack versions. It handles shared allocation IDs and inlined frames, and rejects unsupported
versions, malformed string lengths, and unmatched frees. Its tests deliberately distinguish
cumulative allocation volume, retained bytes, and simultaneous peak.

Heaptrack's backtrace filter affects printed stack groups but not its overall totals or size
histogram. Its merged stack peaks are also unsuitable for this lookup-only peak definition.
The event parser follows the [v1.5.0 event reader](https://github.com/KDE/heaptrack/blob/v1.5.0/src/analyze/accumulatedtracedata.cpp)
and independently computes filtered live bytes at every allocation/free event. Function attribution
is inclusive and depends on debug stack information. The `first_lookup` frame takes precedence
over its nested `steady_lookup` frame to prevent double counting first-use costs.
