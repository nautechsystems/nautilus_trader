# Migrate from v1 to v2

NautilusTrader v2 is the Rust core and PyO3 Python package under `python/`. It becomes the primary
Python path when `develop` switches to that package. Until that cutover, the main distribution and
general documentation still describe the legacy v1 Cython package. Use this guide to prepare a
migration.

After cutover, v1 moves to the `develop_v1` branch for approximately three months of critical
security backports. It does not receive new feature or parity work.

The v1 and v2 packages both install and import as `nautilus_trader`, so use a separate virtual
environment for each and never install both into one.

## Install v2

Install a release candidate from PyPI in a fresh environment:

```bash
uv venv --python 3.14
source .venv/bin/activate
uv pip install --pre nautilus_trader
```

Run this block outside a NautilusTrader source checkout. The repository's `exclude-newer` uv policy
can filter out newly published release-candidate wheels.

To build from source, build the package in its dedicated `python/.venv`:

```bash
make build-debug-v2
cd python
.venv/bin/python -c 'import nautilus_trader; print(nautilus_trader.__version__)'
```

The root `.venv` and root `pyproject.toml` belong to the legacy v1 build during the transition.
See [Installation](docs/getting_started/installation.md) for platform support and package-index options.

## Port Python code

Core strategy, data, order, risk, portfolio, backtest, and live workflows remain available. Update
imports and configuration to the new module paths:

| v1 path                                                        | v2 path                                                   |
| -------------------------------------------------------------- | --------------------------------------------------------- |
| `nautilus_trader.backtest.engine.BacktestEngine`               | `nautilus_trader.backtest.BacktestEngine`                 |
| `nautilus_trader.backtest.node.BacktestNode`                   | `nautilus_trader.backtest.BacktestNode`                   |
| `nautilus_trader.live.node.TradingNode`                        | `nautilus_trader.live.LiveNode`                           |
| `nautilus_trader.config.StrategyConfig`                        | `nautilus_trader.trading.StrategyConfig`                  |
| Adapter classes from `nautilus_trader.adapters.<venue>.config` | Rust/PyO3 classes from `nautilus_trader.adapters.<venue>` |

### Common API renames

V2 shortens common strategy and cache names. The `QuoteTick`, `TradeTick`, and
`register_indicator_for_*_ticks` names do not change.

| v1 name                              | v2 name                        |
| ------------------------------------ | ------------------------------ |
| `on_quote_tick`                      | `on_quote`                     |
| `on_trade_tick`                      | `on_trade`                     |
| `on_order_book`                      | `on_book`                      |
| `on_order_book_deltas`               | `on_book_deltas`               |
| `on_order_book_depth`                | `on_book_depth`                |
| `subscribe_quote_ticks`              | `subscribe_quotes`             |
| `subscribe_trade_ticks`              | `subscribe_trades`             |
| `unsubscribe_quote_ticks`            | `unsubscribe_quotes`           |
| `unsubscribe_trade_ticks`            | `unsubscribe_trades`           |
| `request_quote_ticks`                | `request_quotes`               |
| `request_trade_ticks`                | `request_trades`               |
| `subscribe_order_book_deltas`        | `subscribe_book_deltas`        |
| `subscribe_order_book_depth`         | `subscribe_book_depth10`       |
| `subscribe_order_book_at_interval`   | `subscribe_book_at_interval`   |
| `unsubscribe_order_book_deltas`      | `unsubscribe_book_deltas`      |
| `unsubscribe_order_book_depth`       | `unsubscribe_book_depth10`     |
| `unsubscribe_order_book_at_interval` | `unsubscribe_book_at_interval` |
| `request_order_book_snapshot`        | `request_book_snapshot`        |
| `request_order_book_deltas`          | `request_book_deltas`          |
| `request_order_book_depth`           | `request_book_depth`           |
| `cache.quote_tick`                   | `cache.quote`                  |
| `cache.trade_tick`                   | `cache.trade`                  |
| `cache.quote_ticks`                  | `cache.quotes`                 |
| `cache.trade_ticks`                  | `cache.trades`                 |
| `cache.quote_tick_count`             | `cache.quote_count`            |
| `cache.trade_tick_count`             | `cache.trade_count`            |

