# Position Replay Comparison

This offline comparison measures synthetic fills through `ExecutionEngine`, not exchange traffic or
real accounts. Read the [benchmark policy](../../BENCHMARKING.md) before interpreting local results.

## Sources and controls

- Baseline: `develop` at `72e6527c73464b55b3bc84cba39be4bf9618a82b`, without the replay index.
- Candidate: published head `ad076509d5f0e58e16b5cfb19ba7c85db9c9759a`, merged with that baseline,
  plus the reviewed allocation-free duplicate lookup for a closed NETTING cycle being discarded.
  This report identifies an uncommitted measured candidate by source hashes, not a new public commit.
- Both variants use identical harness sources and lockfile, Rust 1.100.0, high precision, native
  `bench-lto` (fat LTO, one codegen unit), macOS 27.0.1/arm64 and Redis 8.6.2.
- Each variant was built into its own initially empty target directory, using at most two jobs at
  nice 15. Builds and measurements were not concurrent; measurements were serial at nice 15.
- Cargo JSON artifact manifest/source/executable paths, pre/post-build source hashes, saved binary
  hashes and each executable's implementation identity were checked. Head and baseline hashes differ.
  All 482 fixtures emitted the expected identity: indexed Position 848 bytes, baseline 832 bytes.
- Each of the 304 Redis fixtures used a new owned server: 304 distinct run IDs, initial `DBSIZE=0`,
  and successful server termination. Memory fixtures did not start Redis.

[Source and binary identities](position_replay_comparison.provenance.json),
[all 148 valid performance groups](position_replay_comparison.csv),
[allocation observations](position_replay_comparison.allocations.json), and
[all recorded failures](position_replay_comparison.failures.json) accompany this report.

The runner was reformatted after measurement. The measured source was preserved and verified against
its recorded SHA-256; its Python AST is identical to the formatted version. Both hashes are recorded
in the provenance file. No measurement is attributed to a semantically different runner.

Earlier shared-Redis results are superseded because server data accumulated between fixtures.
A subsequent run was rejected when final identity checks found identical binaries under the two
variant labels. Its samples were preserved as invalid evidence and are not included here. The
independent-target run above replaces it.

## Measurement boundaries

The timer covers only synchronous `ExecutionEngine::process`, including Redis adapter submission.
Redis startup, readiness checks, fixture construction/seeding, order preparation, correctness checks,
and output formatting are outside it. Writer disposal/drain and reload have separate CSV operations.
Submission latency is not a Redis acknowledgement, durable-write latency, throughput or P99 result.

`cold` means that the canonical cached position has not queried its replay lookup before the first
fill. `warm` explicitly calls `has_replay_trade_id(MISSING)` and `requires_replay_state` before timing.
The baseline performs the same queries but has no index. Neither label promises a cold/warm CPU cache.
Later calls inherit the state left by prior engine and adapter operations; the index is not reset.
Duplicate checks serialize a before-state outside timing. Unique reopen measurements do not.

Continuing-fill and flip fixtures seed a closed/reopened carried-history position before timing.
At history 36,000, continuing fixtures process 20 fills and flip fixtures five flips; small continuing
fixtures process 100 fills. The new reopen fixtures instead seed a genuinely closed cycle of `n`
opening fills plus one closing fill, then measure exactly one reopen. Both carry and discard modes
check unique fills and earliest, middle and closing-trade duplicates, cold and warm. Duplicate cases
must preserve the complete closed position and snapshot count.

Performance entries are medians of three round medians with alternating variant order. For reopen,
each round contains one observation: only three observations per variant, not a latency distribution.
CPU frequency, thermal state, ASLR and unrelated desktop activity were not controlled. Ranges in the
CSV are descriptive round minima/maxima, not confidence intervals or statistical significance.

## Results

The following are synchronous process times, baseline → candidate. Redis rows use fresh instances.

