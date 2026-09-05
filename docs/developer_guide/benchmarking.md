# Benchmarking

Use this guide to write, run, and profile NautilusTrader benchmarks. It contains benchmark layout,
examples, local commands, and the measurement procedure for published results.

For benchmark scope, evidence requirements, and CI policy, see
[`/BENCHMARKING.md`](../../BENCHMARKING.md) at the repository root.

---

## Tooling overview

Select a tool based on the work and result:

| Tool                                                      | What it measures                          | Use it for                                                |
| --------------------------------------------------------- | ----------------------------------------- | --------------------------------------------------------- |
| [Criterion](https://docs.rs/criterion/latest/criterion/)  | Wall-clock time with confidence intervals | Operations above roughly 100 ns and elapsed time          |
| [iai](https://docs.rs/iai/latest/iai/)                    | Retired CPU instructions under Cachegrind | Small, deterministic operations and change detection      |
| [CodSpeed](https://codspeed.io/docs/instruments/cpu)      | Simulated CPU cost and cache behavior     | Stable pull request comparisons of deterministic CPU work |
| [flamegraph](https://github.com/flamegraph-rs/flamegraph) | Sampled call-stack profile                | Locating work inside a representative slow path           |

Criterion reports user-visible elapsed time. iai produces stable counts for the same binary,
toolchain, and inputs without requiring host noise controls. Compare iai results only under the
same code generation assumptions, and use Criterion when elapsed time is the required result.

---

## Directory layout

Each crate keeps its benchmarks in a local `benches/` folder:

```text
crates/<crate_name>/
└── benches/
    ├── foo_criterion.rs
    └── foo_iai.rs
```

Register each benchmark explicitly in the crate's `Cargo.toml` so
`cargo bench` discovers it:

```toml
[[bench]]
name = "foo_criterion"
path = "benches/foo_criterion.rs"
harness = false

[[bench]]
name = "foo_iai"
path = "benches/foo_iai.rs"
harness = false
```

To opt into the nightly CI performance workflow, register the benchmark and add its crate to
`CI_BENCH_CRATES` in the workspace `Makefile` when the list does not already include it. Add a
deterministic Criterion target to `CODSPEED_BENCH_TARGETS` when CPU simulation preserves what the
benchmark intends to measure. Do not add iai, Criterion's `iter_custom` or `with_filter` APIs,
OS-dependent work, or concurrent wall-clock benchmarks to the CodSpeed subset.

---

## Writing Criterion benchmarks

1. **Set up outside the timing loop.** All work that doesn't change between
   iterations belongs in the surrounding code or in `iter_batched_ref`'s
   setup closure, not in the body passed to `iter`.
2. **Wrap inputs in `black_box`** so the optimizer doesn't fold them away.
3. **Use `iter_batched_ref` for mutating benches.** It excludes input
   `Drop` from the timed region, which otherwise dominates the measurement
   on benches that own large structures.
4. **Add `Throughput::Elements(n)`** to size-parameterized groups so
   Criterion reports per-element throughput.
5. **Comment intent.** State what the benchmark is measuring (the hot path,
   the worst case, the cache-cold case) so a future reader understands
   what regressing it would mean.

```rust
use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

const SIZES: &[usize] = &[10, 100, 1_000];

fn bench_my_op(c: &mut Criterion) {
    let mut group = c.benchmark_group("module/my_op");

    for &n in SIZES {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched_ref(
                || populate(n),
                |state| state.run(black_box(n)),
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

criterion_group!(benches, bench_my_op);
criterion_main!(benches);
```

---

## Writing iai benchmarks

`iai` requires functions that take no parameters. Use it for small, pure operations so the measured
instruction count stays focused on the intended work.

```rust
use std::hint::black_box;

fn bench_add() -> i64 {
    let a = black_box(123);
    let b = black_box(456);
    a + b
}

iai::main!(bench_add);
```

Allocations, randomness, and system calls add their own instructions to the result. Keep variable
setup outside the measured function and compare counts produced by the same toolchain and target.

---

## Running benches locally

| Goal                               | Command                                                               |
| ---------------------------------- | --------------------------------------------------------------------- |
| All benches in one crate           | `cargo bench -p nautilus-execution`                                   |
| One core bench module              | `cargo bench -p nautilus-execution --bench matching_core`             |
| One engine bench module            | `cargo bench -p nautilus-execution --bench matching_engine`           |
| One core benchmark name pattern    | `cargo bench -p nautilus-execution --bench matching_core -- iterate`  |
| One engine benchmark name pattern  | `cargo bench -p nautilus-execution --bench matching_engine -- submit` |
| Quick smoke run (low sample count) | `cargo bench ... -- --quick`                                          |
| All nightly registered benches     | `make cargo-ci-benches`                                               |
| Build the CodSpeed subset          | `make cargo-codspeed-build`                                           |
| Check the built CodSpeed subset    | `make cargo-codspeed-run`                                             |

Criterion writes HTML reports to `target/criterion/`. Open
`target/criterion/report/index.html`. The report includes per-bench violin
plots, confidence intervals, and comparisons against the previous run's
saved baseline.

`make install-tools` installs the pinned `cargo-codspeed` version. A local CodSpeed run checks that
the selected benchmark targets build and register, but it does not upload measurements. The
`codspeed-benchmarks` job in `.github/workflows/performance.yml` measures and uploads the results.

### Canonical backtest workloads

The canonical backtest cases use the first 10,000 rows of the checked-in
`test_data/btc-perp-20211231-20220201_1m.csv` file. A shared fixture drives replay-only, scheduled
market-order, passive limit-order, and bar-EMA scenarios. The correctness test and both timed paths
use the same fixture loader and exact result fingerprints.

Run the semantic check first:

```bash
CARGO_BUILD_JOBS=16 cargo test --locked -p nautilus-backtest \
    --test integration canonical_backtest_workloads::
```

Then run Criterion in test mode to confirm that every affected benchmark case executes without
collecting samples:

```bash
CARGO_BUILD_JOBS=16 cargo bench --locked -p nautilus-backtest \
    --bench engine -- canonical --test
```

The `run_preloaded` cases load the CSV and build the engine outside the returned `iter_custom`
duration. The `load_build_run` cases include CSV loading, engine setup, data registration, and
`BacktestEngine::run`. Both exclude result projection and fingerprint verification from the
reported duration, while still checking the result after every measured iteration.

See [`crates/backtest/benches/BENCHMARKS.md`](../../crates/backtest/benches/BENCHMARKS.md) for the
published baseline, measurement record, and current profile target.

The [v2 migration guide](../../MIGRATION_V2.md#compare-backtest-performance) contains the
cross-version backtest comparison procedure.

---

## Measure Criterion for publication

Use the `bench-lto` profile for Criterion results that will be reported or published. The profile
inherits from `release`, preserves full debug symbols, enables fat LTO, and uses one code generation
unit. The default `bench` profile keeps full debug symbols without LTO and is better suited to local
iteration.

1. Quiesce the machine. On Linux, set the CPU governor to `performance` when you administer the
   host and can restore its prior state:

   ```bash
   sudo cpupower frequency-set -g performance
   ```

1. On Linux, disable ASLR for the benchmark process and run the selected benchmark with
   `bench-lto`:

   ```bash
   setarch "$(uname -m)" -R cargo bench --profile bench-lto -p <crate> --bench <name>
   ```

1. Run multiple full sessions and report whether each case uses its best or median result.

1. Record the CPU model, kernel or operating system, Rust toolchain, and build profile with the
   results:

   ```text
   Hardware: <CPU model>, <kernel or operating system>
   Toolchain: <rustc version>
   Profile: bench-lto (release + lto = "fat" + codegen-units = 1, debug = full)
   ```

For deeper analysis, control hyper-threading and dynamic frequency scaling in firmware. Published
results must record those controls when they differ from the normal host state.

iai runs under Cachegrind's virtual CPU model, so host quiescence, frequency scaling, and ASLR do
not affect its instruction counts. Run iai without the Criterion noise controls.

---

## Generating a flamegraph

`cargo-flamegraph` produces a sampled call-stack profile for one bench. Use it
when a benchmark regresses and the responsible inner call is unclear.

1. Install once per machine:

   ```bash
   cargo install flamegraph
   ```

2. Run a specific bench with the `bench` profile:

   ```bash
   cargo flamegraph --bench matching -p nautilus-common --profile bench
   ```

3. Open `flamegraph.svg` in a browser and zoom into hot paths.

### Linux

`perf` must be available. On Debian/Ubuntu:

```bash
sudo apt install linux-tools-common linux-tools-$(uname -r)
```

If `perf_event_paranoid` blocks the run:

```bash
sudo sh -c 'echo 1 > /proc/sys/kernel/perf_event_paranoid'
```

A value of `1` is usually enough. Set it back to `2` (default) afterwards
or persist via `/etc/sysctl.conf`.

### macOS

`DTrace` requires root, so `cargo flamegraph` must be run with `sudo`.

:::warning
Running with `sudo` creates files in `target/` owned by root, causing
permission errors with subsequent `cargo` commands. You may need to remove
root-owned files manually or run `sudo cargo clean`.
:::

```bash
sudo cargo flamegraph --bench matching -p nautilus-common --profile bench
```

The `bench` profile keeps full debug symbols, so flamegraphs render with
readable function names without bloating production binaries (which still
use `panic = "abort"` and are built via `[profile.release]`).

> **Note** Benchmark binaries are compiled with the custom `[profile.bench]`
> defined in the workspace `Cargo.toml`. That profile inherits from
> `release` and sets `debug = "full"`, preserving full optimisation *and*
> debug symbols so tools like `cargo flamegraph` or `perf` produce
> human-readable stack traces.

---

## Templates

Starter files live in [`docs/dev_templates/`](../dev_templates/):

- **Criterion**: [`criterion_template.rs`](../dev_templates/criterion_template.rs)
- **iai**: [`iai_template.rs`](../dev_templates/iai_template.rs)

Copy the template into the target crate's `benches/`, adjust imports and
group names, register in `Cargo.toml`, and start measuring.