### Public API migration matrix

V2 uses specific names for component and model identities:

| v1 member          | v2 member                              |
| ------------------ | -------------------------------------- |
| `Actor.id`         | `DataActor.actor_id`                   |
| `Strategy.id`      | `Strategy.strategy_id`                 |
| `ExecAlgorithm.id` | `ExecutionAlgorithm.exec_algorithm_id` |
| Event `id`         | `event_id`                             |
| Report `id`        | `report_id`                            |
| Account `type`     | `account_type`                         |

Collection and lifecycle inspection also changes shape:

| v1 member                      | v2 member                                                     |
| ------------------------------ | ------------------------------------------------------------- |
| `Order.events`                 | `Order.events()`                                              |
| `Position.adjustments`         | `Position.adjustments()`                                      |
| `Position.client_order_ids`    | `Position.client_order_ids()`                                 |
| `Position.events`              | `Position.events()`                                           |
| `Position.trade_ids`           | `Position.trade_ids()`                                        |
| `Position.venue_order_ids`     | `Position.venue_order_ids()`                                  |
| `OrderList.orders`             | `client_order_ids()`, then resolve each ID through the cache  |
| `OrderList.first`              | Resolve `first_client_order_id` through the cache             |
| `Portfolio.initialized`        | `Portfolio.is_initialized()`                                  |
| `Portfolio.analyzer`           | `statistics()`, `snapshots()`, and `nautilus_trader.analysis` |
| `Actor.state`/`Strategy.state` | `DataActor.state()`/`Strategy.state()`                        |
| `ExecAlgorithm.state`          | `ExecutionAlgorithm.state` remains a property                 |
| `Component.is_running`         | `is_running()`                                                |
| `Component.is_stopped`         | `is_stopped()`                                                |
| `Component.is_disposed`        | `is_disposed()`                                               |
| `Component.is_degraded`        | `is_degraded()`                                               |
| `Component.is_faulted`         | `is_faulted()`                                                |

V1 `is_initialized` means that a component has advanced beyond `PRE_INITIALIZED`. V2 `is_ready()`
means exactly `READY`, so it is not an equivalent replacement while a component is running,
stopped, degraded, disposed, or faulted. Inspect `state()` on `DataActor` and `Strategy`, or the
`state` property on `ExecutionAlgorithm`, and compare it with `ComponentState.PRE_INITIALIZED`.

Read the v1 `Strategy` runtime properties `order_id_tag`, `oms_type`, `external_order_claims`,
`manage_contingent_orders`, `manage_gtd_expiry`, `use_uuid_client_order_ids`, and
`use_hyphens_in_client_order_ids` through the same-name properties on `Strategy.config`. The two
client-order-ID formatting options on a strategy-owned `OrderFactory` use the same config. A
standalone factory has no equivalent flag readback.

Historical requests use type-specific batch callbacks in v2:

| v1 data through `on_historical_data` | v2 callback                   | v2 argument                   |
| ------------------------------------ | ----------------------------- | ----------------------------- |
| Custom data                          | `on_historical_data`          | One `CustomData`              |
| Book snapshot                        | `on_book`                     | One `OrderBook`               |
| Book deltas                          | `on_historical_book_deltas`   | `Sequence[OrderBookDelta]`    |
| Book depth                           | `on_historical_book_depth`    | `Sequence[OrderBookDepth10]`  |
| Quote ticks                          | `on_historical_quotes`        | `Sequence[QuoteTick]`         |
| Trade ticks                          | `on_historical_trades`        | `Sequence[TradeTick]`         |
| Funding rates                        | `on_historical_funding_rates` | `Sequence[FundingRateUpdate]` |
| Bars                                 | `on_historical_bars`          | `Sequence[Bar]`               |

