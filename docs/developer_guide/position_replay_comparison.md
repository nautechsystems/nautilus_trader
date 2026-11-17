# Position replay comparison

This offline scenario compares carried-history lookup costs through `ExecutionEngine` with a
real, disposable loopback Redis instance. It does not connect to an exchange or use account data.
Read [the benchmark policy](../../BENCHMARKING.md) before interpreting these local measurements.

## Measured sources and environment

The November 17, 2026 comparison incorporates `develop` as checked before this run:

- Baseline: `c69e0c0ceba8c57080a6f1da69ce3a68ea5c70c7`.
- Indexed production sources: merge of `15ad6f7a73392c11da897b9537762cd969648adf` and that baseline,
  with no additional production-code changes. Both sides add identical test-only examples.
- The examples and runner published with this report are the exact measured versions. Only
  documentation/evidence files were updated after measurement; the production and harness sources
  were not changed. Use the commit containing this report, not a later moving branch, as `INDEXED`.
- Rust `1.100.0`, Apple M4 Pro, macOS `27.0.1`/aarch64, Redis `8.6.2`, high precision and native
  `bench-lto` (fat LTO, one codegen unit); no profile overrides.
- Identical lockfile SHA-256:
  `4bfd7fd3d34f6098d85f0c73f8dae67841cd6c6081f6e78e4f4b4701e95bfa37`.

[Source and binary hashes](position_replay_comparison.provenance.json) identify the artifacts.
[All 120 result groups](position_replay_comparison.csv) include medians and the minimum/maximum
of the three round medians, in nanoseconds. The earlier `0bbc9c98` versus `15ad6f7` measurements
are a separate historical experiment and are not mixed into this report.

## Build an equivalent new comparison

Use a checkout containing these examples as `HARNESS`, and two clean comparison worktrees at the
specified revisions as `BASELINE` and `INDEXED`. Set `BIN` to a new output directory and `TARGET` to
an existing shared Cargo target directory. Use absolute paths for these variables. Do not overwrite
an existing investigation worktree, change production sources, or clean shared caches.

Copy both `crates/infrastructure/examples/replay_latest_*.rs` files from `HARNESS` into each
comparison worktree's `crates/infrastructure/examples/` directory, creating that directory if needed.
The examples do not exist at the baseline revision; their addition is test-only. Build the
worktrees sequentially using their identical lockfiles and the same toolchain, environment and flags:

```bash
mkdir "$BIN" || exit 1
for variant in baseline head; do
    if [ "$variant" = baseline ]; then
        checkout="$BASELINE"
    else
        checkout="$INDEXED"
    fi
    (
        cd "$checkout"
        CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$TARGET" \
            nice -n 15 cargo +1.100.0 build --locked --offline --profile bench-lto \
            -p nautilus-infrastructure \
            --example replay_latest_comparison --example replay_latest_memory \
            --features redis,nautilus-model/high-precision
    ) || exit 1
    cp "$TARGET/bench-lto/examples/replay_latest_comparison" "$BIN/$variant-engine"
    cp "$TARGET/bench-lto/examples/replay_latest_memory" "$BIN/$variant-memory"
done
```

`--offline` requires dependencies to be cached. Record checkout revisions, source and lock hashes,
compiler version, machine and build commands before running. The script records binary hashes but
cannot infer which sources produced them. Never build concurrently with sampling. CPU frequency,
ASLR, thermal state and unrelated desktop work were not controlled in this comparison;
its figures are local descriptive evidence, not authoritative performance-host baselines.

## Run safely

Use Python 3.10 or later and native `redis-server` on macOS/Linux (or Linux inside WSL). The runner
requires POSIX priority support and lowers itself and its children to nice 15, never increasing
priority. It fails if this cannot be applied. It accepts no existing Redis endpoint: it starts a
loopback-only process with disk persistence disabled, checks the reported Redis PID against its
child PID, and stops that child on exit. Do not invoke the Rust examples against an application
Redis database. A high port number alone does not prove isolation.

Choose a previously nonexistent output directory for each invocation. The runner refuses existing
directories and opens individual logs exclusively, preserving older results:

```bash
python3 "$HARNESS/scripts/benchmark-position-replay.py" \
    --binary-dir "$BIN" --output-dir "$SMOKE_OUTPUT" --smoke
python3 "$HARNESS/scripts/benchmark-position-replay.py" \
    --binary-dir "$BIN" --output-dir "$MEASUREMENT_OUTPUT"
```

Use `--redis-server /path/to/redis-server` if it is not on PATH. `--smoke` omits performance rounds
but still runs small-history correctness fixtures and the memory probes. Test runner refusal and
termination without compiling or running the real benchmarks:

```bash
python3 scripts/test-benchmark-position-replay.py
```

## Workloads and interpretation

- The seed has already closed and reopened in the model. Initial reopen time is excluded.
- Continuing fills and repeated flips execute through `ExecutionEngine::process`. The synchronous
  timer includes adapter submission, **not Redis acknowledgment or durable-write latency**.
