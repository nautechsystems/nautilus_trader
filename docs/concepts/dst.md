# DST

<!-- Keep this title as "DST"; longer titles do not render well in the left navigation. -->

Deterministic simulation testing (DST) runs NautilusTrader under a seed-controlled runtime. This
guide defines:

- The reproducibility guarantees for seed-controlled execution.
- The conditions required for those guarantees.
- The source seams and checks that enforce the contract.
- The paths where deterministic execution stops.

Source locations accompany each claim so users and auditors can check the contract against the
code.

:::note
A downstream harness that depends on NautilusTrader's determinism consumes the version of this
document at its pinned NautilusTrader commit. A change to this document is a contract change for
those consumers and should be reviewed as one.
:::

## What DST is

DST controls the sources of nondeterminism in a concurrent system. Within the contract defined
below, one seed determines task scheduling, timer firings, and random values. Two runs with the same
seed, binary, configuration, and platform produce identical observable behavior. Record a failing
seed to replay the same execution.

A conventional async runtime draws scheduling decisions from ambient process state, including:

- Task wake order.
- Timer resolution.
- OS thread scheduling.
- Randomized hash seeds.

A conventional test harness does not control these sources, so a race that appears once in CI can
be difficult to reproduce. DST replaces them with a seeded pseudorandom sequence. Varying the seed
explores different interleavings; reusing it selects the same interleaving.

