# Backtest Engine Benchmarks

This document records the native canonical benchmark, its dated optimization decisions, the
v1.231.0 and v2 comparison, and the benchmark's CI role. Use the native sections to reproduce the
workload or evaluate another optimization under equivalent controls. Use the version comparison to
inspect the released Cython and Rust/PyO3 results and their raw evidence.

The latest native profile, collected on 2026-08-30 at commit `3dee76e70e4dc66cf705a90629924ec22e5cb3ab`,
found no safe production candidate above the 10.872622191% shared-host detection floor. The dated
records remain relevant because they define retained optimizations, rejected variants, and the
conditions required to reconsider them.

## Native canonical benchmark

### Workload

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

Each scenario runs under two input modes. The legacy mode submits one interleaved `Vec<Data>`
through `BacktestEngine::add_data`. The typed mode submits the same rows as a quote batch followed
by a bar batch through `BacktestEngine::add_data_batch`, so equal-timestamp order matches the legacy
stream. Typed cases append `_typed` to the scenario name, such as `replay_only_typed`; legacy cases
keep the bare scenario name so earlier baselines stay comparable. The correctness test requires both
modes to produce identical canonical results.

Before timing, the benchmark checks these counts, all 20,000 processed data events, execution
event counts, the canonical account digest, and the complete `CanonicalBacktestResult` digest.
The correctness test also requires the preloaded and freshly loaded paths to produce identical
canonical results. The complete result digest is exact within the active precision mode. Filled
workloads lock separate standard and `high-precision` digests because portfolio statistics
preserve exact `f64` bits; event counts, action counts, and account digests remain the same.

### Absolute baseline environment

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

### Measurement controls

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

Before changing production code, establish a fresh noise spread for the selected timed boundary.
Normalize its profiled cost to that boundary, then estimate the share the candidate can realistically
remove. Stop after profiling when that conservative estimate is at or below the spread. When it is
only marginally above, reduce noise or choose a larger target before implementing the candidate.
Reconsider a rejected candidate only after a material implementation, dependency, toolchain, or
representative-workload change, or after reducing the noise spread enough to resolve its prior result.

### Reproduce the matrix

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
The typed cases use the same commands with `_typed` appended to each scenario name.

For policy and the general noise-reduction recipe, see
[`BENCHMARKING.md`](../../../BENCHMARKING.md) at the repository root.

### 2026-08-10 absolute baseline

Lower is better. Each case processes 20,000 data events.

The table reports the median of three interleaved runs on the same host. Refresh the full matrix
after a substantive backtest-engine change or dependency upgrade. Focused comparisons do not
replace it. Absolute values vary by machine; treat a result inside its recorded noise band as
equivalent. This matrix predates the 2026-08-29 L1 replacement, which remeasured the replay-only
preloaded case.

| Scenario                | Preloaded `run()` median | Noise band, spread      | Full load/setup/run median | Noise band, spread      |
| ----------------------- | -----------------------: | ----------------------- | -------------------------: | ----------------------- |
| Replay only             |                23.285 ms | 22.398-23.641 ms, 5.3%  |                  30.141 ms | 30.136-30.289 ms, 0.5%  |
| Scheduled market orders |                25.920 ms | 25.377-28.271 ms, 11.2% |                  32.946 ms | 32.073-36.032 ms, 12.0% |
| Passive limit orders    |                39.318 ms | 38.887-43.839 ms, 12.6% |                  46.627 ms | 45.375-46.839 ms, 3.1%  |
| Bar EMA cross           |                37.326 ms | 37.103-41.650 ms, 12.2% |                  44.734 ms | 42.925-44.753 ms, 4.1%  |

### Initial profile

The host's `perf_event_paranoid=4` blocked `cargo flamegraph`; no kernel setting or privilege
changed. GNU `gprofng` 2.42 supplied unprivileged one-millisecond clock sampling. The original
collection used this command, with the measured executable identified by its SHA-256 above:

```bash
setarch "$(uname -m)" -R gprofng collect app -p 1 -t 25s -F off \
    -a usedldobjects -O /tmp/nautilus-backtest-bar-ema.er \
    target/bench-lto/deps/engine-9b29539648f11cbf \
    --bench --profile-time 15 \
    '^backtest_engine/canonical/run_preloaded/bar_ema_cross$'

gprofng display text -functions /tmp/nautilus-backtest-bar-ema.er
```

The executable name contains a Cargo-generated artifact hash. After rebuilding, substitute the new
`engine-*` path and use a new output directory.

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

## Optimization record

The 2026-08-29 and 2026-08-30 campaigns used profiles to select targets and elapsed-time
comparisons to make adoption decisions. Unless a campaign states otherwise, its comparison used
the Threadripper 9980X host above with Ubuntu 24.04.4 LTS and Linux 7.0.0-28-generic. All 128 logical
CPU governors remained `powersave`; ASLR was disabled per process, and the benchmark thread was not
pinned. No campaign changed a host control. Rust and Cargo 1.98.0 with LLVM 22.1.8 built each
measured executable once for reuse with standard precision, the default empty `nautilus-backtest`
feature set, and `bench-lto`. Campaigns that copied executables to immutable paths record their
SHA-256 and ELF build IDs below.

Each candidate comparison established a fresh baseline spread before candidate timing, alternated
baseline and candidate executables, and checked the exact canonical fingerprints. The campaign
section records stricter host-load acceptance rules, different sampling controls, or any missing
artifact identity. A profile share selects a target; it never establishes an elapsed-time change.