| Scenario                                       | Baseline      | Candidate     |
| ---------------------------------------------- | ------------: | ------------: |
| Nonvirtual, 36,000, cold first continuing fill | 2.610208 ms   | 2.050917 ms   |
| Nonvirtual, 36,000, warm later continuing fill | 1.601042 ms   | 0.009416 ms   |
| Nonvirtual, 8, warm later continuing fill      | 12.041 µs     | 9.208 µs      |
| Nonvirtual, 36,000, warm later flip            | 6.896563 ms   | 7.821646 ms   |
| Nonvirtual, 8, warm later flip                 | 34.625 µs     | 36.063 µs     |
| Virtual, 36,000, warm later continuing fill    | 105.738541 ms | 106.377250 ms |
| Virtual, 36,000, warm later flip               | 214.166542 ms | 217.392959 ms |
| Memory, 36,000, cold discard reopen            | 7.090333 ms   | 6.214625 ms   |
| Memory, 36,000, warm discard reopen            | 4.356291 ms   | 6.270500 ms   |
| Nonvirtual Redis, 36,000, cold discard reopen  | 30.205083 ms  | 32.585541 ms  |
| Nonvirtual Redis, 36,000, warm discard reopen  | 28.291541 ms  | 28.826625 ms  |

Warm continuing fills show the clear lookup benefit in this run. Flips, virtual persistence and
reopen do not show a uniform improvement. In particular, the long warm-flip median increased 13.4%
and the memory warm-discard median increased 43.9%; their round ranges overlap. Long cold Redis
reopen increased 7.9% with overlapping ranges. The small warm-flip baseline has a large outlier
(round medians 31.792-1,033.562 µs versus 33.896-37.646 µs), so the median alone hides variability.
Do not summarize these results as an across-the-board speedup or proof of no regressions.

## Allocation observations

The model-only heap probe reports requested live bytes. Inline Position size changes from 832 to
848 bytes, independently of heap allocations. At 36,000 historical fills, owned clone heap is
16,129,508 bytes on both variants and a lightweight snapshot allocates 1,956 bytes on both.
The candidate's first lookup after cloning adds 3,211,344 heap bytes; the baseline adds zero.
That heap delta does not include the separate 16-byte inline increase. Candidate first-query heap
costs at histories 8, 64 and 4,096 are 864, 6,352 and 401,488 bytes respectively.

A separate executable instruments whole-engine synchronous memory-backend processing. The ordinary
timing executable has no counting allocator. At 36,000 fills, cold unique reopen observations are:

| Mode and metric                          | Baseline bytes | Candidate bytes |
| ---------------------------------------- | -------------: | --------------: |
| Discard: total requested allocations     | 37,397,174     | 37,397,238      |
| Discard: extra peak live requested bytes | 37,383,604     | 37,383,668      |
| Discard: retained live-byte change       | -16,120,518    | -16,120,454     |
| Carry: total requested allocations       | 69,654,070     | 76,076,658      |
| Carry: extra peak live requested bytes   | 37,383,604     | 40,595,012      |
| Carry: retained live-byte change         | 16,135,930     | 19,347,338      |

The discard path is not allocation-free: it still requests about 37.4 MB across the whole engine
call. The +64-byte difference does not show a history-sized extra index allocation. This comparison
is against unindexed develop, not the old indexed reopen implementation, so it does not quantify
bytes saved by the reviewed fix. The specific duplicate-check branch avoids initializing the index;
that narrower property must not be confused with the allocation behavior of the entire reopen.

Requested allocations count the requested new size of each successful allocation/reallocation.
Peak is the extra net live requested-byte high-water mark relative to call entry; it is not physical
allocator peak or RSS, and cannot observe temporary old/new blocks inside `System::realloc`.
Retained change can be negative when old history is dropped. These process-global counters are used
only in separate synchronous memory fixtures, not Redis workers or production workloads.

## Correctness and recorded failures

The complete run has 482 fixtures: 442 passed and 40 recorded failures. The preliminary smoke has
154 fixtures: 146 passed and eight recorded failures. A completed runner is not an all-tests-pass claim.

| Category                              | Passed | Failed |
| ------------------------------------- | -----: | -----: |
| Original full-state/duplicate checks  | 40     | 8      |
| Original continuing/flip measurements | 144    | 0      |
| Closed-cycle full-state checks        | 120    | 8      |
| Closed-cycle measurements             | 72     | 24     |
| Separate engine allocation probes     | 64     | 0      |
| Model heap probes                     | 2      | 0      |

