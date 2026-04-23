# DST Scope Inventory

Companion to `docs/concepts/dst.md`. Lists every area where the DST contract
does not apply, so "what the contract does not cover" is explicit rather than
implicit.

Each entry carries one of four tags:

- **closed**: area that was audited, seams are in place, no open work.
- **gated**: area that escapes the simulator but is compiled out under
  `cfg(madsim)`, or has a cfg-gated alternative.
- **scoped-out**: area outside the 16 in-scope crates, or outside the DST
  contract by design. No work planned for this phase.
- **unresolved**: area where either the seam is incomplete, no seam exists
  yet, or verification is pending.

The 16 in-scope crates are:

- `analysis`
- `common`
- `core`
- `cryptography`
- `data`
- `execution`
- `indicators`
- `live`
- `model`
- `network`
- `persistence`
- `portfolio`
- `risk`
- `serialization`
- `system`
- `trading`

## Dependencies

### Non-aliased crates reachable from the DST path

| Crate               | Role                       | Tag         |
|---------------------|----------------------------|-------------|
| `tokio‑rustls`      | TLS transport              | scoped‑out  |
| `tokio‑tungstenite` | WebSocket framing          | scoped‑out  |
| `reqwest`           | HTTP client                | scoped‑out  |
| `redis`             | Redis driver               | scoped‑out  |
| `sqlx`              | Postgres driver            | scoped‑out  |

**Notes.** These crates build on real `tokio` and route I/O through real OS
sockets. Under `simulation` + `cfg(madsim)`, they run unmodified and the
kernel socket and TLS state they touch is outside the deterministic scheduler.
`redis` and `sqlx` live in `nautilus-infrastructure`, which is not one of the
16 in-scope crates; the other three ship inside `nautilus-network`.

**Mitigation.** The initial DST target is order-lifecycle determinism inside
the trading core, not transport-layer fault injection. Transport determinism
would require per-crate `madsim` shims that do not exist at this time.
Documented in `docs/concepts/dst.md` under "Transport-layer I/O is not
simulated".

### Adapter I/O surfaces

| Area                                             | Tag        |
|--------------------------------------------------|------------|
| All crates under `crates/adapters/`              | scoped‑out |
| Adapter‑level `chrono::Utc::now`                 | scoped‑out |
| Adapter‑level `SystemTime::now`                  | scoped‑out |
| Adapter‑level `Uuid::new_v4`                     | scoped‑out |
| Adapter‑owned WebSocket / HTTP clients           | scoped‑out |

**Notes.** Adapter crates are not in the 16-crate in-scope set. Each adapter
has its own clock, RNG, and transport call sites. The `check-dst-conventions`
hook does not scan adapter crates.

**Mitigation.** Adapters must be audited individually before they enter the
DST path. The audit output per adapter would be a document analogous to this
file, plus the equivalent call-site refactors and a hook scope extension.

### Python interpreter and PyO3 bindings

| Area                                                  | Tag         |
|-------------------------------------------------------|-------------|
| `crates/*/src/python/` (PyO3 bindings)                | scoped‑out  |
| `crates/*/src/ffi/` (C FFI)                           | scoped‑out  |
| `nautilus_trader/` (Python package)                   | scoped‑out  |
| `tests/` (Python tests)                               | scoped‑out  |
| `examples/` (Python example scripts)                  | scoped‑out  |

**Notes.** DST runs under a native Rust test harness. No Python interpreter
starts during a DST run, so no code reachable only from Python call paths
is part of the contract. Clock, RNG, and threading call sites behind PyO3
bindings or under the Python package are out of scope as a policy, not as a
weakness.

The `check-dst-conventions` hook encodes this by skipping `/python/` and
`/ffi/` paths inside the in-scope crates. Non-Rust directories are
ignored by construction.

**Effect on the contract.** A Rust path that is also exported to Python
must still satisfy the contract when reached from the native DST harness.
Only call paths reachable *exclusively* from Python are out of scope.

**Objective framing.** The primary objective of DST is reliability of the
Rust engine itself: the order lifecycle, reconciliation, matching, risk,
and execution state machines. Deterministic replay of user strategies is
a later, secondary goal that becomes available once strategies are
authored in Rust or driven by the harness directly. A Python strategy
that calls `time.time()`, issues arbitrary network requests, or relies
on thread scheduling can vary its command stream between runs; the Rust
core will process the varying stream deterministically, but end-to-end
replay from a Python entry point is not guaranteed.

