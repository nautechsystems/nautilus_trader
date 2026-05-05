# Benchmarking

NautilusTrader is performance-sensitive software. This document describes
how the project approaches benchmarking: what we measure, why, when, and
with what tools. It is intended for contributors and reviewers who need to
understand the policy before writing or evaluating performance work.

For practitioner detail (how to write a benchmark, run it locally, generate
a flamegraph, registered templates) see
[`docs/developer_guide/benchmarking.md`](docs/developer_guide/benchmarking.md).

---

## Purpose

Benchmarks exist to answer one of two questions:

1. **How fast is this code today, in absolute terms?** Used when sizing a
   workload, comparing alternatives, or judging whether an optimization is
   worth the change in complexity.
2. **Did this change shift performance in either direction?** Used as a
   change-detection signal: regressions to catch, improvements to confirm.
   Primarily lives in CI.

These two purposes call for different tools and different rigor. We use
**Criterion** for the first and prefer instruction-counting tools (**iai**)
for the second. A single benchmark may serve both purposes, but the
methodology each requires is different and that distinction runs through
the rest of this document.

---

## Approach

A few principles shape how the project treats benchmarks.

**Benchmarks are documentation.** A benchmark records what we considered the
hot path, what inputs we judged realistic, and what the resulting cost looked
like at a point in time. Future contributors read benchmarks to understand
where time goes, not just to verify changes haven't regressed.

**Prefer measuring real units of work.** A benchmark that times a meaningful
public method on a populated structure is more useful than one that times a
private helper in isolation. The former survives refactoring; the latter
breaks every time the caller changes.

**Bench what you optimize, not what is easy to bench.** Adding a benchmark
because the code is convenient to drive does not justify the maintenance
cost. New benchmarks should target hot paths or paths under active
optimization, not arbitrary functions.

**Absolute numbers vary by machine; relative numbers vary less.** Wall-clock
figures are not portable across hardware, and even ratios shift with cache
sizes, microarchitecture, and frequency behavior. Compare back-to-back on
the same machine when you need a meaningful delta. CI change detection
runs on a fixed self-hosted machine; local numbers from a developer laptop
should not be quoted as authoritative.

**Don't optimize without measuring.** Profile or bench first. The codebase
is large enough that intuition about hot paths is unreliable.

**Don't claim a win without a bench.** Performance claims in PR descriptions
or release notes should reference a benchmark or profile. "Faster" without
numbers is not actionable for reviewers and won't survive the next refactor.

---

## What we bench, and when

Benchmarks are added in three contexts. Each has different scope and
maintenance expectations.

### 1. Hot-path micro-benchmarks (per crate)

These live in each crate's `benches/` folder and target individual functions
or public methods judged to be hot paths. Examples:

- `crates/execution/benches/matching_core.rs`: the `OrderMatchingCore` add /
  delete / lookup / iterate API.
- `crates/common/benches/matching.rs`: message-bus topic matching.
- `crates/common/benches/cache_orders.rs`: order cache query and ingest.

Add one when:

- A new optimization or refactor needs a baseline so future changes can be
  measured against it.
- A code path is performance-sensitive and lacks coverage.
- A reviewer asks for evidence of a change's performance impact.

Skip when:

- The function is straight-line code with no allocations or branches.
- The function is on a cold administrative path (startup, config validation,
  diagnostics).
- An existing benchmark already covers the relevant work via a higher-level
  call.

### 2. End-to-end / scenario benchmarks

These exercise larger units of work: ingesting a tick burst through the
data engine, replaying a market session, dispatching through the live-node
runner. Heavier to maintain but a closer proxy for user-observable
performance than a single-function micro. Examples live under
`crates/data/benches/`, `crates/live/benches/`, and the Python performance
suite in `tests/performance_tests/`. Note that `crates/live/benches/` are
still scoped (e.g. dispatch only, not the full select loop); the deeper
runner-plus-engine workload is the ignored stress test at
`crates/live/tests/stress.rs`.

### 3. CI change-detection benches

A subset of crates runs benchmarks in CI on pushes to the `nightly`
branch via the
[`performance` workflow](.github/workflows/performance.yml). The included
crates are listed in the `CI_BENCH_CRATES` variable of the workspace
`Makefile` (currently `nautilus-core`, `nautilus-model`, `nautilus-common`,
and `nautilus-live`). To opt a new bench into nightly CI execution,
register it in its crate's `Cargo.toml` and ensure that crate is in
`CI_BENCH_CRATES`.

CI does not currently fail PRs on Rust benchmark deltas: the performance
workflow only runs on pushes to `nightly`, not on PR opens. Contributors
investigating a suspected regression or confirming a claimed improvement
should run a local Criterion comparison against `develop` for any PR that
materially changes a hot path; the nightly run is consulted after the
fact.

