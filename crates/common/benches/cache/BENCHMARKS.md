# Exchange-Rate Allocation Profile

This local profile measures the temporary allocations made by `Cache::try_get_xrate` after the
single-pass fallback-bar implementation. At 20 populated currency pairs, a lookup allocates
82 times with quotes and 87 times with bar fallback. The measured lookup allocations are all freed.
The counts justify a focused experiment on transient allocation volume; they do not establish
how much native CPU time the allocator consumes or prove that a proposed optimization is faster.

## Measurement context

The production source is unchanged from `fa3fbaf8bfef97e67b2e2bc23cdd25b7bb64e1bb`.
Measurements on 2026-09-16 use `crates/common/examples/cache_xrate_profile.rs` and the shared
fixtures in this directory. Refresh the profile after changing these workloads, exchange-rate
implementation, compiler, features, allocator, or measurement host:

- AMD Ryzen Threadripper 9980X, Linux 7.0.0-31-generic, rustc 1.98.1 (48a229cea 2026-09-01).
- `bench-lto`: optimized, fat LTO, one code generation unit, full debug information; crate default features, standard precision (`high-precision` disabled).
- Four Cargo build jobs; execution pinned to logical CPU 63, process ASLR disabled, CPU 63 governor
  temporarily `performance`, then restored to `powersave`. SMT and boost remain enabled.
- Shared host: other builds may run, the SMT sibling is not reserved, and timing is local evidence.
- Heaptrack 1.5.0, interpreted format v3, standard system allocator; no custom Rust allocator.
- Three fresh native processes and three fresh profiled processes per case, each with one first
  lookup followed by 30 batches of 100 lookups. Every lookup checks its exact result.

The example's batch timer includes lookup, result checking, and loop overhead. Setup, currency
initialization, JSON output, and teardown are outside the timer. The timing columns below report
per-lookup batch estimates: median of each process's 30 batch means, then median across processes.
`us` denotes microseconds. The interval contains the three process medians. These are not single-call latency percentiles or
Criterion confidence intervals. The overhead column divides the profiled median by the native median.
Raw batch samples are emitted as JSON by the example. Heaptrack timings do not serve as native baselines.

Measured example executable SHA-256:
`546af7d71c1cdde0cfd71d59d5831667253db3b79d6a6de9e31bd75e989a7bb7`.

## Workloads and lookup costs

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

Allocation calls include reallocation events. Bytes are requested allocation sizes, not allocator
usable sizes or RSS. Lookup peak is the maximum sum of live requested bytes allocated below the
steady-lookup frame, replayed in event order. It excludes pre-existing cache data and setup and is
not the sum of independent stack peaks. Zero steady-lookup bytes remain at process exit in every run.
Counts, bytes, and lookup peak agree across all three runs of every case.

| Mode   | Pairs/venue | Venues | Allocs/call | Bytes/call | Lookup peak bytes | Native us | Run interval us  | Overhead |
| ------ | ----------- | ------ | ----------- | ---------- | ----------------- | --------- | ---------------- | -------- |
| quotes | 5           | 1      | 27          | 2724       | 1448              | 1.239     | [1.233, 1.243]   | 5.77x    |
| quotes | 20          | 1      | 82          | 12092      | 5456              | 5.340     | [5.336, 5.376]   | 4.45x    |
| bars   | 5           | 1      | 30          | 3920       | 1532              | 2.751     | [2.728, 2.776]   | 3.31x    |
| bars   | 20          | 1      | 87          | 17256      | 5888              | 12.301    | [12.231, 12.375] | 2.64x    |
| mixed  | 5           | 1      | 30          | 3920       | 1532              | 2.755     | [2.747, 2.803]   | 3.39x    |
| mixed  | 20          | 1      | 87          | 17256      | 5888              | 12.560    | [12.409, 12.563] | 2.54x    |
| scan   | 5           | 1      | 11          | 1048       | 636               | 0.605     | [0.600, 0.606]   | 4.93x    |
| scan   | 20          | 1      | 11          | 1048       | 636               | 0.692     | [0.682, 0.702]   | 4.45x    |
| mixed  | 5           | 4      | 30          | 3920       | 1532              | 3.611     | [3.592, 3.628]   | 2.86x    |
| mixed  | 20          | 4      | 87          | 17256      | 5888              | 17.500    | [17.420, 17.568] | 2.22x    |