### Raw threads and blocking calls

| Site                                                     | Tag        |
|----------------------------------------------------------|------------|
| `logging/logger.rs:318` writer thread                    | gated      |
| `crates/core/src/collections.rs` tests                   | gated      |
| `crates/core/src/time.rs` stress tests                   | gated      |
| `crates/common/src/live/runner.rs` tests                 | gated      |
| `crates/common/src/runner.rs` tests                      | gated      |
| `crates/live/src/runner.rs` tests                        | gated      |
| `crates/common/src/actor/registry.rs` tests              | gated      |
| `crates/network/src/python/http.rs` PyO3 blocking bridge | scoped‑out |

**Notes.** The logging writer thread is wrapped in
`#[cfg(not(all(feature = "simulation", madsim)))]` and log events are
discarded in simulation. All other non-test sites either sit in inline
`#[cfg(test)]` modules (excluded by the hook) or live under
`crates/network/src/python/`, which is part of the PyO3 bridge and skipped by
the hook path filter.

**Mitigation.** Rule 4 of the hook bans raw thread creation outside these
three escape cases:

- `#[cfg(test)]` test modules
- `#[cfg(not(madsim))]` or
  `#[cfg(not(all(feature = "simulation", madsim)))]` production sites
- an inline `// dst-ok` marker

Adding a new production thread on the DST path fails pre-commit.

### Real-`tokio` primitives under `cfg(madsim)`

`tokio` submodules `sync`, `io`, `select!` (macro), `fs`, and `net` route to
real `tokio` even under `simulation` + `cfg(madsim)`. This is the Option B
re-export design: only `time`, `task`, `runtime`, and `signal` switch; the
rest stays on real tokio to avoid type mismatches with transitive crates.

| Submodule         | Tag        | Notes                                            |
|-------------------|------------|--------------------------------------------------|
| `tokio::sync`     | scoped‑out | mpsc / oneshot / broadcast on real tokio         |
| `tokio::io`       | scoped‑out | AsyncRead / AsyncWrite on real tokio             |
| `tokio::select!`  | closed     | macro uses real tokio, but `biased;` pins order  |
| `tokio::fs`       | scoped‑out | real filesystem I/O                              |
| `tokio::net`      | scoped‑out | real sockets; used by transport crates           |

**Rationale.** Option B keeps tokio as the real crate at whatever version the
workspace uses; only four tokio submodules flip to `madsim`. Extending the
flip to `sync` / `io` / `fs` / `net` would require rebuilding
`tokio-tungstenite`, `tokio-rustls`, and `reqwest` against a shimmed
`tokio::net::TcpStream`, which the audit ruled out as too invasive. See
`docs/concepts/dst.md` under "Layer 1: runtime swap" for the covered surface.

**Effect on the contract.** Channel delivery order on `tokio::sync` is still
deterministic under `cfg(madsim)` because the sender and receiver tasks are
scheduled by the madsim executor even though the channel implementation is
real. Real-tokio `io` / `fs` / `net` calls inside the DST path bypass the
simulator silently.

**In-scope call sites.** `nautilus-network` is in-scope and uses real
`tokio::net` and `tokio::io` directly. Representative sites:

- `crates/network/src/net.rs:37` re-exports `tokio::net::{TcpListener, TcpStream}`
- `crates/network/src/socket/client.rs:46,356` uses `tokio::io::{AsyncReadExt, AsyncWriteExt}` and `tokio::io::split`
- `crates/network/src/tls.rs:22` uses `tokio::io::{AsyncRead, AsyncWrite}`
- `crates/network/src/websocket/types.rs:26,29` aliases `MaybeTlsStream<tokio::net::TcpStream>`

These run on real sockets under `simulation` + `cfg(madsim)`. The transport
surface behind them (`tokio-tungstenite`, `tokio-rustls`) is listed above
under "Non-aliased crates". Together they define the transport-layer
scope-out from the contract.

## Collections

### `AHashMap` / `AHashSet` outside `manager.rs`