The original eight failures reproduce historical-duplicate/nonvirtual Redis reload errors on both
variants, with exit 2 and the exact `T-FLIP-2` duplicate diagnostic. They are failures, not passes.

The other 32 failures are plain-fill nonvirtual `reopen-carry`: eight correctness fixtures and 24
measurement fixtures. Both identified variants preserve old replay history in memory but reload
only the new cycle from Redis. Economic and every non-history serialized field agree; complete
history does not (`n + 2` expected versus one restored event). The full-state assertion is retained.
The runner only recognizes this exact fixture, abort signal, assertion and complete diagnostic line;
unexpected exits or differences stop the run. All 24 failed measurement fixtures are excluded from
the normal performance summary, with their raw evidence retained separately.

`Position::requires_replay_state` requests full persistence for corrections, settlements, price
restatements and retained virtual cycles, but not ordinary carried nonvirtual fills. Existing reopen
persistence coverage includes a prior fill void and exercises a different path. The independently
built baseline reproduces the same limitation, so this observation is not evidence of a new index
regression. It remains a recovery gap for follow-up; this change does not alter persistence semantics.

## Reproduce safely

Use native Redis and Python 3.10+ on macOS/Linux, or Linux inside WSL. The runner requires POSIX
priority support and lowers itself and children to nice 15. It accepts no existing Redis endpoint,
never flushes another database, checks its own child PID/run ID/empty database, and reaps that child
on success, failure, timeout or SIGTERM. Do not invoke the examples against an application database.

Use clean comparison worktrees at the revisions above and copy the identical `replay_latest_*.rs`
examples plus `common/replay_comparison.rs` into both. Register all examples with `doc = false`;
comparison and allocation examples require `required-features = ["redis"]`. The harness additions
are test-only. Preserve the lockfile and unmodified profile.

Use distinct absolute `TARGET_BASELINE` and `TARGET_INDEXED` directories. Do not share compiled
outputs between variants. Reuse `CARGO_HOME` for downloaded dependencies; never clean existing user
caches or overwrite investigation worktrees. Build sequentially, then copy only the exact artifacts
reported by Cargo JSON into a new binary directory:

```bash
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$TARGET_INDEXED" \
    nice -n 15 cargo +1.100.0 build --locked --offline --profile bench-lto \
    -p nautilus-infrastructure --features redis,nautilus-model/high-precision \
    --example replay_latest_comparison --example replay_latest_memory \
    --example replay_latest_allocations --message-format=json > indexed-artifacts.jsonl
```

Repeat from the baseline checkout with `TARGET_BASELINE`. Verify artifact manifest/source paths,
source hashes before and after compilation, copied hashes, and each executable's `BUILD_IDENTITY`
against its variant. On this host the expected Position sizes are 832 and 848; additionally verify
that baseline model-query allocation is zero and indexed allocation is positive. The three pairs
of binaries must not have identical hashes. A Cargo `fresh` label alone does not establish identity.

Save copies as `baseline-engine`, `baseline-memory`, `baseline-allocations` and corresponding `head-`
names. Use fresh output directories and do not compile while sampling:

```bash
python3 scripts/benchmark-position-replay.py --binary-dir "$BIN" \
    --baseline-allocations "$BIN/baseline-allocations" \
    --head-allocations "$BIN/head-allocations" --output-dir "$SMOKE_OUTPUT" --smoke
python3 scripts/benchmark-position-replay.py --binary-dir "$BIN" \
    --baseline-allocations "$BIN/baseline-allocations" \
    --head-allocations "$BIN/head-allocations" --output-dir "$MEASUREMENT_OUTPUT"
```

Review `run-results.json`, per-fixture Redis metadata and raw stdout/stderr, including nonzero exits,
before aggregating. Never merge failed-recovery timings or earlier invalid experiments into valid
performance groups. This report is local descriptive evidence, not a controlled performance-host baseline.