The Python performance suite (`tests/performance_tests/`) runs through
[CodSpeed](https://codspeed.io/) on the same nightly workflow. The
nightly dashboard surfaces both regressions and improvements; both are
worth investigating when they cross the noise threshold.

### Python performance tests vs Rust benches

Add a Rust bench (Criterion or iai under `crates/<crate>/benches/`) when
the work is in Rust and you want either an absolute number or an
instruction-count change signal. Add a Python performance test
(`tests/performance_tests/...`, picked up by CodSpeed) when the work
crosses the Cython/PyO3 boundary or measures end-user Python API cost
that wouldn't show up in a pure-Rust bench. The two suites are
complementary: the Rust suite tracks engine performance, the Python suite
tracks the API surface users actually call.

---

## Tooling at a glance

| Framework                                                     | Measures                                  | Use for                                              |
|---------------------------------------------------------------|-------------------------------------------|------------------------------------------------------|
| [**Criterion**](https://docs.rs/criterion/latest/criterion/)  | Wall‑clock time with confidence bands     | Anything ≥ 100 ns; absolute measurement; comparison. |
| [**iai**](https://docs.rs/iai/latest/iai/)                    | Retired CPU instructions (via Cachegrind) | Sub‑100 ns functions; CI change detection.           |
| [**flamegraph**](https://github.com/flamegraph-rs/flamegraph) | Sampled call‑stack profile                | Investigating where time goes inside a slow bench.   |

Criterion produces wall-clock numbers. They reflect what the user actually
experiences but vary with CPU frequency, thermal state, scheduler decisions,
ASLR, and cache state. Reduce that noise (see
[Reducing noise](#reducing-noise) below) before quoting them.

iai counts machine instructions under valgrind's Cachegrind. For a fixed
binary, toolchain, inputs, and environment the count is deterministic, so
small changes in count are a reliable change signal in either direction.
The count is not directly comparable to wall-clock time, and counts
measured on different binaries (toolchain bumps, codegen changes) shift in
ways that are not portable. Use iai to detect changes on the same machine
and toolchain, not to size workloads.

For setup, examples, and templates, see the
[developer guide](docs/developer_guide/benchmarking.md).

---

## Recording results

We record benchmark results in three places, depending on context.

**Inline Criterion HTML reports.** Each `cargo bench` run writes
`target/criterion/<group>/<id>/report/index.html`. Criterion's saved
baselines (in the same directory) provide PR-vs-base comparisons when the
two runs are done back-to-back on the same machine.

**Release notes.** When a change produces a measurable performance
improvement, add a brief entry under "Internal Improvements" naming the
optimized component. Don't paste full bench tables into release notes;
one line is enough. Larger headline numbers belong in the PR description.

**PR descriptions.** Substantive optimization or restructure efforts that
span multiple changes should include a "headline numbers" table in the PR
description, with hardware and toolchain noted alongside (see the example
below). The PR description is the durable home for those numbers; release
notes get one terse line.

We do not currently maintain a checked-in historical bench database.
Long-term records live wherever the CI workflow uploads them
(CodSpeed for the Python suite; Criterion HTML on the runner for Rust).

---

## Reducing noise

For Criterion runs whose numbers will be reported or compared, reduce noise
before measuring:

- **Build with the `bench` profile.** It inherits from `release` and
  preserves full debug symbols (configured in the workspace `Cargo.toml`).
  `cargo bench` uses this profile by default.
- **Quiesce the machine.** Close other workloads. On Linux, set the CPU
  governor to `performance`:

  ```bash
  sudo cpupower frequency-set -g performance
  ```

- **Disable ASLR for repeatability** (Linux):

  ```bash
  setarch -R cargo bench -p <crate> --bench <name>
  ```

- **Disable hyper-threading and dynamic frequency scaling** in BIOS for
  deeper analysis. Not required for casual measurement.
- **Run the bench multiple times** and take the best or median per case.
  Criterion's confidence intervals already help, but multiple full runs
  catch session-level drift.
- **Record the machine.** When publishing numbers, note CPU model,
  kernel/OS, Rust toolchain, and which build profile produced them.
  Numbers without this context are not actionable.

Example header to accompany published results:

```text
Hardware: AMD Ryzen Threadripper 9980X (64C), Linux 6.17.0
Toolchain: rustc 1.95.0
Profile: bench (inherits from release, debug = full)
```

For iai, none of the above applies: Cachegrind's virtual CPU model means
instruction counts do not depend on machine quiescence or frequency
scaling. Run it directly without the noise mitigations above.

---

## Summary

- Two questions, two tools: Criterion for absolute time, iai for change detection.
- Bench what you optimize, not what is easy to bench.
- Reduce noise before quoting numbers; record the machine when you do.
- Opt into nightly CI execution by adding the crate to the `cargo-ci-benches` recipe.
- Treat existing benchmarks as documentation of what we believe is hot.
  Regressing one without explanation is a code-review concern.

For implementation detail (writing benches, running locally, flamegraphs,
templates), see
[`docs/developer_guide/benchmarking.md`](docs/developer_guide/benchmarking.md).
