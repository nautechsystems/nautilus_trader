# Backtest Engine Benchmarks

These numbers were measured on 2026-08-10. The tables report the median of three interleaved runs
on the same host. Refresh them after a substantive backtest engine change or dependency upgrade.
Absolute numbers vary by machine; treat a result inside the recorded noise band as equivalent.

## Canonical workload

The matrix loads the first 10,000 rows of the checked-in
`test_data/btc-perp-20211231-20220201_1m.csv` file. It normalizes prices and volumes to the
instrument precision, then derives a zero-spread quote from each raw close so `LAST` bars and
market orders share one deterministic recorded price stream. Every scenario processes the same
10,000 quotes and 10,000 one-minute bars with bypass logging, analysis disabled, default optional
services, and one simulated venue.

| Scenario                | Submitted | Rejected | Filled | Canceled | Positions | Accounts |
| ----------------------- | --------: | -------: | -----: | -------: | --------: | -------: |
| Replay only             |         0 |        0 |      0 |        0 |         0 |        1 |
| Scheduled market orders |        64 |        0 |     64 |        0 |        32 |        1 |
| Passive limit orders    |        64 |        0 |      0 |       64 |         0 |        1 |
| Bar EMA cross           |       450 |        0 |    450 |        0 |       225 |        1 |

Before timing, the benchmark checks these counts, all 20,000 processed data events, execution
event counts, the canonical account digest, and the complete `CanonicalBacktestResult` digest.
The correctness test also requires the preloaded and freshly loaded paths to produce identical
canonical results. The complete result digest is exact within the active precision mode. Filled
workloads lock separate standard and `high-precision` digests because portfolio statistics
preserve exact `f64` bits; event counts, action counts, and account digests remain the same.

## Environment

| Item                 | Value                                                                      |
| -------------------- | -------------------------------------------------------------------------- |
| CPU                  | AMD Ryzen Threadripper 9980X, 64 cores, 128 threads                        |
| OS                   | Ubuntu 24.04.4 LTS, `x86_64`                                               |
| Kernel               | Linux 7.0.0-28-generic                                                     |
| Repository revision  | `31b04e2886369c8951872f84b5ad6169953b19aa`                                 |
| Measured source tree | `7817e1e15122ecb6e749c33b8d6d093696cb96c6`                                 |
| Benchmark executable | SHA-256 `e28182146a5d344391894b6eac90efc5fa7effb370fa6d2025b8f3025ff9fa7d` |
| Rust                 | `rustc 1.97.1`, LLVM 22.1.6                                                |
| Cargo                | `cargo 1.97.1`                                                             |
| Cargo features       | Crate default feature set (empty); standard precision                      |
| Profile              | `bench-lto`: release, fat LTO, one codegen unit, full debug information    |

The measured source tree contains staged changes on top of the repository revision above. Changes
made after the measurement add feature-specific result expectations outside the returned timing;
they do not alter the standard-precision fixture, engine configuration, actions, or `run()` path.

## Measurement controls

- CPU governor: observed `powersave`; no host state was changed.
- Governor policy: the repository recommends `performance` for published numbers. Because this
  baseline used `powersave`, compare it only with runs under the same governor. Repeat the matrix
  under `performance` before using it as a release or pull-request headline baseline.
- ASLR: disabled per benchmark process with `setarch "$(uname -m)" -R`.
- CPU scheduling: SMT and boost enabled; the benchmark thread was not pinned.
- Machine state: benchmark runs started after other Rust builds on the host completed.
- Warm-up: 3 seconds per case before each measurement.
- Sampling: 50 samples over a 5-second target measurement period per case.
- Run order: each round alternated `run_preloaded` and `load_build_run` for replay, scheduled
  market orders, passive limit orders, and bar EMA cross. The second and third rounds rotated the
  scenario order.
- Aggregation: median of three point estimates. Spread is `(maximum - minimum) / median`; the noise
  band is the observed minimum to maximum range for that row.

## How to reproduce

```bash
CARGO_BUILD_JOBS=16 cargo build --locked -p nautilus-backtest \
    --profile bench-lto --bench engine

for round in \
    "canonical_1:passive_limit_orders bar_ema_cross replay_only scheduled_market_orders" \
    "canonical_2:scheduled_market_orders replay_only bar_ema_cross passive_limit_orders" \
    "canonical_3:replay_only scheduled_market_orders passive_limit_orders bar_ema_cross"
do
    baseline=${round%%:*}
    scenarios=${round#*:}
    for scenario in $scenarios; do
        CARGO_BUILD_JOBS=16 setarch "$(uname -m)" -R \
            cargo bench --locked -p nautilus-backtest --profile bench-lto \
            --bench engine -- "^backtest_engine/canonical/.*/${scenario}$" \
            --warm-up-time 3 --measurement-time 5 --sample-size 50 \
            --save-baseline "$baseline" --noplot --quiet
    done
done
```