| Date       | Target or protocol                      | Fresh decision threshold | Result                                 | Decision               |
| ---------- | --------------------------------------- | -----------------------: | -------------------------------------- | ---------------------- |
| 2026-08-29 | Redundant L1 deletion                   |                  0.6843% | 20.32% median reduction                | Retained               |
| 2026-08-29 | L1 level and order-map allocation reuse |                    1.56% | 7.42% median reduction                 | Retained               |
| 2026-08-29 | Same-price L1 B-tree node reuse         |             7.178740975% | All pairs within the baseline spread   | Removed                |
| 2026-08-29 | Trade ID stack formatting               |             4.478442932% | 16.351% median reduction               | Retained               |
| 2026-08-30 | Same-ID L1 order-map value replacement  |             5.266290514% | 5.004521922% median reduction          | Removed                |
| 2026-08-30 | Empty command-queue fast path           |             6.326209262% | 4.712818562% estimated removable share | Not implemented        |
| 2026-08-30 | Shared-host measurement controls        |     50% spread reduction | Neither candidate protocol qualified   | Existing controls kept |
| 2026-08-30 | Pending queue snapshot                  |                   3.790% | 10.302% median reduction               | Retained               |
| 2026-08-30 | Current-HEAD target search              |            10.872622191% | No eligible target                     | No candidate           |
| 2026-09-04 | Typed batch input                       |    Per-row baseline band | Typed within legacy round bands        | Retained               |

### 2026-08-29: L1 replacement

This campaign profiled all four canonical scenarios under both exact benchmark boundaries. As
percentages of total captured CPU samples, inclusive time for
`process_trade_ticks_from_bar` ranged from 13.67% to 47.12%, and inclusive time for
`OrderBook::update_trade_tick` ranged from 7.78% to 29.75%. `OrderBook::update_book_bid` and
`update_book_ask` removed the current top order immediately before `BookLadder::add(order, 0)`
cleared the ladder and inserted the replacement.

The candidate removed that redundant deletion. The baseline was the signed commit
`Optimize BacktestEngine inactive service paths`. Three paired sessions alternated immutable
baseline and candidate executables for canonical replay-only `run_preloaded`. Each run used a
3-second warm-up, a 5-second measurement target, and 50 samples.

| Pair order         | Baseline median | Candidate median | Reduction |
| ------------------ | --------------: | ---------------: | --------: |
| Baseline/candidate |       23.548 ms |        18.823 ms |    20.07% |
| Candidate/baseline |       23.053 ms |        17.790 ms |    22.83% |
| Baseline/candidate |       23.219 ms |        18.501 ms |    20.32% |

The median reduction was 20.32%. A fresh three-run pre-edit replay-only session established the
0.6843% observed noise spread, which every comparison pair cleared. The canonical correctness test
matched every exact result fingerprint for all four scenarios under both boundaries after the
change. The remaining seven canonical cases were fingerprint-verified but not re-timed.

### 2026-08-29: L1 level reuse

The baseline was the signed `Optimize OrderBook L1 replacement` commit. Fresh one-millisecond
`gprofng` profiles covered all four scenarios under both benchmark boundaries. As percentages of
total captured CPU samples, inclusive samples for `BookLadder::add` remained material in every case:

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

Later test and documentation additions do not alter the library source compiled into the measured
candidate executable. The paired run found no concurrent Rust build and recorded 95% to 99% CPU
idle in its host samples.

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

### 2026-08-29: Rejected same-price L1 node reuse

The baseline was the signed `Optimize OrderBook L1 level reuse` commit
`f87112bd2e356ce621fe4b09bfd329c70adf6735`. Fresh one-millisecond `gprofng` profiles covered all
four canonical scenarios under both boundaries. Inclusive samples for
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

Accepted sessions recorded no concurrent Cargo work and 95% to 98% CPU idle. Runs with concurrent
Cargo work or CPU idle below the predeclared 95% floor were discarded.

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

### 2026-08-29: Trade ID formatting

The baseline was the signed `Document OrderBook L1 optimization rejection` commit
`30dbec8ea3b10796c333247d5214e3886758e53f`. Fresh one-millisecond `gprofng` profiles covered all
four canonical scenarios under both benchmark boundaries. As percentages of total captured CPU
samples, inclusive samples for
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

### 2026-08-30: Rejected L1 order-map value replacement

The baseline was the signed `Optimize IdsGenerator trade ID formatting` commit
`e67a439364f5ea2c08e836e85507374ceeb31157`. Fresh one-millisecond `gprofng` profiles covered all
four canonical scenarios under both benchmark boundaries. As percentages of total captured CPU
samples, inclusive samples for `IndexMap<u64, BookOrder>::insert_full` under
`BookLadder::replace_l1` were:

| Scenario                | Full load/setup/run | Preloaded `run()` |
| ----------------------- | ------------------: | ----------------: |
| Replay only             |               3.71% |             5.29% |
| Scheduled market orders |               3.06% |             3.66% |
| Passive limit orders    |               2.72% |             3.24% |
| Bar EMA cross           |               1.38% |             1.21% |

The insertion appeared beneath `BookLadder::replace_l1` in the call tree for the canonical quote-
and trade-tick paths. Profile mode included setup and fingerprint work, so these shares selected the
target but do not support the elapsed-time comparison. The candidate directly replaced the sole
stored order value when its ID matched the replacement ID, preserving the existing key and FIFO
slot. Different IDs and multi-order levels retained the prior clear-and-add path. Price replacement,
cache and batch reset, zero-size clearing, B-tree reinsertion, and public APIs remained unchanged.

| Item                 | Baseline                                                           | Candidate                                                          |
| -------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| Repository revision  | `e67a439364f5ea2c08e836e85507374ceeb31157`                         | Baseline revision plus the measured patch                          |
| Measured source tree | `ea1da17cb0d66fb911c035e8665ea871448f621c`                         | `e410781943a9c867c5786aff75aefe33a0cc10d4`                         |
| Executable SHA-256   | `acc072d1d9a2acd445992f68130903de6fc81680c03e02665581222076bfe9bc` | `d0eb66474ad835514d77493984b404dc5a8a744b0363ab1af3850784086590cb` |
| ELF build ID         | `a55d3682b2e6f094043a152c52cd5116195f0a08`                         | `0b8e55224e259f5210344cdf6fc50dc03e5d6462`                         |

