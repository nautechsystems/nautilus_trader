# Benchmarking

This guide explains how NautilusTrader measures Rust performance, when to
use each tool and the conventions you should follow when adding new benches.

---

## Tooling overview

Nautilus Trader relies on **two complementary benchmarking frameworks**:

| Framework | What is it? | What it measures | When to prefer it |
|-----------|-------------|------------------|-------------------|
| [**Criterion**](https://docs.rs/criterion/latest/criterion/) | Statistical benchmark harness that produces detailed HTML reports and performs outlier detection. | Wall-clock run time with confidence intervals. | End-to-end scenarios, anything slower than ≈100 ns, visual comparisons. |
| [**iai**](https://docs.rs/iai/latest/iai/) | Deterministic micro-benchmark harness that counts retired CPU instructions via hardware counters. | Exact instruction counts (noise-free). | Ultra-fast functions, CI gating via instruction diff. |

Most hot code paths benefit from **both** kinds of measurements.

---

## Directory layout

Each crate keeps its performance tests in a local `benches/` folder:

```text
crates/<crate_name>/
└── benches/
    ├── foo_criterion.rs   # Criterion group(s)
    └── foo_iai.rs         # iai micro benches
```

`Cargo.toml` must list every benchmark explicitly so `cargo bench` discovers
them:

```toml
[[bench]]
name = "foo_criterion"             # file stem in benches/
path = "benches/foo_criterion.rs"
harness = false                    # disable the default libtest harness
```

---

## Writing Criterion benchmarks

1. Perform **all expensive set-up outside** the timing loop (`b.iter`).
2. Wrap inputs/outputs in `black_box` to prevent the optimiser from removing
   work.
3. Group related cases with `benchmark_group!` and set `throughput` or
   `sample_size` when the defaults aren’t ideal.

```rust
use criterion::{black_box, Criterion, criterion_group, criterion_main};

fn bench_my_algo(c: &mut Criterion) {
    let data = prepare_data(); // heavy set-up done once

    c.bench_function("my_algo", |b| {
        b.iter(|| my_algo(black_box(&data)));
    });
}

criterion_group!(benches, bench_my_algo);
criterion_main!(benches);
```

---

## Writing iai benchmarks

`iai` requires functions that take **no parameters** and return a value (which
can be ignored). Keep them as small as possible so the measured instruction
count is meaningful.

```rust
use iai::black_box;

fn bench_add() -> i64 {
    let a = black_box(123);
    let b = black_box(456);
    a + b
}

iai::main!(bench_add);
```

---

## Running benches locally

- **All benches** for every crate: `make cargo-bench` (delegates to `cargo bench`).
- **Single crate**: `cargo bench -p nautilus-core`.
- **Single benchmark file**: `cargo bench -p nautilus-core --bench time`.

Criterion writes HTML reports to `target/criterion/`; open `target/criterion/report/index.html` in your browser.

---

## Templates

Ready-to-copy starter files live in `docs/dev_templates/`.

- **Criterion**: `criterion_template.rs`
- **iai**: `iai_template.rs`

Copy the template into `benches/`, adjust imports and names, and start measuring!