The `check-dst-conventions` hook enforces `IndexMap` / `IndexSet` only in
`crates/live/src/manager.rs`. The 16 in-scope crates host `AHashMap` and
`AHashSet` in ~85 other files, used for:

- cache indexes (`crates/common/src/cache/mod.rs`)
- order book state (`crates/model/src/orderbook/`)
- account balances (`crates/model/src/accounts/`)
- portfolio state (`crates/portfolio/src/`)
- execution engine state (`crates/execution/src/engine/`)
- matching engine state (`crates/execution/src/matching_engine/engine.rs`)
- data aggregation (`crates/data/src/aggregation.rs`)
- msgbus switchboard (`crates/common/src/msgbus/switchboard.rs`)

| Area                                                   | Tag        |
|--------------------------------------------------------|------------|
| `AHashMap` / `AHashSet` in `manager.rs`                | closed     |
| `AHashMap` / `AHashSet` in `matching_engine/engine.rs` | unresolved |
| `AHashMap` / `AHashSet` elsewhere                      | unresolved |

**Notes.** `AHash` randomizes its hasher per process. Iteration order over
these collections varies across runs. The `manager.rs` call site was flagged
by the codebase audit as the only surface where iteration order drives
downstream event publication, so only that file is hook-enforced.

The matching engine is called out separately because it holds six
`AHashMap` fields on the fill path (`bid_consumption`, `ask_consumption`,
`queue_ahead`, `queue_excess`, `queue_pending`, `cached_filled_qty`) and
iterates `queue_ahead.keys()` in the resting-order walk. Iteration order
drives the sequence in which the seeded `FillModel` RNG is consumed, so a
hash-seed change reorders fills even with `FillModel(random_seed=...)`
pinned. Reported as a backtest-determinism break in issue
[#3914](https://github.com/nautechsystems/nautilus_trader/issues/3914).

**Effect on the contract.** The public backtesting guide promises
deterministic results under a seeded `FillModel`; the matching-engine
`AHashMap` breaks that promise across process boundaries. This is a
user-visible scope hole, not a latent one.

**Mitigation.** Other call sites need a per-file review to determine whether
iteration order affects observable state. The audit classified them as
lookup-only pending review. Any hash-collection site that feeds observable
state on the DST path is a future scope-hole closure.

## Randomness

### Raw RNG sources

| Source                 | Production sites                                     | Tag        |
|------------------------|------------------------------------------------------|------------|
| `rand::thread_rng`     | 0                                                    | closed     |
| `fastrand`             | 0                                                    | closed     |
| `getrandom`            | 0                                                    | closed     |
| `OsRng`                | 0                                                    | closed     |
| `rand::rng()`          | `core/uuid.rs:56`, `execution/models/fill.rs:85`, `network/backoff.rs:105` | unresolved |
| `Uuid::new_v4` (tests) | `core/uuid.rs:380`                                   | closed     |
| `Uuid::new_v4` (prod)  | `execution/matching_engine/ids_generator.rs:167,179` | unresolved |

**Notes.** `rand::rng()` is the `rand` 0.9 replacement for `rand::thread_rng`
and draws from the same per-thread CSPRNG. Rule 2 of the hook does not
currently match `rand::rng` (it covers `rand::thread_rng`, `fastrand::`,
`getrandom::`, and `OsRng`), so these three sites are not flagged by the
hook.

The call sites are:

- `crates/core/src/uuid.rs:56` in `UUID4::new()`. Reachable from production
  factories such as `crates/common/src/factories/order.rs` and
  `crates/common/src/messages/execution/report.rs` as well as
  `crates/risk/src/engine/mod.rs`.
- `crates/execution/src/models/fill.rs:85` when `FillModel::new()` is
  constructed with `random_seed=None`. When a seed is provided the model
  routes through `StdRng::seed_from_u64`, which is deterministic.
- `crates/network/src/backoff.rs:105` for reconnect jitter.

`Uuid::new_v4` is the separate `uuid` crate path in the matching engine ID
generator, gated behind `use_random_ids` on `IdsGenerator`. The default ID
scheme is deterministic (`{venue}-{raw_id}-{count}`). Under the hood,
`Uuid::new_v4` also reaches `getrandom`, which condition 5 of the contract
in `docs/concepts/dst.md` lists as not intercepted by the current `madsim`
wiring.

**Mitigation.** Close both in one follow-up:

1. Route the production branches through `madsim::rand` under `cfg(madsim)`.
2. Extend Rule 2 of the hook to ban `rand::rng` and `Uuid::new_v4` in the
   in-scope crates. Call sites that remain intentionally non-deterministic
   (for example `network/backoff.rs` jitter) mark with `// dst-ok` and a
   reason.

Deferred until a DST scenario exercises these call sites. Adapter-crate
RNG sources (`Uuid::new_v4`, `chrono::Utc::now`-seeded paths, venue-side
randomness) remain `scoped-out` per "Adapter I/O surfaces" above.

### `tokio::task::LocalSet` and `spawn_blocking`

| Primitive                     | Tag    |
|-------------------------------|--------|
| `tokio::task::LocalSet`       | gated  |
| `tokio::task::spawn_blocking` | gated  |

**Notes.** Neither primitive is supported under `madsim`. The contract in
`docs/concepts/dst.md` conditions 7 and 8 require both to be cfg-gated out
on the DST path. The codebase audit did not find production `LocalSet` or
`spawn_blocking` inside the 16 in-scope crates; Rule 4 of the hook bans
`spawn_blocking` without a preceding `#[cfg(not(all(feature = "simulation",
madsim)))]` attribute.

**Effect on the contract.** Neither primitive is reachable on the DST path
today. If one is added, it must carry a cfg gate or an inline `// dst-ok`
with a reason. No runtime enforcement exists beyond the hook.

## Time

### `Instant::now` / `SystemTime::now` call sites

| Site                                              | Tag    |
|---------------------------------------------------|--------|
| `common/testing.rs:81` `wait_until`               | closed |
| `common/testing.rs:108` `wait_until_async`        | closed |
| `execution/engine/mod.rs:822,847` init log timing | closed |
| `common/cache/mod.rs:3895` audit timing           | closed |
| `common/cache/mod.rs:569,904` log timing          | closed |
| `model/defi/reporting.rs:59,123` progress log     | closed |
| `core/time.rs` stress tests                       | closed |
| `network/socket/client.rs` test timers            | closed |
| `network/websocket/client.rs` test timers         | closed |

**Notes.** These sites either sit inside `#[cfg(test)]`, are covered by the
hook's file-level allowlist (`common/cache/mod.rs`,
`model/defi/reporting.rs`), or carry an inline `// dst-ok` marker with a
short reason (`common/testing.rs`, `execution/engine/mod.rs`). The seam
definition site in `crates/core/src/time.rs` is also allowlisted.