[FoundationDB](https://apple.github.io/foundationdb/testing.html) uses the pattern to test a
production distributed database. In the Rust ecosystem,
[madsim](https://crates.io/crates/madsim) intercepts `tokio` primitives to provide a
deterministic scheduler.

DST targets concurrency defects such as:

- Channel wakeup ordering.
- Drain races during shutdown.
- Startup sequencing.
- Reconciliation ordering.
- Recovery-path correctness.

Other test layers cannot exhaustively cover these interleavings. A deterministic scheduler can
explore them across seeds and replay a failing schedule.

## Goals

| Goal                          | Requirement                                                                                                 |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Seed-reproducible execution   | The in-scope runtime produces the same observable behavior for the same seed and inputs.                    |
| Explicit scope                | Every fallback to real time, unseeded randomness, or other nondeterminism is documented.                    |
| Enforcement in source         | Static checks reject banned patterns on the DST path before they rely on reviewer attention.                |
| Minimum required interception | Time, task scheduling, and randomness route through deterministic sources only where the contract needs it. |

## Approach

The implementation has two layers. The first replaces selected `tokio` primitives with `madsim`.
The second controls sources of nondeterminism outside those primitives.

### Layer 1: runtime swap

Under the `simulation` Cargo feature on `nautilus-common`, four facade submodules select simulation
behavior when `RUSTFLAGS="--cfg madsim"` is set:

| Submodule | Covers                                          | Normal build  | Simulation build                                                 | Adoption                                          |
| --------- | ----------------------------------------------- | ------------- | ---------------------------------------------------------------- | ------------------------------------------------- |
| `time`    | Timers, intervals, and monotonic `Instant`.     | `tokio`       | `madsim`                                                         | Complete.                                         |
| `task`    | Spawning and joining async tasks.               | `tokio`       | `madsim`                                                         | Complete.                                         |
| `runtime` | Runtime builder and handle.                     | `tokio`       | `madsim`                                                         | Complete.                                         |
| `signal`  | Process signals such as `ctrl_c` and `SIGTERM`. | `tokio` or OS | `madsim` for `ctrl_c`; a never-completing future for `terminate` | Partial. See [Signal handling](#signal-handling). |

These re-exports live in `nautilus_common::live::dst`. DST-path call sites for `time`, `task`, and
`runtime` import from this module. Normal builds resolve the imports to `tokio`; `simulation` with
`cfg(madsim)` resolves them to `madsim`.

The `sync`, `io`, `fs`, and `net` submodules, plus the `select!` macro, continue to use real
`tokio`. Transitive crates such as `tokio-tungstenite`, `tokio-rustls`, and `reqwest` are
unaffected.

### Layer 2: nondeterminism substitution

Nondeterminism outside the aliased runtime needs explicit seams.

#### Wall-clock time

Wall-clock reads route through `nautilus_core::time::duration_since_unix_epoch`. Under simulation,
the seam calls `madsim::time::TimeHandle::try_current()` to preserve Unix-epoch semantics for order
and fill timestamps.

Plain `#[rstest]` bodies run outside a madsim runtime. In that context, the seam falls back to
`SystemTime::now()`, which uses the same real syscall as a normal build. Production paths under
simulation run inside a madsim runtime and receive virtual time.

#### Monotonic time

Monotonic reads route through `nautilus_common::live::dst::time::Instant`. Normal builds resolve
the type to `tokio::time::Instant`, preserving compatibility with
`#[tokio::test(start_paused = true)]`. Simulation builds resolve it to `madsim::time::Instant`.

#### Network-local monotonic time

Network code routes monotonic reads through `nautilus_network::dst::time`. `nautilus-network` sits
below `nautilus-common` in the dependency graph, so it provides a local re-export module with the
same behavior.

#### Iteration order

Observable iteration order uses `IndexMap`, `IndexSet`, or an explicit sort instead of relying on
`AHashMap` or `AHashSet`. `AHash` randomizes its hasher per process. Stable order is required when
iteration controls event publication or the sequence of draws from a seeded `FillModel`.

#### Select polling order

Every production `tokio::select!` site on the DST path starts with `biased;`. Without it, an
unintercepted RNG chooses the branch polling order.

## Determinism contract

Under the conditions below, a run identified by `(seed, binary hash, configuration hash)` on the
same platform produces bitwise-identical:

- Scheduling order of async tasks.
- Timer firings (virtual monotonic and virtual wall-clock).
- RNG output from `madsim::rand`.
- Delivery order on `tokio::sync` channels.

### Required conditions

The contract holds only when every row below is satisfied:

| Source of nondeterminism | Required condition                                                       | Failure when bypassed                                                                                                                                  |
| ------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Build selection          | Enable the `simulation` feature and set `RUSTFLAGS="--cfg madsim"`.      | Either setting alone falls back to real `tokio` without an error. The cfg also activates madsim's libc intercepts for `clock_gettime` and `getrandom`. |
| `tokio::select!`         | Put `biased;` first in every production block on the DST path.           | An unintercepted RNG chooses the polling order.                                                                                                        |
| Monotonic time           | Use `nautilus_common::live::dst::time` or `nautilus_network::dst::time`. | Direct `std::time::Instant::now` reads the host clock.                                                                                                 |
| Wall-clock time          | Use `nautilus_core::time::duration_since_unix_epoch`.                    | Direct clock reads bypass virtual time.                                                                                                                |
| Randomness               | Use `madsim::rand`.                                                      | `rand::thread_rng`, `rand::rng()`, `fastrand`, `getrandom`, and `OsRng` are not intercepted.                                                           |
| Iteration order          | Use `IndexMap`, `IndexSet`, or sort at the point of use.                 | Randomized hash iteration changes observable ordering.                                                                                                 |
| Local tasks              | Gate out `tokio::task::LocalSet` under simulation.                       | `madsim` does not provide `LocalSet`; use `spawn_local` without it.                                                                                    |
| Blocking tasks           | Gate out or remove `tokio::task::spawn_blocking`.                        | The blocking call escapes the deterministic scheduler.                                                                                                 |

## Static enforcement

Static enforcement has two layers:

| Layer                   | Enforces                                                                                                                  |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Clippy policy           | `clippy.toml` and `[workspace.lints.clippy]` reject direct `getrandom::{fill,u32,u64}` calls and `tokio::task::LocalSet`. |
| `check-dst-conventions` | The pre-commit hook applies path-aware and cfg-aware structural checks that Clippy cannot express cleanly.                |

The hook lives at `.pre-commit-hooks/check_dst_conventions.sh` and runs in the standard pre-commit
suite and CI. Rules 1 to 6 apply to all 17 in-scope workspace crates. Rule 7 applies to the nine
crates on the madsim build path.

| Rule | Rejects                                                                                                                                        | Scope or exception                                                                                          |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| 1    | Raw `std::time::Instant::now()`, `SystemTime::now()`, `jiff::Timestamp::now()`, and `jiff::Zoned::now()` reads, including imported bare forms. | Allows the wall-clock seam, audited log or progress timing, and marked lines.                               |
| 2    | Raw `rand::thread_rng`, `rand::rng()`, `fastrand::`, `getrandom::`, `OsRng`, and `Uuid::new_v4()` usage.                                       | Allows cfg-gated and marked lines.                                                                          |
| 3    | Production `tokio::select!` blocks without `biased;` in the first three lines.                                                                 | Excludes tests and marked lines.                                                                            |
| 4    | `std::thread::spawn`, `std::thread::Builder::new`, and `tokio::task::spawn_blocking`.                                                          | Allows test-only, non-madsim, and marked sites.                                                             |
| 5    | `AHashMap` or `AHashSet` in the reconciliation manager and matching engine.                                                                    | Covers the two audited files; the remaining file set stays outside this static rule until audited.          |
| 6    | Direct `tokio::net::TcpStream::connect` and `tokio::net::TcpListener::bind` calls.                                                             | Callers must use `nautilus_network::net`, which swaps to `turmoil::net` under the `turmoil` feature.        |
| 7    | Raw `tokio::{time,task,runtime,signal}` paths in production code on the madsim build path.                                                     | Allows the facade, process-wide real Tokio runtime, test infrastructure, cfg-gated sites, and marked lines. |

The hook supports two exception forms:

- An inline `// dst-ok` marker on a specific line, typically accompanied by a short reason (for
  example, log-only wall-clock timing that does not affect state).
- A small file-level allowlist in the hook script itself for sites classified as
  leave-alone in the codebase audit (log timing in the cache module, log-record timestamping
  in the logging bridge and writer, progress reporting in the DeFi module).

The hook excludes:

- Files under `tests/`, `python/`, and `ffi/` directories.
- Files named `tests.rs`, `*_test.rs`, or `*_tests.rs`.
- Lines inside an inline `#[cfg(test)]` module.

These paths are not part of the production DST contract.

### In-scope crates

The transitive closure of `nautilus-live` contains 16 in-scope crates:

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

The hook also covers `backtest`, bringing the total to 17 crates.

Adapter crates and infrastructure crates (Redis, Postgres) are out of scope. Their DST
suitability requires a separate audit before they enter the DST path.

## Network seed soaks

[Turmoil](https://crates.io/crates/turmoil) simulates the network under a seeded scheduler. The
`nautilus-network` transport tests use it to reach link and reconnect orderings outside the madsim
runtime swap.

### Fixed-seed nightly tests

The nightly suite runs reproducible scenarios for:

- Initial connection.
- Reconnection.
- Network partitions.
- Closing during reconnection.
- Closing during backoff.
- Repeated server drops with exact message-order assertions.

### Reconnect seed soak

An ignored reconnect test sweeps Turmoil seeds until stopped or until the configured limit is
reached. For each seed, the soak runs the Tungstenite WebSocket backend first. It then runs Sockudo
when the `transport-sockudo` feature is enabled, giving both backends the same schedule search path.

| Variable                                  | Default   | Effect                                              |
| ----------------------------------------- | --------- | --------------------------------------------------- |
| `NAUTILUS_TURMOIL_SOAK_START`             | `0`       | Selects the first seed, allowing a sweep to resume. |
| `NAUTILUS_TURMOIL_SOAK_COUNT`             | Unbounded | Stops after the specified number of seeds.          |
| `NAUTILUS_TURMOIL_SOAK_PROGRESS_INTERVAL` | `100`     | Logs progress after this many seeds per backend.    |

### Run the soak

Run the continuous soak with:

```bash
scripts/soak-network-turmoil.sh
```

Run a bounded soak with:

```bash
env NAUTILUS_TURMOIL_SOAK_COUNT=100 scripts/soak-network-turmoil.sh
```

Each seed enables random node order and link latency from 1 ms to 25 ms. The scenario repeatedly
drops the server, cycles the client through reconnect states, and asserts exact application-message
order.

The soak does not enable Turmoil `fail_rate`. For TCP, packet loss without a retransmit model would
overstate the client delivery contract in an order-preservation test.

### Platform coverage

The Turmoil tests use a simulated network and are not gated to Linux, so the seed sweep also runs on
macOS. Several real localhost socket and WebSocket unit tests use `target_os = "linux"` for CI
stability. A macOS run therefore leaves that host TCP coverage untouched. Treat the complete
network test set as covered only after a run on Linux CI or a Linux workstation.

## Implementation notes

These sections map each deterministic seam to its source, purpose, and exceptions.

### Iteration-order seams

Production sites use ordered collections or explicit sorting when iteration is observable on the
DST path.

#### Matching engine

`crates/execution/src/matching_engine/mod.rs` uses eleven ordered collections:

- `execution_bar_types`
- `execution_bar_deltas`
- `account_ids`
- `cached_filled_qty`
- `post_match_order_ids`
- `bid_consumption`
- `ask_consumption`
- `queue_pending`
- `queue_ahead_orders`
- `queue_ahead_total`
- `queue_excess`

Removals from `queue_pending`, `queue_ahead_orders`, and `queue_ahead_total` use `.shift_remove()`
so deleting an entry does not reorder the remainder.

#### Reconciliation manager

Rule 5 covers `crates/live/src/execution/manager.rs`. The `orders` and `fills` maps in
`ReconciliationResult` use `IndexMap` in `crates/execution/src/reconciliation/types.rs`.

#### Account balances

The account trait returns `IndexMap` from:

- `balances`
- `balances_total`
- `balances_free`
- `balances_locked`
- `starting_balances`

Balance and margin storage on `BaseAccount` and `MarginAccount` also uses `IndexMap`. The
`commissions` and `leverages` fields remain `AHashMap`.

#### Position commissions

`Position::commissions` in `crates/model/src/position.rs` uses `IndexMap`. Position snapshots consume
the map through `.values()` in `crates/model/src/events/position/snapshot.rs`, making its order
observable.

#### Portfolio aggregation

`crates/portfolio/src/portfolio.rs` stores `unrealized_pnls`, `realized_pnls`, and `net_positions`
in `IndexMap`. `accumulate_mark_values` builds an `IndexMap<Currency, Decimal>`.

#### Data engine

`crates/data/src/engine/` uses ordered storage for `book_snapshot_counts`, `bar_aggregators`, and
`BookSnapshotInfos`. Iterated removals use `.shift_remove()`.

#### Execution engine

`ExecutionEngine.clients` uses `IndexMap`. The `client_ids` and `venues` accumulators in
`get_clients_for_orders()` use `IndexSet`.

#### Backtest engine and exchange

`BacktestEngine.venues` and `SimulatedExchange.matching_engines` preserve venue and instrument
iteration order for:

- Settlement.
- Expiration.
- Liquidation.
- Seeded `FillModel` draws.

The source locations are `crates/backtest/src/engine.rs` and `crates/backtest/src/exchange.rs`.

#### Trading algorithm

`strategy_event_handlers` in `crates/trading/src/algorithm/core.rs` uses `IndexMap` to drive ordered
`msgbus::unsubscribe_*` fan-out.

#### Analyzer

`account_balances` and `account_balances_starting` in `crates/analysis/src/analyzer.rs` use
`IndexMap`.

#### Cache API

`get_orders_for_ids` and `get_positions_for_ids` in `crates/common/src/cache/mod.rs` sort returned
vectors by `client_order_id` and `position_id`. The underlying storage keeps `AHashSet` because it
has set semantics.

#### Instrument store

`InstrumentStore.instruments` in `crates/common/src/providers.rs` uses `IndexMap` with the `ahash`
hasher. The order is observable because these adapters publish one `DataEvent::Instrument` per
entry from `get_all()` or `list_all()`:

- Betfair.
- Derive.
- Polymarket.

#### Order emulator

`on_reset` in `crates/execution/src/order_emulator/emulator.rs` sorts three drained sets before
ordered `msgbus::unsubscribe_*` fan-out:

- `subscribed_quotes`
- `subscribed_trades`
- `subscribed_strategies`

The quote and trade paths also advance the seeded `UUID4::new` draw sequence. Storage remains on
`AHashSet`.

#### WebSocket subscriptions

`topics_from_map` in `crates/network/src/websocket/subscription.rs` sorts its returned vector to
preserve reconnect replay order behind `all_topics()` while storage remains on `DashMap` with
`AHashSet` values.

#### Unordered collection limits

`AHashMap` / `AHashSet` sites in the `nautilus-live` closure are lookup-only, behind concurrent
shared-ownership wrappers (`Arc<DashMap>`, `AtomicMap`), or feed into commutative aggregation.
`backtest` contains additional hash collections outside rule 5's two-file enforcement scope,
including pre-run validation and result maps. Treat their iteration order as outside the static
guarantee until each path is audited.

### Time seams

Remaining `Instant::now` and `SystemTime::now` sites are tests, file-allowlisted locations, or
marked exceptions:

| Location                             | Use                                                      | Treatment                                                  |
| ------------------------------------ | -------------------------------------------------------- | ---------------------------------------------------------- |
| `crates/common/src/testing.rs`       | `wait_until` and `wait_until_async` timers.              | Inline `// dst-ok`: test controls use real time by design. |
| `crates/execution/src/engine/mod.rs` | Initialization log timing in `load_cache`.               | Inline `// dst-ok`: timing does not affect DST state.      |
| `crates/common/src/cache/mod.rs`     | Timing in `check_integrity` and `audit_own_order_books`. | File allowlist.                                            |
| `crates/model/src/defi/reporting.rs` | Progress logging.                                        | File allowlist.                                            |
| `crates/core/src/time.rs`            | Wall-clock seam definition.                              | Explicit seam exception.                                   |

`jiff::Timestamp::now` and `jiff::Zoned::now` are hook-banned in the in-scope crates. The remaining
timestamp call sites are the logging bridge and writer, scoped out under
[Logging runs on real OS threads](#logging-runs-on-real-os-threads).
`crates/core/src/datetime.rs::is_within_last_24_hours` routes through
`nautilus_core::time::nanos_since_unix_epoch()` and compares in `u64` nanos directly.

### Randomness seams

Production randomness on the DST path uses three routed sites.

#### UUID generation

`crates/core/src/uuid.rs::UUID4::new()` uses `madsim::rand::thread_rng()` inside a madsim runtime
under simulation. Normal builds and plain `#[rstest]` bodies outside a madsim runtime use
`rand::rng()`.

Production simulation paths run inside the runtime and consume seeded bytes. Order and event
factories in `nautilus-common` and `nautilus-risk` reach this seam.

#### Fill model randomness

`crates/execution/src/models/fill.rs::default_std_rng()` follows the same runtime split.
`ProbabilisticFillState::new()` calls it when no seed is provided. When a caller supplies a seed,
`StdRng::seed_from_u64` is deterministic without the seam.

#### Random matching-engine IDs

The `use_random_ids` path in `crates/execution/src/matching_engine/ids_generator.rs` calls
`nautilus_core::UUID4::new()` for position and venue order IDs. The default ID scheme,
`{venue}-{raw_id}-{count}`, is deterministic without random bytes.

#### Transport-layer exception

One site carries a marker: jitter sampling for reconnect backoff in
`crates/network/src/backoff.rs`, marked `// dst-ok` as transport layer.

### Tokio submodule split

Only `time`, `task`, `runtime`, and `signal` route through `madsim`. The `sync`, `io`, `fs`, and
`net` submodules, plus the `select!` macro, stay on real `tokio` under simulation.

A wider swap would require rebuilding these dependencies against a shimmed
`tokio::net::TcpStream`:

- `tokio-tungstenite`
- `tokio-rustls`
- `reqwest`

That dependency replacement is outside the current scope.

The in-scope direct uses are:

| Location                              | Real Tokio surface                         | Boundary                                                                 |
| ------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------------ |
| `crates/network/src/net.rs`           | `tokio::net::{TcpListener, TcpStream}`     | Re-exports the normal transport behind the `crate::net` seam.            |
| `crates/network/src/socket/client.rs` | `tokio::io::{AsyncReadExt, AsyncWriteExt}` | Performs I/O on the selected transport.                                  |
| `crates/network/src/tls.rs`           | `tokio::io::{AsyncRead, AsyncWrite}`       | Defines TLS I/O bounds.                                                  |
| `crates/network/src/socket/types.rs`  | `tokio::io::{ReadHalf, WriteHalf}`         | Splits `MaybeTlsStream<TcpStream>`; `TcpStream` comes from `crate::net`. |

These paths use real sockets even under simulation. The `tokio::sync` channel implementation also
remains real, but madsim schedules its sender and receiver tasks. Channel delivery order therefore
remains part of the deterministic scheduling contract.

### Raw thread escape rules

Rule 4 of the hook bans raw thread spawning outside three escape cases:

- `#[cfg(test)]` test modules.
- `#[cfg(not(madsim))]` or `#[cfg(not(all(feature = "simulation", madsim)))]` production
  sites (for example, the logging writer thread).
- An inline `// dst-ok` marker.

`tokio::task::LocalSet` and `tokio::task::spawn_blocking` are not supported under
`madsim`. The codebase audit found no production sites for either inside the in-scope
crates; new sites must carry a cfg gate or `// dst-ok` marker.

### Logging tests under simulation

The logging writer thread is cfg-gated out under simulation; under `cfg(madsim)` log
events are dropped. Tests that initialize the file-logging writer would either hang or assert
against an empty log file, so the affected submodules are gated out at the module
boundary:

- `crates/common/src/logging/logger.rs::tests::serial_tests`.
- `crates/common/src/logging/macros.rs::tests`.

`logger.rs::tests::sim_tests::test_init_under_madsim_skips_writer_thread_and_forces_bypass`
runs under simulation and pins the gated behavior.

## Scope boundaries

The contract is limited by the boundaries below. Each subsection identifies behavior that can vary
between runs or remains outside the audited DST path.

### Python and FFI are not in DST scope

DST runs under a native Rust test harness and does not start a Python interpreter. The contract
excludes:

- PyO3 bindings under `crates/*/src/python/`.
- Rust FFI modules under `crates/core/src/ffi/` and `crates/model/src/ffi/`.
- The Python package under `python/nautilus_trader/`.

Code reachable only through these bindings is out of scope. Any Rust path reachable from the native
DST harness must satisfy the contract, even when the same type is also exported through a binding.

The `check-dst-conventions` hook encodes this policy by skipping `/python/` and `/ffi/` paths in the
in-scope crates. Clock, RNG, and threading calls behind those paths do not apply to the contract.

DST primarily covers the order lifecycle, reconciliation, matching, risk, and execution state
machines in the Rust engine. User strategies are replayable only when written in Rust or driven
through a Rust-native test harness.

A Python strategy can vary its command stream by:

- Calling `time.time()`.
- Issuing arbitrary network requests.
- Relying on OS thread scheduling.

The Rust core processes that command stream according to its deterministic contract, but DST does
not guarantee end-to-end replay from a Python entry point.

### Platform-scoped

`madsim`'s libc overrides for `clock_gettime` and `getrandom` are platform-specific. The contract
does not claim cross-platform bitwise reproducibility. A seed that reproduces a failure on Linux
x86_64 may not reproduce it on macOS aarch64.

### Non-aliased dependencies escape silently

A dependency escapes the simulator without an error when it reaches the OS through:

- Direct `libc` calls.
- A `std::net` bypass.
- Unrouted randomness such as `fastrand` or `OsRng`.

The in-scope crates have been audited. Adapter and infrastructure crates require separate audits
before entering the DST path.

### Transport-layer I/O is not simulated

The following dependencies use real `tokio` internally:

- `tokio-tungstenite`
- `tokio-rustls`
- `reqwest`
- `redis`
- `sqlx`

WebSocket and HTTP I/O therefore use real networking under simulation. The contract covers order
lifecycle determinism, while Turmoil provides the separate network simulation described in
[Network seed soaks](#network-seed-soaks). General transport determinism would require per-crate
`madsim` shims that do not exist.

The following test modules drive real localhost sockets and are cfg-gated out under
`all(feature = "simulation", madsim)`:

- `crates/network/src/socket/client.rs::tests`
- `crates/network/src/socket/client.rs::rust_tests`
- `crates/network/src/websocket/client.rs::tests`
- `crates/network/src/websocket/client.rs::rust_tests`
- `crates/network/tests/integration/websocket_proxy.rs`

Their production paths reach madsim time primitives through `dst::time::*`, which panic when called
from a `#[tokio::test]` runtime.

The retry modules in `crates/network/src/retry.rs` run under both runtimes. Their test attributes
switch between `#[tokio::test(start_paused = true)]` and `#[madsim::test]`; time reads and sleeps use
`crate::dst::time`; explicit virtual-time advances use a cfg-gated `advance_clock` function. The same
test bodies therefore cover normal and simulation builds.

### Signal handling

`nautilus_common::live::dst::signal` exposes routed `ctrl_c` and `terminate` re-exports. The run loop
in `crates/live/src/node/mod.rs` uses them. Under `cfg(madsim)`, tests can inject node shutdown through
`madsim::runtime::Handle::send_ctrl_c`. Adapter binary entry points still call
`tokio::signal::ctrl_c` directly and remain out of scope.

### Logging runs on real OS threads

The logging subsystem spawns a writer thread via `std::thread::Builder` and uses
`std::sync::mpsc`. Under simulation, the thread is not spawned and log events are dropped.
Log output is outside the determinism contract: the writer only writes, never reads or mutates
simulation state.

### Adapters

Adapter crates are out of scope. Depending on the adapter, they contain:

- Direct `jiff::Timestamp::now` or `jiff::Zoned::now` calls.
- Direct `SystemTime::now` calls.
- Unrouted RNG calls.
- Raw transport clients.

An adapter must be audited for these sites before the DST contract covers its behavior.

## Relationship to other testing layers

DST complements existing testing; it does not replace any of it.

| Layer                   | Covers                                               | DST relationship                                |
| ----------------------- | ---------------------------------------------------- | ----------------------------------------------- |
| Unit tests              | Pure logic, calculations, parsers, transformers.     | Unchanged.                                      |
| Integration tests       | Component interaction, I/O boundaries.               | Unchanged. DST runs alongside, not in place of. |
| Property-based tests    | Invariants over input domains (parsers, roundtrips). | Unchanged.                                      |
| Acceptance tests        | End-to-end backtest and live scenarios.              | Unchanged.                                      |
| Deterministic sim (DST) | Async timing, scheduling, recovery correctness.      | Adds seed-replayable exploration.               |

DST covers async concurrency and state-machine correctness. Representative failures include:

- A shutdown message dropped under one task wakeup order.
- A reconciliation event lost when iteration order changes.

The other testing layers retain responsibility for their existing scopes.

## Status

### Runtime swap

Layer 1 is implemented. `nautilus_common::live::dst` exposes routed re-exports for `time`, `task`,
`runtime`, and `signal`. Production call sites for `time`, `task`, and `runtime` use the seam.
Signal adoption remains partial; see [Signal handling](#signal-handling).

### Nondeterminism substitution

Layer 2 is implemented across the 17 in-scope crates. Seams cover wall-clock time, monotonic time,
randomness, and observable iteration order. [Implementation notes](#implementation-notes) lists the
audited paths and remaining exceptions.

### Static enforcement status

`check-dst-conventions` runs in pre-commit and CI. It covers the load-bearing structural conditions
and permits reviewed per-line exceptions through `// dst-ok`.

### Runtime verification limit

This repository does not run an end-to-end same-seed diff over an in-scope application path. The
seam design and static checks support the reproducibility contract, but no regression gate verifies
identical observable behavior across complete runs.

### Simulation smoke gate

The dedicated workflow and local pre-flight use the same DST targets:

| Entry point                 | Relevant order                                            | Purpose                                                  |
| --------------------------- | --------------------------------------------------------- | -------------------------------------------------------- |
| `.github/workflows/dst.yml` | `check-code-sim` > `cargo-test-sim`                       | Runs the nightly and manually dispatched DST smoke gate. |
| `make pre-flight`           | `check-code-sim` > `cargo-test-sim` > `cargo-test-extras` | Fails early on DST lint before the Rust test suites.     |

`check-code-sim` runs pinned stable Clippy with `--features simulation` and `cfg(madsim)` across
`nautilus-common`, `nautilus-core`, `nautilus-event-store`, `nautilus-network`,
`nautilus-execution`, and `nautilus-live`.

`cargo-test-sim` uses two feature-coherent nextest invocations:

| Precision | Packages                                                                                                              | Features                    | Selection                                                                                      |
| --------- | --------------------------------------------------------------------------------------------------------------------- | --------------------------- | ---------------------------------------------------------------------------------------------- |
| Standard  | `nautilus-common`, `nautilus-core`, `nautilus-event-store`, `nautilus-network`, `nautilus-execution`, `nautilus-live` | `simulation`                | All compatible common, event-store, network, and execution tests; focused live and core tests. |
| High      | `nautilus-common`, `nautilus-execution`                                                                               | `simulation,high-precision` | All tests in both packages.                                                                    |

Nextest compiles the selected library and test targets, so the gate does not run a separate Cargo
build. The two invocations resolve each feature set once across their package sets. Together they
exercise seam-routed `QuantityRaw` and `PriceRaw` paths at both fixed-point widths: `u64` and `u128`.

#### Common tests

The standard-precision run executes all simulation-compatible `nautilus-common` tests. Its feature
graph propagates `nautilus-core/simulation`, selecting the `wall_clock_now` cfg branch throughout
the suite.

Plain `#[rstest]` bodies run outside a madsim runtime and use the seam's `SystemTime::now()` fallback.
This is the same path madsim's libc shim takes outside a runtime.

The `LiveClock` test module is cfg-gated out because its plain `#[rstest]` cases start `LiveTimer`
tasks without a madsim runtime, and most wait for wall-clock progress.

`live::dst::tests::test_dst_wall_clock_advances_with_virtual_time` runs under `#[madsim::test]` and
asserts that `nanos_since_unix_epoch` advances with `madsim::time::sleep`. This pins virtual
wall-clock behavior inside the runtime.

#### Event store tests

The standard-precision run executes all simulation-compatible `nautilus-event-store` tests. It
exercises the synchronous event and marker writers under `cfg(madsim)`, including deterministic
sequence ordering. Tests that depend on blocking OS threads retain native coverage and are gated
out because those threads run outside madsim's scheduling control.

The static convention hook retains its existing 17-crate production-code scope. This smoke lane
compiles and tests the event store's existing simulation implementation without extending that
contract.

#### Live startup reconciliation

The focused `nautilus-live` regression runs under madsim. It verifies that a pending mass-status
request reaches its configured timeout, reports the expected error, and cleans up the node without
entering a real Tokio timer.

#### Network tests

The run executes all `nautilus-network` tests except transport-bound modules cfg-gated out at the
source. Coverage includes virtual-time seam tests for sleep, timeout, and the rate limiter, plus the
retry suites that exercise backoff timing.

#### Execution tests

The run executes all `nautilus-execution` tests. These plain `#[rstest]` cases exercise cfg-gated
branches in the matching engine, fill model, and execution engine without entering a madsim
runtime. `default_std_rng()` therefore takes its host-RNG fallback in these tests.

#### Core seam tests

The focused `nautilus-core` selection pins `wall_clock_now` against virtual time.

#### Overall gate coverage

`#[madsim::test]` cases in `nautilus-common`, `nautilus-core`, `nautilus-network`, and
`nautilus-live` provide deterministic-scheduler coverage. The complete gate catches drift in the
cfg-gated seams but does not verify end-to-end determinism.

## Further reading

- `.pre-commit-hooks/check_dst_conventions.sh` defines the seven enforcement rules in full and
  documents the `// dst-ok` marker convention.
- [FoundationDB testing philosophy](https://apple.github.io/foundationdb/testing.html).
- [TigerBeetle simulation testing blog posts](https://tigerbeetle.com/blog/).
- [madsim repository](https://github.com/madsim-rs/madsim), the deterministic runtime.
- [Turmoil repository](https://github.com/tokio-rs/turmoil), the deterministic network simulator.
