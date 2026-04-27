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

The `check-dst-conventions` hook enforces `IndexMap` / `IndexSet` in
`crates/live/src/manager.rs` and `crates/execution/src/matching_engine/engine.rs`.
The 16 in-scope crates host `AHashMap` and `AHashSet` in ~85 other files, used for:

- cache indexes (`crates/common/src/cache/mod.rs`)
- order book state (`crates/model/src/orderbook/`)
- account balances (`crates/model/src/accounts/`)
- portfolio state (`crates/portfolio/src/`)
- execution engine state (`crates/execution/src/engine/`)
- data aggregation (`crates/data/src/aggregation.rs`)
- msgbus switchboard (`crates/common/src/msgbus/switchboard.rs`)

| Area                                                       | Tag        |
|------------------------------------------------------------|------------|
| `AHashMap` / `AHashSet` in `manager.rs`                    | closed     |
| `AHashMap` / `AHashSet` in `matching_engine/engine.rs`     | closed     |
| `Position::commissions` in `model/src/position.rs`         | closed     |
| `model/src/orderbook/` AHashSet usage                      | closed     |
| `model/src/orders/` AHashSet usage                         | closed     |
| `model/src/accounts/` balances + margins fields            | closed     |
| `core/src/collections.rs` AHash sites                      | closed     |
| `core/src/serialization.rs` AHash sites                    | closed     |
| `data/engine/` bar aggregator + book snapshot maps         | closed     |
| `execution/engine/` clients + reconciliation result maps   | closed     |
| `trading/algorithm/core.rs` strategy event handlers        | closed     |
| `common/cache/mod.rs` orders/positions Vec returns         | closed     |
| `portfolio/portfolio.rs` PnL aggregation maps              | closed     |
| `analysis/analyzer.rs` account balance maps                | closed     |
| `AHashMap` / `AHashSet` elsewhere                          | closed     |

**Notes.** `AHash` randomizes its hasher per process. Iteration order over
these collections varies across runs. `manager.rs` and
`matching_engine/engine.rs` are the two surfaces where iteration order was
known to drive observable state, so both files are hook-enforced.

