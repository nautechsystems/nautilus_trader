# Benchmarking

This document is the practitioner reference for writing and running
NautilusTrader benchmarks. It covers tooling specifics, directory layout,
example code, local execution, and flamegraph profiling.

For policy (what we benchmark, when, with what rigor, how it ties into CI),
see [`/BENCHMARKING.md`](../../BENCHMARKING.md) at the repository root.

---

## Tooling overview

NautilusTrader uses two complementary Rust benchmarking frameworks:

| Framework                                                    | What it measures                          | When to prefer it                                    |
|--------------------------------------------------------------|-------------------------------------------|------------------------------------------------------|
| [**Criterion**](https://docs.rs/criterion/latest/criterion/) | Wall‑clock time with confidence bands     | Anything ≥ 100 ns; absolute measurement; comparison. |
| [**iai**](https://docs.rs/iai/latest/iai/)                   | Retired CPU instructions (via Cachegrind) | Sub‑100 ns functions; CI regression detection.       |

Most hot code paths benefit from both. Criterion gives the user-visible
number; iai gives a noise-free regression signal.

:::note
iai is deterministic (immune to system noise) but results are
machine-specific. Use it for regression detection within CI, not for
cross-machine comparisons.
:::

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

To opt into the nightly CI performance workflow, add the crate to the
`cargo-ci-benches` recipe in the workspace `Makefile`.

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

`iai` requires functions that take no parameters. Keep them small so the
instruction count is meaningful and so changes outside the function don't
leak into the measurement.

```rust
use std::hint::black_box;

fn bench_add() -> i64 {
    let a = black_box(123);
    let b = black_box(456);
    a + b
}

iai::main!(bench_add);
```

Setup that varies between runs (allocations, randomness, system calls)
will inflate instruction counts in misleading ways. iai is best for pure,
allocation-free functions.

---

## Running benches locally

| Goal                                | Command                                                              |
|-------------------------------------|----------------------------------------------------------------------|
| All benches in one crate            | `cargo bench -p nautilus-execution`                                  |
| One bench module                    | `cargo bench -p nautilus-execution --bench matching_core`            |
| One specific bench by name pattern  | `cargo bench -p nautilus-execution --bench matching_core -- iterate` |
| Quick smoke run (low sample count)  | `cargo bench ... -- --quick`                                         |
| All CI-tracked benches              | `make cargo-ci-benches`                                              |

Criterion writes HTML reports to `target/criterion/`. Open
`target/criterion/report/index.html`. The report includes per-bench violin
plots, confidence intervals, and comparisons against the previous run's
saved baseline.

---

## Generating a flamegraph

`cargo-flamegraph` produces a sampled call-stack profile for one bench.
Useful when a bench shows a regression but it's not obvious which inner
call is responsible.

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

Ready-to-copy starter files live in [`docs/dev_templates/`](../dev_templates/):

- **Criterion**: [`criterion_template.rs`](../dev_templates/criterion_template.rs)
- **iai**: [`iai_template.rs`](../dev_templates/iai_template.rs)

Copy the template into the target crate's `benches/`, adjust imports and
group names, register in `Cargo.toml`, and start measuring.
