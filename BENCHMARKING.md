# Benchmarking

NautilusTrader is performance-sensitive software. This policy explains what
the project measures, which tools and controls it uses, and how contributors
and reviewers should evaluate performance work.

For instructions on writing and running benchmarks, generating flamegraphs,
and using the registered templates, see
[`docs/developer_guide/benchmarking.md`](docs/developer_guide/benchmarking.md).

---

## Purpose

Benchmarks provide absolute measurements for workload sizing, implementation
comparisons, and decisions about whether an optimization justifies its
complexity.

Back-to-back comparisons show whether a proposed change shifts performance.
This evidence is most useful when both runs use the same workload, toolchain,
machine, and measurement controls.

Choose a measurement based on the question it must answer. Criterion reports
elapsed time for representative work, iai provides an instruction-count signal
for small and deterministic operations, and a profile identifies where a
workload spends its time. A workload may support more than one measurement,
but each result must follow its tool's method.

---

## Approach

Apply these principles when adding or evaluating benchmarks.

**Benchmarks are documentation.** A benchmark records what we considered the
hot path, what inputs we judged realistic, and what the resulting cost looked
like at a point in time. Future contributors read benchmarks to understand
where time goes and which behavior a performance change must preserve. An
unexplained regression is a code-review concern.

**Prefer measuring real units of work.** A benchmark that times a meaningful
public method on a populated structure is more useful than one that times a
private implementation detail. The public method usually survives refactoring
and continues to represent user-visible work.

**Target hot paths.** Adding a benchmark because the code is convenient to
drive does not justify the maintenance cost. New benchmarks should target hot
paths or work under active optimization.

**Compare equivalent wall-clock runs.** Wall-clock figures vary with cache
sizes, microarchitecture, frequency behavior, thermal state, scheduler
decisions, and ASLR. Compare back-to-back on the same machine under the same
controls when evaluating a delta. Use a designated, documented performance
host for authoritative absolute baselines; treat other machines as local
evidence.

**Measure before optimizing.** Profile or benchmark first. The codebase is
large enough that intuition about hot paths is unreliable.

**Support performance claims with evidence.** Performance claims in pull
request descriptions or release notes should reference a benchmark or
profile so reviewers can evaluate and reproduce the result.

---

## What we bench

Choose the workload scope based on the behavior the result needs to represent.

### Hot-path microbenchmarks

These live in each crate's `benches/` folder and target individual functions
or public methods judged to be hot paths. Examples:

- `crates/execution/benches/matching_core.rs`: the `OrderMatchingCore` add,
  delete, lookup, and iterate API.
- `crates/execution/benches/matching_engine.rs`: market data, order submission,
  passive matching, resting fills, modify, and cancel paths through
  `OrderMatchingEngine`.
- `crates/common/benches/matching.rs`: message-bus topic matching.
- `crates/common/benches/cache/orders.rs`: order cache query and ingest.

Add one when:

- A new optimization or refactor needs a baseline so future changes can be
  measured against it.
- A code path is performance-sensitive and lacks coverage.
- A reviewer asks for evidence of a change's performance impact.

A separate microbenchmark is usually unnecessary when:

- The function is straight-line code with no allocations or branches.
- The function is on a cold administrative path such as startup,
  configuration validation, or diagnostics.
- A higher-level benchmark already covers the relevant work.

### Scenario benchmarks

These exercise larger units of work: ingesting a tick burst through the
data engine, replaying a market session, dispatching through the live-node
runner. They cost more to maintain but provide a closer proxy for
user-observable performance than a single-function microbenchmark. Examples
live under `crates/backtest/benches/`, `crates/data/benches/`, and
`crates/live/benches/`.

The backtest engine benchmark covers single-stream and multi-stream market
data replay plus representative order workloads. Its
[canonical v2 matrix](crates/backtest/benches/BENCHMARKS.md) replays checked-in
raw data with replay-only, scheduled market-order, passive limit-order, and
bar-strategy scenarios. Each scenario checks an exact semantic fingerprint
before Criterion measures either preloaded `run()` time or full data loading,
engine setup, and `run()` time.

The benchmarks under `crates/live/benches/` cover scoped operations such as
dispatch. The ignored stress test at `crates/live/tests/integration/stress.rs`
covers the deeper runner and engine workload.

---

## Automated benchmark execution

The [`performance` workflow](.github/workflows/performance.yml) runs two benchmark paths:

- `performance-benchmarks` executes the registered Rust Criterion and iai benchmarks on pushes to
  `nightly`. This path checks benchmark execution but does not compare deltas or gate pull requests.
- `codspeed-benchmarks` runs selected deterministic Criterion targets under CodSpeed CPU simulation
  for pull requests targeting `develop` and pushes to `develop` or `test-performance`. CodSpeed uses
  the `develop` results as the comparison baseline for pull request reports.

The workspace [`Makefile`](Makefile) defines both scopes. `CI_BENCH_CRATES` is the authoritative
crate set for the full nightly run. `CODSPEED_BENCH_CRATES` and `CODSPEED_BENCH_TARGETS` define the
CodSpeed subset. CodSpeed excludes iai targets, Criterion's `iter_custom` and `with_filter` APIs,
OS-dependent work, and concurrent wall-clock workloads because CPU simulation does not preserve
their intended measurement.

For a pull request that materially changes a hot path outside the CodSpeed subset, run a local
Criterion comparison against `develop`. The nightly workflow provides an execution check after the
change reaches `nightly`.