Canonical replay-only `run_preloaded` was selected before viewing candidate timings because the
target had its largest profile share there, at 5.29%, and this boundary excludes setup from the
returned duration. Three fresh baseline sessions measured 14.343661167, 14.200984417, and
13.595796071 ms. Their 14.200984417 ms median and 5.266290514% full spread set the predeclared
threshold.

Three paired sessions alternated the immutable executables. Each run used an isolated Criterion
home, a 3-second warm-up, a 5-second measurement target, and 50 samples. Accepted sessions recorded
no competing Cargo or Rust compiler work and minimum CPU idle from 96.47% to 97.22%. Sessions with
competing Rust work or CPU idle below the predeclared 95% floor were discarded.

| Pair order         | Baseline median | Candidate median |     Reduction |
| ------------------ | --------------: | ---------------: | ------------: |
| Baseline/candidate | 14.705979833 ms |  14.134475857 ms |  3.886201278% |
| Candidate/baseline | 14.955419750 ms |  13.287750143 ms | 11.150938155% |
| Baseline/candidate | 15.101249333 ms |  14.345504000 ms |  5.004521922% |

The median reduction was 5.004521922%. The result is no measurable change because pairs one and
three did not clear the 5.266290514% baseline spread. The immutable candidate matched all eight
exact canonical fingerprints in a clean session with 95.79% to 97.89% CPU idle. Focused tests
covered the same-ID direct replacement, different-ID fallback, and multi-order fallback with exact
order, price, size, side, and cache assertions. The production candidate and its focused tests were
removed. All three focused cases, the canonical workload matrix, rustfmt, Clippy, and Rustdoc with
the `defi` feature passed. This benchmark record also passed markdownlint and table normalization.

Do not select this same-ID direct value replacement again under the same implementation, toolchain,
workload, and noise conditions. Reconsider it only after a material implementation, dependency,
toolchain, or representative-workload change, or after reducing baseline spread enough to resolve
the observed range.

### 2026-08-30: Command-queue drain profile

The baseline was the signed `Document OrderBook L1 value replacement rejection` commit
`466cb9e9036e207de33c83294dcf294c5a29f3a9`. Fresh one-millisecond `gprofng` profiles covered all
four canonical scenarios under both benchmark boundaries. Book-ladder work, its B-tree operations,
and order-map operations were excluded before target selection.

The largest remaining production lead was `BacktestEngine::drain_command_queues`. The table reports
its inclusive share of all captured CPU samples. Build-process observations count matching Cargo,
Rust compiler, Clippy, and Make process rows in the one-second host samples.

| Scenario                | Boundary         | CPU samples | Drain share | Average CPU idle | Build-process observations |
| ----------------------- | ---------------- | ----------: | ----------: | ---------------: | -------------------------: |
| Replay only             | `run_preloaded`  |    14.970 s |       6.75% |           95.74% |                          0 |
| Replay only             | `load_build_run` |    15.681 s |       5.17% |           97.07% |                          0 |
| Scheduled market orders | `run_preloaded`  |    15.352 s |       5.03% |           97.07% |                          0 |
| Scheduled market orders | `load_build_run` |    15.135 s |       4.08% |           96.85% |                          0 |
| Passive limit orders    | `run_preloaded`  |    15.590 s |       3.81% |           94.15% |                        102 |
| Passive limit orders    | `load_build_run` |    15.419 s |       3.07% |           91.65% |                        104 |
| Bar EMA cross           | `run_preloaded`  |    15.225 s |       1.81% |           82.88% |                        195 |
| Bar EMA cross           | `load_build_run` |    15.656 s |       1.65% |           95.01% |                         55 |

Profile mode includes setup and fingerprint work outside the elapsed `iter_custom` boundary, so
these percentages identify a target but do not support an elapsed-time claim. Every profile
completed the canonical fingerprint checks. The later profiles ran alongside unrelated builds;
`gprofng` sampled only the benchmark process, while the host observations above record the shared
load that can affect elapsed-time repeatability.

| Item                 | Baseline                                                           |
| -------------------- | ------------------------------------------------------------------ |
| Repository revision  | `466cb9e9036e207de33c83294dcf294c5a29f3a9`                         |
| Measured source tree | `53e7fff4bdb1b79ab8655d8c7344c77c84f45b3c`                         |
| Executable SHA-256   | `01395c683933c2ebdbc81c12297b82815f37c2e2436a91432a66cd3e8b46a909` |
| ELF build ID         | `9727c4118397d3b55eef571912620d76b6dffce9`                         |

Canonical replay-only `run_preloaded` was selected because command-queue draining had its largest
profile share there and this boundary excludes setup from the returned duration. Within the
repeated run path, the direct work in the trading and data command drains accounted for 0.826
seconds against 10.516 seconds inclusive in `BacktestEngine::run`, a 7.854697604% actionable upper
bound. An empty-queue fast path would retain the thread-local borrows, emptiness tests, execution
event drain, and settling checks. The predeclared realistic removal estimate was 60% of the upper
bound, or 4.712818562% end to end.

Three fresh baseline runs used isolated homes, a 3-second warm-up, a 5-second measurement target,
50 samples, and the same immutable executable. Normal shared-host work continued during every run.

| Run  | Baseline median | Average CPU idle | Build-process observations |
| ---: | --------------: | ---------------: | -------------------------: |
|    1 | 14.415631417 ms |           63.86% |                        319 |
|    2 | 15.378694250 ms |           59.64% |                        359 |
|    3 | 15.223379333 ms |           56.09% |                        251 |

The 15.223379333 ms median and 6.326209262% full spread set the admission threshold. The
4.712818562% realistic removal estimate did not clear that threshold, so no production candidate,
candidate executable, paired comparison, or candidate fingerprint run was created. This campaign
establishes change-detection limits under normal shared-host load, not quiet-host absolute timing.
Reconsider an empty command-queue fast path after reducing the fresh spread below its conservative
effect estimate, or select a larger eligible non-book cost.