At 20 pairs with quotes, quote-table construction accounts for 32 allocations and 5,332 bytes per lookup.
Graph construction and traversal in `get_exchange_rate` account for another 50 allocations and
6,760 bytes. Bar fallback adds five allocations and 5,164 bytes to the quote-table stage.
The bar-table attribution is included in quote-table attribution; do not sum those inclusive groups.
Heaptrack stacks identify temporary formatted pair strings, hash-table growth, and graph vectors.

The extra venues increase mixed-case lookup time without changing lookup allocation counts or peak. Allocator
traffic therefore cannot explain the entire mixed/multi-venue cost. A small experiment on temporary
pair strings or graph storage is warranted, followed by a native Criterion comparison. Persistent
caches, invalidation rules, and public API changes require a separate design decision.

## First use and setup

The runner initializes the 20 base currencies and USD before constructing the fixture. Across these
runs, that stage makes 99 allocations requesting 15,260 bytes, peaks at 11,496 live requested bytes,
and retains 8,112 bytes. `ustr::total_allocated()` increases by 4,960 bytes at currency initialization;
that measures interned-string usage, not all registry allocations or reserved interner capacity.
The interner itself is initialized before this stage, so its initial infrastructure belongs to
`other` in the event report.

Fixture construction interns the remaining identifiers. Both the first lookup and steady state
add zero interned-string bytes in every case. First-lookup allocation counts, bytes, and peak equal
one steady-state lookup. These are fresh-process first calls after populated-cache setup, not an
artificial test that clears the global interner or invents currencies during lookup.

Setup peaks remain separate from the lookup table:

| Mode   | Pairs/venue | Venues | Setup peak bytes | Process peak bytes |
| ------ | ----------- | ------ | ---------------- | ------------------ |
| quotes | 5           | 1      | 4808200          | 17515364           |
| quotes | 20          | 1      | 19232344         | 31938807           |
| bars   | 5           | 1      | 400061504        | 412768666          |
| bars   | 20          | 1      | 1600245560       | 1612952021         |
| mixed  | 5           | 1      | 402941748        | 415648911          |
| mixed  | 20          | 1      | 1609846488       | 1622552950         |
| scan   | 5           | 1      | 80974708         | 93681870           |
| scan   | 20          | 1      | 80997484         | 93703945           |
| mixed  | 5           | 4      | 1611757944       | 1624465107         |
| mixed  | 20          | 4      | 6439351272       | 6452057734         |

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

Install Heaptrack 1.5.0 and `zstd`, then run each row three times in fresh processes under the same
CPU controls. Arguments are mode, pairs per venue, bar steps per side, venues, and lookups per batch:

```bash
profile_bin=target/bench-lto/examples/cache_xrate_profile
taskset -c 63 setarch "$(uname -m)" -R "$profile_bin" mixed 20 25 4 100
heap_out=$(mktemp -d)
taskset -c 63 setarch "$(uname -m)" -R heaptrack --record-only -o "$heap_out/xrate" \
    "$profile_bin" mixed 20 25 4 100
zstd -dc "$heap_out/xrate.zst" | python3 scripts/profile-xrate-heap.py
heaptrack_print "$heap_out/xrate.zst" --filter-bt-function steady_lookup -p 0 -T 0
```

Divide `steady_lookup.allocations` and `steady_lookup.allocated_bytes` by the JSON `lookups` count;
leave `peak_bytes` unnormalized. Function-level groups include the first lookup, so divide their
allocation counts and allocated bytes by `lookups + 1` (3,001 for the commands above). Keep all raw streams until analysis is complete. The parser accepts
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
