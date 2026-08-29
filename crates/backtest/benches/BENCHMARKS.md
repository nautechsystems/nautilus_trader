# Backtest Engine Benchmarks

The canonical absolute baseline was measured on 2026-08-10. Its tables report the median of three
interleaved runs on the same host. Refresh the full matrix after a substantive backtest engine
change or dependency upgrade. Later sections can record focused comparisons without replacing the
full matrix. Absolute numbers vary by machine; treat a result inside the recorded noise band as
equivalent.

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

This historical full matrix predates the [L1 replacement follow-up](#l1-replacement-follow-up),
which remeasures its replay-only preloaded case.

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

The profile promoted the bar-execution trade-tick path for investigation, starting with the
order-book updates beneath `process_trade_ticks_from_bar`. Each fixture row also supplies a quote
at the same recorded close, so this target applies to the mixed quote-and-bar workload rather than
an isolated bar path. The profile alone did not justify a code change. The 2026-08-10 baseline
includes no production optimization.

## L1 replacement follow-up

A 2026-08-29 follow-up profiled all four canonical scenarios under both exact benchmark boundaries
on the same host. As percentages of total captured CPU samples, inclusive time for
`process_trade_ticks_from_bar` ranged from 13.67% to 47.12%, and inclusive time for
`OrderBook::update_trade_tick` ranged from 7.78% to 29.75%. `OrderBook::update_book_bid` and
`update_book_ask` removed the current top order immediately before `BookLadder::add(order, 0)`
cleared the ladder and inserted the replacement.

The candidate removed that redundant deletion. The baseline was the signed commit
`Optimize BacktestEngine inactive service paths`. Three paired sessions alternated immutable
baseline and candidate executables for the `run_preloaded` boundary and `replay_only` scenario.
Each executable was built once from its source tree and reused unchanged. Both used `rustc 1.98.0`,
Cargo 1.98.0, standard precision with default crate features, and the `bench-lto` profile. The
governor remained `powersave`, no host control changed, and ASLR was disabled per benchmark process.
Each run used a 3-second warm-up, a 5-second measurement target, and 50 samples.

| Pair order         | Baseline median | Candidate median | Reduction |
| ------------------ | --------------: | ---------------: | --------: |
| Baseline/candidate |       23.548 ms |        18.823 ms |    20.07% |
| Candidate/baseline |       23.053 ms |        17.790 ms |    22.83% |
| Baseline/candidate |       23.219 ms |        18.501 ms |    20.32% |

The median reduction was 20.32%. A fresh three-run pre-edit replay-only session established the
0.6843% observed noise spread, which every comparison pair cleared. The canonical correctness test
matched every exact result fingerprint for all four scenarios under both boundaries after the
change. The remaining seven canonical cases were fingerprint-verified but not re-timed.

## L1 level reuse follow-up

A second 2026-08-29 follow-up used the signed `Optimize OrderBook L1 replacement` commit as its
baseline. This section supersedes the [L1 replacement follow-up](#l1-replacement-follow-up) as the
current replay-only preloaded reference. Fresh one-millisecond `gprofng` profiles covered all four
scenarios under both benchmark boundaries. As percentages of total captured CPU samples, inclusive
samples for `BookLadder::add` remained material in every case:

| Scenario                | Full load/setup/run | Preloaded `run()` |
| ----------------------- | ------------------: | ----------------: |
| Replay only             |              31.49% |            38.19% |
| Scheduled market orders |              23.09% |            26.41% |
| Passive limit orders    |              19.97% |            22.47% |
| Bar EMA cross           |              10.14% |             9.81% |

In replay only, sampled ladder work came from `OrderBook::update_trade_tick` under
`process_trade_ticks_from_bar` and from `OrderBook::update_quote_tick`. Both called
`BookLadder::add`. The same profile ranked allocation and `IndexMap` insertion among its largest
entries. Profile mode included setup and fingerprint work, so these percentages selected the
target but do not support the elapsed-time claim.

The candidate reuses the existing L1 `BookLevel` and its order-map allocation for internal quote
and trade tick replacement. It still clears every other level, the order cache, and incomplete
batch state before inserting the replacement. Zero-size updates still clear the side, and the
replacement remains available to later cache-backed updates and deletes.

| Item                 | Baseline                                                           | Candidate                                                          |
| -------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| Repository revision  | `ca823f818faa7535c1d147c5ce93e6031e2a997c`                         | Baseline revision plus the measured patch                          |
| Measured source tree | `3a5e146fb179836395b46cbb92944415d8bbc811`                         | `df8a9729980e701905293515ed992521662b2ea7`                         |
| Executable SHA-256   | `8c26d4b18dfe6743f095714f30b4a826f1de29c7bb8a7983ffc1ceb72bd9706d` | `22fc1721f71e6bf8b6c4b823ad8722b7b4e89c3b7350d86a2c109ac738a7abbb` |
| ELF build ID         | `0b1066eccb0b8ff5a287c200527a097e13783b7f`                         | `af4947418d7bbde1c1ef9535ab68f9e4e8be6a84`                         |

Later test and documentation additions do not alter the library source compiled into the measured candidate executable.

Both executables were built once with the build command in [How to reproduce](#how-to-reproduce)
under the `bench-lto` profile and reused unchanged. Both used `rustc 1.98.0` with LLVM 22.1.8,
Cargo 1.98.0, standard precision, and the default empty `nautilus-backtest` feature set. The host was
the Threadripper 9980X system described above with Linux 7.0.0-28-generic. All 128 governors remained
`powersave`; no host control changed. ASLR was disabled per process, and the benchmark thread was not
pinned. The paired run found no concurrent Rust build and recorded 95% to 99% CPU idle in its host samples.

Three sessions alternated the immutable executables for canonical replay-only `run_preloaded`.
Each run used a 3-second warm-up, a 5-second measurement target, and 50 samples.

| Pair order         | Baseline median | Candidate median | Reduction |
| ------------------ | --------------: | ---------------: | --------: |
| Baseline/candidate |       19.784 ms |        18.162 ms |     8.20% |
| Candidate/baseline |       19.214 ms |        17.804 ms |     7.34% |
| Baseline/candidate |       19.333 ms |        17.897 ms |     7.42% |

The median reduction was 7.42%. Three pre-edit baseline sessions measured 19.533, 19.552, and
19.838 ms, for a 1.56% spread; every comparison pair cleared that threshold. This spread describes
repeatability within this session, so absolute medians are not comparable with earlier follow-ups.
The canonical correctness test matched every exact result fingerprint for all four scenarios under
both boundaries before and after the production change. Focused quote-tick tests cover positive
replacement, incomplete-batch reset, zero-size clearing, and deletion after replacement. The other
seven canonical cases were fingerprint-verified but not re-timed.

## Rejected same-price L1 node reuse

A third 2026-08-29 follow-up used the signed `Optimize OrderBook L1 level reuse` commit
`f87112bd2e356ce621fe4b09bfd329c70adf6735` as its baseline. Fresh one-millisecond `gprofng`
profiles again covered all four canonical scenarios under both boundaries. Inclusive samples for
`BookLadder::replace_l1` ranged from 8.94% to 35.74% of total captured CPU samples. Its B-tree
insertion ranged from 3.00% to 13.27%, and removing the current node ranged from 1.34% to 5.09%.

The candidate kept the sole existing `BookLevel` in the B-tree when its `BookPrice` matched the
replacement, then replaced its orders and reset the order cache and incomplete batch state in
place. Empty ladders, changed prices, and multiple existing levels continued through the prior
remove, clear, and reinsert path.

| Item                 | Baseline                                                           | Candidate                                                          |
| -------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| Repository revision  | `f87112bd2e356ce621fe4b09bfd329c70adf6735`                         | Baseline revision plus the measured patch                          |
| Measured source tree | `a069f478ef0315c385ef0fe45e3ea532703562de`                         | `39d7fdd7a3f8eb695d62b09308c4d82fe531ec4e`                         |
| Executable SHA-256   | `4c949cc095beae17d7047084bf8df588d06af438fbe4c8c9fc765f61ff52f9b6` | `b898927bb34d0ed0c0a584f78d8f627dd4ca6d72580841d2dbaf3be2806d06ed` |
| ELF build ID         | `15677b977ae773c38763608ec3fd4c2f30629f6e`                         | `bea9f4f9f30208c5d4edcbffc965548800a573d4`                         |

Both executables were built once with the command in [How to reproduce](#how-to-reproduce) and
reused unchanged. Both used Rust and Cargo 1.98.0, standard precision, the default empty
`nautilus-backtest` feature set, and the `bench-lto` profile. The Threadripper 9980X host retained
its `powersave` governors, ASLR was disabled per process, no host control changed, and accepted
sessions recorded no concurrent Cargo work and 95% to 98% CPU idle. Runs with concurrent Cargo work
or CPU idle below the predeclared 95% floor were discarded.

Canonical replay-only `run_preloaded` was selected before viewing timing results because it had the
largest `BookLadder::replace_l1` inclusive share at 35.74% and excluded setup from the returned
duration. Three fresh baseline sessions measured 16.492150083, 17.152273000, and 17.723467333 ms.
Their 17.152273000 ms median and 7.178740975% full spread set the predeclared threshold. Three paired
sessions then alternated the immutable executables with a 3-second warm-up, a 5-second measurement
target, 50 samples, and isolated Criterion homes.

| Pair order         | Baseline median | Candidate median |     Reduction |
| ------------------ | --------------: | ---------------: | ------------: |
| Baseline/candidate | 18.788075900 ms |  18.277741900 ms |  2.716265373% |
| Candidate/baseline | 18.306483200 ms |  18.615707100 ms | -1.689149667% |
| Baseline/candidate | 18.577500800 ms |  17.914931000 ms |  3.566517408% |

The three reductions remained inside the 7.178740975% baseline spread, including one regression, so
the result is no measurable change. The correctness matrix matched all eight exact canonical
fingerprints. The measured candidate was removed. Its direct B-tree lookup could also panic for an
opposite-side replacement. A corrected equality-gated variant passed focused correctness checks but
was not timed, so it did not change the rejection.

Do not select this same-price in-map replacement again solely because `BookLadder::replace_l1` or
its B-tree operations remain prominent in a profile. Reconsider it only after a material
implementation, standard-library, toolchain, or representative-workload change, or after reducing
baseline spread enough to resolve the observed range. A different measured cost within
`BookLadder::replace_l1` remains eligible for investigation.

## Trade ID formatting follow-up

A fourth 2026-08-29 follow-up used the signed `Document OrderBook L1 optimization rejection`
commit `30dbec8ea3b10796c333247d5214e3886758e53f` as its baseline. Fresh one-millisecond `gprofng`
profiles covered all four canonical scenarios under both benchmark boundaries. This section
supersedes the [L1 level reuse follow-up](#l1-level-reuse-follow-up) as the current replay-only
preloaded reference. As percentages of total captured CPU samples, inclusive samples for
`IdsGenerator::generate_trade_id` and for the allocation-backed formatting entry point were:

| Scenario                | Full load/setup/run ID generation | Full load/setup/run formatting | Preloaded `run()` ID generation | Preloaded `run()` formatting |
| ----------------------- | --------------------------------: | -----------------------------: | ------------------------------: | ---------------------------: |
| Replay only             |                            13.07% |                          9.87% |                          14.52% |                        8.94% |
| Scheduled market orders |                            10.55% |                          8.48% |                          12.37% |                        8.37% |
| Passive limit orders    |                             8.19% |                          6.57% |                           8.44% |                        5.89% |
| Bar EMA cross           |                             4.07% |                          4.49% |                           4.48% |                        4.05% |

The formatting percentages are the inclusive share for `alloc::fmt::format::format_inner`.
Profile mode included setup and fingerprint work, so these shares selected the target but do not
support the elapsed-time claim.

The candidate replaces the `format!` call in `IdsGenerator::generate_trade_id` with a stack buffer.
It writes the fixed-width lowercase hexadecimal hash and the zero-padded decimal execution counter
directly, then passes the resulting string slice through the same `TradeId::from` validation. The
counter still increments before hashing and formatting. Checked counter overflow, unchecked
wrapping, minimum three-digit padding, digit growth, and the 36-character limit retain their prior
behavior.

| Item                 | Baseline                                                           | Candidate                                                          |
| -------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| Repository revision  | `30dbec8ea3b10796c333247d5214e3886758e53f`                         | Baseline revision plus the measured patch                          |
| Measured source tree | `86007d5fb76ff719cbfb63b0a54697baeb107b10`                         | `6ded9b806f789d83e8e74a2fb315d02beedeaf24`                         |
| Executable SHA-256   | `e96730816eb9c6f3c57c98b06acad2da9cea76989cf084794f257da39e86c4bb` | `acbfd21e0acec786e808f487e8f37f23a7157f1702f33760441783068671f4b1` |
| ELF build ID         | `4e634bb099808fb65fd385bc0ef9d860d7872b6a`                         | `28332a01915deb7ad09a6e51148853e98aac859e`                         |

Both executables were built once with the command in [How to reproduce](#how-to-reproduce), copied
to immutable paths, and reused unchanged. Both used Rust and Cargo 1.98.0, LLVM 22.1.8, standard
precision, the default empty `nautilus-backtest` feature set, and the `bench-lto` profile. The host
was the Threadripper 9980X system described above with Linux 7.0.0-28-generic. All 128 governors
remained `powersave`; no host control changed. ASLR was disabled per process, and the benchmark
thread was not pinned.

Three fresh baseline sessions measured 16.147417250, 16.537059000, and 16.888020000 ms. Their
16.537059000 ms median and 4.478442932% full spread set the predeclared threshold.
Canonical replay-only `run_preloaded` was selected before viewing candidate timing because it had
the largest `IdsGenerator::generate_trade_id` inclusive share at 14.52% and excluded setup from the
returned duration.

Three paired sessions alternated the immutable executables. Each run used an isolated Criterion
home, a 3-second warm-up, a 5-second measurement target, and 50 samples. Accepted sessions recorded
no concurrent Cargo, Rust compiler, Clippy, or Make work and 98.21% to 98.35% CPU idle. Sessions
where a sibling build appeared or CPU idle fell below the predeclared 95% floor were discarded.

| Pair order         | Baseline median | Candidate median | Reduction |
| ------------------ | --------------: | ---------------: | --------: |
| Baseline/candidate |    15.917690 ms |     13.617237 ms |   14.452% |
| Candidate/baseline |    16.763418 ms |     13.247239 ms |   20.975% |
| Baseline/candidate |    15.990977 ms |     13.376372 ms |   16.351% |

The median reduction was 16.351%, and every pair cleared the baseline spread threshold. The
optimization was retained. The immutable baseline and candidate executables matched all eight exact
canonical fingerprints. The canonical workload matrix test passed after the change. Focused trade
ID tests cover exact hash width and case, counter padding boundaries, the maximum valid length,
oversized values, counter mutation before validation, checked and unchecked overflow behavior,
reset behavior, and the existing Rust parity fixtures. All 31 tests in the `ids_generator` module
passed, along with focused rustfmt and Clippy checks. The later test strengthening and this benchmark
documentation do not alter the library code compiled into the measured candidate executable.

## v1.231.0 and v2 comparison

The 2026-08-27 comparison uses the Python
[`benchmark-backtest-versions.py`](../../../scripts/benchmark-backtest-versions.py) driver to run
the released v1.231.0 Cython engine and the v2 Rust/PyO3 engine at revision
`908c571caec0af086c1d1a8edbcf7bcbb07d6621`. The checked-in
[`v1-v2-results.json`](v1-v2-results.json) file contains every elapsed sample, the full workload
matrix, full runtime identities once per version, an identity digest for every sample, host state,
fingerprints, medians, spreads, ratios, and gaps.

### Runtime identity

| Item               | v1 Cython                                                          | v2 Rust/PyO3                                                       |
| ------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| Package            | `1.231.0`                                                          | `2.0.0rc4`                                                         |
| Source revision    | `27a8e54e7ac3c57d6cbf8891f0283dfbaee97317`                         | `908c571caec0af086c1d1a8edbcf7bcbb07d6621`                         |
| Wheel SHA-256      | `687fe654278de95b788d352272c3970adc33bdd8936da50f96d63d748c205fae` | `894dc4e2d60d9672dcfb6d61d58877324caf14466dc0d560545fb69525888a9f` |
| Extension SHA-256  | `bfda18b09cc5bf4a6cf83249d3552a2f767bf47774341525f14193986fc308b5` | `93e2b5034a251a4069f9109c789602dba41424a12b7248435d60852a5379c681` |
| Python             | `3.12.3`                                                           | `3.12.3`                                                           |
| Precision          | High precision                                                     | High precision                                                     |
| Engine module      | `nautilus_trader.backtest.engine`                                  | `nautilus_trader.backtest`                                         |
| Embedded revision  | Not available                                                      | Matches the requested v2 source revision                           |
| Install provenance | Loaded extension byte-matches the wheel member and records its URL | Loaded extension byte-matches the wheel member and records its URL |

The driver SHA-256 is
`b3636a7b1ff8a121580495c7c8da2eb61f7ae8d3ea95b5c49c9589f2aa5818a9`. The result file SHA-256 is
`4ff2ae000c93333f70d0769f90ce0323710dd7ac7b389e7f21e28cdd183fac45`.

### Measurement method

The matrix covers 16 cases across quote and trade replay, bars, L2 deltas, depth snapshots,
multiple instruments and streams, market and passive orders, cancellation, GTD expiry, and order
type triggers. `run_preloaded` times only `BacktestEngine.run()` after setup and data loading.
`load_build_run` includes fixture generation, engine construction, data registration, and `run()`.

Each case receives one warmup and one timed sample in each of five interleaved sessions. Sessions
alternate the v1/v2 execution order and rotate or reverse the case order. The driver checks the
complete wheel, extension, source, and runtime identity before the matrix. Source identity includes
the revision plus content hashes for staged diffs, unstaged diffs, and untracked files. After every
timed sample, its worker repeats that proof and checks its canonical digest against the initial
identity. The driver also checks the complete event, order, position, and account fingerprint after
every timed iteration. All 320 timed iterations matched across versions. The tables report the
median of the five session samples. Spread is `(maximum - minimum) / median`, so rows with a wide
spread need more samples before they support a close comparison.

The host had an AMD Ryzen Threadripper 9980X with 64 cores and 128 threads, Linux
7.0.0-28-generic, and the `performance` governor. The benchmark process ran with ASLR disabled. The
load averages were 1.81, 6.14, and 9.37 before the run and 4.12, 5.40, and 8.11 after it. The
coordinator and both measured environments ran Python 3.12.3.

### Preloaded `run()` results

Lower is better. A negative gap means v2 was faster.

| Scenario                     | v1 median  | v1 spread | v2 median  | v2 spread | v2/v1  | v2 gap |
| ---------------------------- | ---------: | --------: | ---------: | --------: | -----: | -----: |
| `quote_trade_replay_small`   |   1.708 ms |     13.3% |   0.358 ms |     36.9% | 0.209x | -79.1% |
| `quote_trade_replay_medium`  |  10.863 ms |     14.7% |   2.443 ms |     21.7% | 0.225x | -77.5% |
| `quote_trade_replay_multi`   |  11.108 ms |     13.8% |   2.728 ms |     10.7% | 0.246x | -75.4% |
| `bar_last_replay`            |  18.116 ms |     13.6% |   3.329 ms |     21.1% | 0.184x | -81.6% |
| `bar_bid_ask_strategy`       |  21.442 ms |     14.6% |   3.019 ms |     35.0% | 0.141x | -85.9% |
| `l2_deltas_queue_passive`    |   9.700 ms |      7.3% |   2.234 ms |     29.9% | 0.230x | -77.0% |
| `depth10_liquidity_market`   | 192.918 ms |     13.0% |  18.106 ms |     16.0% | 0.094x | -90.6% |
| `alternating_market_small`   |  11.948 ms |     24.1% |   0.907 ms |     31.1% | 0.076x | -92.4% |
| `alternating_market_medium`  |  42.676 ms |     14.2% |   3.605 ms |     48.9% | 0.084x | -91.6% |
| `alternating_market_large`   | 179.753 ms |      7.7% |  12.921 ms |     16.2% | 0.072x | -92.8% |
| `accumulating_market_small`  |  23.002 ms |     11.1% |   6.552 ms |     18.8% | 0.285x | -71.5% |
| `accumulating_market_medium` |  86.222 ms |     10.6% |  50.856 ms |     11.4% | 0.590x | -41.0% |
| `accumulating_market_large`  | 363.737 ms |     14.9% | 596.268 ms |      7.0% | 1.639x | +63.9% |
| `passive_cancel`             |  18.658 ms |     10.1% |   4.898 ms |     12.1% | 0.263x | -73.7% |
| `gtd_expiry`                 |  19.632 ms |     14.6% |   4.976 ms |      9.0% | 0.253x | -74.7% |
| `order_type_trigger_sweep`   |  11.138 ms |     16.9% |   4.612 ms |      8.1% | 0.414x | -58.6% |

### Full load, setup, and run results

| Scenario                     | v1 median  | v1 spread | v2 median  | v2 spread | v2/v1  | v2 gap |
| ---------------------------- | ---------: | --------: | ---------: | --------: | -----: | -----: |
| `quote_trade_replay_small`   |   4.064 ms |     21.2% |   3.090 ms |     15.1% | 0.760x | -24.0% |
| `quote_trade_replay_medium`  |  26.984 ms |     13.2% |  12.023 ms |    272.9% | 0.446x | -55.4% |
| `quote_trade_replay_multi`   |  26.778 ms |     15.5% |  12.011 ms |      7.6% | 0.449x | -55.1% |
| `bar_last_replay`            |  31.159 ms |      5.1% |  10.085 ms |      9.0% | 0.324x | -67.6% |
| `bar_bid_ask_strategy`       |  47.413 ms |      9.7% |  16.262 ms |    124.5% | 0.343x | -65.7% |
| `l2_deltas_queue_passive`    |  23.576 ms |     16.3% |  10.802 ms |     16.9% | 0.458x | -54.2% |
| `depth10_liquidity_market`   | 391.817 ms |     13.0% | 121.995 ms |     13.1% | 0.311x | -68.9% |
| `alternating_market_small`   |  13.514 ms |     10.7% |   3.937 ms |     43.5% | 0.291x | -70.9% |
| `alternating_market_medium`  |  49.978 ms |      9.1% |   9.595 ms |     18.7% | 0.192x | -80.8% |
| `alternating_market_large`   | 210.688 ms |      7.4% |  31.740 ms |     18.7% | 0.151x | -84.9% |
| `accumulating_market_small`  |  25.806 ms |     18.9% |   9.449 ms |     17.6% | 0.366x | -63.4% |
| `accumulating_market_medium` |  94.710 ms |     11.4% |  56.087 ms |     15.0% | 0.592x | -40.8% |
| `accumulating_market_large`  | 393.466 ms |     10.9% | 596.190 ms |      6.1% | 1.515x | +51.5% |
| `passive_cancel`             |  35.270 ms |      7.7% |  15.667 ms |     18.0% | 0.444x | -55.6% |
| `gtd_expiry`                 |  36.109 ms |     11.6% |  15.364 ms |     11.9% | 0.425x | -57.5% |
| `order_type_trigger_sweep`   |  22.527 ms |     12.7% |  11.520 ms |     10.3% | 0.511x | -48.9% |

V2 was faster in 30 of the 32 boundary and scenario combinations. The two slower rows are the same
4,000-fill accumulating-position case. V2 was slower in every session for that case: the preloaded
gap ranged from 52.6% to 68.8%, and the full-path gap ranged from 47.3% to 60.5%. The 250-fill and
1,000-fill variants remained faster in v2, which places the observed crossover between 1,000 and
4,000 accumulated fills. Eight rows had a spread above 25.0%; the widest was 272.9% for the v2
full-path quote-and-trade medium case. Four samples ranged from 11.648 to 12.656 ms, while one was
44.461 ms. Its 0.446x median ratio is not a close comparison. The raw samples remain the appropriate
evidence for close comparisons.

### Accumulating-position profile

Both slower rows use the same 4,000-fill workload. A native Criterion mirror profiles the preloaded
engine path, and a separate Python profile covers fixture generation, engine construction, data
registration, `run()`, and the untimed fingerprint check. The native benchmark executable used the
standard `bench` profile with full debug information and had SHA-256
`c483f46b083b7fc2bba51e5ee0d4988dc66857b2ef75bb5ee0bf8043b6c42447`.

#### Preloaded native profile

```bash
gprofng collect app -p 1 -t 25s -F off -a usedldobjects \
    -O /tmp/nautilus-backtest-v2-accumulating-native-final.er \
    target/release/deps/engine-a1a9bcaf3d07bace \
    --bench --profile-time 15 \
    '^backtest_engine/position_history/accumulating_market_orders/4000$'

gprofng display text -functions \
    /tmp/nautilus-backtest-v2-accumulating-native-final.er
```

The experiment recorded 12.589 seconds of CPU samples over 12.609 seconds. It attributed 8.059
seconds exclusive, or 64.0%, to full `Position` cloning. `Cache::position_owned` accounted for
4.073 seconds inclusive, or 32.4%; `Cache::update_position` accounted for 4.494 seconds inclusive,
or 35.7%; and `ExecutionEngine::update_position` accounted for 5.943 seconds inclusive, or 47.2%.
`BacktestEngine::run` accounted for 11.781 seconds inclusive, or 93.6%.

GNU `gprofng` 2.42 warned that the collection interval changed from 1,000 microseconds to zero at
process exit and that the data may be unreliable. The near-complete CPU sample duration and the
concentrated call paths make the profile useful for attribution, but not for elapsed-time claims.
The same warning left a Python-hosted profile with negligible CPU coverage, so its percentages were
discarded.

#### Full-path Python profile

Python's built-in deterministic profiler separately measured one warmup and 20 fingerprinted
full-path iterations:

```bash
benchmark_profile_python=/tmp/nautilus-backtest-compare.FvurtH/env-v2-release-clean/bin/python
benchmark_profile_artifact=/tmp/nautilus-backtest-compare.FvurtH/wheels-v2-correct/nautilus_trader-2.0.0rc4-cp312-cp312-manylinux_2_39_x86_64.whl

"$benchmark_profile_python" scripts/benchmark-backtest-versions.py identity \
    --artifact "$benchmark_profile_artifact" \
    --source-commit 908c571caec0af086c1d1a8edbcf7bcbb07d6621 \
    --source-root . \
    --expected-version 2.0.0rc4 \
    --expected-backend pyo3 \
    > /tmp/nautilus-backtest-v2-profile-identity-final.json
benchmark_profile_identity_digest=$(
    "$benchmark_profile_python" -c \
        'import hashlib,json,sys; value=json.load(open(sys.argv[1])); payload=json.dumps(value,sort_keys=True,separators=(",", ":")).encode(); print("sha256:"+hashlib.sha256(payload).hexdigest())' \
        /tmp/nautilus-backtest-v2-profile-identity-final.json
)

"$benchmark_profile_python" -m cProfile \
    -o /tmp/nautilus-backtest-v2-accumulating-full-release-final.prof \
    scripts/benchmark-backtest-versions.py worker \
    --scenario accumulating_market_large \
    --boundary load_build_run \
    --iterations 20 \
    --expected-identity-digest "$benchmark_profile_identity_digest" \
    --artifact "$benchmark_profile_artifact" \
    --source-commit 908c571caec0af086c1d1a8edbcf7bcbb07d6621 \
    --source-root . \
    --expected-version 2.0.0rc4 \
    --expected-backend pyo3
```

The profile has SHA-256 `ebafc46de8b29bf83a5435e7844f5cb9acff942a62579978098a8befc2a5d638`.
The recorded identity digest resolved to
`sha256:e4cdc9e2d20be719289b6c9caa76fc9341c7cf8d857c5bfb9b07aa15bd350f40`. Every profile iteration
recorded the same full identity as the elapsed run and produced the same semantic fingerprint.
Across 21 runs including warmup, `BacktestEngine.run()` ranked first among timed workload operations
at 15.279 seconds cumulative. Fixture generation accounted for 0.597 seconds, and engine
construction and data registration accounted for 0.115 seconds. The fingerprint projection and
validation consumed 2.108 seconds after the elapsed interval. The complete identity proof consumed
16.519 seconds after the timed samples and is not an engine hotspot. Instrumentation overhead means
these durations are not comparable to the wall-clock table; their ranking shows that the full
boundary remains dominated by `run()`.

The wall-clock tables above remain the performance evidence. Together, the profiles identify
position ownership and cloning as the first target for a separate optimization pass; this change
does not modify production behavior.

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

The canonical matrix and the version comparison do not measure peak RSS, broad catalog or streaming
workloads, multiple venues, or pull-request gating. The `performance`-governor baseline still has
eight rows with spread above 25%. Establish lower-noise repeatability for those rows before using
these numbers as release or pull-request gates.
