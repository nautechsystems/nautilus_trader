# Position replay review harnesses

These synthetic offline examples support the replay-index review. They contain no account
credentials and do not connect to an exchange. Run Redis cases only against a disposable,
loopback-only Redis instance with persistence disabled, never an existing application database.
The numeric port check alone does not establish isolation.

Build from the repository root using the pinned toolchain and existing shared cache:

```sh
CARGO_INCREMENTAL=0 cargo build --locked --profile nextest \
  -p nautilus-infrastructure --examples --features nautilus-model/high-precision
```

The `nextest` profile is for correctness here, not reported optimized performance. `--offline`
can be added when dependencies are already cached. Do not clean the target between variants.

## Recovery

```sh
./target/nextest/examples/replay_recovery DISPOSABLE_REDIS_PORT
```

This checks a real price restatement after warming the original index, then close/reopen,
Redis adapter add/load, a further fill, update, and a second load. Both virtual and nonvirtual
IDs use replay snapshots because of the correction. Redis itself stays running; this does not
check service restart durability, non-snapshot append recovery, or execution-engine routing.
The related codec regression runs with:

```sh
CARGO_INCREMENTAL=0 cargo test --locked --profile nextest \
  -p nautilus-model --test position_replay_recovery --features high-precision,test-support
```

## Repeated engine flips

```sh
./target/nextest/examples/replay_index_comparison 8 memory 0 cold duplicate 3 full
./target/nextest/examples/replay_index_comparison 36000 memory 0 warm duplicate 3 full
```

Arguments are history size, backend (`memory`, `redis-nonvirtual`, or `redis-virtual`), Redis
port (zero for memory), starting index state (`cold` or `warm`), first candidate
(`unique`, `unique-only`, or `duplicate`), flip count, and validation (`full` or `economics`).
Use `full` for correctness, including serialized position-state comparisons. The harness
executes `ExecutionEngine::process`; timings cover synchronous processing, not Redis durable
acknowledgment. A historical trade ID submitted through a fresh order checks unchanged Position
state, not unchanged order status. `unique-only` avoids duplicate probes; it must not be used
to imply the omitted duplicate persistence path passed.

A known nonvirtual Redis historical-duplicate append/reload failure is reported as exit code 2,
not success. Other assertion failures are nonzero. Keep it visible and separate from successful
normal-flow measurements.

## Comparison boundaries

The original before/after reference revisions are
`8dfef4b7725f8371d96c56cc0c7cdec5525a398d` and
`9feb6e79951f4cf4d47b50f9d08944a9798ceca1`. The supplemental fixed-lock model comparison keeps
all workspace sources and Cargo.lock at the latter revision except for baseline position.rs.
That is an isolated model comparison, not two complete historical checkouts.

Native published-profile model benchmarking uses the existing target:

```sh
CARGO_INCREMENTAL=0 cargo bench --locked --profile bench-lto \
  -p nautilus-model --bench position_replay_criterion --features high-precision,test-support \
  -- '/8$' --sample-size 20 --measurement-time 1 --warm-up-time 1
```

Record the exact source, lockfile, toolchain and profile for each variant. Keep engine opt3
artifact comparisons distinct from native model bench-lto results. No complete native bench-lto
engine comparison is claimed by these examples.

The merged combination with develop `0bbc9c981fddea5a2f48ccb56b95dad5cb03832a` was validated
on Rust 1.100.0: model library tests and codec recovery passed, as did these examples
for Redis snapshot recovery and in-memory engine historical duplicates. Those are correctness
checks; the earlier performance measurements were not rerun on this merged combination.
