# Benchmarking

This guide explains how NautilusTrader measures Rust performance, when to
use each tool and the conventions you should follow when adding new benches.

---

## Tooling overview

NautilusTrader relies on **two complementary benchmarking frameworks**:

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
2. Wrap inputs/outputs in `black_box` to prevent the optimizer from removing
   work.
3. Group related cases with `benchmark_group!` and set `throughput` or
   `sample_size` when the defaults aren’t ideal.

```rust
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

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
use std::hint::black_box;

fn bench_add() -> i64 {
    let a = black_box(123);
    let b = black_box(456);
    a + b
}

iai::main!(bench_add);
```

---

## Running benches locally

- **Single crate**: `cargo bench -p nautilus-core`.
- **Single benchmark module**: `cargo bench -p nautilus-core --bench time`.
- **CI performance benches**: `make cargo-ci-benches` (runs the crates included
  in the CI performance workflow one at a time to avoid the mixed-panic-strategy
  linker issue).

Criterion writes HTML reports to `target/criterion/`; open `target/criterion/report/index.html` in your browser.

### Generating a flamegraph

`cargo-flamegraph` lets you see a sampled call-stack profile of a single
benchmark. On Linux it uses `perf`, and on macOS it uses `DTrace`.

1. Install `cargo-flamegraph` once per machine (it installs a `cargo flamegraph`
   subcommand automatically).

   ```bash
   cargo install flamegraph
   ```

2. Run a specific bench with the symbol-rich `bench` profile.

   ```bash
   # example: the matching benchmark in nautilus-common
   cargo flamegraph --bench matching -p nautilus-common --profile bench
   ```

3. Open the generated `flamegraph.svg` in your browser and zoom into hot paths.

#### Linux

On Linux, `perf` must be available. On Debian/Ubuntu, you can install it with:

```bash
sudo apt install linux-tools-common linux-tools-$(uname -r)
```

If you see an error mentioning `perf_event_paranoid` you need to relax the
kernel’s perf restrictions for the current session (root required):

```bash
sudo sh -c 'echo 1 > /proc/sys/kernel/perf_event_paranoid'
```

A value of `1` is typically enough; set it back to `2` (default) or make
the change permanent via `/etc/sysctl.conf` if desired.

#### macOS

On macOS, `DTrace` requires root permissions, so you must run `cargo flamegraph`
with `sudo`:

```bash
# Note the use of sudo
sudo cargo flamegraph --bench matching -p nautilus-common --profile bench
```

> **Warning**
> Running with `sudo` will create files in your `target/` directory that are
> owned by the `root` user. This can cause permission errors with subsequent
> `cargo` commands.
>
> To fix this, you may need to remove the root-owned files manually, or simply
> run `sudo cargo clean` to remove the entire `target/` directory.

Because `[profile.bench]` keeps full debug symbols the SVG will show readable
function names without bloating production binaries (which still use
`panic = "abort"` and are built via `[profile.release]`).

> **Note** Benchmark binaries are compiled with the custom `[profile.bench]`
> defined in the workspace `Cargo.toml`.  That profile inherits from
> `release-debugging`, preserving full optimisation *and* debug symbols so that
> tools like `cargo flamegraph` or `perf` produce human-readable stack traces.

---

## Templates

Ready-to-copy starter files live in `docs/dev_templates/`.

- **Criterion**: `criterion_template.rs`
- **iai**: `iai_template.rs`

Copy the template into `benches/`, adjust imports and names, and start measuring!
