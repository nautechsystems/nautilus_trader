# Migrate From v1 to v2

Use this guide to port the legacy v1 Cython package to the v2 Rust core and PyO3 Python package on `develop`.
The v2 Python package lives under `python/`.

Legacy v1 lives on `develop_v1` and receives only critical security backports for approximately three months
after the v2 cutover. It receives no feature or parity work.

> **Important:** Both versions install and import as `nautilus_trader`. Use separate virtual environments;
> never install v1 and v2 into the same environment.

Port one workflow at a time. Start with [imports and configuration](#update-imports-and-configuration), then port the
[components](#port-actors-strategies-and-algorithms) and [model APIs](#update-model-and-portfolio-code) your
workflow uses. Review [node setup](#update-backtest-and-live-nodes), [persisted
data](#migrate-catalogs-and-streaming), and the [known limitations](#known-limitations) before cutover.

For PostgreSQL-backed nodes, complete the [database migration](#migrate-postgresql-databases) before startup.
Rust callers also need the [Rust integration changes](#update-rust-integrations).

## Install v2

Outside a source checkout, install a PyPI release candidate in a fresh environment:

```bash
uv venv --python 3.14
source .venv/bin/activate
uv pip install --pre nautilus_trader
```

The repository's `exclude-newer` uv policy can filter out newly published release-candidate wheels.

To build from source, run from the repository root:

```bash
make build-debug
uv run --project python --no-sync python -c \
  'import nautilus_trader; print(nautilus_trader.__version__)'
```

The source build uses the `python/.venv` and `target/` directories. See
[Installation](docs/getting_started/installation.md) for platform support and package-index options.

## Update imports and configuration

### Import paths

Core strategy, data, order, risk, portfolio, backtest, and live workflows remain available. Update imports:

| v1 path                                                        | v2 path                                                   |
| -------------------------------------------------------------- | --------------------------------------------------------- |
| `nautilus_trader.backtest.engine.BacktestEngine`               | `nautilus_trader.backtest.BacktestEngine`                 |
| `nautilus_trader.backtest.node.BacktestNode`                   | `nautilus_trader.backtest.BacktestNode`                   |
| `nautilus_trader.live.node.TradingNode`                        | `nautilus_trader.live.LiveNode`                           |
| `nautilus_trader.model.enums.OrderSide`                        | `nautilus_trader.model.OrderSide`                         |
| `nautilus_trader.model.identifiers.TraderId`                   | `nautilus_trader.model.TraderId`                          |
| `nautilus_trader.config.StrategyConfig`                        | `nautilus_trader.config.StrategyConfig`                   |
| Adapter classes from `nautilus_trader.adapters.<venue>.config` | Rust/PyO3 classes from `nautilus_trader.adapters.<venue>` |

### Config and type names

Import core configs from `nautilus_trader.config` and adapter configs from the adapter's public module, such
as `nautilus_trader.adapters.databento`:

| v1 config             | v2 config                  |
| --------------------- | -------------------------- |
| `ActorConfig`         | `DataActorConfig`          |
| `ExecAlgorithmConfig` | `ExecutionAlgorithmConfig` |
| `ExecEngineConfig`    | `ExecutionEngineConfig`    |
| `LoggingConfig`       | `LoggerConfig`             |
| `TradingNodeConfig`   | `LiveNodeConfig`           |

`ControllerConfig` has no direct replacement: define controller fields on a `DataActorConfig` subclass, then
refer to that class through `ImportableControllerConfig`.

V2 uses `Execution` in project-owned type names and omits `Live` from ordinary client names:

| v1 or earlier v2 name           | v2 name                              |
| ------------------------------- | ------------------------------------ |
| `<Venue>ExecClientConfig`       | `<Venue>ExecutionClientConfig`       |
| `BetfairDataConfig`             | `BetfairDataClientConfig`            |
| `BetfairExecConfig`             | `BetfairExecutionClientConfig`       |
| `DatabentoLiveClientConfig`     | `DatabentoDataClientConfig`          |
| `LiveDataClientConfig`          | `DataClientConfig`                   |
| `LiveExecClientConfig`          | `ExecutionClientConfig`              |
| `LiveExecEngineConfig`          | `LiveExecutionEngineConfig`          |
| `ImportableExecAlgorithmConfig` | `ImportableExecutionAlgorithmConfig` |
| `ExecFactoryExtractor`          | `ExecutionFactoryExtractor`          |
| `SimExecFactoryExtractor`       | `SimulatedExecutionFactoryExtractor` |

`ExecAlgorithmId`, its associated `exec_*` fields, and `ExecTester` retain their established names. Venue
protocol terms such as `ExecType` also remain unchanged. The extractor aliases are Rust extension APIs under
`nautilus_system::python::registry`.

Replace v1 `NautilusConfig`, `NautilusKernelConfig`, config factories, and encoding/path utilities with
concrete configs and registration methods. V2 `ImportableConfig` and `ImportableFactoryConfig` support
client config and factory import paths; see the [Python adapter guide](docs/developer_guide/python_adapters.md).

### Python config subclasses

Python v2 strategies subclass `Strategy` and override lifecycle or data callbacks:

```python
from nautilus_trader.config import StrategyConfig
from nautilus_trader.trading import Strategy


class MyStrategyConfig(StrategyConfig):
    pass


class MyStrategy(Strategy):
    def on_start(self) -> None:
        pass
```

V1 configs are msgspec `Struct` classes; v2 configs are Rust/PyO3 types. Update imports and subclass
constructors. Annotated custom fields on v1 `StrategyConfig` subclasses do not carry over.

For custom v2 fields, declare keyword-only `__init__` arguments, accept `**_kwargs` for base keywords, and
call `super().__init__()` without arguments. The base reads its fields in `__new__` and ignores unrecognized
keywords; no `__new__` override is needed. Never reuse a base field name. See the [strategy config
example][python-v2-strategy-config].

### Config readback and secrets

Immutable configs expose non-secret constructor values as read-only properties, including engine, backtest
venue/run, live reconciliation, and data/execution tester settings.
`LiveRiskEngineConfig.max_notional_per_order` returns validated strings even for integer or decimal inputs.

Potential credentials use bounded inspection properties instead of raw readback:

| Constructor field                                    | Inspection property                   |
| ---------------------------------------------------- | ------------------------------------- |
| `BacktestDataConfig.catalog_fs_storage_options`      | `catalog_fs_storage_option_keys`      |
| `BacktestDataConfig.catalog_fs_rust_storage_options` | `catalog_fs_rust_storage_option_keys` |

Raw storage-option fields and adapter credentials remain private. Some configs offer `has_*` checks for proxy,
database, or gateway credentials without returning values. Retain reusable secrets in application state.

### Betfair

- `BetfairDataClientConfig` remains the data factory input, while `BetfairExecClientConfig` becomes
  `BetfairExecutionClientConfig`.
- `BetfairInstrumentProviderConfig` no longer exists as a separate config. Its
  `account_currency`, `default_min_notional`, `event_type_ids`, `event_type_names`, `event_ids`,
  `market_ids`, `country_codes`, `market_types`, `min_market_start_time`, and
  `max_market_start_time` fields move directly onto `BetfairDataClientConfig`.
- Execution reconciliation uses `BetfairExecutionClientConfig.reconcile_market_ids` directly.
  `reconcile_market_ids_only` still controls whether the filter applies.
- Rename `stream_heartbeat_ms` to `stream_heartbeat_secs` and `stream_idle_timeout_ms` to
  `stream_heartbeat_timeout_secs`, then convert configured values from milliseconds to seconds.
- `certs_dir` is removed because v2 uses interactive login. The HTTP keepalive interval is fixed
  internally at 36,000 seconds rather than exposed as `keep_alive_secs`.

### Databento

- `DatabentoDataClientConfig` remains the factory input. It keeps
  `use_exchange_as_venue`, `bars_timestamp_on_close`, and `venue_dataset_map`, adds the required
  `publishers_filepath`, and accepts `api_key` as a private constructor value.
- The v1 startup preload fields `instrument_ids` and `parent_symbols` are removed. V2 handles live
  subscriptions and historical instrument requests directly instead of configuring an instrument
  provider preload.
- `http_gateway`, `live_gateway`, `timeout_initial_load`, `mbo_subscriptions_delay`, and
  `reconnect_timeout_mins` are not accepted by the Python `DatabentoDataClientConfig` constructor.
  Reconnection stays internal to the client.

### Interactive Brokers

| V1 field or alias         | V2 replacement                                                                  |
| ------------------------- | ------------------------------------------------------------------------------- |
| `legacy_market_data_type` | Pass `market_data_type` to `InteractiveBrokersDataClientConfig`.                |
| `legacy_load_ids`         | Pass `load_ids` to `InteractiveBrokersInstrumentProviderConfig`.                |
| `legacy_load_contracts`   | Pass `load_contracts` to the instrument provider config.                        |
| `legacy_symbology_method` | Pass `symbology_method` to the instrument provider config.                      |
| `pickle_path`             | Pass or set `cache_path` on the instrument provider config.                     |
| `routing`                 | Pass `RoutingConfig` to `LiveNodeBuilder.add_data_client` or `add_exec_client`. |
| `dockerized_gateway`      | Start the gateway outside v2, then pass its `host` and `port`.                  |

V2 retains writable `instrument_provider` fields on the data and execution client configs and `cache_path` on
the provider config. A non-`None` `dockerized_gateway` is rejected because Python v2 does not own the
container lifecycle.

### Bybit

`bybit_bar_spec_to_interval` takes `BarAggregation` and step; v1 took the aggregation's integer value and
step.

## Port actors, strategies, and algorithms

### Strategy and cache renames

`QuoteTick`, `TradeTick`, and `register_indicator_for_*_ticks` retain their names:

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
| `subscribe_order_book_depth`         | `subscribe_book_depth`         |
| `subscribe_order_book_at_interval`   | `subscribe_book_at_interval`   |
| `unsubscribe_order_book_deltas`      | `unsubscribe_book_deltas`      |
| `unsubscribe_order_book_depth`       | `unsubscribe_book_depth`       |
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

### Identity and lifecycle inspection

Replace generic identity names:

| v1 member          | v2 member                              |
| ------------------ | -------------------------------------- |
| `Actor.id`         | `DataActor.actor_id`                   |
| `Strategy.id`      | `Strategy.strategy_id`                 |
| `ExecAlgorithm.id` | `ExecutionAlgorithm.exec_algorithm_id` |
| Event `id`         | `event_id`                             |
| Report `id`        | `report_id`                            |
| Account `type`     | `account_type`                         |

| v1 member                      | v2 member                                     |
| ------------------------------ | --------------------------------------------- |
| `Actor.state`/`Strategy.state` | `DataActor.state()`/`Strategy.state()`        |
| `ExecAlgorithm.state`          | `ExecutionAlgorithm.state` remains a property |
| `Component.is_running`         | `is_running()`                                |
| `Component.is_stopped`         | `is_stopped()`                                |
| `Component.is_disposed`        | `is_disposed()`                               |
| `Component.is_degraded`        | `is_degraded()`                               |
| `Component.is_faulted`         | `is_faulted()`                                |

V1 `is_initialized` means any state beyond `PRE_INITIALIZED`; v2 `is_ready()` means exactly `READY`. It is not
equivalent for a running, stopped, degraded, disposed, or faulted component. Inspect `state()` on `DataActor`
and `Strategy`, or the `state` property on `ExecutionAlgorithm`, and compare it with
`ComponentState.PRE_INITIALIZED`.

`Cache.actor_ids()` is removed. Rust integrations can use `Trader::actor_ids()`; Python v2 does not expose a
direct actor-ID collection.

### Strategy config and order factories

Read these v1 `Strategy` runtime properties from `Strategy.config`: `order_id_tag`, `oms_type`,
`manage_contingent_orders`, `manage_gtd_expiry`, `use_uuid_client_order_ids`, and
`use_hyphens_in_client_order_ids`.

Read v1 `external_order_claims` from `Strategy.config.external_order_instrument_ids`. This field stores
serializable intent; to replace active claims, call `Strategy.set_external_order_instrument_ids(...)` after
registration.

`OrderFactory.trader_id` and `strategy_id` remain available. Strategy-owned factories use the client-order-ID
options on `Strategy.config`. Standalone factories have no flag readback; retain those values in application
config.

### Historical callbacks

Route historical results to type-specific callbacks:

| v1 data through `on_historical_data` | v2 callback                   | v2 argument                                |
| ------------------------------------ | ----------------------------- | ------------------------------------------ |
| Custom data                          | `on_historical_data`          | One `CustomData` or `Sequence[CustomData]` |
| Book snapshot                        | `on_book`                     | One `OrderBook`                            |
| Book deltas                          | `on_historical_book_deltas`   | `Sequence[OrderBookDelta]`                 |
| Book depth                           | `on_historical_book_depth`    | `Sequence[OrderBookDepth]`                 |
| Quote ticks                          | `on_historical_quotes`        | `Sequence[QuoteTick]`                      |
| Trade ticks                          | `on_historical_trades`        | `Sequence[TradeTick]`                      |
| Funding rates                        | `on_historical_funding_rates` | `Sequence[FundingRateUpdate]`              |
| Bars                                 | `on_historical_bars`          | `Sequence[Bar]`                            |

`on_historical_data` handles only custom data; typed historical results never fall through to it. Single
`CustomData` responses arrive as an object, batches as one list, including empty lists.
`on_historical_mark_prices` and `on_historical_index_prices` support native batch delivery, but the public
Python API cannot initiate those requests.

### Events and custom subscriptions

Replace the removed `on_event` hook with `on_time_event` for timers, `on_order_event` for aggregate order
handling, or `on_position_event` for aggregate position handling. For custom messaging, use `on_signal` or a
typed data subscription.

For custom subscriptions, specify an identifier to select only that identity. Without one, subscriptions also
receive identified payloads with the same type and metadata.

Replace `nautilus_trader.data.OptionChainManager` with `DataActor.subscribe_option_chain(...)` or
`Strategy.subscribe_option_chain(...)`. Handle each aggregated `OptionChainSlice` in `on_option_chain(slice)`;
call `unsubscribe_option_chain(series_id)` to stop.

### Order modification and cancellation

Pass client order IDs when modifying or canceling orders:

| v1 method                    | v2 method                                  |
| ---------------------------- | ------------------------------------------ |
| `modify_order(order, ...)`   | `modify_order(order.client_order_id, ...)` |
| `cancel_order(order, ...)`   | `cancel_order(order.client_order_id, ...)` |
| `cancel_orders(orders, ...)` | `cancel_orders(client_order_ids, ...)`     |

`Strategy.cancel_all_orders()` affects only orders associated with that strategy by default. Pass
`strategy_only=False` to retain v1's broader instrument-and-side scope.

### Execution algorithms

Python v2 `ExecutionAlgorithm` routes orders and does not inherit the full `Actor` surface. Override
`on_order`, `on_order_list`, order/position callbacks, lifecycle callbacks, or `on_signal`. Move market-data,
historical, and indicator-driven work to `DataActor` or `Strategy`. V1 `on_save` and `on_load` have no
algorithm callback; retain that state in application config or move the stateful component to `DataActor` or
`Strategy`.

The runtime owns command routing and calls `execute`; do not call or override `execute` as the algorithm
entrypoint.

An execution algorithm cannot submit a spawned order with a live emulation trigger. Use
`emulation_trigger=None`; Python raises `ValueError` if `submit_order` receives a triggered child.

> **Important:** After a failed spawn with `reduce_primary=True`, discard or refresh the caller-held
> primary order: quantity restoration updates the cached primary order.

V2 `OrderList` stores client order IDs. The runtime resolves them through the cache and calls
`on_order_list(order_list, orders)` with `orders` in client order ID order.

- With an override, the runtime calls `on_order_list` once and never also calls `on_order`.
- Without an override, the default calls `on_order` once per resolved order; v1 did not fan out lists.

Change `on_order_list(self, order_list)` to `on_order_list(self, order_list, orders)`.

Use these replacements for the inherited v1 surface:

| V1 `ExecAlgorithm` / `Actor` capability | Python v2 contract                                                                |
| --------------------------------------- | --------------------------------------------------------------------------------- |
| `cache`                                 | Available as a read-only property after node or engine registration.              |
| `portfolio`                             | Available as a read-only property after node or engine registration.              |
| `greeks`                                | Construct `GreeksCalculator(self.cache, self.clock)` after registration.          |
| `msgbus`                                | Use component topic methods for Python objects or signals for lightweight values. |
| Registered indicators                   | Use `DataActor` or `Strategy` for indicator-driven workflows.                     |
| Market-data subscriptions and callbacks | Use `DataActor` or `Strategy`; algorithms inspect cache and routed events.        |
| Lifecycle state and control             | Use `is_*()` and lifecycle methods; the Rust component remains authoritative.     |
| Direct `register(...)`                  | Use `BacktestEngine.add_exec_algorithm` or `LiveNode.add_exec_algorithm`.         |

#### Messages and signals

`DataActor`, `Strategy`, and `ExecutionAlgorithm` expose `publish_message(topic, message)`,
`subscribe_topic(topic, handler, priority=0)`, and `unsubscribe_topic(topic, handler)` for arbitrary
in-process Python objects. Handlers receive the original object synchronously, and subscriptions belong to the
subscribing component. See [Python topic messaging](docs/concepts/message_bus.md#python-topic-messaging) for
callable identity, lifecycle, and thread requirements.

For lightweight signals:

- Call `subscribe_signal(name)` during `on_start`.
- Handle `on_signal(signal)`.
- Call `publish_signal(name, value)`.

Signal values use their string representation. Raw message-bus endpoint registration remains a runtime
internal API.

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

#### Parameters and config subclasses

`exec_algorithm_params` keys and values must be strings, matching Rust's `IndexMap<Ustr, Ustr>`.
Encode values when constructing orders and parse them in the algorithm, for example:
`exec_algorithm_params={"horizon_secs": "300", "interval_secs": "10"}`.

`ExecutionAlgorithmConfig` supports custom Python fields. Its `__new__` applies base fields before `__init__`,
so initialize only custom attributes there. Declare them keyword-only and accept `**_kwargs` for base
keywords. The base ignores unmatched keywords; validate optional custom inputs in `__init__`.

```python
from nautilus_trader.config import ExecutionAlgorithmConfig
from nautilus_trader.model import ExecAlgorithmId


class RoutedAlgorithmConfig(ExecutionAlgorithmConfig):
    def __init__(
        self,
        *,
        horizon_secs: str,
        interval_secs: str,
        **_kwargs,
    ) -> None:
        super().__init__()
        self.horizon_secs = horizon_secs
        self.interval_secs = interval_secs


config = RoutedAlgorithmConfig(
    exec_algorithm_id=ExecAlgorithmId("ROUTED"),
    horizon_secs="300",
    interval_secs="10",
    log_events=False,
)
algorithm = RoutedAlgorithm(config)
```

#### Export and registration

In an algorithm subclass `__init__`, call `super().__init__(config)` to retain the Python instance and config
for export. Define algorithm and config classes at module scope so exported import paths resolve.

For backtest or live use:

- Register instances with `BacktestEngine.add_exec_algorithm` or `LiveNode.add_exec_algorithm`.
- Export paths and config values with `algorithm.to_importable_config()`.
- Register the exported config with `BacktestEngine.add_exec_algorithm_from_config` or
  `LiveNode.add_exec_algorithm_from_config`.

Register DataActor-based compatibility algorithms with `add_exec_algorithm_from_config`.

Nodes normally drive lifecycle transitions. Direct lifecycle methods remain available for control-plane
integrations and dispatch the same Python callbacks.

## Update model and portfolio code

### Typed model objects

V2 removes the PyCapsule boundary between Python and Rust. Pass model objects directly and use normal Python
type checks:

| v1 API                                        | v2 migration                                  |
| --------------------------------------------- | --------------------------------------------- |
| `nautilus_trader.core.is_pycapsule(value)`    | `isinstance(value, ExpectedModelType)`        |
| `model.as_pycapsule()`                        | Pass the model object                         |
| `OrderBookDeltas.from_pycapsule(capsule)`     | Use the `OrderBookDeltas` object directly     |
| Databento `load_*_as_pycapsule(...)`          | Call the corresponding `load_*(...)` method   |
| Adapter callbacks receiving `PyCapsule`       | Handle the typed model object                 |
| `BacktestEngine.add_data` with duck typing    | Pass supported NautilusTrader model objects   |
| Duck-typed portfolio-statistic position input | Pass `nautilus_trader.model.Position` objects |

`nautilus_trader.model.CustomData` exposes its payload through `.data` and is accepted by `DataActor.publish_data()`.
The separate `nautilus_trader.common.CustomData` byte container exposes `.value` and is not accepted by that method.

### Order and position inspection

| v1 member                   | v2 member                                                                        |
| --------------------------- | -------------------------------------------------------------------------------- |
| `Order.events`              | `Order.events()`                                                                 |
| `Position.adjustments`      | `Position.adjustments()`                                                         |
| `Position.client_order_ids` | `Position.client_order_ids()`                                                    |
| `Position.events`           | `Position.events()`                                                              |
| `Position.trade_ids`        | `Position.trade_ids()`                                                           |
| `Position.venue_order_ids`  | `Position.venue_order_ids()`                                                     |
| `OrderList.orders`          | `order_list.client_order_ids()`, then `cache.order(client_order_id)` for each ID |
| `OrderList.first`           | `cache.order(order_list.first_client_order_id)` if the ID is not `None`          |

- A direct `Position.apply` fill that crosses zero resets the open entry price to the flipping fill.
  V1 retains the old side's entry price.
- `Position.apply` validates the fill's instrument ID, position ID, and ordinary trade-ID uniqueness
  before mutation. Invalid fills leave the position unchanged and raise `ValueError`; v1 raises
  `KeyError` for a duplicate trade ID and does not validate both identities on every apply.
- `Order.avg_px` and `Order.slippage` are `decimal.Decimal`; v1 exposed `float`. Weighted averages
  no longer pass through `f64`: `Decimal("0.70000") == 0.7` is `False`. Compare with `Decimal("0.7")`
  or convert the operand with `Decimal(str(value))`.
- `Order.to_dict()` serializes `avg_px` and `slippage` as strings, like other decimal fields.
  Convert with `Decimal(...)` before arithmetic.

### Order books and depth

Python v2 uses `OrderBookDepth` for variable-length sides and does not export `OrderBookDepth10`. Inspect side
lengths instead of assuming ten padded levels; sides can be empty or unequal. `OrderBookDepthDataWrangler`
returns `OrderBookDepth`.

- V2 `BookLevel` comparisons follow ladder priority: bids sort from highest to lowest price, and
  asks sort from lowest to highest. Equality includes the side, and ordering levels from opposite
  sides raises `TypeError`. Sort on `level.price` when code needs side-independent price order.
- V2 `OrderBook` pickle data is not interchangeable with v1 pickle data. Rebuild snapshots from
  source market data when moving between the implementations.
- `OrderBookDeltas.is_snapshot` tests `F_SNAPSHOT` on the batch flags, which come from the final
  delta. The old implementation tested whether the first delta had a `CLEAR` action, so a clear
  without the flag is not a snapshot.

### Enums and optional values

`AggressorSide.BUYER` and `AggressorSide.SELLER` become `AggressorSide.BUY` and `AggressorSide.SELL`. String,
serde, and PostgreSQL output also use `BUY` and `SELL`; legacy `BUYER` and `SELLER` input remains deprecated
compatibility input.

These Python compatibility attributes now evaluate to `None`, not enum members:

- `OrderSide.NO_ORDER_SIDE`
- `PositionSide.NO_POSITION_SIDE`
- `ContingencyType.NO_CONTINGENCY`
- `TrailingOffsetType.NO_TRAILING_OFFSET`
- `TriggerType.NO_TRIGGER`

Use `is None` for absence checks. The legacy `NO_*` string tokens still parse to `None`, but the attributes no
longer have enum properties such as `.name` or `.value` and do not appear in `variants()`.

`BookOrder.side`, `DatabentoImbalance.side`, `OrderStatusReport.order_side`,
`OrderStatusReport.contingency_type`, and `OrderStatusReport.trailing_offset_type` can return `None`. Concrete
orders also return `None` from `closing_side(PositionSide.FLAT)`. Check for absence before accessing enum
properties. Use `OrderBookDelta.clear(...)` to construct a clear delta.

### Instrument and indicator inspection

V2 exposes consistent read-only inspection across economic instrument types. Properties include `asset_class`,
`instrument_class`, currencies, fees, margins, quantity and price limits, `multiplier`, and `tick_scheme`;
values may be `None` or a documented default. `SyntheticInstrument` is formula-derived, so inspect its `id`,
`components`, `formula`, price precision and increment, and timestamps instead.

| v1 name                                        | v2 name                    |
| ---------------------------------------------- | -------------------------- |
| `instrument.tick_scheme_name`                  | `instrument.tick_scheme`   |
| `AdaptiveMovingAverage.period` or `.period_er` | `.period_efficiency_ratio` |
| `AdaptiveMovingAverage.period_alpha_fast`      | `.period_fast`             |
| `AdaptiveMovingAverage.period_alpha_slow`      | `.period_slow`             |
| `LinearRegression.R2`                          | `LinearRegression.r2`      |
| `DirectionalMovement.value`                    | `.pos` and `.neg`          |
| `DataType.type`                                | `DataType.type_name`       |
| `Bar.is_revision`                              | removed                    |

V1 `DirectionalMovement.value` never changed from zero, so v2 exposes the meaningful positive and negative
outputs instead.

- Instrument `activation_utc` and `expiration_utc` properties return UTC-aware `datetime.datetime`
  values. Use `activation_ns` and `expiration_ns` when exact nanosecond precision is required.
- v2 caches `OptionGreeks` for option fee calculation; this extends v1.

High-precision prices and quantities use exact scaled integers. Parse decimal text exactly; do not use v1
display strings to check compatibility.

### Margin accounts

`MarginAccount.margin()`, `MarginAccount.margins()`, and `MarginAccount.account_margins()` keep their v1
names. Other read-only queries rename their measure or scope:

| v1 name                       | v2 name                         |
| ----------------------------- | ------------------------------- |
| `margin_init()`               | `initial_margin()`              |
| `margins_init()`              | `initial_margins()`             |
| `margin_maint()`              | `maintenance_margin()`          |
| `margins_maint()`             | `maintenance_margins()`         |
| `margin_for_currency()`       | `account_margin()`              |
| `margin_init_for_currency()`  | `account_initial_margin()`      |
| `margin_maint_for_currency()` | `account_maintenance_margin()`  |
| `account_margins_init()`      | `account_initial_margins()`     |
| `account_margins_maint()`     | `account_maintenance_margins()` |
| `total_margin_init()`         | `total_initial_margin()`        |
| `total_margin_maint()`        | `total_maintenance_margin()`    |

Python exposes only read-only margin queries from the table. Rust engine commands `update_margin`,
`clear_margin`, `clear_account_margin`, `clear_initial_margin`, `clear_maintenance_margin`, and
`set_margin_model` are not bound; this does not imply they were available in v1. Existing Python methods
`update_initial_margin`, `update_maintenance_margin`, `set_default_leverage`, and `set_leverage` remain
unchanged.

### Portfolio queries

| v1 member               | v2 member                                                 |
| ----------------------- | --------------------------------------------------------- |
| `Portfolio.initialized` | `Portfolio.is_initialized()`                              |
| `Portfolio.analyzer`    | `statistics()`, `snapshots()`, and `register_statistic()` |

These query renames have no compatibility aliases:

| v1 name                | v2 name                            |
| ---------------------- | ---------------------------------- |
| `margins_init()`       | `instrument_initial_margins()`     |
| `margins_maint()`      | `instrument_maintenance_margins()` |
| `is_flat()`            | `is_net_flat()`                    |
| `is_completely_flat()` | `is_completely_net_flat()`         |

- `unrealized_pnl()`, `total_pnl()`, and `net_exposure()` retain optional `price` inputs for fresh
  calculations that never replace cached values.
- `realized_pnl()`, `realized_pnls()`, `unrealized_pnl()`, `unrealized_pnls()`, `total_pnl()`,
  `total_pnls()`, `net_exposure()`, and `net_exposures()` accept `target_currency`. They fail closed
  on missing prices/conversions or exact-arithmetic overflow. Collection queries never return partial results.
- When both `venue` and `account_id` are accepted, they must identify the same account.
- `account()` returns a detached value. `build_snapshot()` samples without recording.
  Python exposes neither engine mutation commands nor the internal recorded realized-PnL cache.
- `PortfolioConfig.use_mark_prices` defaults to `true`; v1 defaulted to `false`. Set it to `false` to
  skip mark prices.
- `PortfolioAnalyzer.realized_pnls()` returns records in ascending event-time order. V1 returned
  position-derived records followed by recorded ones, which was not chronological.
- Registered PnL statistics run for every analyzed currency, including runs that closed no trades,
  where they receive an empty list. `Win Rate` and its peers therefore report NaN for such runs
  rather than being absent.
- A portfolio statistic that raises no longer propagates. V2 logs the error and routes it through
  `sys.unraisablehook`, because the calculation crosses into Rust where there is no error channel.

### Custom portfolio statistics

Subclass `nautilus_trader.analysis.PortfolioStatistic` and register it on the portfolio instead of an
analyzer. Class names still derive statistic names: `MyCustomRatio` registers as "My Custom Ratio".

```python
from nautilus_trader.analysis import PortfolioStatistic


class TradeCount(PortfolioStatistic):
    def calculate_from_realized_pnls(self, realized_pnls: list[float]) -> float | None:
        return float(len(realized_pnls))


engine.portfolio.register_statistic(TradeCount())
```

- v1 passed `pd.Series`; v2 passes `dict[int, float]` keyed by UNIX nanoseconds for returns and
  `list[float]` for realized PnLs. Rewrite any Series-specific code.
- `calculate_from_orders` is gone. No v1 or v2 analyzer ever supplied order data to it, so a v1
  implementation of that method never ran.

V1's protected `_check_valid_returns` and `_downsample_to_daily_bins` methods and the
`fully_qualified_name()` classmethod have no v2 equivalent.
Registrations reach `Portfolio.statistics()`, `BacktestResult`, and post-run analysis logs, and survive
repeated queries and analyzer resets. See [Custom statistics](docs/concepts/portfolio.md#custom-statistics).

## Update backtest and live nodes

### Register components and clients

Register components on the node and clients on its builder:

| v1 config field                        | v2 migration                                                           |
| -------------------------------------- | ---------------------------------------------------------------------- |
| `NautilusKernelConfig.message_bus`     | Pass `msgbus` to `BacktestEngineConfig` or `LiveNodeConfig`.           |
| `NautilusKernelConfig.actors`          | Call `add_actor` or `add_actor_from_config` on the node.               |
| `NautilusKernelConfig.strategies`      | Call `add_strategy` or `add_strategy_from_config` on the node.         |
| `NautilusKernelConfig.exec_algorithms` | Call `add_exec_algorithm` or `add_exec_algorithm_from_config`.         |
| `TradingNodeConfig.data_clients`       | Call `LiveNodeBuilder.add_data_client`.                                |
| `TradingNodeConfig.exec_clients`       | Call `LiveNodeBuilder.add_exec_client` or `add_simulated_exec_client`. |

- For `BacktestNode`, call `node.build()` first. Then pass the run config ID and a constructed
  component to `add_actor`, `add_strategy`, or `add_exec_algorithm`, or use the corresponding
  `_from_config` method. Call `node.run()` after registration.
- For `LiveNode`, register constructed components or importable configs with the same method pairs
  before calling `run()` or `run_async()`.

`LiveNode.add_actor` accepts constructed Python actor instances. Registration applies the actor's config,
derives its ID, and rejects duplicate IDs or registration after the node leaves its idle state.

Alternatively, put client name-to-config mappings in `LiveNodeConfig.data_clients` and `exec_clients`,
then supply `data_factories` and `exec_factories` to `LiveNode.build(...)` or `LiveNodeBuilder.from_config(...)`.

### Timeouts

On `LiveNodeConfig`, timeout names now state their unit and the post-stop wait is a delay:

| v1 field                 | v2 `LiveNodeConfig` field     |
| ------------------------ | ----------------------------- |
| `timeout_connection`     | `timeout_connection_secs`     |
| `timeout_reconciliation` | `timeout_reconciliation_secs` |
| `timeout_portfolio`      | `timeout_portfolio_secs`      |
| `timeout_disconnection`  | `timeout_disconnection_secs`  |
| `timeout_post_stop`      | `delay_post_stop_secs`        |
| `timeout_shutdown`       | `timeout_shutdown_secs`       |

`BacktestEngineConfig` keeps the v1 timeout names without the `_secs` suffix, except that `timeout_post_stop`
becomes `delay_post_stop`.

### Backtest models and settlement

Replace v1 import-path wrappers for Python/Cython fill, fee, latency, margin, and simulation modules, including
`Importable*ModelConfig`, `MarginModelConfig`, their factories, and `SimulationModuleConfig`. Construct
models/modules directly and pass them to the backtest venue: `ProbabilisticFillModel`, `FixedFeeModel`,
`StaticLatencyModel`, `StandardMarginModel`, `LeveragedMarginModel`, or `FXRolloverInterestModule`. Native
Rust models compose at compile time; v1 provided no runtime native model plugins.

- Backtest venues no longer accept `settlement_prices`. Add `InstrumentClose` data with
  `close_type=InstrumentCloseType.CONTRACT_EXPIRED`; its exact `close_price` settles futures, binary
  contracts, and option close legs at expiry.
- An omitted backtest `default_leverage` now selects 10x for margin accounts and 1x for cash
  accounts. Set `default_leverage=Decimal(1)` to retain v1's unleveraged behavior.

### Backtest post-run inspection

V2 keeps `BacktestNode` engines internal; the v1 `get_engine` and `get_engines` calls are unavailable. For
post-run inspection, set `BacktestRunConfig.dispose_on_completion=False`; the `True` default drops engine
state. Then pass the run config ID to the node inspection methods:

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

The [getting-started backtest guides](docs/getting_started/index.md) show the high-level `BacktestNode` and
low-level `BacktestEngine` APIs.

### Live factories and trader identity

Execution factories now consume the corresponding execution client config directly. Remove
`DeriveExecFactoryConfig` and `HyperliquidExecFactoryConfig` wrappers, and pass `DeriveExecutionClientConfig`
or `HyperliquidExecutionClientConfig` to `add_exec_client`.

The live node owns the trader identity. Remove `trader_id` from adapter execution client config construction;
`LiveNodeConfig` or `LiveNode.builder(...)` supplies it to every execution factory. Keep the venue-specific
`account_id` on the execution client config. The Bybit, Coinbase, and Interactive Brokers execution factories
now use no-argument constructors.

### Live inspection and event loops

Inspect Rust-owned state through `node.cache` and `node.portfolio`; these shared wrappers expose no runtime
internals. Choose the run method by loop ownership:

| Method                  | Contract                                                                                |
| ----------------------- | --------------------------------------------------------------------------------------- |
| `LiveNode.run()`        | Runs on the calling thread, owns signal handling, and blocks until shutdown.            |
| `LiveNode.run_async()`  | Runs on the caller's asyncio loop and resolves once the node has stopped.               |
| `LiveNodeHandle.stop()` | Requests graceful shutdown and returns immediately; the active run completes afterward. |

Replace the removed `LiveNode.start()` and `LiveNode.poll()` sequence with `run()`, or await `run_async()`
on a host-owned loop. Both run the same startup ordering, maintenance, external message-bus ingress,
reconciliation, and shutdown.

For a hosted node:

1. Capture `cache`, `portfolio`, and `handle()` before starting; `run_async()` lends the node to its
   coroutine for the run's duration.
1. Start `run_async()` in a supervised task. Wait for the handle to report `Running` and watch for
   unexpected task completion.
1. Request shutdown with `LiveNodeHandle.stop()`, await graceful run completion, then dispose the node.

Use `LiveNodeHandle.stop()` as the external stop path in either mode. See the [hosted event-loop
recipe](docs/concepts/live.md#hosted-event-loops) for the full contract.

### Cache backing

`DatabaseConfig` has no public v2 Python equivalent. For live trading, configure Redis or Postgres cache
backing through `LiveNodeBuilder`; this does not restore the generic v1 `DatabaseConfig` workflow. See [cache
database configuration](docs/how_to/configure_live_trading.md#cache-database-configuration).

### Networking

The generic Python APIs under `nautilus_trader.network` have no v2 public Python equivalent: `HttpClient`,
`HttpMethod`, `HttpResponse`, `SocketClient`, `WebSocketClient`, `SocketConfig`, `WebSocketConfig`, `Quota`,
network exceptions, and the `http_*` functions. Adapter-specific low-level Python WebSocket clients and their
request, error, and channel-control types are also removed. Use `LiveNode` data and execution clients for
streaming venue workflows, retained adapter HTTP clients for supported direct requests, or the Rust
`nautilus-network` crate for custom networking. `TransportBackend` remains available from
`nautilus_trader.network` only for adapter config transport selection.

### Python API contract

Use the generated stubs in `python/nautilus_trader/` for the Rust-bound Python contract. Documented Python
client, provider, and importable-config classes are also public interfaces. Extra runtime attributes on
adapter wire DTOs are outside the contract. These runtime-callable methods have no stubs and cannot be
resolved by static type checkers:

- `KrakenFuturesHttpClient.edit_orders_batch`
- `KrakenFuturesHttpClient.submit_orders_batch`
- `KrakenSpotHttpClient.submit_orders_batch`

Check the stub before replacing a v1 convenience method or copying a v1 adapter config field. See the [Python
concept guide](docs/concepts/python.md) for ownership and API boundaries, and the [Rust-native Python
examples][python-v2-examples] for live-node builders, adapter factories, strategies, actors, and
data/execution testers.

### Custom live adapters

Subclass clients from `nautilus_trader.live.clients`:

| v1 base class          | v2 base class      |
| ---------------------- | ------------------ |
| `LiveDataClient`       | `DataClient`       |
| `LiveMarketDataClient` | `MarketDataClient` |
| `LiveExecutionClient`  | `ExecutionClient`  |

Register custom factories through `LiveNodeBuilder.add_data_client` or `add_exec_client`, or use the
config-based registration above. Official adapters use the same builder methods; sandbox execution uses
`add_simulated_exec_client`. Custom clients run async work on the node's Python loop, inspect a read-only
cache, and emit typed data, events, and reports through queued output. V1 Cython adapters need porting.
See the [Python adapter guide](docs/developer_guide/python_adapters.md) and
[support boundaries][python-support-boundaries].

## Migrate catalogs and streaming

### Catalog queries and legacy data

`DataBackendSession`, `DataQueryResult`, and `ParquetDataCatalog.backend_session()` are removed. Query a
catalog with `ParquetDataCatalog.query(...)` or a typed method such as `query_quote_ticks(...)`, which return
typed Python objects, or stream Arrow data with `query_data_arrow_stream(...)`.

The v2 Python catalog API and Arrow schemas differ from v1. Migrate recognized legacy files before querying
them with v2. The [Parquet migration guide](docs/how_to/migrate_parquet_catalog.md) covers schema
fingerprints, tested source formats, and recovery from partial migrations.

Catalog order-event data written before `activation_price` and `OrderFilled.info` were added cannot be read by
the new schema. Regenerate or migrate that data before upgrading a catalog in place.

`DataCatalogConfig` and `StreamingConfig` have v2-native Python equivalents for `BacktestNode`. Configure
existing built-in-data catalog queries and Feather output through `BacktestEngineConfig`. These configs do not
restore the v1 factory, download, custom-data, or generic serialization workflows.

### Streaming output and rotation

`StreamingConfig` writes Feather files to a local `writer_path` and promotes them into an optional `catalog`,
which can be remote. It takes rotation through one `RotationConfig`, with intervals and the time of day in
integer nanoseconds:

| v1 `StreamingConfig` fields                              | v2 `StreamingConfig` argument                                          |
| -------------------------------------------------------- | ---------------------------------------------------------------------- |
| `catalog_path`, `fs_protocol`, `fs_rust_storage_options` | `writer_path` for Feather files, plus `catalog=DataCatalogConfig(...)` |
| `rotation_mode=SIZE`, `max_file_size`                    | `rotation_config=RotationConfig.size(max_size)`                        |
| `rotation_mode=INTERVAL`, `rotation_interval`            | `rotation_config=RotationConfig.interval(interval_ns)`                 |
| `rotation_mode=SCHEDULED_DATES`, `rotation_interval`     | `rotation_config=RotationConfig.scheduled_dates(interval_ns, ...)`     |
| `rotation_time`, `rotation_timezone`                     | `schedule_ns` and `timezone` of `RotationConfig.scheduled_dates`       |
| `rotation_mode=NO_ROTATION`                              | `rotation_config=RotationConfig.no_rotation()`, or omit it             |

For unfiltered streaming, omit `data_types`, `record_types`, `instrument_types`, and `record_filters`. To
select records, use non-empty lists or a non-empty Python `record_filters` dictionary. Rust rejects explicit
empty lists; Python treats empty lists and dictionaries as omitted filters.

### Compression and storage options

`DataCatalogConfig` takes `batch_size`, `max_row_group_size`, and `compression` as typed fields; `compression`
is a codec name: `uncompressed`, `snappy`, `gzip`, `brotli`, `lz4`, `lz4_raw`, or `zstd`. Set these as fields,
not `params` keys: `params` carries only options for an external catalog backend, and the Parquet catalog
rejects any `params` key. `ParquetDataCatalog(compression=...)` takes the Parquet codec codes `0`
(uncompressed), `1` (Snappy), `2` (gzip), `4` (Brotli), `5` (LZ4), and `6` (zstd), and rejects LZO (`3`) and
unknown codes. Both `lz4` and code `5` write Parquet `LZ4_RAW`; see [compression and row
groups](docs/concepts/data/catalog.md#compression-and-row-groups).

Catalog storage options are `object_store` configuration keys. V1's Rust backend logged and ignored an unknown
key, such as GCS `project_id`; v2 fails with an error that names the key. `BacktestDataConfig` passes
`catalog_fs_storage_options` to the same backend when `catalog_fs_rust_storage_options` is unset, so translate
fsspec-only options to `object_store` keys, such as `anon` to `skip_signature`. See [storage
options](docs/concepts/data/catalog.md#filesystem-protocols-and-storage-options).

## Update Rust integrations

### Catalog traits and backends

This example copies v2 quote rows through the shared catalog traits:

```rust
use nautilus_model::data::NautilusDataType;
use nautilus_persistence::catalog::{
    traits::{CatalogReader, CatalogWriter},
    types::CatalogQuery,
};

fn copy_quotes(
    source: &mut dyn CatalogReader,
    target: &mut dyn CatalogWriter,
) -> anyhow::Result<()> {
    let query = CatalogQuery::new(NautilusDataType::QuoteTick);
    let mut session = source.query_batch_session(&query, Some(1_000))?;

    while let Some(batch) = session.next_batch()? {
        target.write_data_batch(&batch, None, None, None)?;
    }

    Ok(())
}
```

Both catalogs must support the operations used, and the destination must accept the source intervals. The
example does not preserve known-empty coverage or convert legacy schemas; use the migration command for those.

#### API stability

Use the public `CatalogReader`, `CatalogWriter`, and `Catalog` traits, `DataCatalog`, and catalog query types.
Register backends through `CatalogFactory` closures and `CatalogFactoryRegistry`. Backend internals are not
extension interfaces.

Pin compatible Rust crate versions independently of Python. Compatible updates preserve these public
contracts; required trait methods or signature changes break source compatibility and require release notes
and migration guidance.

#### Backend limits

- Optional operations can return `PersistenceError::Unsupported`; distinguish this from a supported operation that fails.
- Parquet supports only the latest view. Historical `CatalogAsOf` queries fail with
  `Parquet catalog does not support historical queries`, rather than `PersistenceError::Unsupported`.
- Query chunk sizes are targets. Equal-timestamp groups can exceed them; instrument and custom queries can collect
  all results before yielding batches.

### Data variants

| v1 Rust variant           | v2 Rust variant     |
| ------------------------- | ------------------- |
| `Data::Delta`             | `Data::BookDelta`   |
| `Data::Deltas`            | `Data::BookDeltas`  |
| `Data::Depth10`           | `Data::BookDepth`   |
| `Data::MarkPriceUpdate`   | `Data::MarkPrice`   |
| `Data::IndexPriceUpdate`  | `Data::IndexPrice`  |
| `Data::FundingRateUpdate` | `Data::FundingRate` |

`Data::BookDeltas` owns a `Box<OrderBookDeltas>` in place of `OrderBookDeltas_API`. Exhaustive matches must
also handle `Data::Instrument`.

### Execution and cache factories

Custom Rust execution factories must accept `TraderId` in their `ExecutionClientFactory::create` or
`SimulatedExecutionClientFactory::create` implementation.

`ExecutionClientFactory::create` requires `clock: Rc<RefCell<dyn Clock>>` after the cache argument,
matching `DataClientFactory::create`. Pass the owning node's clock when calling an execution factory directly.
`SimulatedExecutionClientFactory` does not add a clock argument. The Python execution client bridge uses the
supplied clock. Existing native execution adapters continue to use their realtime clocks internally; this
signature change does not add test-clock support to those adapters.

Custom Rust cache database adapters used with live orders must implement the batch `index_order_clients`
operation. The default trait implementation rejects non-empty claims.

### Tokio runtime

For `set_runtime`, use `Builder::new_multi_thread().enable_all()`; current-thread runtimes are rejected.

### Order history

Construct `OrderCore` with `OrderCore::new`, apply events with `OrderCore::apply`, and inspect history with
`OrderCore::events()`. Direct `events` field access and struct-literal construction are unavailable.
`OrderCore::prepend_events` retains history during transformation, preserving event order without applying
state transitions. Reconstruct replacement history with `OrderAny::from_events`. The serialized order format
is unchanged.

## Migrate PostgreSQL databases

Postgres-backed deployments must run `nautilus database init` before starting a v2 node. The `order.avg_px`
and `order.slippage` columns move from `double precision` to `NUMERIC`, and the node fails at connect time
while the old column types remain.

Existing databases whose `AGGRESSOR_SIDE` enum still contains `BUYER` and `SELLER` also need this one-time
migration before ingesting v2 data:

```sql
ALTER TYPE AGGRESSOR_SIDE RENAME VALUE 'BUYER' TO 'BUY';
ALTER TYPE AGGRESSOR_SIDE RENAME VALUE 'SELLER' TO 'SELL';
```

Do not run those statements if the enum already contains `BUY` and `SELL`.

The Postgres cache is scoped to the node's trader ID. `nautilus database init` qualifies the order and
position snapshot keys and the order-position index key with the trader, and fails with the offending rows if
any snapshot or index row has no resolvable trader.

Account events persisted before trader scoping have no trader, and nothing establishes which trader owns them.
While any exist, every node using the database fails to connect, and the error lists the affected accounts.
Assign each one to its trader before starting a node:

```bash
nautilus database assign-account --account-id <ACCOUNT_ID> --trader-id <TRADER_ID>
```

The command connects to Postgres directly, so it works while nodes are blocked.

## Compare backtest performance

Use `scripts/benchmark-backtest-versions.py` for a wall-clock comparison between the released v1 Cython engine
and the v2 PyO3 engine. The driver owns one shared scenario matrix and normalizes the small API differences at
runtime. It rejects a run before timing unless both environments use the expected package version, backend,
source revision, Python version, and precision mode. For v2, it also requires the requested source revision to
be embedded in the loaded extension.

Run the comparison on a quiet host. These commands create isolated release environments and a detached v1
worktree without changing the current branch:

```bash
COMPARE_ROOT=$(mktemp -d /tmp/nautilus-backtest-compare.XXXXXX)
COMPARE_PYTHON=$(uv python find 3.13)
git worktree add --detach "$COMPARE_ROOT/v1" v1.231.0

uv venv --python "$COMPARE_PYTHON" "$COMPARE_ROOT/env-v1"
(
    cd "$COMPARE_ROOT/v1"
    uvx --from uv==0.11.33 uv build --wheel --python "$COMPARE_PYTHON" \
        --out-dir "$COMPARE_ROOT/wheels-v1"
)
V1_WHEEL=$(find "$COMPARE_ROOT/wheels-v1" -type f -name 'nautilus_trader-*.whl')
UV_LINK_MODE=copy uv pip install --no-cache \
    --python "$COMPARE_ROOT/env-v1/bin/python" "$V1_WHEEL"

uv venv --python "$COMPARE_PYTHON" "$COMPARE_ROOT/env-v2"
uv pip install --python "$COMPARE_ROOT/env-v2/bin/python" maturin==1.14.1 patchelf
(
    cd python
    CARGO_BUILD_JOBS=16 "$COMPARE_ROOT/env-v2/bin/maturin" build --release \
        --interpreter "$COMPARE_ROOT/env-v2/bin/python" --out "$COMPARE_ROOT/wheels-v2"
)
V2_WHEEL=$(find "$COMPARE_ROOT/wheels-v2" -type f -name 'nautilus_trader-*.whl')
UV_LINK_MODE=copy uv pip install --no-cache \
    --python "$COMPARE_ROOT/env-v2/bin/python" "$V2_WHEEL"

V1_COMMIT=$(git -C "$COMPARE_ROOT/v1" rev-parse HEAD)
V2_COMMIT=$(git rev-parse HEAD)
"$COMPARE_PYTHON" scripts/benchmark-backtest-versions.py compare \
    --v1-python "$COMPARE_ROOT/env-v1/bin/python" \
    --v1-artifact "$V1_WHEEL" \
    --v1-source "$COMPARE_ROOT/v1" \
    --v1-commit "$V1_COMMIT" \
    --v2-python "$COMPARE_ROOT/env-v2/bin/python" \
    --v2-artifact "$V2_WHEEL" \
    --v2-source "$PWD" \
    --v2-commit "$V2_COMMIT" \
    --sessions 5 \
    --output "$COMPARE_ROOT/results.json"
```

### Timing boundaries

Both environments run each boundary back-to-back; case order reverses or rotates across sessions.

| Boundary         | Timed work                                                                         |
| ---------------- | ---------------------------------------------------------------------------------- |
| `run_preloaded`  | Only `BacktestEngine.run()`, after fixtures, engine construction, and registration |
| `load_build_run` | Instrument/data fixtures, engine construction, data registration, and `run()`      |

Repeat `--scenario <name>` or `--boundary <name>` on `compare` to select a subset; omit both for the full matrix.
The driver requires at least three full sessions.

### Identity and result checks

- Before timing, the coordinator verifies that each loaded extension byte-matches its wheel member;
  it rechecks full identities after the run.
- After every timed sample, the worker repeats the complete wheel, extension, source, and runtime
  identity proof and compares its canonical digest with the coordinator's initial identity.
- Source identity hashes the revision, staged/unstaged diffs, and untracked file contents.
- Exact event, order, position, and account fingerprints are checked after every timed iteration,
  outside the measured duration.

### Output

JSON stores each full identity once and each full fingerprint once per selected scenario/boundary, then binds
every sample to them by digest. It includes all elapsed samples, observed host state, boundary definitions,
medians, minimum-to-maximum spread, v2/v1 ratios, and percentage gaps. The driver records CPU governor and
`perf_event_paranoid` values without changing host controls.

## Known limitations

- **Request callbacks**: Python does not provide v1 joined-response, pending-request cleanup, or late and
  duplicate delivery convenience behavior.
- **Message-bus backing**: use built-in configs such as `RedisMessageBusConfig`; arbitrary Python
  message-bus factory classes remain unsupported. V1
  `MessageBusConfig(database=DatabaseConfig(...), external_streams=[...])` maps to the builder calls
  `LiveNodeBuilder.with_msgbus_config(...)` and
  `LiveNodeBuilder.with_external_msgbus_factory(RedisMessageBusConfig(...))`. See
  [live message-bus configuration][live-message-bus-config]. The existing
  `RedisMessageBusFactory(RedisMessageBusConfig(...))` wrapper remains supported.
- **Cache backing**: custom Python clients do not support database cache backing in either launch mode. `run_async()`
  also rejects database cache backing; use `run()` for database-backed native clients. See
  [hosted event loops](docs/concepts/live.md#hosted-event-loops).
- **PostgreSQL state**: the cache loads positions but not synthetics. It does not persist actor or strategy
  state or write cache heartbeats. Redis backing supports these operations.
- **Snapshot publishing**: external message-bus publishing of serialized order and position snapshots remains deferred.
- **Live runtime fields**: Python `LiveNodeConfig` accepts `streaming` but has no v1 kernel-level `emulator` field.
  `loop_debug=True` is rejected by the Rust live runtime. Order emulation remains available through
  order emulation triggers (`emulation_trigger`).
- **Backtest data**: `BacktestNode` catalog config does not support v1 data-client factories, a download
  engine, on-the-fly downloads, custom data, or data frames.
- **Custom statistics**: `BacktestNode` builds engines internally, so a custom portfolio statistic cannot be registered
  before a node run. Use `BacktestEngine` directly when a run needs one.
- **Provider filters**: instrument-provider filter dictionaries are not a common v2 adapter contract. Hyperliquid v2
  loads its configured instrument universe and does not accept the v1 `instrument_provider` field.
  Check each adapter's Rust/PyO3 config rather than copying v1 provider examples.

The [v2 roadmap][v2-roadmap] tracks the wider post-cutover surface. Release-specific breaking changes remain
in [RELEASES.md][release-notes].

[live-message-bus-config]: docs/how_to/configure_live_trading.md#messagebus-configuration
[python-support-boundaries]: docs/concepts/python.md#support-boundaries
[python-v2-examples]: examples/README.md#live-adapter-examples
[python-v2-strategy-config]: python/tests/strategies/ema_cross.py
[release-notes]: RELEASES.md
[v2-roadmap]: https://github.com/nautechsystems/nautilus_trader/issues/4042