### 2026-08-30: Shared-host repeatability calibration

This calibration compared replay-only measurement controls at signed commit
`c16fceaac8f82954645e52a66705313510e1781a` on a permanently shared host. It did not profile
another target or change production code, tests, or the benchmark harness.

The adoption rule was fixed before timing. A candidate protocol had to preserve the canonical
fingerprints and reduce the five-run full spread by at least 50% relative to the unpinned control.
Fixed affinity was preferred when it qualified because it retained the existing duration. Longer
sampling could replace qualifying fixed affinity only by halving its spread again; if fixed affinity
failed, longer sampling could still qualify by halving the control spread.

| Item                 | Value                                                               |
| -------------------- | ------------------------------------------------------------------- |
| Repository revision  | `c16fceaac8f82954645e52a66705313510e1781a`                          |
| Measured source tree | `a0a49e5e7070f8bb583527856e259bff86c5a393`                          |
| Executable SHA-256   | `4ec258a07cb061a71bce87ae7795ed882eecb2e0045f090ca7a792cf2a0b57fb`  |
| ELF build ID         | `968af998aa4aa2b2c5098251765b53eb267547ae`                          |
| Rust                 | `rustc 1.98.0`, LLVM 22.1.8                                         |
| Cargo                | `cargo 1.98.0`                                                      |
| Cargo features       | Default empty `nautilus-backtest` feature set; standard precision   |
| Profile              | `bench-lto`: release, fat LTO, one codegen unit, full debug symbols |