Typed historical results no longer fall through to `on_historical_data`; that hook handles custom
data. `on_historical_mark_prices` and `on_historical_index_prices` are available for native batch
delivery, but the current public Python API does not initiate those requests.

The generic `on_event` hook is removed. Replace timer handling with `on_time_event`, aggregate order
handling with `on_order_event`, and aggregate position handling with `on_position_event`. For
custom messaging, use `on_signal` or a typed data subscription instead of overriding `on_event`.

Python v2 `ExecutionAlgorithm` does not inherit the full actor surface. Move market-data and
historical callbacks to `DataActor` or `Strategy`. Its inherited v1 `on_save` and `on_load` hooks
also have no v2 algorithm callback; retain that state in application configuration or move the
stateful component to `DataActor` or `Strategy`. Change `on_order_list(self, order_list)` to
`on_order_list(self, order_list, orders)`.

V2 strategy order changes take client order IDs rather than order objects:

| v1 method                    | v2 method                                  |
| ---------------------------- | ------------------------------------------ |
| `modify_order(order, ...)`   | `modify_order(order.client_order_id, ...)` |
| `cancel_order(order, ...)`   | `cancel_order(order.client_order_id, ...)` |
| `cancel_orders(orders, ...)` | `cancel_orders(client_order_ids, ...)`     |

### Inspection and state renames

V2 exposes consistent read-only inspection across economic instrument types. Properties include
`asset_class`, `instrument_class`, currencies, fees, margins, quantity and price limits,
`multiplier`, and `tick_scheme`; values may be `None` or a documented default.
`SyntheticInstrument` is formula-derived, so inspect its `id`, `components`, `formula`, price
precision and increment, and timestamps instead.

Several v1 inspection names have direct v2 replacements:

| v1 name                                              | v2 name                               |
| ---------------------------------------------------- | ------------------------------------- |
| `instrument.symbol`                                  | `instrument.id.symbol`                |
| `instrument.venue`                                   | `instrument.id.venue`                 |
| `instrument.activation_utc`                          | `instrument.activation_ns`            |
| `instrument.expiration_utc`                          | `instrument.expiration_ns`            |
| `instrument.tick_scheme_name`                        | `instrument.tick_scheme`              |
| `AdaptiveMovingAverage.period` or `.period_er`       | `.period_efficiency_ratio`            |
| `AdaptiveMovingAverage.period_alpha_fast`            | `.period_fast`                        |
| `AdaptiveMovingAverage.period_alpha_slow`            | `.period_slow`                        |
| `LinearRegression.R2`                                | `LinearRegression.r2`                 |
| `DirectionalMovement.value`                          | `.pos` and `.neg`                     |
| `CustomData.data`                                    | `CustomData.value`                    |
| `DataType.type`                                      | `DataType.type_name`                  |
| `OrderBookDelta.is_add/is_clear/is_delete/is_update` | inspect `OrderBookDelta.action`       |
| `OrderBookDeltas.is_snapshot`                        | inspect `OrderBookDeltas.flags`       |
| `BookLevel.side`                                     | use the containing bid or ask context |
| `Bar.is_revision`                                    | removed                               |

`activation_ns` and `expiration_ns` contain UNIX nanoseconds; convert them to the datetime type
used by the application when calendar-time inspection is needed. V1 `DirectionalMovement.value`
never changed from zero, so v2 exposes the meaningful positive and negative outputs instead.

### Config readback and sensitive values

V2 immutable configs expose non-secret constructor values as read-only properties. This includes
engine, backtest venue and run, live reconciliation, and data/execution tester settings.
`LiveRiskEngineConfig.max_notional_per_order` returns v2's validated strings even when constructed
from Python integers or decimal values.

Potential credentials and consumed callbacks use bounded inspection properties instead of raw
readback:

| Constructor field                                    | Inspection property                   |
| ---------------------------------------------------- | ------------------------------------- |
| `BacktestDataConfig.catalog_fs_storage_options`      | `catalog_fs_storage_option_keys`      |
| `BacktestDataConfig.catalog_fs_rust_storage_options` | `catalog_fs_rust_storage_option_keys` |
| `SocketConfig.handler`                               | `has_handler`                         |
| `WebSocketConfig.headers`                            | `header_names`                        |
| `WebSocketConfig.proxy_url`                          | `has_proxy_url`                       |

These raw fields and adapter credentials remain private. Some configs provide `has_*` checks for
credential-bearing proxy, database, or gateway settings without returning their values. Keep
secrets and callbacks in application-owned state if they must be reused.

Betfair configuration moves and flattens in v2:

- `BetfairDataClientConfig` becomes `BetfairDataConfig`, and `BetfairExecClientConfig` becomes `BetfairExecConfig`.
- `BetfairInstrumentProviderConfig` no longer exists as a separate config. Its
  `account_currency`, `default_min_notional`, `event_type_ids`, `event_type_names`, `event_ids`,
  `market_ids`, `country_codes`, `market_types`, `min_market_start_time`, and
  `max_market_start_time` fields move directly onto `BetfairDataConfig`.
- Execution reconciliation uses `BetfairExecConfig.reconcile_market_ids` directly.
  `reconcile_market_ids_only` still controls whether the filter applies.
- `certs_dir` is removed because v2 uses interactive login. The HTTP keepalive interval is fixed
  internally at 36,000 seconds rather than exposed as `keep_alive_secs`.

Databento configuration also changes shape:

- `DatabentoDataClientConfig` becomes `DatabentoLiveClientConfig`. It keeps
  `use_exchange_as_venue`, `bars_timestamp_on_close`, and `venue_dataset_map`, adds the required
  `publishers_filepath`, and accepts `api_key` as a private constructor value.
- The v1 startup preload fields `instrument_ids` and `parent_symbols` are removed. V2 handles live
  subscriptions and historical instrument requests directly instead of configuring an instrument provider preload.
- `http_gateway`, `live_gateway`, `timeout_initial_load`, `mbo_subscriptions_delay`, and
  `reconnect_timeout_mins` are not accepted by the v2 live-node config. Reconnection remains an
  internal client concern; do not copy those v1 fields into v2 config construction.

V1 types from `nautilus_trader.config` move beside their owning runtime. For example,
`BacktestRunConfig` comes from `nautilus_trader.backtest` and `PortfolioConfig` from
`nautilus_trader.portfolio`.

Use the generated type stubs in `python/nautilus_trader/` as the supported Python contract for the
names they declare, not as an exhaustive inventory of runtime-visible names. Two reviewed
class-member exceptions apply:

- Some adapter wire DTOs appear in public signatures or as runtime results. Their readback
  properties are outside the supported member contract and omitted from the stubs.
  `NON_CONTRACT_DTO_CLASSES` in the stub guard records the set.
- `KrakenFuturesHttpClient.edit_orders_batch`, `KrakenFuturesHttpClient.submit_orders_batch`, and
  `KrakenSpotHttpClient.submit_orders_batch` remain callable at runtime but absent from the stubs
  because `pyo3_stub_gen` cannot represent their complex tuple parameter types. Static type
  checkers cannot resolve these methods until the generator supports those parameter types.

The [Python v2 examples][python-v2-examples] show current live-node builders, adapter factories,
strategies, actors, and data/execution testers.

Python v2 strategies subclass `Strategy` and override lifecycle or data callbacks:

```python
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


class MyStrategyConfig(StrategyConfig):
    pass


class MyStrategy(Strategy):
    def on_start(self) -> None:
        pass
```

Annotated custom fields on a v1 `StrategyConfig` subclass do not carry over. In v2, remove custom
keyword arguments in `__new__` before the PyO3 base validates them, then assign the fields in
`__init__`. See the
[v2 strategy config example][python-v2-strategy-config].

### Backtest node post-run inspection