**Mitigation.** Reviewer attention keeps the allowlist small. Any new
`Instant::now` or `SystemTime::now` read on the DST path fails Rule 1 of
the hook unless it carries `// dst-ok` with a reason.

### `chrono::Utc::now` call sites

| Site                                                       | Tag         |
|------------------------------------------------------------|-------------|
| `common/logging/bridge.rs:58` log timestamp                | scoped‑out  |
| `common/logging/writer.rs:148,161,265,281,324,355,359` log file rotation | scoped‑out  |
| `core/datetime.rs:404` helper for current nanos            | unresolved  |

**Notes.** Rule 1 of the hook matches `Instant::now` and `SystemTime::now`
only; it does not match `chrono::Utc::now`. The logging bridge and writer
sites are scoped out under "Logging runs on real OS threads" in
`docs/concepts/dst.md`: logging is outside the determinism contract, the
writer thread is cfg-gated out under `cfg(madsim)`, and log events are
dropped. The helper in `crates/core/src/datetime.rs:404` is reachable from
non-logging call paths and is a real scope hole that the hook does not
catch.

**Mitigation.** Close with two changes:

1. Route `core/datetime.rs:404` through
   `nautilus_core::time::duration_since_unix_epoch`.
2. Extend Rule 1 of the hook to match `chrono::Utc::now` in the in-scope
   crates, allowlisting the logging sites at file level.

## Deferred items

### Signal handling call-site migration

`nautilus_common::live::dst::signal::ctrl_c` re-exports the deterministic
`ctrl_c` shim. One production call site has not been migrated yet:
`crates/live/src/node.rs:834` still calls `tokio::signal::ctrl_c` directly.

