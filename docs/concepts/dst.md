# Deterministic Simulation Testing

Deterministic simulation testing (DST) runs NautilusTrader under a seed-controlled runtime so that
timing-sensitive execution behavior is bitwise reproducible from a single integer. This guide
explains what DST is, how NautilusTrader supports it, what guarantees the support provides, and
where those guarantees stop.

The goal is a published contract that external users and auditors can verify: the determinism
NautilusTrader claims is backed by source-level evidence and enforced at commit time by a pre-
commit hook that runs in continuous integration.

## Introduction

### What DST is

DST is a testing technique for concurrent systems. A single seed fully determines an execution,
including task scheduling, timer firings, and random values. Two runs with the same seed, binary,
and configuration produce identical observable behavior. When a property fails, the seed is the
reproduction: the same seed replays the failure every time.

The approach grew out of distributed databases. FoundationDB pioneered the pattern for production
storage systems, documented in the SIGMOD 2021 paper *"FoundationDB: A Distributed Unbundled
Transactional Key Value Store"*. TigerBeetle applies it to financial systems with public blog
posts and talks. Antithesis offers VM-level determinism as a commercial service. Runtime-level
work in the Rust ecosystem includes [madsim](https://github.com/madsim-rs/madsim), which
intercepts `tokio` primitives to yield a deterministic scheduler.

The common thread: classes of bugs that escape unit, integration, property, and acceptance
testing still surface under seed-replayable scheduling. Channel wakeup ordering, drain races at
shutdown, startup sequencing, reconciliation ordering, and recovery-path correctness all involve
interleavings that traditional tests cannot exhaustively cover but a deterministic scheduler can
explore systematically.

### What this guide covers

NautilusTrader's DST support has two halves:

- **The contract**: what the runtime guarantees under seed-controlled execution, and under which
  conditions.
- **The enforcement**: the source-level seams that implement the contract and the pre-commit
  hook that keeps them in place.

This guide documents both.

## Goals

- **Seed-reproducible execution** for the in-scope portion of the NautilusTrader runtime.
- **Honest scope**. The contract lists what is covered and what is not. No silent fallbacks to
  real wall-clock time or unseeded RNG; conditions that weaken the guarantee are enumerated.
- **Enforcement in source**. A pre-commit hook fails commits that add banned patterns to the DST
  path, so the contract stays true without relying on reviewer attention.
- **Minimum necessary instrumentation**. The seams route time, task scheduling, and randomness
  through a deterministic source only where the contract requires it; everything else runs
  unchanged.

## Approach

The implementation has two layers.

### Layer 1: runtime swap

Under the `simulation` Cargo feature on `nautilus-common`, four `tokio` submodules are routed
through `madsim` when `RUSTFLAGS="--cfg madsim"` is set:

- `time` (timers, intervals, monotonic `Instant`).
- `task` (spawning and joining async tasks).
- `runtime` (the runtime builder and handle).
- `signal` (process signals such as `ctrl_c`; re-export is available, call-site adoption is
  partial as noted under [scope boundaries](#signal-handling)).

These re-exports live in `nautilus_common::live::dst`. DST-path call sites for `time`, `task`,
and `runtime` import from this module rather than directly from `tokio`, so toggling the feature
switches the async runtime in one place for the primitives that are fully routed. Under normal
builds, the re-exports resolve to real `tokio`. Under `simulation` + `cfg(madsim)`, they resolve
to `madsim`'s deterministic counterparts.

Everything else that `tokio` provides (`sync`, `io`, `select!` as a macro, `fs`, `net`) uses real
`tokio` unconditionally. Transitive crates (`tokio-tungstenite`, `tokio-rustls`, `reqwest`) are
unaffected.

### Layer 2: nondeterminism substitution

Nondeterminism outside the async runtime is redirected through explicit seams:

- **Wall-clock reads** go through `nautilus_core::time::duration_since_unix_epoch`. Under
  simulation this routes to `madsim::time::TimeHandle`, preserving Unix-epoch semantics for
  order and fill
  timestamps.
- **Monotonic reads** go through `nautilus_common::live::dst::time::Instant`. The type resolves
  to `tokio::time::Instant` on normal builds (for compatibility with `tokio::test(start_paused)`
  test helpers) and `madsim::time::Instant` under simulation.
- **Network-local monotonic reads** go through `nautilus_network::dst::time`. The crate sits
  below `nautilus-common` in the dependency graph and exposes a local re-export module with the
  same semantics.
- **Hash iteration order** in the reconciliation manager uses `IndexMap` and `IndexSet` rather
  than `AHashMap` and `AHashSet`. `AHash` randomizes its hasher per process; insertion-order
  iteration is needed where order drives downstream event publication.
- **`tokio::select!` polling order** uses the `biased;` modifier at every production site on the
  DST path. Unbiased `select!` polls branches in an order chosen by an unintercepted RNG.

## Determinism contract

Under the conditions below, a run identified by `(seed, binary hash, configuration hash)` on the
same platform produces bitwise-identical:

1. Scheduling order of async tasks.
2. Timer firings (virtual monotonic and virtual wall-clock).
3. RNG output from `madsim::rand`.
4. Channel delivery order on `tokio` primitives.

### Required conditions

The contract holds only when all of the following are true:

1. The `simulation` Cargo feature is active and `RUSTFLAGS="--cfg madsim"` is set. Both are
   required. The feature activates the deterministic runtime; the cfg flag activates `madsim`'s
   libc-level `clock_gettime` and `getrandom` intercepts. One without the other silently falls
   back to real `tokio` and breaks determinism without an error.
2. Every `tokio::select!` call site on the DST path uses the `biased;` modifier.
3. Monotonic time reads route through the DST seam (either `nautilus_common::live::dst::time` or
   `nautilus_network::dst::time`), not `std::time::Instant::now` directly.
4. Wall-clock time reads route through `nautilus_core::time::duration_since_unix_epoch`.
5. Randomness routes through `madsim::rand`. `rand::thread_rng`, `fastrand`, `getrandom`, and
   `OsRng` are not intercepted.
6. Iteration-order-sensitive collections use `IndexMap` or `IndexSet`, not `AHashMap` or
   `AHashSet`.
7. `tokio::task::LocalSet` construction is cfg-gated out under simulation. `madsim` does not
   provide `LocalSet`; `spawn_local` works without it.
8. `tokio::task::spawn_blocking` call sites are cfg-gated or removed. A blocking call escapes
   the deterministic scheduler.

## Static enforcement

A pre-commit hook named `check-dst-conventions` enforces the structural conditions in source.
The hook lives at `.pre-commit-hooks/check_dst_conventions.sh`, runs as part of the standard
pre-commit suite, and runs in continuous integration. It covers the 16 in-scope workspace crates
and fails the commit when it detects any of:

- Raw `std::time::Instant::now()` or `SystemTime::now()` reads, including bare forms when the
  enclosing file imports the type from `std::time`.
- Raw RNG usage (`rand::thread_rng`, `fastrand::`, `getrandom::`, `OsRng`).
- `tokio::select!` blocks missing `biased;` within the first three lines.
- `std::thread::spawn`, `std::thread::Builder::new`, or `tokio::task::spawn_blocking` calls that
  lack a preceding `#[cfg(test)]`, `#[cfg(not(madsim))]`, or
  `#[cfg(not(all(feature = "simulation", madsim)))]` attribute.
- `AHashMap` or `AHashSet` in `crates/live/src/manager.rs`.

The hook supports two exception forms:

- An inline `// dst-ok` marker on a specific line, typically accompanied by a short reason (for
  example, log-only wall-clock timing that does not affect state).
- A small file-level allowlist in the hook script itself for sites classified as
  leave-alone in the codebase audit (log timing in the cache module, progress reporting in the
  DeFi module).

Test files, files under `tests/`, `python/`, and `ffi/` directories, and lines inside an inline
`#[cfg(test)]` module are excluded because they are not part of the DST path.

### In-scope crates

The hook applies to the 16 workspace crates in the transitive closure of `nautilus-live`:

- `analysis`, `common`, `core`, `cryptography`, `data`, `execution`, `indicators`, `live`,
  `model`, `network`, `persistence`, `portfolio`, `risk`, `serialization`, `system`, `trading`.

Adapter crates and infrastructure crates (Redis, Postgres) are out of scope. Their DST
suitability requires a separate audit before they enter the DST path.

## Scope boundaries

The contract is deliberately narrow. The following weakenings are explicit, not oversights.

### Platform-scoped

`madsim`'s libc overrides for `clock_gettime` and `getrandom` are platform-specific.
Cross-platform bitwise reproducibility is not claimed. A seed that reproduces a failure on Linux
x86_64 may not reproduce on macOS aarch64.

### Non-aliased dependencies escape silently

Any dependency that reaches the OS through a non-aliased path (direct `libc` calls, `std::net`
bypass, crates using `fastrand` or `OsRng`) escapes the simulator without raising an error. The
in-scope crates have been audited; adapter crates and infrastructure crates require their own
audits before entering the DST path.

### Transport-layer I/O is not simulated

`tokio-tungstenite`, `tokio-rustls`, `reqwest`, `redis`, and `sqlx` use real `tokio` internally.
Under simulation, WebSocket and HTTP I/O run on real networking. This is intentional: the
initial target is order lifecycle determinism, not transport fault injection. Transport-layer
determinism would require per-crate `madsim` shims that do not exist at this time.

### Signal handling

`nautilus_common::live::dst::signal` exposes a routed `ctrl_c` re-export, but not every
production call site has adopted it yet. Code that still calls `tokio::signal::ctrl_c` directly
bypasses the seam. Call sites will be migrated as the determinism contract is tightened; until
then, signal handling under `cfg(madsim)` should be treated as out of scope.

### Logging runs on real OS threads

The logging subsystem spawns a writer thread via `std::thread::Builder` and uses
`std::sync::mpsc`. Under simulation, the thread is not spawned and log events are dropped.
Log output is outside the determinism contract: the writer only writes, never reads or mutates
simulation state.

### Adapters

Adapter crates are out of scope for the initial DST contract. Each adapter has its own set of
`chrono::Utc::now`, `SystemTime::now`, `Uuid::new_v4`, and transport-layer call sites. An
adapter that enters the DST path must be audited for direct clock, RNG, and transport usage
before its behavior can be covered by the contract.

## Relationship to other testing layers

DST complements existing testing; it does not replace any of it.

| Layer                   | Covers                                               | DST relationship                                |
|-------------------------|------------------------------------------------------|-------------------------------------------------|
| Unit tests              | Pure logic, calculations, parsers, transformers.     | Unchanged.                                      |
| Integration tests       | Component interaction, I/O boundaries.               | Unchanged. DST runs alongside, not in place of. |
| Property‑based tests    | Invariants over input domains (parsers, roundtrips). | Unchanged.                                      |
| Acceptance tests        | End‑to‑end backtest and live scenarios.              | Unchanged.                                      |
| Deterministic sim (DST) | Async timing, scheduling, recovery correctness.      | Adds seed‑replayable exploration.               |

DST's unique value is in the intersection of async concurrency and state-machine correctness.
Bugs such as "a message at shutdown is dropped under a specific wakeup ordering" or "a
reconciliation event is lost when iteration order reverses" are the target class. For anything
else, the pre-existing test layers are the right tool.

## Status

As of the current state of this repository:

- Layer 1 (runtime swap) is implemented. `nautilus_common::live::dst` exposes routed re-exports
  for `time`, `task`, `runtime`, and `signal`. Production call sites for `time`, `task`, and
  `runtime` route through the seam; signal call-site adoption is partial.
- Layer 2 (nondeterminism substitution) is implemented across the 16 in-scope crates. Seams
  exist for wall-clock time, monotonic time, and iteration order. The RNG policy is enforced
  structurally (no raw RNG sources on the DST path); no production RNG is currently on the DST
  path.
- Static enforcement via `check-dst-conventions` is active in pre-commit and CI.
- Runtime verification of the contract under `cfg(madsim)` (same-seed diff harness) is planned
  follow-up work and is not yet part of this repository.

## Further reading

- `.pre-commit-hooks/check_dst_conventions.sh` defines the five enforcement rules in full and
  documents the `// dst-ok` marker convention.
- External references: the [FoundationDB testing
  philosophy](https://apple.github.io/foundationdb/testing.html), the [TigerBeetle simulation
  testing blog posts](https://tigerbeetle.com/blog/), and the
  [madsim repository](https://github.com/madsim-rs/madsim) for the deterministic runtime.