V2 keeps `BacktestNode` engines internal; the v1 `get_engine` and `get_engines` calls are unavailable.
For post-run inspection, set `BacktestRunConfig.dispose_on_completion=False`; the `True` default
drops engine state. Then pass the run config ID to the node inspection methods:

```python
config = BacktestRunConfig(..., dispose_on_completion=False)
node = BacktestNode([config])
results = node.run()

cache = node.get_engine_cache(config.id)
portfolio = node.get_engine_portfolio(config.id)
statistics = portfolio.statistics()
fills = node.generate_fills_report(config.id)
```

These additional reports also take the run config ID first:

- `generate_orders_report`
- `generate_order_fills_report`
- `generate_positions_report`
- `generate_account_report`

### Live node inspection and host-loop integration

V2 exposes the Rust-owned cache and portfolio through `node.cache` and `node.portfolio`. These
shared wrappers provide normal inspection without exposing runtime internals.

Choose the lifecycle method based on who owns the loop:

| Method    | Contract                                                                                    |
| --------- | ------------------------------------------------------------------------------------------- |
| `run()`   | Owns the full lifecycle and blocks until shutdown.                                          |
| `start()` | Completes startup and returns, but does not service post-start channel traffic.             |
| `poll()`  | Processes traffic queued at call entry, returns its count, and does not wait for more.      |
| `stop()`  | Blocks through shutdown and services runner traffic during the residual-event grace period. |

`run()` also owns maintenance, external message-bus ingress, signal handling, and automatic
shutdown. A host that owns its loop must call `start()` once and schedule `poll()` repeatedly:

```python
import asyncio


async def service_live_node(node):
    node.start()
    try:
        while application_running():
            node.poll()
            await asyncio.sleep(0.01)
    finally:
        node.stop()
        node.dispose()
```

`poll()` services time events, execution events, trading commands, data events, and data commands.
Traffic arriving during a call remains queued for the next host cycle. The host decides when to
stop a node in polling mode.

### Order factory configuration readback

`OrderFactory.trader_id` and `strategy_id` remain available. For the v1
`use_uuid_client_order_ids` and `use_hyphens_in_client_order_ids` flags, read `Strategy.config` in a
v2 strategy. Standalone factories provide no equivalent flag readback; retain those values in
application configuration if needed.

### Execution algorithms

Python v2 `ExecutionAlgorithm` remains a routed-order component rather than inheriting the full
`Actor` authoring surface. Supported override points include:

- `on_order`
- Order and position callbacks
- Lifecycle callbacks
- `on_signal`

The runtime owns command routing and calls `execute`; do not call or override `execute` as the
algorithm entrypoint.

The supported authoring surface has these v1 dispositions:

| V1 `ExecAlgorithm` / `Actor` capability | Python v2 contract                                                            |
| --------------------------------------- | ----------------------------------------------------------------------------- |
| `cache`                                 | Available as a read-only property after node or engine registration.          |
| `portfolio`                             | Available as a read-only property after node or engine registration.          |
| `greeks`                                | Construct `GreeksCalculator(self.cache, self.clock)` after registration.      |
| `msgbus`                                | Not exposed; use signals for supported custom messaging.                      |
| Registered indicators                   | Use `DataActor` or `Strategy` for indicator-driven workflows.                 |
| Market-data subscriptions and callbacks | Use `DataActor` or `Strategy`; algorithms inspect cache and routed events.    |
| Lifecycle state and control             | Use `is_*()` and lifecycle methods; the Rust component remains authoritative. |
| Direct `register(...)`                  | Use `BacktestEngine.add_exec_algorithm` or `LiveNode.add_exec_algorithm`.     |

Signals replace direct message-bus access on Python v2 `DataActor`, `Strategy`, and
`ExecutionAlgorithm`:

- Call `subscribe_signal(name)` during `on_start`.
- Handle `on_signal(signal)`.
- Call `publish_signal(name, value)`.

Signal values use their string representation. Raw message-bus endpoints and handlers remain
runtime internals.