The clean source tree was built with the command in [Reproduce the matrix](#reproduce-the-matrix),
and the executable was copied to a read-only path. The source and copied executable hashes matched;
every run rechecked the executable hash before and after execution. All 15 runs used isolated
Criterion homes and canonical replay-only `run_preloaded`. The benchmark preflight verified all
four scenarios under both canonical boundaries before registering the filtered case, and every run
completed those exact fingerprint checks.

The host remained the Threadripper 9980X system described above with Ubuntu 24.04.4 LTS and Linux
7.0.0-28-generic. SMT and boost remained enabled, and the 128 logical CPUs retained the `powersave`
governor. No host control changed. Affinity used logical CPU 63, the highest-numbered first SMT
thread in the process's allowed `0-127` set. It maps to core 63 on socket 0 and NUMA node 0; logical
CPU 127 is its SMT sibling.

| Configuration                       | Affinity | Warm-up | Measurement | Samples |
| ----------------------------------- | -------- | ------: | ----------: | ------: |
| Existing control                    | Unpinned |     3 s |         5 s |      50 |
| Fixed affinity                      | CPU 63   |     3 s |         5 s |      50 |
| Fixed affinity plus longer sampling | CPU 63   |     3 s |        15 s |     100 |

The interleaved order by round was:

- `control/fixed/long`
- `fixed/long/control`
- `long/control/fixed`
- `control/long/fixed`
- `long/fixed/control`

Normal concurrent host work continued throughout. Average CPU idle and CPU 63 utilization come
from one-second `mpstat` samples over each full Criterion process. Build-process observations sum
matching Cargo, Rust compiler, Clippy, and Make process rows across one-second samples; no run was
rejected for host load or timing.

| Round/order | Configuration                       | Median run time | Average CPU idle | CPU 63 utilization | Build-process observations |
| ----------: | ----------------------------------- | --------------: | ---------------: | -----------------: | -------------------------: |
|         1/1 | Existing control                    | 15.281284250 ms |           87.02% |              1.18% |                        174 |
|         1/2 | Fixed affinity                      | 14.741385667 ms |           86.02% |            100.00% |                        132 |
|         1/3 | Fixed affinity plus longer sampling | 14.840072056 ms |           88.40% |            100.00% |                        266 |
|         2/1 | Fixed affinity                      | 14.789702917 ms |           87.18% |            100.00% |                        112 |
|         2/2 | Fixed affinity plus longer sampling | 14.746318111 ms |           89.07% |            100.00% |                        236 |
|         2/3 | Existing control                    | 15.264372500 ms |           90.55% |              2.01% |                        109 |
|         3/1 | Fixed affinity plus longer sampling | 15.266930222 ms |           92.77% |            100.00% |                        165 |
|         3/2 | Existing control                    | 14.932666667 ms |           91.21% |              2.30% |                        100 |
|         3/3 | Fixed affinity                      | 15.200084333 ms |           92.88% |             96.27% |                         71 |
|         4/1 | Existing control                    | 15.107950083 ms |           94.19% |              2.10% |                         51 |
|         4/2 | Fixed affinity plus longer sampling | 15.194157611 ms |           90.58% |            100.00% |                        220 |
|         4/3 | Fixed affinity                      | 15.242091583 ms |           88.26% |             95.81% |                        128 |
|         5/1 | Fixed affinity plus longer sampling | 15.004388833 ms |           91.78% |            100.00% |                        115 |
|         5/2 | Fixed affinity                      | 15.308534250 ms |           96.43% |            100.00% |                          4 |
|         5/3 | Existing control                    | 14.459969083 ms |           96.18% |              2.01% |                         10 |

Median range is the minimum to maximum of the five Criterion median point estimates. Full spread is
`(maximum - minimum) / median` across those estimates.

| Configuration                       | Median range                 | Median of medians | Full spread  | Spread reduction | Decision    |
| ----------------------------------- | ---------------------------- | ----------------: | -----------: | ---------------: | ----------- |
| Existing control                    | 14.459969083-15.281284250 ms |   15.107950083 ms | 5.436311095% |                - | Retain      |
| Fixed affinity                      | 14.741385667-15.308534250 ms |   15.200084333 ms | 3.731219978% |    31.364855458% | Not adopted |
| Fixed affinity plus longer sampling | 14.746318111-15.266930222 ms |   15.004388833 ms | 3.469732202% |    36.174877754% | Not adopted |

Neither candidate cleared the required 2.718155548% maximum spread. Retain the existing unpinned
3-second warm-up, 5-second measurement, and 50-sample controls on this permanently shared host.
Future optimization admission for this workload requires a conservative expected end-to-end effect
of at least twice the retained fresh spread, or 10.872622191%. This is the practical detection floor
until another calibration establishes a qualifying protocol.

### 2026-08-30: Pending queue snapshot

The baseline was the signed `Document OrderBook L1 value replacement rejection` commit
`1f7d5a12896f6f53cce917293cb574d5b58d255f`. A one-millisecond `gprofng` profile of canonical
passive limit orders ranked `OrderMatchingEngine::adjust_l1_queue_on_price_move` at 2.365 seconds
inclusive across a 15.289-second process. Its main `BacktestEngine::run` descendant accounted for
1.863 seconds within the function's 10.096-second run stack and included 0.990 seconds in
`IndexMap<ClientOrderId, PriceRaw>::get_index_of` and 0.612 seconds in `Cache::order`. Profile mode
included setup and fingerprint work, and samples could not distinguish every map operation, so these
figures selected the target but do not support the elapsed-time claim.

The candidate captures each pending client order ID with its stored price in a reusable scratch
vector before adjusting L1 queue positions. Iteration retains the `IndexMap` order and the same
captured price while avoiding the second lookup by client order ID. Side filtering, crossed and
equal-price transitions, closed-order and stale-entry cleanup, and pending snapshot promotion remain
unchanged. The scratch vector retains capacity for the peak pending-order count.

| Item                               | Baseline                                                           | Candidate                                                          |
| ---------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| Repository revision                | `1f7d5a12896f6f53cce917293cb574d5b58d255f`                         | Baseline revision plus the measured patch                          |
| Measured source tree               | `dbcfb4da69522cea9197dc0be059f3f9aa234057`                         | `39d520056c7f922639414657217064fa57172522`                         |
| Canonical executable SHA-256       | `d0930d5c6f11900edb49cb4e4b244d1dcbb7c07d2e7fd97fb58b80231d11412c` | `e2ec640cc5e5d2e79aaa30984955c6186ab39363ba5e0c6abf2a3dc78714f461` |
| Canonical ELF build ID             | `b37a4bf1ad9b6c800e26bcc00ebb4ad30f0262e2`                         | `19d3cf907eba11fb8d179b6648b52419c161bc49`                         |
| Matching-engine executable SHA-256 | `0d8784d3939b27da3918400399e0a9b11fa2a66aedcf79b8eb483b649cac67c0` | `be1e964c8017a80c7e60eb64e173fde339da51002778c42362d82c34d982eac0` |
| Matching-engine ELF build ID       | `b187b760a3b5e73269dfbc76010207912fdf47b1`                         | `9472e9a6489e67585e28c99b32e8982264977f54`                         |

Five fresh baseline sessions measured 24.635, 24.909, 25.066, 25.099, and 25.110 ms. Their
25.066 ms median and 1.895% full spread established the fresh noise bound. The adoption threshold
was fixed at twice that spread, or 3.790%, before candidate timing. Canonical passive limit-order
`run_preloaded` was selected because it exercised the profiled queue path and excluded setup from
the returned duration.

Three paired sessions alternated the immutable executables. Each run used an isolated Criterion
home, a 3-second warm-up, a 5-second measurement target, and 50 samples. The accepted pairs finished
before unrelated Cargo work appeared and recorded 96.47% average CPU idle.

| Pair order         | Baseline median | Candidate median | Reduction |
| ------------------ | --------------: | ---------------: | --------: |
| Baseline/candidate |       25.129 ms |        22.245 ms |   11.477% |
| Candidate/baseline |       24.769 ms |        22.566 ms |    8.894% |
| Baseline/candidate |       24.800 ms |        21.729 ms |   12.383% |

The median baseline and candidate results were 24.800 and 22.245 ms, a 10.302% reduction. Every
pair cleared the 3.790% adoption threshold, so the optimization was retained. The immutable
executables matched all eight exact canonical fingerprints.

Separate matching-engine measurements used the existing
`matching_engine/queue_position/quote_l1_pending/{1,32,256}` cases. The table reports the median of
three Criterion point estimates for each immutable executable.

| Pending orders | Baseline median | Candidate median | Reduction |
| -------------: | --------------: | ---------------: | --------: |
|              1 |       161.58 us |        170.36 us |    -5.43% |
|             32 |       607.23 us |        367.94 us |    39.41% |
|            256 |       4.1747 ms |        2.1106 ms |    49.44% |

The one-order result did not reproduce the end-to-end direction and is not used as evidence for the
optimization. The 32-order and 256-order cases show that the removed lookup scales with pending
queue size; the canonical passive workload supplies the adoption evidence. The canonical workload
matrix, focused L1 queue-position and queue-invariant tests, rustfmt, and Clippy passed after the
change. This documentation does not alter either measured executable.

### 2026-08-30: Current-HEAD profile

This profile covered the canonical matrix at signed commit
`3dee76e70e4dc66cf705a90629924ec22e5cb3ab`. The revision contains the retained pending-queue
optimization from parent `680e3c6a53` and the subsequent live-position snapshot persistence change.
The run built new executables from the source tree and did not reuse the parent's profiles,
binaries, or timings.

| Item                               | Value                                                              |
| ---------------------------------- | ------------------------------------------------------------------ |
| Repository revision                | `3dee76e70e4dc66cf705a90629924ec22e5cb3ab`                         |
| Measured source tree               | `e1e5d3a986578700ea928f674805224ce12c8dcd`                         |
| Canonical executable SHA-256       | `fd89586f7f42a4f0dea753762470733b0dc981ef1b94255d27c0099c4a1d4f81` |
| Canonical ELF build ID             | `f2bcddb09432240a71dac65a7743448edea2ba2e`                         |
| Matching-engine executable SHA-256 | `3e5b63e9345172e8194bc34381e444f9817f989f5a6a15d6a58753b66b9af10e` |
| Matching-engine ELF build ID       | `95edb939922685399a5ff94cac73f8b165caadd5`                         |

Both executables used Rust and Cargo 1.98.0, LLVM 22.1.8, standard precision, and the `bench-lto`
profile. Each executable was copied to a read-only path and its SHA-256 was checked before and after
use. The canonical build completed without another compiler or broad test running. The
matching-engine build overlapped unrelated sibling-checkout builds, so its build duration is
discarded. Timings below come only from direct execution of the copied immutable executable under
the declared shared-load regime. Direct execution listed the exact one-pending-order case before
measurement.

The 3.790% threshold in the preceding section came from twice the 1.895% spread of that quiet
canonical baseline session. It applies only to that retained comparison. The earlier shared-host
calibration set a 10.872622191% floor from twice its 5.436311095% spread. Repeated compiler and test
activity prevented a stable quiet window during this follow-up, so target admission used the
10.872622191% uncontrolled shared-load floor before any candidate timing. No result from this round
uses 3.790% as its adoption threshold.

`gprofng` collected a fresh 15-second profile for every canonical scenario and timing boundary from
the immutable current-HEAD executable. The first profile averaged 97.61% CPU idle without another
compiler or build process. The remaining seven used the declared shared-load regime, averaged
78.44%-91.00% idle, and recorded their concurrent build context. All eight runs preserved the
executable hash and passed the exact canonical fingerprints.

| Boundary         | Scenario                | Largest relevant profile result                              | Inclusive share | Decision                                                                                   |
| ---------------- | ----------------------- | ------------------------------------------------------------ | --------------: | ------------------------------------------------------------------------------------------ |
| `run_preloaded`  | Replay only             | `BookLadder::replace_l1`                                     |          36.65% | No safe small candidate; same-price node reuse and same-ID map replacement remain rejected |
| `load_build_run` | Replay only             | `BookLadder::replace_l1`                                     |          27.64% | Same rejected L1 replacement variants                                                      |
| `run_preloaded`  | Scheduled market orders | `BookLadder::replace_l1`                                     |          22.47% | Same rejected L1 replacement variants                                                      |
| `load_build_run` | Scheduled market orders | `BookLadder::replace_l1`                                     |          18.93% | Same rejected L1 replacement variants                                                      |
| `run_preloaded`  | Passive limit orders    | `BookLadder::replace_l1`; pending-queue adjustment was 7.24% |          18.24% | L1 variants rejected; pending-queue path already retained and below the floor              |
| `load_build_run` | Passive limit orders    | `BookLadder::replace_l1`; pending-queue adjustment was 6.07% |          16.10% | L1 variants rejected; pending-queue path already retained and below the floor              |
| `run_preloaded`  | Bar EMA cross           | Canonical-result array normalization                         |          17.97% | Outside the elapsed timer; largest production result was below the floor                   |
| `load_build_run` | Bar EMA cross           | Canonical-result array normalization                         |          16.35% | Outside the elapsed timer; largest production result was below the floor                   |

The fresh load-path `parse_price` share reached 8.70%, `IdsGenerator::generate_trade_id` reached
6.96%, and command-queue draining remained below the shared-load floor. The ID generator is already
optimized, and the command-queue empty fast path remains unchanged. Link-time optimization folded
identical standard-hash monomorphizations under one displayed symbol, including calls from distinct
order and book maps, so those samples do not identify one source-level target. No different
production bottleneck had a conservative removable share comfortably above 10.872622191%. This
round therefore stops without a production candidate or paired candidate timing.

The retained pending-queue microbenchmark needed a repeatability bound before its earlier -5.43%
one-order point estimate could be classified. Five fresh current-HEAD sessions ran the existing
`matching_engine/queue_position/quote_l1_pending/1` case from the immutable matching-engine
executable. Each session used a separate Criterion result directory, a 3-second warm-up, a 5-second
measurement target, 50 samples, and per-process ASLR disablement. The shared host averaged 60.07%
CPU idle during these sessions.

| Session | Criterion point estimate |
| ------: | -----------------------: |
|       1 |               175.885 us |
|       2 |               174.181 us |
|       3 |               178.234 us |
|       4 |               183.288 us |
|       5 |               178.874 us |

The 178.234 us median and 174.181-183.288 us range give a 5.109% full spread. Twice that spread is
10.218%, still below the existing 10.873% shared-load admission floor. The prior -5.43% direction is
within practical repeatability noise and does not establish a one-order regression. It also does not
prove that a smaller effect is absent. The 32-order and 256-order scaling results remain explanatory;
the retained canonical end-to-end comparison remains the adoption evidence.

### 2026-09-04: Typed batch input

The baseline was the `Stream backtest data lazily across multiple data configs` commit. The
candidate adds `BacktestEngine::add_data_batch`, which stores a homogeneous `DataBatch` in the
replay iterator without constructing per-item `Data` values, and routes `add_data` through the same
validation and bookkeeping. This record establishes the first typed rows of the canonical matrix;
the earlier dated records predate them.

| Item                 | Baseline                                                           | Candidate                                                          |
| -------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| Repository revision  | `ec1894d6fab4fb3768caadbcd6ab6956e0568f13`                         | Baseline revision plus the measured patch                          |
| Measured source tree | `c4870b8b94356ab080c23f8b64e0cb75f53c85e0`                         | `1635c1b51a7dac1ae93810bce550273bb8b938fd`                         |
| Executable SHA-256   | `f4607a58bb7966167bb163e0de3bd1b78f22e4ad817f5acd31c9a401b13435f5` | `b1f2e5dc7cea1b562c599ac3e6abbc8f1c3cb77b585fe00a80761e50848291a1` |
| ELF build ID         | `9dc0cb2b64f08a95cbe0568cddcd4270f9719747`                         | `45fee22becea1c54db8108afce7301eddfeac069`                         |

Documentation edits after the build do not alter the measured candidate executable. Three rounds
followed the rotated scenario order from the reproduce loop. Within each scenario, the baseline
legacy case, the candidate legacy case, and the candidate typed case ran back to back, each with a
3-second warm-up, a 5-second measurement target, 50 samples, and an isolated Criterion home. Each
row reports the median of the three round medians, and a band is the range of those medians. No
Cargo work ran during the session.

The first comparison checks the legacy `add_data` path after its bookkeeping moved into a shared
function. Every candidate median falls inside or below its baseline band except bar EMA cross under
the full boundary, where the candidate rounds overlap the baseline rounds. The path did not regress.

| Scenario                | Boundary            | Baseline median | Baseline band, spread  | Candidate median | Change |
| ----------------------- | ------------------- | --------------: | ---------------------- | ---------------: | -----: |
| Replay only             | Preloaded `run()`   |       14.761 ms | 14.664-15.214 ms, 3.7% |        14.631 ms | -0.88% |
| Scheduled market orders | Preloaded `run()`   |       16.362 ms | 15.553-16.899 ms, 8.2% |        16.111 ms | -1.53% |
| Passive limit orders    | Preloaded `run()`   |       22.097 ms | 21.913-22.437 ms, 2.4% |        22.127 ms | +0.14% |
| Bar EMA cross           | Preloaded `run()`   |       21.956 ms | 21.857-22.378 ms, 2.4% |        22.040 ms | +0.38% |
| Replay only             | Full load/setup/run |       21.333 ms | 21.099-21.851 ms, 3.5% |        21.166 ms | -0.78% |
| Scheduled market orders | Full load/setup/run |       23.195 ms | 22.501-23.748 ms, 5.4% |        22.893 ms | -1.30% |
| Passive limit orders    | Full load/setup/run |       29.054 ms | 28.567-29.078 ms, 1.8% |        28.470 ms | -2.01% |
| Bar EMA cross           | Full load/setup/run |       29.117 ms | 28.845-29.188 ms, 1.2% |        29.282 ms | +0.57% |

The second comparison runs typed and legacy input on the candidate executable. Both modes matched
every exact canonical fingerprint, and the correctness test found no divergence between their
canonical results. Every typed row's round medians overlap the legacy row's round medians. The typed
mode carries a small structural cost on this fixture: it merges a quote stream and a bar stream
through the replay heap, while the legacy mode replays one mixed stream on the single-stream path.

| Scenario                | Boundary            | Legacy median | Typed median | Typed band, spread      | Change |
| ----------------------- | ------------------- | ------------: | -----------: | ----------------------- | -----: |
| Replay only             | Preloaded `run()`   |     14.631 ms |    15.061 ms | 14.875-15.440 ms, 3.8%  | +2.94% |
| Scheduled market orders | Preloaded `run()`   |     16.111 ms |    16.072 ms | 16.000-16.521 ms, 3.2%  | -0.24% |
| Passive limit orders    | Preloaded `run()`   |     22.127 ms |    21.962 ms | 21.959-22.537 ms, 2.6%  | -0.74% |
| Bar EMA cross           | Preloaded `run()`   |     22.040 ms |    22.344 ms | 22.242-22.853 ms, 2.7%  | +1.38% |
| Replay only             | Full load/setup/run |     21.166 ms |    21.406 ms | 21.386-21.777 ms, 1.8%  | +1.13% |
| Scheduled market orders | Full load/setup/run |     22.893 ms |    23.838 ms | 22.705-25.178 ms, 10.4% | +4.13% |
| Passive limit orders    | Full load/setup/run |     28.470 ms |    29.310 ms | 28.354-29.526 ms, 4.0%  | +2.95% |
| Bar EMA cross           | Full load/setup/run |     29.282 ms |    29.033 ms | 29.032-29.579 ms, 1.9%  | -0.85% |

The typed path was retained. Its benefit on this fixture is loaded-data memory rather than replay
time: the canonical workload holds 20,000 items, so the representation difference is small against
the run itself.

#### Representation size and loaded-data memory

`Data` stores every item at the size of its largest inline variant, while a typed batch stores each
item at its own size. The sizes below are `size_of` results for the standard-precision build on this
host.

| Type                | Bytes | Storage in `Data` |
| ------------------- | ----: | ----------------- |
| `Data`              |   168 | Enum              |
| `DataRef`           |    16 | Borrowed view     |
| `QuoteTick`         |    96 | Inline            |
| `TradeTick`         |   112 | Inline            |
| `Bar`               |   160 | Inline            |
| `OrderBookDelta`    |    96 | Inline            |
| `OrderBookDeltas`   |    72 | Boxed             |
| `OrderBookDepth10`  | 1,088 | Boxed             |
| `MarkPriceUpdate`   |    48 | Inline            |
| `IndexPriceUpdate`  |    48 | Inline            |
| `FundingRateUpdate` |    72 | Inline            |
| `OptionGreeks`      |   160 | Inline            |
| `InstrumentStatus`  |    64 | Inline            |
| `InstrumentClose`   |    56 | Inline            |

Loaded-data memory was measured with a temporary program that built the same items as a
`Vec<Data>` and as a `BatchView<T>` in separate processes, then read the process high-water mark
(`VmHWM`) before and after construction. GNU `time` maximum resident set sizes agreed within 1.4%.
Each row is the median of three runs.

| Payload            |     Items | `Vec<Data>` | `BatchView<T>` | Bytes per item | Reduction |
| ------------------ | --------: | ----------: | -------------: | -------------- | --------: |
| `QuoteTick`        | 1,000,000 |   166.0 MiB |       97.4 MiB | 174 to 102     |     41.4% |
| `OrderBookDepth10` |   200,000 |   248.4 MiB |      213.3 MiB | 1,303 to 1,119 |     14.1% |

The quote reduction matches the 72-byte difference between `Data` and `QuoteTick`. The depth
reduction matches the 168-byte enum plus the allocator header that `Data::BookDepth10` spends on
each boxed snapshot.

## v1.231.0 and v2 comparison

The 2026-08-27 comparison used the Python
[`benchmark-backtest-versions.py`](../../../scripts/benchmark-backtest-versions.py) driver to run
the released v1.231.0 Cython engine and the v2 Rust/PyO3 engine at revision
`908c571caec0af086c1d1a8edbcf7bcbb07d6621`. The raw
[`v1-v2-results.json`](v1-v2-results.json) record contains every elapsed sample, the full workload
matrix, full runtime identities once per version, per-sample identity digests, host state,
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

The measured driver SHA-256 was
`b3636a7b1ff8a121580495c7c8da2eb61f7ae8d3ea95b5c49c9589f2aa5818a9`. The checked-in result file
still has SHA-256 `4ff2ae000c93333f70d0769f90ce0323710dd7ac7b389e7f21e28cdd183fac45`.

### Measurement method

The matrix covers 16 cases across quote and trade replay, bars, L2 deltas, depth snapshots,
multiple instruments and streams, market and passive orders, cancellation, GTD expiry, and order
type triggers. `run_preloaded` times only `BacktestEngine.run()` after setup and data loading.
`load_build_run` includes fixture generation, engine construction, data registration, and `run()`.

Each of five interleaved sessions gave every case one warmup and one timed sample. Sessions
alternated v1/v2 execution order and rotated or reversed case order. Before the matrix, the driver
checked the complete wheel, extension, source, and runtime identity. Source identity included the
revision and content hashes for staged diffs, unstaged diffs, and untracked files. After every timed
sample, the worker repeated that proof, checked its canonical digest against the initial identity,
and verified the complete event, order, position, and account fingerprint. All 320 timed iterations
matched across versions.

The tables report the median of five session samples. Spread is
`(maximum - minimum) / median`; rows with a wide spread need more samples before they support a
close comparison.

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

V2 was faster in 30 of the 32 boundary and scenario combinations. Both slower rows are the same
4,000-fill accumulating-position case. V2 was slower in every session for that case: the preloaded
gap ranged from 52.6% to 68.8%, and the full-path gap ranged from 47.3% to 60.5%. The 250-fill and
1,000-fill variants remained faster in v2, which places the observed crossover between 1,000 and
4,000 accumulated fills. Eight rows had a spread above 25.0%; the widest was 272.9% for the v2
full-path quote-and-trade medium case. Four samples ranged from 11.648 to 12.656 ms, while one was
44.461 ms. Its 0.446x median ratio does not support a close comparison; use the raw samples for one.

### Accumulating-position profile

Both slower rows use the same 4,000-fill workload. A native Criterion mirror profiled the preloaded
engine path. A separate Python profile covered fixture generation, engine construction, data
registration, `run()`, and the untimed fingerprint check. The native benchmark executable used the
standard `bench` profile with full debug information and had SHA-256
`c483f46b083b7fc2bba51e5ee0d4988dc66857b2ef75bb5ee0bf8043b6c42447`.

#### Preloaded native profile

GNU `gprofng` 2.42 collected one-millisecond samples from
`backtest_engine/position_history/accumulating_market_orders/4000`, with a 25-second collection
ceiling, a 15-second Criterion profile interval, descendant following disabled, and loaded-object
metadata enabled. The experiment recorded 12.589 seconds of CPU samples over 12.609 seconds. It
attributed 8.059 seconds exclusive, or 64.0%, to full `Position` cloning. `Cache::position_owned`
accounted for 4.073 seconds inclusive, or 32.4%. `Cache::update_position` accounted for 4.494
seconds inclusive, or 35.7%. `ExecutionEngine::update_position` accounted for 5.943 seconds
inclusive, or 47.2%.
`BacktestEngine::run` accounted for 11.781 seconds inclusive, or 93.6%.

GNU `gprofng` 2.42 warned that the collection interval changed from 1,000 microseconds to zero at
process exit and that the data may be unreliable. The near-complete CPU sample duration and the
concentrated call paths make the profile useful for attribution, but not for elapsed-time claims.
The same warning left a Python-hosted profile with negligible CPU coverage, so its percentages were
discarded.

#### Full-path Python profile

Python's deterministic profiler measured one warmup and 20 fingerprinted `load_build_run`
iterations of `accumulating_market_large`. The driver first proved the v2 wheel, source commit,
version, PyO3 backend, and runtime identity, then required that identity digest for the profiled
worker. The profile has SHA-256
`ebafc46de8b29bf83a5435e7844f5cb9acff942a62579978098a8befc2a5d638`. The identity digest was
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
position ownership and cloning as the first target for a separate optimization pass. No production
behavior changed during this profiling pass.

## CI policy

Keep the canonical Rust workloads in the existing nightly Criterion run. The
[pinned `codspeed-criterion-compat` 5.0.1 limitations](https://github.com/CodSpeedHQ/codspeed-rust/blob/v5.0.1/crates/criterion_compat/README.md#not-supported)
list `iter_custom` as unsupported, but this matrix needs `iter_custom` to return only the preloaded
`run()` duration while constructing a fresh engine for every iteration. CodSpeed's
[instrument documentation](https://codspeed.io/docs/instruments) also distinguishes its
single-run, hardware-agnostic simulation from wall-clock measurements on CodSpeed-managed
bare-metal runners. Neither mode reproduces this fixed-machine absolute baseline.

`nautilus-backtest` and its registered `engine` benchmark already run through the nightly
Criterion workflow, so no registration change is required. Nightly uses the default non-LTO
`bench` profile with full debug information; its timings are not comparable to the fixed-host
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