| Area                                           | Tag        |
|------------------------------------------------|------------|
| `nautilus_common::live::dst::signal` re‑export | closed     |
| `crates/live/src/node.rs:834` call site        | unresolved |
| Adapter‑bin `ctrl_c` sites                     | scoped‑out |

**Effect on the contract.** Under `simulation` + `cfg(madsim)`, node shutdown
driven by `ctrl_c` runs on real-tokio signal handling and is not injectable
from test code. The rest of the node's behavior is deterministic; only the
shutdown-signal arrival is not.

**Mitigation.** Documented in `docs/concepts/dst.md` under "Signal handling":
call sites will be migrated as the contract is tightened; until then, signal
handling under `cfg(madsim)` is out of scope.

### Logger file-logging tests under `cfg(madsim)`

Under `RUSTFLAGS="--cfg madsim" cargo test --features simulation` on
`nautilus-common`, the tests that exercise the logging writer thread fail
because the thread is cfg-gated out and log events are dropped.

| Area                                     | Tag        |
|------------------------------------------|------------|
| Logger writer thread under `cfg(madsim)` | gated      |
| File‑logging tests under `cfg(madsim)`   | unresolved |

**Effect on the contract.** Log output is explicitly outside the determinism
contract (the writer writes, never reads or mutates simulation state), so
losing it under simulation does not weaken the contract. The test failures
are a reflection of the gate, not a correctness issue in the gate itself.

**Mitigation.** Two options for the follow-up:

1. Cfg-gate the affected tests out under `cfg(madsim)`.
2. Provide an inline log-event sink under `cfg(madsim)` so log tests can
   observe the channel without a writer thread.

Either is acceptable. Choice deferred until the determinism regression gate
(Phase 1 Item 1 of the sign-off plan) exercises the logging subsystem.

### Runtime verification of the contract

The contract in `docs/concepts/dst.md` is enforced structurally at commit
time via `check-dst-conventions`. It is not yet verified dynamically: no
same-seed diff harness runs under `cfg(madsim)` on any in-scope code path.

| Area                                     | Tag        |
|------------------------------------------|------------|
| Static enforcement in source             | closed     |
| Dynamic same‑seed diff harness           | unresolved |

**Effect on the contract.** The structural conditions (Rule 1 to Rule 5)
are enforced. The claim that a seed produces identical observable behavior
across repeated runs is plausible from the seam design but has no running
regression gate yet.

**Mitigation.** Phase 1 Item 1 of the determinism sign-off plan (forcing
function plan) builds the same-seed diff harness. The harness lives in
`nautilus_dst`, not in this repository, and consumes the seams landed in
this phase. Until it exists, "same seed reproduces the failure every time"
remains a design intent, not a verified property.

## Summary

| Tag        | Count |
|------------|-------|
| closed     | 18    |
| gated      | 10    |
| scoped‑out | 23    |
| unresolved | 8     |

Unresolved entries at the end of this phase:

1. `AHashMap` / `AHashSet` in `crates/execution/src/matching_engine/engine.rs`
   (user-visible backtest-determinism break, issue #3914)
2. `AHashMap` / `AHashSet` elsewhere outside `crates/live/src/manager.rs`
3. `rand::rng()` in `core/uuid.rs`, `execution/models/fill.rs`, and
   `network/backoff.rs`
4. `Uuid::new_v4` in `execution/matching_engine/ids_generator.rs` when
   `use_random_ids` is active
5. `chrono::Utc::now` in `core/datetime.rs:404`
6. `tokio::signal::ctrl_c` call site in `crates/live/src/node.rs`
7. Logger file-logging tests under `cfg(madsim)`
8. Dynamic same-seed diff harness

Items 1 through 6 are source-level follow-ups in this repository. Item 7
is a test-only follow-up in this repository. Item 8 lives in
`nautilus_dst`.

Adapter crates and Python / FFI bindings remain `scoped-out`. A per-adapter
audit must land before any individual adapter can enter the DST path, but
the audit is not itself on the critical path for this phase. Python paths
are out of scope as a permanent policy, not a pending audit: DST runs
under a native Rust harness and no Python interpreter starts during a DST
run.