```python
from nautilus_trader.common import GreeksCalculator
from nautilus_trader.trading import ExecutionAlgorithm


class RoutedAlgorithm(ExecutionAlgorithm):
    def on_start(self) -> None:
        self._greeks = GreeksCalculator(self.cache, self.clock)
        self.subscribe_signal("execution-control")

    def on_signal(self, signal) -> None:
        self.log.info(f"Received {signal.value}")

    def on_order(self, order) -> None:
        instrument = self.cache.instrument(order.instrument_id)
        portfolio_ready = self.portfolio.is_initialized()
        self.log.info(f"Routing {instrument.id}; portfolio ready={portfolio_ready}")
```

Constructed instances and importable configs work in backtest and live workflows:

- Register v2 `ExecutionAlgorithm` instances with `LiveNode.add_exec_algorithm`.
- Register DataActor-based compatibility algorithms with `add_exec_algorithm_from_config`.

Nodes normally drive lifecycle transitions. Direct lifecycle methods remain available for
control-plane integrations and dispatch the same Python callbacks.

Port one workflow at a time and verify the generated stub before replacing a v1 convenience method.
Do not assume that a v1 adapter config field also exists on its v2 Rust config.

## Accepted contract differences

The cutover accepts these differences from v1:

- Custom data flows as native `CustomData` without the v1 wrapper semantics.
- v2 caches `OptionGreeks` for option fee calculation; this extends v1.
- `Bar.is_revision` is not exposed on the v2 Python surface. Do not depend on it during migration.
- A direct `Position.apply` fill that crosses zero resets the open entry price to the flipping fill.
  v1 retains the old side's entry price; the v2 behavior is the go-forward contract.
- `PortfolioConfig.use_mark_prices` defaults to `true`; v1 defaulted to `false`. Set it to `false` to
  skip mark prices.
- v2 `OrderList` stores client order IDs instead of order objects:
  - Replace `order_list.orders` with `order_list.client_order_ids()`, then resolve each ID through
    `cache.order(client_order_id)`.
  - Replace `order_list.first` with `cache.order(order_list.first_client_order_id)` after checking
    the ID is not `None`.
- Catalog order-event data written before `activation_price` and `OrderFilled.info` were added cannot
  be read by the new schema. Regenerate or migrate that data before upgrading a catalog in place.

## Deferred limits

These gaps can affect migration but do not block supported cutover workflows:

- Python request callback, join, and pending-request convenience semantics are not complete.
- Python cannot inject Redis cache databases or external message-bus backing factories into
  `LiveNode`; Rust builders still expose those backings.
- SQL cache position and synthetic loads, actor and strategy state persistence, and heartbeat remain
  incomplete. The audited restart workflow uses the Redis backing through Rust builders; Python
  `LiveNode` configuration cannot select that backing yet.
- External message-bus publishing of serialized order and position snapshots remains deferred.
- V2 `BacktestNode` does not yet support the v1 `StreamingConfig` and `DataCatalogConfig` iterator
  workflow.
- Instrument-provider filter dictionaries are not a common v2 adapter contract. Hyperliquid v2
  loads its configured instrument universe and does not accept the v1 `instrument_provider` field.
  Check each adapter's Rust/PyO3 config rather than copying v1 provider examples.
- The published quickstart and backtesting tutorials still use v1 imports and configuration. Use the
  generated v2 stubs, the [v2 backtest acceptance tests][python-v2-backtest-tests] for backtesting,
  and [Python v2 examples][python-v2-examples] for live and adapter workflows while those tutorials
  are ported.

The [v2 roadmap][v2-roadmap] tracks the wider post-cutover surface. Release-specific breaking
changes remain in [RELEASES.md][release-notes].

[python-v2-backtest-tests]: python/tests/acceptance/test_backtest.py
[python-v2-examples]: python/examples/
[python-v2-strategy-config]: python/tests/strategies/ema_cross.py
[release-notes]: RELEASES.md
[v2-roadmap]: https://github.com/nautechsystems/nautilus_trader/issues/4042
