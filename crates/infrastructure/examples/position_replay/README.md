# Position Replay Examples

Run these offline examples to exercise position replay.
The [#5551 benchmark report][report] records the measured revisions, reproduction steps,
results, allocation observations, and recovery failures.
See the [earlier harnesses][earlier] for previous comparisons and their validation notes.

Run Redis cases only against a disposable loopback instance with persistence disabled.
The examples connect to `127.0.0.1` and require a port of at least 20000. Verify isolation
before running an example directly. `scripts/benchmark-position-replay.py` starts and verifies
a fresh, empty instance for each Redis fixture.

Build from the repository root with the pinned toolchain:

```bash
CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 cargo build --locked --profile nextest \
  -p nautilus-infrastructure --examples --features nautilus-model/high-precision
```

Use the `nextest` profile for correctness checks. Add `--offline` when all dependencies are cached.

## Recovery

```bash
./target/nextest/examples/replay_recovery DISPOSABLE_REDIS_PORT
```

This checks a real price restatement after warming the original index, then close/reopen,
Redis adapter add/load, a further fill, update, and a second load. Both virtual and nonvirtual
IDs use replay snapshots because of the correction. Redis itself stays running; this does not
check service restart durability, non-snapshot append recovery, or execution-engine routing.
The related codec regression runs with:

```bash
CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 cargo test --locked --profile nextest \
  -p nautilus-model --test position_replay_recovery --features high-precision,test-support
```

## Repeated engine flips

```bash
./target/nextest/examples/replay_latest_comparison 8 memory 0 cold duplicate 3 full flip
./target/nextest/examples/replay_latest_comparison 36000 memory 0 warm duplicate 3 full flip
```

Pass the history size, backend (`memory`, `redis-nonvirtual`, or `redis-virtual`), port (zero for
memory), index state (`cold` or `warm`), first candidate, count, validation level, and optional mode.
The default mode is `flip`. It accepts `unique`, `unique-only`, or `duplicate` as the first
candidate. The `continue` mode requires `unique-only`. The `reopen-carry` and `reopen-discard`
modes also accept `middle` and `latest` and require a count of one.

Use `full` for correctness checks, including serialized position-state comparisons, or
`economics` for performance runs. Timings cover synchronous `ExecutionEngine::process`, including
adapter submission. Measure Redis drain and reload separately. A historical duplicate submitted
through a fresh order checks the position state, and `unique-only` skips duplicate probes.

The historical-duplicate flip originally failed with exit code 2 on nonvirtual Redis. The full flip
fixture now checks that no-op fills do not emit position events or snapshots and that direct and
bulk reload agree. It does not repair databases already containing duplicate entries. The distinct
plain-fill `reopen-carry` case still reports `RELOAD_HISTORY_MISMATCH` because only the new cycle
reloads; this change does not alter that behavior.

## Performance comparison

Use `bench-lto` and a separate `CARGO_TARGET_DIR` for each revision. For separate timing-only
invocations of the measurement workloads, use identical fixture sources in both checkouts. If
copying this directory and the three `replay_latest_*` manifest entries for those invocations,
preserve the target paths and feature requirements.

The combined runner has no timing-only mode: it performs correctness checks before timing. By
default both binaries must pass duplicate-flip reload. To reproduce the historical failure, build
the affected revision with its
original matching comparison source and pass `--legacy-duplicate-reload baseline` (or `both` when
both binaries predate the fix). The current fixture adds event assertions that can stop an old
engine before reload, so it cannot be used to reproduce the original exit-2 signature. Verify the
executables before running `scripts/benchmark-position-replay.py`.
See the [benchmark policy](../../../../BENCHMARKING.md) and the [recorded comparison][report]
for measurement limits and commands for the archived layout.

Native published-profile model benchmarking uses the existing target:

```bash
CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 cargo bench --locked --profile bench-lto \
  -p nautilus-model --bench position_replay_criterion --features high-precision,test-support \
  -- '/8$' --sample-size 20 --measurement-time 1 --warm-up-time 1
```

[report]: https://github.com/nautechsystems/nautilus_trader/blob/1be5ebe4a060ad85e8a006118c3b0bf549b288db/docs/developer_guide/position_replay_comparison.md
[earlier]: https://github.com/nautechsystems/nautilus_trader/tree/1be5ebe4a060ad85e8a006118c3b0bf549b288db/crates/infrastructure/examples