---

## Python benchmarks and Rust benches

Add a Rust bench under `crates/<crate>/benches/` when the measured work stays
inside Rust. Use a Python benchmark when the workload must include end-user
PyO3 API cost.

The nightly workflow runs the registered Rust benches, and CodSpeed compares its selected Rust
Criterion subset. The workspace dependency aliases `codspeed-criterion-compat` as `criterion`, so
the same source continues to run through standard `cargo bench` commands.

Use a Python benchmark when the workload must include end-user PyO3 API cost. The
[`benchmark-backtest-versions.py`](scripts/benchmark-backtest-versions.py) driver compares the
released v1 Cython engine with the v2 PyO3 engine in isolated environments. It records the package,
backend, source revision, wheel hash, loaded extension hash, Python version, and precision mode
before timing. The loaded extension must byte-match the corresponding wheel member. After every
timed sample, its worker repeats the complete wheel, extension, source, and runtime identity proof
and checks its canonical digest against the coordinator's initial identity. Raw output stores each
full identity and selected scenario/boundary fingerprint once, then binds every sample to both by
digest. Source identity hashes staged diffs, unstaged diffs, and untracked file contents in addition
to the revision. The driver also requires the v2 extension's embedded build revision to match the
requested source revision. Any future Python CI integration must provide the same identity evidence
for the package built from `python/pyproject.toml`.

---

## Choosing a tool

| Task                                                  | Tool                                                      | Result                                    |
| ----------------------------------------------------- | --------------------------------------------------------- | ----------------------------------------- |
| Measure elapsed time or compare implementations       | [Criterion](https://docs.rs/criterion/latest/criterion/)  | Wall-clock time with confidence intervals |
| Detect instruction-count changes in a small operation | [iai](https://docs.rs/iai/latest/iai/)                    | Retired CPU instructions under Cachegrind |
| Compare deterministic CPU work in pull requests       | [CodSpeed](https://codspeed.io/docs/instruments/cpu)      | Simulated CPU cost and cache metrics      |
| Locate work inside a representative slow path         | [flamegraph](https://github.com/flamegraph-rs/flamegraph) | Sampled call-stack profile                |

Criterion produces wall-clock numbers. They reflect what the user actually
experiences but vary with CPU frequency, thermal state, scheduler decisions,
ASLR, and cache state. Apply the project's measurement controls before
publishing them.

iai counts machine instructions under Cachegrind. For a fixed binary,
toolchain, and input, the count gives a stable change signal. It is not
directly comparable to wall-clock time, and code generation changes can shift
it independently of runtime performance.

CodSpeed CPU simulation runs each selected Criterion case once under a simulated CPU. It provides
a stable pull request comparison signal, not an authoritative wall-clock measurement. Confirm a
material regression or improvement with Criterion under the project's measurement controls.

For setup, examples, and templates, see the
[developer guide](docs/developer_guide/benchmarking.md).

---

## Recording results

Record benchmark results according to how readers will use them.

**Inline Criterion HTML reports.** Each `cargo bench` run writes
`target/criterion/<group>/<id>/report/index.html`. Criterion's saved
baselines in the same directory support local back-to-back comparisons.

**Component baseline reports.** A checked-in `BENCHMARKS.md` records a
reproducible baseline, its revision, and the measurement method. Refresh that
report explicitly when a new baseline replaces the published one.

**Pull request descriptions.** Substantive optimization or restructuring work
should include a headline comparison with the hardware, toolchain, and build
profile. The pull request provides the durable context for the change and its
measurements.

**CodSpeed history.** CodSpeed stores the selected Rust benchmark results for `develop`,
`test-performance`, and pull requests targeting `develop`. Use its reports to identify candidate
regressions, then use the benchmark's native tool when the decision requires elapsed time or
instruction counts.

**Release notes.** A measured user-visible improvement belongs under
"Internal Improvements" as a brief entry naming the optimized component. Keep
detailed result tables in the pull request.

Pull request descriptions and Git history retain measurement context outside the CodSpeed subset,
while checked-in component reports publish the reproducible baseline for their stated revision.

---

## Measurement requirements

For Criterion runs whose numbers will be reported or compared:

- **Build with the right profile.** Two profiles inherit from `release` and
  preserve full debug symbols:
  - `bench`: the `cargo bench` default. It omits LTO for faster local
    iteration and ad-hoc comparison.
  - `bench-lto`: adds `lto = "fat"` and `codegen-units = 1` to match the
    production release binary. Use it for published results in component
    `BENCHMARKS.md` reports, pull request descriptions, and release notes.
- **Quiesce the machine.** Close other workloads. On Linux, use the
  performance CPU governor and disable ASLR for the benchmark process.
- **Reserve firmware controls for deeper analysis.** Hyper-threading and
  dynamic frequency scaling can be controlled when the investigation needs
  tighter conditions.
- **Run the benchmark multiple times.** Report the best or median per case and
  state which aggregate was used. Criterion's confidence intervals describe
  each run; multiple full runs expose session-level drift.
- **Record the measurement context.** Published results include the CPU model,
  kernel or operating system, Rust toolchain, and build profile.

Cachegrind's virtual CPU model makes host quiescence and frequency scaling
irrelevant to iai instruction counts. Run iai directly while keeping the
binary, toolchain, and input fixed.

Follow the
[published Criterion measurement procedure](docs/developer_guide/benchmarking.md#measure-criterion-for-publication)
for the commands and result header.