The matching-engine closure flipped nine fields to `IndexMap`
(`execution_bar_types`, `execution_bar_deltas`, `account_ids`,
`cached_filled_qty`, `bid_consumption`, `ask_consumption`, `queue_ahead`,
`queue_excess`, `queue_pending`) and replaced `.remove()` on the iterated
queue maps with `.shift_remove()` to preserve insertion order across
deletes. This closes issue
[#3914](https://github.com/nautechsystems/nautilus_trader/issues/3914): the
seeded `FillModel` RNG is now consumed against the same resting-order
sequence across runs, restoring the determinism promise in the backtesting
guide.

The `model` crate audit closed three rows. `Position::commissions` flipped
to `IndexMap` because `events/position/snapshot.rs` builds the
`PositionSnapshot.commissions` `Vec<Money>` from `position.commissions
.values()`, and the snapshot is an observable event on the DST path.
`OwnBookLadder.cache` flipped to `IndexMap<ClientOrderId, BookPrice>`
(with `.shift_remove()` on iterated removals) because
`OwnOrderBook::bid_client_order_ids()` and `ask_client_order_ids()`
collect `cache.keys()` into public `Vec<ClientOrderId>`s. The remaining
`orderbook/` `AHashSet<OrderStatus>` parameters are membership filters
only. The `orders/` files use `AHashSet` for duplicate detection and a
static cancellable-status set for O(1) membership; neither is iterated.

The `model/src/accounts/` audit closed the trait-level scope hole. The
`Account` trait now returns `IndexMap` from `balances`, `balances_total`,
`balances_free`, `balances_locked`, and `starting_balances`.
`BaseAccount.balances` and `BaseAccount.balances_starting` flipped to
`IndexMap`, `MarginAccount.margins` and `MarginAccount.account_margins`
flipped to `IndexMap`. The trigger was
`portfolio/src/manager.rs::generate_account_state` building
`AccountState.balances` and `AccountState.margins` `Vec`s from
`.values()` iteration on those fields, so the regenerated event ordering
is now deterministic. `BaseAccount.commissions`, `MarginAccount.leverages`,
`CashAccount.balances_locked`, and `BettingAccount.balances_locked` stay
on `AHashMap`: lookup-only or no observable in-scope iteration.

The `core/src/collections.rs` and `core/src/serialization.rs` audits
closed the remaining structural sites in the `core` crate without code
changes. `AtomicMap<K, V>` and `AtomicSet<K>` wrap
`ArcSwap<AHashMap<K, V>>` / `ArcSwap<AHashSet<K>>` and belong to the
concurrent shared-ownership family alongside `Arc<DashMap>` and
`Arc<RwLock<AHashMap>>`; consumers (e.g. the Bybit websocket client's
`bar_types_cache` and `instruments_cache`) use them for snapshot
`.get()` lookups, so iteration order is not on the DST path. The
`MapLike for AHashMap` and `SetLike for AHashSet` impls plus the
`From<AHashMap>` / `From<AHashSet>` conversions are stability surfaces
for callers that hold AHash data. The `serialization::sorted_hashset`
module produces deterministic output by construction:
`serialize` collects into `Vec<&T>` and calls `sort_unstable()` before
emitting, so the source iteration order is erased.

The `data/engine/` audit closed three fields. `book_snapshot_counts`
flipped to `IndexMap` because `subscribed_book_snapshots()` returns
`book_snapshot_counts.keys()` collected into a public `Vec<InstrumentId>`.
`bar_aggregators` flipped to `IndexMap` because `start()`, `stop()`, and
`reset()` iterate `.values()` and `.keys()` to drive timer
start/stop/restart on the simulation clock. The `BookSnapshotInfos` type
alias (`Rc<RefCell<AHashMap<InstrumentId, BookSnapshotInfo>>>`) flipped
to `IndexMap` because `BookSnapshotter::snapshot()` iterates `.values()`
on each timer tick to publish snapshots on the message bus. Iterated
removes were converted to `.shift_remove()`. `bar_aggregator_handlers`,
`book_intervals`, `book_updaters`, `book_snapshotters`,
`option_chain_managers`, `book_deltas_subs`, `book_depth10_subs`, and
`pending_option_chain_requests` stay on `AHash`: lookup or
membership-only.

The `execution/engine/` audit closed two surfaces. `ExecutionEngine.clients`
flipped to `IndexMap` because `client_ids()`, `get_clients_mut()`, and
`get_all_clients()` return `Vec`s built from `.keys()` / `.values()`,
and `get_clients_for_orders()` iterates these to fan out commands.
The local `client_ids: AHashSet<ClientId>` and `venues: AHashSet<Venue>`
in `get_clients_for_orders()` flipped to `IndexSet` so the routing-fan-out
order is preserved. `ReconciliationResult.orders` and
`ReconciliationResult.fills` flipped to `IndexMap` because
`crates/live/src/manager.rs::adjust_fills_using_position_reports`
iterates `result.orders` and `result.fills` to populate `final_orders`
/ `final_fills` (both already `IndexMap`); the upstream chain
`mass_status.order_reports() -> extract_*_for_instrument -> result`
is now end-to-end deterministic. `submit_order_commands` in
`order_manager/manager.rs` stays on `AHash`: every consumer
(`emulator.rs`) uses `.contains_key()` / `.pop_submit_order_command()`,
no iteration on the DST path. `external_clients`, `external_order_claims`,
`oms_overrides`, and `routing_map` stay on `AHash` / `HashMap`:
lookup-only.

The `trading/algorithm/core.rs::strategy_event_handlers` field flipped
to `IndexMap` because `unsubscribe_all_strategy_events` in
`algorithm/mod.rs` calls `take_strategy_event_handlers()` and then
iterates the returned map to fire `msgbus::unsubscribe_*` for each
strategy. The unsubscribe order is observable on the message bus.
Sibling fields `exec_spawn_ids`, `subscribed_strategies`, and
`pending_spawn_reductions` stay on `AHash`: lookup / membership-only.

The `common/cache/mod.rs` audit kept the `CacheIndex` itself on
`AHashSet` (set semantics, fast lookup) but made the public Vec returns
deterministic at the cache API boundary. `get_orders_for_ids` and
`get_positions_for_ids` now sort their output by `client_order_id` and
`position_id` before returning, which propagates to every caller of
`cache.orders*()` and `cache.positions*()` (notably the own-book
replay at `execution/engine/mod.rs::load_cache` and the per-algorithm
cancel cascade at `trading/strategy/mod.rs`). The `actor_ids()`,
`strategy_ids()`, and `exec_algorithm_ids()` returns stay typed as
`AHashSet` to preserve set semantics; the single iterator on the DST
path (`strategy/mod.rs` cancel cascade) sorts the IDs locally before
fan-out. This pattern matches the existing `msgbus/core.rs`
`matching_subscriptions` which `.sort()`s the candidate Vec before
dispatch.

The `portfolio/portfolio.rs` audit closed seven surfaces. The storage
fields `unrealized_pnls`, `realized_pnls`, and `net_positions` flipped
to `IndexMap`, with `.shift_remove()` for the one iterated delete in
`update_instrument_id`. The aggregation entry points
`unrealized_pnls()`, `realized_pnls()`, `net_exposures()`,
`total_pnls()`, `mark_values()`, and `equity()` now build their
currency accumulators in `IndexMap` (via the `accumulate_mark_values`
helper updated to take `&mut IndexMap<Currency, f64>`); the
`instrument_ids` dedup inside `unrealized_pnls()`, `realized_pnls()`,
and `equity()` flipped to `IndexSet` so the deterministic order from
the now-sorted `cache.positions(...)` flows through to the returned
`IndexMap<Currency, Money>`. The `mark_values` and `equity` `unpriced`
locals stay on `AHashSet` (membership-only). The `xrate_cache` local
inside `accumulate_mark_values` stays on `AHashMap` (lookup-only).
`venues_missing_price`, `snapshot_*`, `bar_close_prices`,
`pending_calcs`, and `last_account_state_log_ts` stay on `AHash`:
lookup or per-instrument membership only.

The `analysis/analyzer.rs` audit flipped two fields. `PortfolioAnalyzer
.account_balances` and `account_balances_starting` now hold `IndexMap`
storage so the chain `BaseAccount.balances` (already `IndexMap`) ->
`account.balances_total()` -> `analyzer.account_balances` ->
`analyzer.currencies()` -> `BacktestEngine::run` per-currency stats
iteration is end-to-end deterministic. `realized_pnls` stays on
`AHashMap`: keyed lookup only, never iterated.

The `AHashMap` / `AHashSet elsewhere` catch-all row is closed by audit:
every remaining site in the in-scope crates was verified to be
lookup-only, behind a concurrent shared-ownership wrapper, fed only into
a commutative aggregation, or sorted before observable use. Sites
covered: `crates/common/cache/{database,quote,fifo}.rs`,
`actor/data_actor.rs`, `actor/registry.rs` (Debug only),
`msgbus/switchboard.rs`, `msgbus/core.rs` (dispatch sorts before fan-out),
`factories/`, `defi/cache.rs`, `defi/switchboard.rs`, `greeks.rs`,
`xrate.rs` (DFS commutative result), `clock.rs`, `component.rs`,
`logging/{logger.rs,mod.rs}` (sorts before observable use), plus
`crates/data/aggregation.rs`, `crates/risk/`, `crates/system/trader.rs`
(commutative time advancement), and the `crates/trading/examples/`
strategies. `crates/network/` AHash sites sit behind concurrent
containers (`Arc<DashMap>` / `AtomicMap`). `crates/persistence/` catalog
write order is a separate reproducibility concern outside the
live-event determinism contract. Any new in-scope `AHashMap` /
`AHashSet` site that drives observable iteration order is now a
regression that the per-area audit notes above protect against.

## Randomness

### Raw RNG sources

| Source                 | Production sites                                     | Tag        |
|------------------------|------------------------------------------------------|------------|
| `rand::thread_rng`     | 0                                                    | closed     |
| `fastrand`             | 0                                                    | closed     |
| `getrandom`            | 0                                                    | closed     |
| `OsRng`                | 0                                                    | closed     |
| `rand::rng()`          | `network/backoff.rs:105`                             | scoped‑out |
| `Uuid::new_v4` (tests) | `core/uuid.rs:380`                                   | closed     |
| `Uuid::new_v4` (prod)  | `execution/matching_engine/ids_generator.rs:167,179` | closed     |

**Notes.** `rand::rng()` is the `rand` 0.9 replacement for `rand::thread_rng`
and draws from the same per-thread CSPRNG. Rule 2 of the hook now matches
`rand::rng()` and skips lines whose preceding 15 lines carry a non-simulation
cfg gate, so cfg-gated production fallbacks pass.

Closed call sites:

- `crates/core/src/uuid.rs:56` in `UUID4::new()` now routes through
  `madsim::rand::thread_rng()` under `cfg(all(feature = "simulation", madsim))`
  and `rand::rng()` under the negation. Reachable from production factories
  such as `crates/common/src/factories/order.rs`,
  `crates/common/src/messages/execution/report.rs`, and
  `crates/risk/src/engine/mod.rs`.
- `crates/execution/src/models/fill.rs:129` `default_std_rng()` now routes
  through `madsim::rand::thread_rng()` under
  `cfg(all(feature = "simulation", madsim))` and `rand::rng()` under the
  negation. Called from `ProbabilisticFillState::new()` when constructed
  with `random_seed=None`. When a seed is provided the model routes through
  `StdRng::seed_from_u64`, which is deterministic.

Allowed-with-marker call sites:

- `crates/network/src/backoff.rs:105` for reconnect jitter. Marked
  `// dst-ok`: transport-layer, out of DST scope per
  `nautilus_dst/docs/compatibility_matrix.md`.

`Uuid::new_v4` was the separate `uuid` crate path in the matching engine ID
generator, gated behind `use_random_ids` on `IdsGenerator`. The default ID
scheme is deterministic (`{venue}-{raw_id}-{count}`). The two `use_random_ids`
branches now route through `nautilus_core::UUID4::new()`, which already
draws from `madsim::rand::thread_rng()` under `cfg(all(feature =
"simulation", madsim))` and from `rand::rng()` under the negation, so the
non-default ID scheme is also deterministic on the DST path. Rule 2 of the
hook now bans bare `Uuid::new_v4()` in the in-scope crates; call sites that
remain intentionally non-deterministic must mark with `// dst-ok` and a
reason. Adapter-crate RNG sources (`Uuid::new_v4`,
`chrono::Utc::now`-seeded paths, venue-side randomness) remain
`scoped-out` per "Adapter I/O surfaces" above.

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
| `core/datetime.rs:404` `is_within_last_24_hours`           | closed      |

**Notes.** Rule 1 of the hook now matches `chrono::Utc::now` in addition
to `Instant::now` and `SystemTime::now`. The logging bridge and writer
sites stay scoped out under "Logging runs on real OS threads" in
`docs/concepts/dst.md` and are entered in the file-level
`RULE1_ALLOWLIST` (logging is outside the determinism contract, the writer
thread is cfg-gated out under `cfg(madsim)`, and log events are dropped).
The helper in `crates/core/src/datetime.rs:404` was reachable from
non-logging call paths and was a real scope hole; it now goes through
`nautilus_core::time::nanos_since_unix_epoch()` and compares directly in
`u64` nanos, dropping the chrono round-trip. Any new `chrono::Utc::now`
call on the DST path now fails pre-commit unless it carries
`// dst-ok` with a reason.

## Deferred items

### Signal handling call-site migration

`nautilus_common::live::dst::signal::ctrl_c` re-exports the deterministic
`ctrl_c` shim. The `crates/live/src/node.rs` run loop now routes through it.

| Area                                           | Tag        |
|------------------------------------------------|------------|
| `nautilus_common::live::dst::signal` re‑export | closed     |
| `crates/live/src/node.rs` call site            | closed     |
| Adapter‑bin `ctrl_c` sites                     | scoped‑out |

**Effect on the contract.** Under `simulation` + `cfg(madsim)`, node shutdown
driven by `ctrl_c` is now injectable from test code via
`madsim::runtime::Handle::send_ctrl_c`.

**Mitigation.** Documented in `docs/concepts/dst.md` under "Signal handling".
The `crates/live/src/node.rs` run loop has been migrated; remaining sites
are adapter-bin entry points which stay scoped-out.

### Logger file-logging tests under `cfg(madsim)`

Under `RUSTFLAGS="--cfg madsim" cargo test --features simulation` on
`nautilus-common`, the tests that exercise the logging writer thread fail
because the thread is cfg-gated out and log events are dropped.

| Area                                     | Tag    |
|------------------------------------------|--------|
| Logger writer thread under `cfg(madsim)` | gated  |
| File‑logging tests under `cfg(madsim)`   | closed |

**Effect on the contract.** Log output is explicitly outside the determinism
contract (the writer writes, never reads or mutates simulation state), so
losing it under simulation does not weaken the contract. The test failures
were a reflection of the gate, not a correctness issue in the gate itself.

**Mitigation.** The affected tests are cfg-gated out under
`cfg(all(feature = "simulation", madsim))`. The gate is applied at the
submodule boundary in both call sites:

- `crates/common/src/logging/logger.rs::tests::serial_tests` (the eight
  file-logging tests that init the logger and read the rotated log file).
- `crates/common/src/logging/macros.rs::tests` (the two macro tests that
  init the logger and assert on log file contents).

Both submodules are wholly file-logging-specific, so the module-level gate
matches the surface that needs to skip. Under simulation, `cargo nextest run`
on `nautilus-common --features simulation` reports the file-logging tests
as filtered out; the writer-gate test (`sim_tests::test_init_under_madsim_
skips_writer_thread_and_forces_bypass`) still runs and pins the gated
behaviour.

Option B (an inline log-event sink under `cfg(madsim)` so log tests can
observe the channel without a writer thread) is deferred to `nautilus_dst`,
where the determinism regression gate (Phase 1 Item 1 of the sign-off plan)
exercises the logging subsystem.

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
| closed     | 36    |
| gated      | 10    |
| scoped‑out | 24    |
| unresolved | 1     |

Unresolved entries at the end of this phase:

1. Dynamic same-seed diff harness

The remaining unresolved item lives in `nautilus_dst`.

Adapter crates and Python / FFI bindings remain `scoped-out`. A per-adapter
audit must land before any individual adapter can enter the DST path, but
the audit is not itself on the critical path for this phase. Python paths
are out of scope as a permanent policy, not a pending audit: DST runs
under a native Rust harness and no Python interpreter starts during a DST
run.