- Nonvirtual append persistence and virtual full snapshots are separate workloads. Drain and reload
  are separate rows, not included in per-fill time. Successful runs check reloaded economic state;
  virtual runs also compare complete replay state. Crash durability is not tested.
- Correctness uses `full`; performance uses `economics` and `unique-only`, without historical
  duplicate probes. Expected nonvirtual historical-duplicate reload failures return exit 2 and are
  recorded separately. The runner verifies the known duplicate diagnostic; any changed result stops
  the run for investigation. An expected failure is not a passing test.
- The full matrix has three alternating before/after rounds. Continuing runs add 100 fills for seed
  histories 8/64 and 20 fills for 36,000; flip runs execute five flips with a unique fill after each.
  Seed history excludes the extra close/reopen events. History grows during each run: an 8-history
  continuing run is not a fixed-size 8-fill microbenchmark.
- Summarize each operation separately: step zero, later steps, drain and reload. Compute each run's
  median, then the median and minimum/maximum of the three run medians. First-operation summaries
  contain only three observations; later flip summaries have four observations per run. Do not
  interpret these ranges as confidence intervals or claim statistical significance.
- The allocation probe reports model-position clone heap, first-query retained allocation, snapshot
  heap and inline size. It does not measure RSS, peak allocation or aggregate engine/cache memory.

## Results and limitations

The main run contains 194 invocations: 40 full correctness cases and 144 normal-flow measurement
processes passed their configured checks, and two memory probes passed. Eight nonvirtual
historical-duplicate reload cases failed, four on each version. A separate 26-invocation smoke
run had 20 correctness passes, two memory passes and four matching reload failures.

All eight main failures reported `Duplicate fill event for position AUDUSD-REPEAT-FLIP: T-FLIP-2`.
In-memory checks passed before reload failed. This fixture submits an old trade ID through a
fresh order, not an ordinary duplicate on the original order. The same diagnostic occurs on
both versions; that observation alone does not establish its root cause or real-venue prevalence.
These are failures, not successful recovery tests, and no persistence fix is included here.

Representative medians of three round medians, baseline to indexed:

| Workload                                                          | Baseline   | Indexed    |
| ----------------------------------------------------------------- | ---------: | ---------: |
| 36,000 seed, nonvirtual, warm start, later continuing fill        | 1.208 ms   | 9.125 us   |
| 36,000 seed, nonvirtual, cold start, first continuing fill        | 2.169 ms   | 1.690 ms   |
| 36,000 seed, nonvirtual, warm start, later flip                   | 7.433 ms   | 7.869 ms   |
| 8 seed, nonvirtual, warm start, later flip                        | 34.271 us  | 40.334 us  |
| 36,000 seed, virtual snapshots, warm start, later continuing fill | 105.429 ms | 101.844 ms |
| 36,000 seed, virtual snapshots, warm start, later flip            | 213.301 ms | 213.007 ms |

The continuing-fill warm path benefits substantially in the long nonvirtual fixture, but cold
reconstruction remains. The long-history flip ranges overlap: baseline 6.305-7.969 ms and indexed
6.274-8.009 ms. Its median is slower here, not evidence of a consistent flip speedup. Small-history
warm flips also have a higher indexed median, with overlapping ranges. Three rounds do not establish
statistical significance. Snapshot-heavy processing shows little improvement in this fixture. These figures
are synchronous process costs, not Redis persistence throughput, durable latency or production P99.

The allocator probe found:

| Seed history | Extra retained heap after first query, indexed | Baseline extra heap |
| ------------ | ---------------------------------------------: | ------------------: |
| 8            | 864 bytes                                      | 0 bytes             |
| 64           | 6,352 bytes                                    | 0 bytes             |
| 4,096        | 401,488 bytes                                  | 0 bytes             |
| 36,000       | 3,211,344 bytes                                | 0 bytes             |

Inline `Position` size was 832 bytes on baseline and 848 bytes on indexed. Clone heap was unchanged
at every measured size (16,129,508 bytes at 36,000), and the second clone did not copy a warm index.
The lightweight snapshot allocated 1,956 bytes in both versions. The roughly 3.06 MiB retained-index
cost at 36,000 is model-level requested live heap, not RSS, peak or total engine/cache memory.

## Validation scope

The integrated source passed `make format` and full `make pre-commit`, targeted example Clippy,
Redis-disabled memory-example compilation, 4,502 high-precision model unit tests (one ignored),
and all eight recovery cases in each of standard and high precision. Both native benchmark builds
and the runner isolation/termination tests passed. The comparison example declares `redis` as a
required feature; memory does not require it.

The full Python/Rust project test suites, full pre-flight, a new model Criterion run, Linux/Windows
execution and crash durability were not tested in this round. The Python extension was not built
in the isolated worktree, so the pre-commit collection hook is not Python runtime-test evidence.
A benchmark result does not by itself settle the public-API, small-history or memory tradeoffs.