Criterion's `median.point_estimate` from each named baseline supplies the three per-row values.

For policy and the general noise-reduction recipe, see
[`BENCHMARKING.md`](../../../BENCHMARKING.md) at the repository root.

## Absolute baseline

Lower is better. Each case processes 20,000 data events.

| Scenario                | Preloaded `run()` median | Noise band, spread      | Full load/setup/run median | Noise band, spread      |
| ----------------------- | -----------------------: | ----------------------- | -------------------------: | ----------------------- |
| Replay only             |                23.285 ms | 22.398-23.641 ms, 5.3%  |                  30.141 ms | 30.136-30.289 ms, 0.5%  |
| Scheduled market orders |                25.920 ms | 25.377-28.271 ms, 11.2% |                  32.946 ms | 32.073-36.032 ms, 12.0% |
| Passive limit orders    |                39.318 ms | 38.887-43.839 ms, 12.6% |                  46.627 ms | 45.375-46.839 ms, 3.1%  |
| Bar EMA cross           |                37.326 ms | 37.103-41.650 ms, 12.2% |                  44.734 ms | 42.925-44.753 ms, 4.1%  |

## Profile

The host's `perf_event_paranoid=4` blocked `cargo flamegraph`; no kernel setting or privilege was
changed. GNU `gprofng` 2.42 supplied unprivileged one-millisecond clock sampling instead. The
experiment is host-local at `/tmp/nautilus-backtest-bar-ema.er`.

```bash
setarch "$(uname -m)" -R gprofng collect app -p 1 -t 25s -F off \
    -a usedldobjects -O /tmp/nautilus-backtest-bar-ema.er \
    target/bench-lto/deps/engine-9b29539648f11cbf \
    --bench --profile-time 15 \
    '^backtest_engine/canonical/run_preloaded/bar_ema_cross$'

gprofng display text -functions /tmp/nautilus-backtest-bar-ema.er
```

The executable name contains a Cargo-generated artifact hash. The SHA-256 in the environment table
identifies the captured binary; after rebuilding, substitute the new `engine-*` path.

The profile captured 13.502 seconds of CPU samples. Criterion's profile mode observes benchmark
setup and post-run fingerprint checks even though `iter_custom` excludes them from the reported
median. `BacktestEngine::run` accounted for 3.098 seconds inclusive, so 10.404 seconds, or 77.1%,
of the captured samples fell outside the timed region. Within the `run` stack, the largest visible
descendant was `OrderMatchingEngine::process_trade_ticks_from_bar` at 1.858 seconds inclusive.
`OrderBook::update_trade_tick` accounted for 1.244 seconds inclusive within that path.

The profile identifies the bar-execution trade-tick path as the next candidate for investigation,
starting with the order-book updates beneath `process_trade_ticks_from_bar`. Each fixture row also
supplies a quote at the same recorded close, so this target applies to the mixed quote-and-bar
workload rather than an isolated bar path. The profile alone does not justify a code change. This
baseline includes no production optimization.

## CI decision

Keep the canonical Rust workloads in the existing nightly Criterion run. The
[`codspeed-criterion-compat` limitations](https://github.com/CodSpeedHQ/codspeed-rust/blob/main/crates/criterion_compat/README.md#known-limitations)
state that `iter_custom` is unsupported, but this matrix needs `iter_custom` to return only the
preloaded `run()` duration while constructing a fresh engine for every iteration. CodSpeed's
[instrument documentation](https://codspeed.io/docs/instruments) also distinguishes its
single-run, hardware-agnostic simulation from wall-clock measurements on CodSpeed-managed
bare-metal runners. Neither mode reproduces this fixed-machine absolute baseline.

`nautilus-backtest` and its registered `engine` benchmark already run through the nightly
Criterion workflow, so no registration change is required. Nightly uses the default non-LTO
`bench` profile with debug information disabled; its timings are not comparable to the fixed-host
`bench-lto` table. The canonical preflight checks every fingerprint during benchmark registration,
and the integration test enforces them in the normal test lane. A mismatch also aborts the backtest
benchmark binary, but the existing crate loop does not reliably propagate an intermediate crate
failure after a later crate succeeds. Treat nightly as a measurement run rather than the semantic
gate. This benchmark matrix does not change that failure policy, and pull-request gating remains
deferred. Reconsider CodSpeed only if its Criterion compatibility supports the required timing
boundary and the project chooses its managed hardware for the authoritative baseline.

## Deferred scope

This first matrix does not measure peak RSS, broad catalog or streaming workloads, multiple
venues, multiple instruments, trigger orders, additional cancel cases, or pull-request gating.
Stabilize the four canonical workloads before adding those dimensions.
