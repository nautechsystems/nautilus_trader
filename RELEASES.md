# NautilusTrader 1.163.0 Beta

Released on TBD (UTC).

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed `MARKET_IF_TOUCHED` and `LIMIT_IF_TOUCHED` trigger behavior

---

# NautilusTrader 1.162.0 Beta

Released on 12th December 2022 (UTC).

### Breaking Changes
- `OrderFactory` bracket order methods consolidated to `.bracket(...)`

### Enhancements
- Extended `OrderFactory` to provide more bracket order types
- Simplified GitHub CI and removed `nox` dependency

### Fixes
- Fixed `OrderBook` sorting for bid side, thanks @gaugau3000
- Fixed `MARKET_TO_LIMIT` order initial fill behaviour
- Fixed `BollingerBands` indicator mid-band calculations, thanks zhp (Discord)

---

# NautilusTrader 1.161.0 Beta

Released on 10th December 2022 (UTC).

### Breaking Changes
- Renamed `OrderFactory.bracket_market` to `OrderFactory.bracket_market_entry`
- Renamed `OrderFactory.bracket_limit` to `OrderFactory.bracket_limit_entry`
- Renamed `OrderFactory` bracket order `price` and `trigger_price` parameters

### Enhancements
- Added support for Python 3.11
- Consolidated config objects to `msgspec` providing better performance and correctness
- Added `OrderFactory.bracket_stop_limit_entry_stop_limit_tp(...)`
- Numerous improvements to the Interactive Brokers adapter, thanks @limx0 and @rsmb7z
- Removed dependency on `pydantic`

### Fixes
- Fixed `STOP_MARKET` order behavior to fill at market on immediate trigger
- Fixed `STOP_LIMIT` order behavior to fill at market on immediate trigger and marketable
- Fixed `STOP_LIMIT` order behavior to fill at market on processed trigger and marketable
- Fixed `LIMIT_IF_TOUCHED` order behavior to fill at market on immediate trigger and marketable
- Fixed Binance start and stop time units for bar (kline) requests, thanks @Tzumx
- `RiskEngineConfig.bypass` set to `True` will now correctly bypass throttlers, thanks @DownBadCapital
- Fixed updating of emulated orders
- Numerous fixes to the Interactive Brokers adapter, thanks @limx0 and @rsmb7z

---

# NautilusTrader 1.160.0 Beta

Released on 28th November 2022 (UTC).

### Breaking Changes
- Removed time portion from generated IDs (affects `ClientOrderId` and `PositionOrderId`)
- Renamed `orderbook.data.Order` to `orderbook.data.BookOrder` (reduce conflicts/confusion)
- Renamed `Instrument.get_cost_currency(...)` to `Instrument.get_settlement_currency(...)` (more accurate terminology)

### Enhancements
- Added emulated contingency orders capability to `OrderEmulator`
- Moved `test_kit` module to main package to support downstream project/package testing

### Fixes
- Fixed position event sequencing: now generates `PositionOpened` when reopening a closed position
- Fixed `LIMIT` order fill characteristics when immediately marketable as a taker
- Fixed `LIMIT` order fill characteristics when passively filled as a maker as quotes move through
- Fixed canceling OTO contingent orders when still in-flight
- Fixed `RiskEngine` notional check when selling cash assets (spot currency pairs)
- Fixed flush on closed file bug for persistence stream writers

---

# NautilusTrader 1.159.0 Beta

Released on 18th November 2022 (UTC).

### Breaking Changes
- Removed FTX integration
- Renamed `SubmitOrderList.list` to `SubmitOrderList.order_list`
- Slight adjustment to bar aggregation (will not use the last close as the open)

### Enhancements
- Implemented `TRAILING_STOP_MARKET` orders for Binance Futures (beta)
- Added `OUO` One-Updates-Other `ContingencyType` with matching engine implementation
- Added bar price fallback for exchange rate calculations, thanks @ghill2

### Fixes
- Fixed dealloc of Rust backing struct on Python exceptions causing segfaults
- Fixed bar aggregation start times for bar specs outside typical intervals (60-SECOND rather than 1-MINUTE etc) 
- Fixed backtest engine main loop ordering of time events with identically timestamped data
- Fixed `ModifyOrder` message `str` and `repr` when no quantity
- Fixed OCO contingency orders which were actually implemented as OUO for backtests
- Fixed various bugs for Interactive Brokers integration, thanks @limx0 and @rsmb7z
- Fixed pyarrow version parsing, thanks @ghill2
- Fixed returning venue from InstrumentId, thanks @rsmb7z

---

# NautilusTrader 1.158.0 Beta

Released on 3rd November 2022 (UTC).

### Breaking Changes
- Added `LiveExecEngineConfig.reconcilation` boolean flag to control if reconciliation is active
- Removed `LiveExecEngineConfig.reconciliation_auto` (unclear naming and concept)
- All Redis keys have changed to a lowercase convention (please either migrate or flush your Redis)
- Removed `BidAskMinMax` indicator (to reduce total package size)
- Removed `HilbertPeriod` indicator (to reduce total package size)
- Removed `HilbertSignalNoiseRatio` indicator (to reduce total package size)
- Removed `HilbertTransform` indicator (to reduce total package size)

### Enhancements
- Improved accuracy of clocks for backtests (all clocks will now match generated `TimeEvent`s)
- Improved risk engine checks for `reduce_only` orders
- Added `Actor.request_instruments(...)` method
- Added `Order.would_reduce_only(...)` method
- Extended instrument(s) Req/Res handling for `DataClient` and `Actor

### Fixes
- Fixed memory management for Rust backing structs (now being properly freed)

---

# NautilusTrader 1.157.0 Beta

Released on 24th October 2022 (UTC).

### Breaking Changes
- None
 
### Enhancements
- Added experimental local order emulation for all order types (except `MARKET` and `MARKET_TO_LIMIT`) see docs
- Added `min_latency`, `max_latency` and `avg_latency` to `HttpClient` base class

### Fixes
- Fixed Binance Spot `display_qty` for iceberg orders, thanks @JackMa
- Fixed Binance HTTP client error logging

---

# NautilusTrader 1.156.0 Beta

Released on 19th October 2022 (UTC).

This will be the final release with support for Python 3.8.

### Breaking Changes
- Added `OrderSide.NONE` enum variant
- Added `PositionSide.NONE` enum variant
- Changed order of `TriggerType` enum variants
- Renamed `AggressorSide.UNKNOWN` to `AggressorSide.NONE` (for consistency with other enums)
- Renamed `Order.type` to `Order.order_type` (reduces ambiguity and aligns with Rust struct field)
- Renamed `OrderInitialized.type` to `OrderInitialized.order_type` reduces ambiguity)
- Renamed `Bar.type` to `Bar.bar_type` (reduces ambiguity and aligns with Rust struct field)
- Removed redundant `check_position_exists` flag
- Removed `hyperopt` as considered unmaintained and there are better options
- Existing pickled data for `QuoteTick` is now **invalid** (change to schema for correctness)
- Existing catalog data for `OrderInitialized` is now **invalid** (change to schema for emulation)

### Enhancements
- Added configurable automated in-flight order status checks
- Added order `side` filter to numerous cache order methods
- Added position `side` filter to numerous cache position methods
- Added optional `order_side` to `cancel_all_orders` strategy method
- Added optional `position_side` to `close_all_positions` strategy method
- Added support for Binance Spot second bars
- Added `RelativeVolatilityIndex` indicator, thanks @graceyangfan
- Extracted `OrderMatchingEngine` from `SimulatedExchange` with refinements
- Extracted `MatchingCore` from `OrderMatchingEngine`
- Improved HTTP error handling and client logging (messages now contain reason)

### Fixes
- Fixed price and size precision validation for `QuoteTick` from raw values
- Fixed IB adapter data parsing for decimal precision
- Fixed HTTP error handling and releasing of response coroutines, thanks @JackMa
- Fixed `Position` calculations and account for when any base currency == commission currency, thanks @JackMa

---

# NautilusTrader 1.155.0 Beta

Released on September 15th 2022 (UTC).

This is an early release to address some parsing bugs in the FTX adapter.

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed parsing bug for FTX futures
- Fixed parsing bug for FTX `Bar`

---

# NautilusTrader 1.154.0 Beta

Released on September 14th 2022 (UTC).

### Breaking Changes
- Changed `ExecEngineConfig` `allow_cash_positions` default to `True` (more typical use case)
- Removed `check` param from `Bar` (always checked for simplicity)

### Enhancements
- Added `MARKET_TO_LIMIT` order implementation for `SimulatedExchange`
- Make strategy `order_id_tag` truly optional and auto incrementing
- Added PsychologicalLine indicator, thanks @graceyangfan
- Added initial Rust parquet integration, thanks @twitu and @ghill2
- Added validation for setting leverages on `CASH` accounts
- De-cythonized live data and execution client base classes for usability

### Fixes
- Fixed limit order `IOC` and `FOK` behaviour, thanks @limx0 for identifying
- Fixed FTX `CryptoFuture` instrument parsing, thanks @limx0
- Fixed missing imports in data catalog example notebook, thanks @gaugau3000
- Fixed order update behaviour, affected orders:
  - `LIMIT_IF_TOUCHED`
  - `MARKET_IF_TOUCHED`
  - `MARKET_TO_LIMIT`
  - `STOP_LIMIT`

---

# NautilusTrader 1.153.0 Beta

Released on September 6th 2022 (UTC).

### Breaking Changes
None

### Enhancements
- Added trigger orders for FTX adapter
- Improved `BinanceBar` to handle enormous quote volumes
- Improved robustness of instrument parsing for Binance and FTX adapters
- Improved robustness of WebSocket message handling for Binance and FTX adapters
- Added `override_usd` option for FTX adapter
- Added `log_warnings` config option for Binance and FTX instrument providers
- Added `TRD_GRP_005` enum variant for Binance spot permissions

### Fixes
- Fixed bar aggregator partial bar handling
- Fixed `CurrencyType` variants in Rust
- Fixed missing `encoding` in Catalog parsing method, thanks @limx0 and @aviatorBeijing

---

# NautilusTrader 1.152.0 Beta

Released on September 1st 2022 (UTC).

### Breaking Changes
- Renamed `offset_type` to `trailing_offset_type`
- Renamed `is_frozen_account` to `frozen_account`
- Removed `bar_execution` from config API (implicitly turned on with bars currently)

### Enhancements
- Added `TRAILING_STOP_MARKET` order implementation for `SimulatedExchange`
- Added `TRAILING_STOP_LIMIT` order implementation for `SimulatedExchange`
- Added all simulated exchange options to `BacktestVenueConfig`

### Fixes
- Fixed creation and caching of order book on subscribing to deltas, thanks @limx0
- Fixed use of `LoopTimer` in live clock for trading node, thanks @sidnvy
- Fixed order cancels for IB adapter, thanks @limx0

---

# NautilusTrader 1.151.0 Beta

Released on August 22nd 2022 (UTC).

### Breaking Changes
None

### Enhancements
- Added `on_historical_data` method with wiring for functionality
- Added 'unthrottled' 0ms order book updates for Binance Futures
- Improved robustness of `WebSocketClient` base during reconnects

### Fixes
- Fixed sdist includes for Rust Cargo files
- Fixed `LatencyModel` integer overflows, thanks @limx0
- Fixed parsing of Binance Futures `FUNDING_FEE` updates
- Fixed `asyncio.tasks.gather` for Python 3.10+

---

# NautilusTrader 1.150.0 Beta

Released on August 15th 2022 (UTC).

### Breaking Changes
- `BacktestEngine` now required venues to be added prior to instruments
- `BacktestEngine` now requires instruments to be added prior to data
- Renamed `Ladder.reverse` to `Ladder.is_reversed`
- Portfolio performance now displays commissions as a negative

### Enhancements
- Added initial backtest config validation for instrument vs venue
- Added initial sandbox execution client
- Added leverage options for `BacktestVenueConfig`, thanks @miller-moore
- Allow `Trader` to run without strategies loaded
- Integrated core Rust clock and timer
- De-cythonize `InstrumentProvider` base class

### Fixes
- Fixed double counting of commissions for single-currency and multi-currency accounts #657

---

# NautilusTrader 1.149.0 Beta

Released on 27th June 2022 (UTC).

### Breaking Changes
- Schema change for `Instrument.info` for `ParquetDataCatalog`

### Enhancements
- Added `DirectionalMovementIndicator` indicator, thanks @graceyangfan
- Added `KlingerVolumeOscillator` indicator, thanks @graceyangfan
- Added `clientId` and `start_gateway` for IB config, thanks @niks199

### Fixes
- Fixed macOS ARM64 build
- Fixed Binance testnet URL
- Fixed IB contract ID dict, thanks @niks199
- Fixed IB `InstrumentProvider` #685, thanks @limx0
- Fixed IB orderbook snapshots L1 value assertion #712 , thanks @limx0

---

# NautilusTrader 1.148.0 Beta

Released on 30th June 2022 (UTC).

### Breaking Changes
None

### Enhancements
- Ported core bar objects to Rust thanks @ghill2
- Improved core `unix_nanos_to_iso8601` performance by 30% thanks @ghill2
- Added `DataCatalog` interface for `ParquetDataCatalog` thanks @jordanparker6
- Added `AroonOscillator` indicator thanks @graceyangfan
- Added `ArcherMovingAveragesTrends` indicator thanks @graceyangfan
- Added `DoubleExponentialMovingAverage` indicator thanks @graceyangfan
- Added `WilderMovingAverage` indicator thanks @graceyangfan
- Added `ChandeMomentumOscillator` indicator thanks @graceyangfan
- Added `VerticalHorizontalFilter` indicator thanks @graceyangfan
- Added `Bias` indicator thanks @graceyangfan

### Fixes
None

---

# NautilusTrader 1.147.1 Beta

Released on 6th June 2022 (UTC).

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed incorrect backtest log timestamps (was using actual time)
- Fixed formatting of timestamps for nanoseconds zulu as per RFC3339

---

# NautilusTrader 1.147.0 Beta

Released on 4th June 2022 (UTC).

### Breaking Changes
None

### Enhancements
- Improved error handling for invalid state triggers
- Improved component state transition behaviour and logging
- Improved `TradingNode` disposal flow
- Implemented core monotonic clock
- Implemented logging in Rust
- Added `CommodityChannelIndex` indicator thanks @graceyangfan

### Fixes
None

---

# NautilusTrader 1.146.0 Beta

Released on 22nd May 2022 (UTC).

### Breaking Changes
- `AccountId` constructor now takes single value string
- Removed redundant `UUIDFactory` and all associated backing fields and calls
- Removed `ClientOrderLinkId` (not in use)

### Enhancements
- Refinements and improvements to Rust core

### Fixes
- Fixed pre-trade notional risk checks incorrectly applied to `MARGIN` accounts
- Fixed `net_qty` in `PositionStatusReport` thanks to @sidnvy
- Fixed `LinearRegression` indicator thanks to @graceyangfan

---

# NautilusTrader 1.145.0 Beta

Released on 15th May 2022 (UTC).

This is an early release due to the build error in the sdist for `1.144.0`.
The error is due to the `nautilus_core` Rust source not being included in the sdist package.

### Breaking Changes
- All raw order constructors now take `expire_time_ns` int64 rather than a datetime
- All order serializations due to `expire_time_ns` option handling
- `PortfolioAnalyzer` moved from `Trader` to `Portfolio`

### Enhancements
- `PortfolioAnalyzer` now available to strategies via `self.portfolio.analyzer`

### Fixes
None

---

# NautilusTrader 1.144.0 Beta

Released on 10th May 2022 (UTC).

### Breaking Changes
- Removed `BacktestEngine.add_ticks()` as redundant with `.add_data()`
- Removed `BacktestEngine.add_bars()` as redundant with `.add_data()`
- Removed `BacktestEngine.add_generic_data()` as redundant with `.add_data()`
- Removed `BacktestEngine.add_order_book_data()` as redundant with `.add_data()`
- Renamed `Position.from_order` to `Position.opening_order_id`
- Renamed `StreamingPersistence` to `StreamingFeatherWriter`
- Renamed `PersistenceConfig` to `StreamingConfig`
- Renamed `PersistenceConfig.flush_interval` to `flush_interval_ms`

### Enhancements
- Added `Actor.publish_signal` for generic dynamic signal data
- Added `WEEK` and `MONTH` bar aggregation options
- Added `Position.closing_order_id` property
- Added `tags` param to `Strategy.submit_order`
- Added optional `check_positon_exists` flag to `Strategy.submit_order`
- Eliminated all use of `unsafe` Rust and C null-terminated byte strings
- The `bypass_logging` config option will also now bypass the `BacktestEngine` logger

### Fixes
- Fixed behaviour of `IOC` and `FOK` time in force instructions
- Fixed Binance bar resolution parsing

---

# NautilusTrader 1.143.0 Beta

Released on 21st April 2022 (UTC).

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed segfault for `CashAccount.calculate_balance_locked` with no base currency
- Various FeatherWriter fixes

---

# NautilusTrader 1.142.0 Beta

Released on 17th April 2022 (UTC).

### Breaking Changes
- `BacktestNode` now requires configs at initialization
- Removed `run_configs` param from `BacktestNode.run()` method
- Removed `return_engine` flag
- Renamed `TradingStrategy` to `Strategy`
- Renamed `TradingStrategyConfig` to `StrategyConfig`
- Changes to configuration object import paths
- Removed redundant `realized_points` concept from `Position`

### Enhancements
- Added `BacktestNode.get_engines()` method
- Added `BacktestNode.get_engine(run_config_id)` method
- Added `Actor.request_instrument()` method (also applies to `Strategy`)
- Added `Cache.snapshot_position()` method
- All configuration objects can now be imported directly from `nautilus_trader.config`
- Execution engine now takes snapshots of closed netted positions
- Performance statistics now based on total positions and snapshots
- Added Binance Spot/Margin external order handling
- Added support for millisecond bar aggregation
- Added configurable `debug` mode for engines (with extra debug logging)
- Improved annualized portfolio statistics with configurable period

### Fixes
None

---

# NautilusTrader 1.141.0 Beta

Released on 4th April 2022 (UTC).

### Breaking Changes
- Renamed `BacktestNode.run_sync()` to `BacktestNode.run()`
- Renamed `flatten_position()` to `close_position()`
- Renamed `flatten_all_positions()` to `close_all_positions()`
- Renamed `Order.flatten_side()` to `Order.closing_side()`
- Renamed `TradingNodeConfig` `check_residuals_delay` to `timeout_post_stop`
- The `SimulatedExchange` will now 'receive' market data prior to the `DataEngine`
  (note that this did not affect any test)
- Tightened requirement for `DataType` types to be subclasses of `Data`
- `CacheDatabaseConfig.type` now defaults to `in-memory`
- `NAUTILUS_CATALOG` env var changed to `NAUTILUS_PATH`
- `DataCatalog` root path now located under `$OLD_PATH/catalog/` from the Nautilus path
- `hiredis` and `redis` are now optional extras as 'redis'
- `hyperopt` is now an optional extra as 'hyperopt'

### Enhancements
- Unify `NautilusKernel` across backtest and live systems
- Improved configuration by grouping into `config` subpackage
- Improved configuration objects and flows
- Numerous improvements to the Binance Spot/Margin and Futures integration
- Added Docker image builds and GH packages
- Added `BinanceFuturesMarkPriceUpdate` type and data stream
- Added generic `subscribe` and `unsubscribe` to template
- Added Binance Futures COIN_M testnet
- The clarity of various error messages was improved

### Fixes
- Fixed multiple instruments in `DataCatalog` (#554), (#560) by @limx0
- Fixed timestamp ordering streaming from `DataCatalog` (#561) by @limx0
- Fixed `CSVReader` (#563) by @limx0
- Fixed slow subscribers to the Binance WebSocket streams
- Fixed configuration of `base_currency` for backtests
- Fixed importable strategy configs (previously not returning correct class)
- Fixed `fully_qualified_name()` format

---

# NautilusTrader 1.140.0 Beta

## Release Notes

Released on 13th March 2022 (UTC).

This is a patch release which fixes a moderate severity security vulnerability in
pillow < 9.0.1:

    If the path to the temporary directory on Linux or macOS contained a space, 
    this would break removal of the temporary image file after im.show() (and related actions), 
    and potentially remove an unrelated file. This been present since PIL.

This release upgrades to pillow 9.0.1.

Note the minor version was incremented in error.

---

# NautilusTrader 1.139.0 Beta

## Release Notes

Released on 11th March 2022 (UTC).

### Breaking Changes
- Renamed `CurrencySpot` to `CurrencyPair`
- Renamed `PerformanceAnalyzer` to `PortfolioAnalyzer`
- Renamed `BacktestDataConfig.data_cls_path` to `data_cls`
- Renamed `BinanceTicker` to `BinanceSpotTicker`
- Renamed `BinanceSpotExecutionClient` to `BinanceExecutionClient`

### Enhancements
- Added initial **(beta)** Binance Futures adapter implementation
- Added initial **(beta)** Interactive Brokers adapter implementation
- Added custom portfolio statistics
- Added `CryptoFuture` instrument
- Added `OrderType.MARKET_TO_LIMIT`
- Added `OrderType.MARKET_IF_TOUCHED`
- Added `OrderType.LIMIT_IF_TOUCHED`
- Added `MarketToLimitOrder` order type
- Added `MarketIfTouchedOrder` order type
- Added `LimitIfTouchedOrder` order type
- Added `Order.has_price` property (convenience)
- Added `Order.has_trigger_price` property (convenience)
- Added `msg` param to `LoggerAdapter.exception()`
- Added WebSocket `log_send` and `log_recv` config options
- Added WebSocket `auto_ping_interval` (seconds) config option
- Replaced `msgpack` with `msgspec` (faster drop in replacement https://github.com/jcrist/msgspec)
- Improved exception messages by providing helpful context
- Improved `BacktestDataConfig` API: now takes either a type of `Data` _or_ a fully qualified path string

### Fixes
- Fixed FTX execution WebSocket 'ping strategy'
- Fixed non-deterministic config dask tokenization

---

# NautilusTrader 1.138.0 Beta

## Release Notes

Released on 15th February 2022 (UTC).

**This release contains numerous method, parameter and property name changes**

For consistency and standardization with other protocols, the `ExecutionId` type 
has been renamed to `TradeId` as they express the same concept with a more 
standardized terminology. In the interests of enforcing correctness and 
safety this type is now utilized for the `TradeTick.trade_id`.

### Breaking Changes
- Renamed `working` orders to `open` orders including all associated methods and params
- Renamed `completed` orders to `closed` orders including all associated methods and params
- Removed `active` order concept (often confused with `open`)
- Renamed `trigger` to `trigger_price`
- Renamed `StopMarketOrder.price` to `StopMarketOrder.trigger_price`
- Renamed all params related to a `StopMarketOrders` `price` to `trigger_price`
- Renamed `ExecutionId` to `TradeId`
- Renamed `execution_id` to `trade_id`
- Renamed `Order.trade_id` to `Order.last_trade_id` (for clarity)
- Renamed other variations and references of 'execution ID' to 'trade ID'
- Renamed `contingency` to `contingency_type`

### Enhancements
- Introduced the `TradeId` type to enforce `trade_id` typing
- Improve handling of unleveraged cash asset positions including Crypto and Fiat spot currency instruments
- Added `ExecEngineConfig` option `allow_cash_positions` (`False` by default)
- Added `TrailingOffsetType` enum
- Added `TrailingStopMarketOrder`
- Added `TrailingStopLimitOrder`
- Added trailing order factory methods
- Added `trigger_type` param to stop orders
- Added `TriggerType` enum
- Large refactoring of order base and impl classes
- Overhaul of execution reports
- Overhaul of execution state reconciliation

### Fixes
- Fixed WebSocket base reconnect handling

---

# NautilusTrader 1.137.1 Beta

## Release Notes

Released on 15th January 2022 (UTC).

This is a patch release which fixes moderate to high severity security vulnerabilities in
`pillow < 9.0.0`:
- PIL.ImageMath.eval allows evaluation of arbitrary expressions, such as ones that use the Python exec method
- path_getbbox in path.c has a buffer over-read during initialization of ImagePath.Path
- path_getbbox in path.c improperly initializes ImagePath.Path

This release upgrades to `pillow 9.0.0`.

---

# NautilusTrader 1.137.0 Beta

## Release Notes

Released on 12th January 2022 (UTC).

### Breaking Changes
- Removed redundant `currency` param from `AccountBalance`
- Renamed `local_symbol` to `native_symbol`
- Removed the `VenueType` enum and `venue_type` param in favour of a `routing` bool flag
- Removed `account_id` param from execution client factories and constructors
- Changed venue generated IDs (order, execution, position) which now begin with the venue ID

### Enhancements
- Added FTX integration for testing
- Added FTX US configuration option
- Added Binance US configuration option
- Added `MarginBalance` object to assist with margin account functionality

### Fixes
- Fixed parsing of `BarType` with symbols including hyphens `-`
- Fixed `BinanceSpotTicker` `__repr__` (was missing whitespace after a comma)
- Fixed `DataEngine` requests for historical `TradeTick`
- Fixed `DataEngine` `_handle_data_response` typing of `data` to `object`

---

# NautilusTrader 1.136.0 Beta

## Release Notes

Released on 29th December 2021.

### Breaking Changes
- Changed `subscribe_data(...)` method (`client_id` now optional)
- Changed `unsubscribe_data(...)` method (`client_id` now optional)
- Changed `publish_data(...)` method (added `data_type`)
- Renamed `MessageBus.subscriptions` method param to `pattern`
- Renamed `MessageBus.has_subscribers` method param to `pattern`
- Removed `subscribe_strategy_data(...)` method
- Removed `unsubscribe_strategy_data(...)` method
- Removed `publish_strategy_data(...)` method
- Renamed `CryptoSwap` to `CryptoPerpetual`

### Enhancements
- Can now modify or cancel in-flight orders live and backtest
- Updated `CancelOrder` to allow None `venue_order_id`
- Updated `ModifyOrder` to allow None `venue_order_id`
- Updated `OrderPendingUpdate` to allow None `venue_order_id`
- Updated `OrderPendingCancel` to allow None `venue_order_id`
- Updated `OrderCancelRejected` to allow None `venue_order_id`
- Updated `OrderModifyRejected` to allow None `venue_order_id`
- Added `DataType.topic` string for improved message bus handling

### Fixes
- Implemented comparisons for `DataType`, `BarSpecification` and `BarType`
- Fixed `QuoteTickDataWrangler.process_bar_data` with `random_seed`

---

# NautilusTrader 1.135.0 Beta

## Release Notes

Released on 13th December 2021.

### Breaking Changes
- Renamed `match_id` to `trade_id`

### Enhancements
- Added bars method to `DataCatalog`
- Improved parsing of Binance historical bars data
- Added `CancelAllOrders` command
- Added bulk cancel capability to Binance integration
- Added bulk cancel capability to Betfair integration

### Fixes
- Fixed handling of `cpu_freq` call in logging for ARM architecture
- Fixed market order fill edge case for bar data
- Fixed handling of `GenericData` in backtests

---

# NautilusTrader 1.134.0 Beta

## Release Notes

Released on 22nd November 2021.

### Breaking Changes
- Changed `hidden` order option to `display_qty` to support iceberg orders
- Renamed `Trader.component_ids()` to `Trader.actor_ids()`
- Renamed `Trader.component_states()` to `Trader.actor_states()`
- Renamed `Trader.add_component()` to `Trader.add_actor()`
- Renamed `Trader.add_components()` to `Trader.add_actors()`
- Renamed `Trader.clear_components()` to `Trader.clear_actors()`

### Enhancements
- Added initial implementation of Binance SPOT integration (beta stage testing)
- Added support for display quantity/iceberg orders

### Fixes
- Fixed `Actor` clock time advancement in backtest engine

---

# NautilusTrader 1.133.0 Beta

## Release Notes

Released on 8th November 2021.

### Breaking Changes
None

### Enhancements
- Added `LatencyModel` for simulated exchange
- Added `last_update_id` to order books
- Added `update_id` to order book data
- Added `depth` param when subscribing to order book deltas
- Added `Clock.timestamp_ms()`
- Added `TestDataProvider` and consolidate test data
- Added orjson default serializer for arrow
- Reorganized example strategies and launch scripts

### Fixes
- Fixed logic for partial fills in backtests
- Various Betfair integration fixes
- Various `BacktestNode` fixes

---

# NautilusTrader 1.132.0 Beta

## Release Notes

Released on 24th October 2021.

### Breaking Changes
- `Actor` constructor now takes `ActorConfig`

### Enhancements
- Added `ActorConfig`
- Added `ImportableActorConfig`
- Added `ActorFactory`
- Added `actors` to `BacktestRunConfig`
- Improved network base classes
- Refine `InstrumentProvider`

### Fixes
- Fixed persistence config for `BacktestNode`
- Various Betfair integration fixes

---

# NautilusTrader 1.131.0 Beta

## Release Notes

Released on 10th October 2021.

### Breaking Changes
- Renamed `nanos_to_unix_dt` to `unix_nanos_to_dt` (more accurate name)
- Changed `Clock.set_time_alert(...)` method signature
- Changed `Clock.set_timer(...)` method signature
- Removed `pd.Timestamp` from `TimeEvent`

### Enhancements
- `OrderList` submission and OTO, OCO contingencies now operational
- Added `Cache.orders_for_position(...)` method
- Added `Cache.position_for_order(...)` method
- Added `SimulatedExchange.get_working_bid_orders(...)` method
- Added `SimulatedExchange.get_working_ask_orders(...)` method
- Added optional `run_config_id` for backtest runs
- Added `BacktestResult` object
- Added `Clock.set_time_alert_ns(...)` method
- Added `Clock.set_timer_ns(...)` method
- Added `fill_limit_at_price` simulated exchange option
- Added `fill_stop_at_price` simulated exchange option
- Improve timer and time event efficiency

### Fixes
- Fixed `OrderUpdated` leaves quantity calculation
- Fixed contingency order logic at the exchange
- Fixed indexing of orders for a position in the cache
- Fixed flip logic for zero sized positions (not a flip)

---

# NautilusTrader 1.130.0 Beta

## Release Notes

Released on 26th September 2021.

### Breaking Changes
- `BacktestEngine.run` method signature change
- Renamed `BookLevel` to `BookType`
- Renamed `FillModel` params

### Enhancements
- Added streaming backtest machinery.
- Added `quantstats` (removed `empyrical`)
- Added `BacktestEngine.run_streaming()`
- Added `BacktestEngine.end_streaming()`
- Added `Portfolio.balances_locked(venue)`
- Improved `DataCatalog` functionality
- Improved logging for `BacktestEngine`
- Improved parquet serialization and machinery

### Fixes
- Fixed `SimulatedExchange` message processing
- Fixed `BacktestEngine` event ordering in main loop
- Fixed locked balance calculation for `CASH` accounts
- Fixed fill dynamics for `reduce-only` orders
- Fixed `PositionId` handling for `HEDGING` OMS exchanges
- Fixed parquet `Instrument` serialization
- Fixed `CASH` account PnL calculations with base currency

---

# NautilusTrader 1.129.0 Beta

## Release Notes

Released on 12th September 2021.

### Breaking Changes
- Removed CCXT adapter (#428)
- Backtest configuration changes
- Renamed `UpdateOrder` to `ModifyOrder` (terminology standardization)
- Renamed `DeltaType` to `BookAction` (terminology standardization)

### Enhancements
- Added `BacktestNode`
- Added `BookIntegrityError` with improved integrity checks for order books
- Added order custom user tags
- Added `Actor.register_warning_event` (also applicable to `TradingStrategy`)
- Added `Actor.deregister_warning_event` (also applicable to `TradingStrategy`)
- Added `ContingencyType` enum (for contingency orders in an `OrderList`)
- All order types can now be `reduce_only` (#437)
- Refined backtest configuration options
- Improved efficiency of `UUID4` using the Rust `fastuuid` Python bindings

### Fixes
- Fixed Redis loss of precision for `int64_t` nanosecond timestamps (#363)
- Fixed behavior of `reduce_only` orders for both submission and filling (#437)
- Fixed PnL calculation for `CASH` accounts when commission negative (#436)

---

# NautilusTrader 1.128.0 Beta - Release Notes

Released on 30th August 2021.

This release continues the focus on the core system, with upgrades and cleanups
to the component base class. The concept of an `active` order has been introduced, 
which is an order whose state can change (is not a `completed` order).

### Breaking Changes
- All configuration due `pydantic` upgrade
- Throttling config now takes string e.g. "100/00:00:01" which is 100 / second
- Renamed `DataProducerFacade` to `DataProducer`
- Renamed `fill.side` to `fill.order_side` (clarity and standardization)
- Renamed `fill.type` to `fill.order_type` (clarity and standardization)

### Enhancements
- Added serializable configuration classes leveraging `pydantic`
- Improved adding bar data to `BacktestEngine`
- Added `BacktestEngine.add_bar_objects()`
- Added `BacktestEngine.add_bars_as_ticks()`
- Added order `active` concept, with `order.is_active` and cache methods
- Added `ComponentStateChanged` event
- Added `Component.degrade()` and `Component.fault()` command methods
- Added `Component.on_degrade()` and `Component.on_fault()` handler methods
- Added `ComponentState.PRE_INITIALIZED`
- Added `ComponentState.DEGRADING`
- Added `ComponentState.DEGRADED`
- Added `ComponentState.FAULTING`
- Added `ComponentState.FAULTED`
- Added `ComponentTrigger.INITIALIZE`
- Added `ComponentTrigger.DEGRADE`
- Added `ComponentTrigger.DEGRADED`
- Added `ComponentTrigger.FAULT`
- Added `ComponentTrigger.FAULTED`
- Wired up `Ticker` data type

### Fixes
- `DataEngine.subscribed_bars()` now reports internally aggregated bars also.

---

# NautilusTrader 1.127.0 Beta

## Release Notes

Released on 17th August 2021.

This release has again focused on core areas of the platform, including a 
significant overhaul of accounting and portfolio components. The wiring between 
the `DataEngine` and `DataClient`(s) has also received attention, and should now 
exhibit correct subscription mechanics.

The Betfair adapter has been completely re-written, providing various fixes and
enhancements, increased performance, and full async support.

There has also been some further renaming to continue to align the platform
as closely as possible with established terminology in the domain.

### Breaking Changes
- Moved margin calculation methods from `Instrument` to `Account`
- Removed redundant `Portfolio.register_account`
- Renamed `OrderState` to `OrderStatus`
- Renamed `Order.state` to `Order.status`
- Renamed `msgbus.message_bus` to `msgbus.bus`

### Enhancements
- Betfair adapter re-write
- Extracted `accounting` subpackage
- Extracted `portfolio` subpackage
- Subclassed `Account` with `CashAccount` and `MarginAccount`
- Added `AccountsManager`
- Added `AccountFactory`
- Moved registration of custom account classes to `AccountFactory`
- Moved registration of calculated account to `AccountFactory`
- Added registration of OMS type per trading strategy
- Added `ExecutionClient.create_account` for custom account classes
- Separate `PortfolioFacade` from `Portfolio`

### Fixes
- Data subscription handling in `DataEngine`
- `Cash` accounts no longer generate spurious margins
- Fix `TimeBarAggregator._stored_close_ns` property name

---

# NautilusTrader 1.126.1 Beta

## Release Notes

Released on 3rd August 2021.

This is a patch release which fixes a bug involving `NotImplementedError` 
exception handling when subscribing to order book deltas when not supported by 
a client. This bug affected CCXT order book subscriptions.

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fix `DataEngine` order book subscription handling

---

# NautilusTrader 1.126.0 Beta

## Release Notes

Released on 2nd August 2021.

This release sees the completion of the initial implementation of the 
`MessageBus`, with data now being handled by Pub/Sub patterns, along with the 
additions of point-to-point and Req/Rep messaging functionality.

An `Actor` base class has been abstracted from `TradingStrategy` which allows
custom components to be added to a `Trader` which aren't necessarily trading 
strategies, opening up further possibilities for extending NautilusTrader with 
custom functionality.

For the sake of simplicity and to favour more idiomatic Python, the null object
pattern is no longer utilized for handling identifiers. This has removed a layer
of 'logical indirection' in certain parts of the codebase, and allows for simpler 
code.

An order is now considered 'in-flight' if it is actively pending a state 
transition i.e. in the `SUBMITTED`,`PENDING_UPDATE` or `PENDING_CANCEL` states.

It is now a well established convention that all integer based timestamps are 
expressed in UNIX nanoseconds, therefore the `_ns` postfix has now been dropped. 
For clarity - time periods/intervals/objects where the units may not be obvious 
have retained the `_ns` postfix.

The opportunity was identified to unify the parameter naming for the concept
of object instantiation by renaming `timestamp_ns` and `ts_recv_ns` to `ts_init`.
Along the same lines, the timestamps for both event and data occurrence have 
been standardized to `ts_event`.

It is acknowledged that the frequent name changes and modifications to core 
concepts may be frustrating, however whilst still in a beta phase - we're taking 
the opportunity to lay a solid foundation for this project to continue to growth 
in the years ahead.

### Breaking Changes
- Renamed `timestamp_ns` to `ts_init`
- Renamed `ts_recv_ns` to `ts_event`
- Renamed various event timestamp parameters to `ts_event`
- Removed null object methods on identifiers

### Enhancements
- Added `Actor` component base class
- Added `MessageBus.register()`
- Added `MessageBus.send()`
- Added `MessageBus.request()`
- Added `MessageBus.response()`
- Added `Trader.add_component()`
- Added `Trader.add_components()`
- Added `Trader.add_log_sink()`

### Fixes
- Various Betfair adapter patches and fixes
- `ExecutionEngine` position flip logic in certain edge cases

---

# NautilusTrader 1.125.0 Beta

## Release Notes

Released on 18th July 2021.

This release introduces a major re-architecture of the internal messaging system.
A common message bus has been implemented which now handles all events via a 
Pub/Sub messaging pattern. The next release will see all data being handled by 
the message bus, please see the related issue for further details on this enhancement.

Another notable feature is the introduction of the order 'in-flight' concept, 
which is a submitted order which has not yet been acknowledged by the 
trading venue. Several properties on `Order`, and methods on `Cache`, now exist
to support this.

The `Throttler` has been refactored and optimized further. There has also been
extensive reorganization of the model sub-package, standardization of identifiers
on events, along with numerous 'under the hood' cleanups and two bug fixes.

### Breaking Changes
- Renamed `MessageType` enum to `MessageCategory`
- Renamed `fill.order_side` to `fill.side`
- Renamed `fill.order_type` to `fill.type`
- All `Event` serialization due to domain refactorings

### Enhancements
- Added `MessageBus` class
- Added `TraderId` to `Order` and `Position`
- Added `OrderType` to OrderFilled
- Added unrealized PnL to position events
- Added order in-flight concept to `Order` and `Cache`
- Improved efficiency of `Throttler`
- Standardized events `str` and `repr`
- Standardized commands `str` and `repr`
- Standardized identifiers on events and objects
- Improved `Account` `str` and `repr`
- Using `orjson` over `json` for efficiency
- Removed redundant `BypassCacheDatabase`
- Introduced `mypy` to the codebase

### Fixes
- Fixed backtest log timestamping
- Fixed backtest duplicate initial account event

---

# NautilusTrader 1.124.0 Beta

## Release Notes

Released on 6th July 2021.

This release sees the expansion of pre-trade risk check options (see 
`RiskEngine` class documentation). There has also been extensive 'under the 
hood' code cleanup and consolidation.

### Breaking Changes
- Renamed `Position.opened_timestamp_ns` to `ts_opened_ns`
- Renamed `Position.closed_timestamp_ns` to `ts_closed_ns`
- Renamed `Position.open_duration_ns` to `duration_ns`
- Renamed Loggers `bypass_logging` to `bypass`
- Refactored `PositionEvent` types

### Enhancements
- Add pre-trade risk checks to `RiskEngine` iteration 2
- Improve `Throttler` functionality and performance
- Removed redundant `OrderInvalid` state and associated code
- Improve analysis reports

### Fixes
- PnL calculations for `CASH` account types
- Various event serializations

---

# NautilusTrader 1.123.0 Beta

## Release Notes

Released on 20th June 2021.

A major feature of this release is a complete re-design of serialization for the
platform, along with initial support for the [Parquet](https://parquet.apache.org/) format.
The MessagePack serialization functionality has been refined and retained.

In the interests of explicitness there is now a convention that timestamps are 
named either `timestamp_ns`, or prepended with `ts`. Timestamps which are 
represented with an `int64` are always in nanosecond resolution, and appended 
with `_ns` accordingly.

Initial scaffolding for new backtest data tooling has been added.

### Breaking Changes
- Renamed `OrderState.PENDING_REPLACE` to `OrderState.PENDING_UPDATE`
- Renamed `timestamp_origin_ns` to `ts_event_ns`
- Renamed `timestamp_ns` for data to `ts_recv_ns`
- Renamed `updated_ns` to `ts_updated_ns`
- Renamed `submitted_ns` to `ts_submitted_ns`
- Renamed `rejected_ns` to `ts_rejected_ns`
- Renamed `accepted_ns` to `ts_accepted_ns`
- Renamed `pending_ns` to `ts_pending_ns`
- Renamed `canceled_ns` to `ts_canceled_ns`
- Renamed `triggered_ns` to `ts_triggered_ns`
- Renamed `expired_ns` to `ts_expired_ns`
- Renamed `execution_ns` to `ts_filled_ns`
- Renamed `OrderBookLevel` to `BookLevel`
- Renamed `Order.volume` to `Order.size`

### Enhancements
- Adapter dependencies are now optional extras at installation
- Added arrow/parquet serialization
- Added object `to_dict()` and `from_dict()` methods
- Added `Order.is_pending_update`
- Added `Order.is_pending_cancel`
- Added `run_analysis` config option for `BacktestEngine`
- Removed `TradeMatchId` in favour of bare string
- Removed redundant conversion to `pd.Timestamp` when checking timestamps
- Removed redundant data `to_serializable_str` methods
- Removed redundant data `from_serializable_str` methods
- Removed redundant `__ne__` implementations
- Removed redundant `MsgPackSerializer` cruft
- Removed redundant `ObjectCache` and `IdentifierCache`
- Removed redundant string constants

### Fixes
- Fixed millis to nanos in `CCXTExecutionClient`
- Added missing trigger to `UpdateOrder` handling
- Removed all `import *`

---

# NautilusTrader 1.122.0 Beta

## Release Notes

Released on 6th June 2021.

This release includes numerous breaking changes with a view to enhancing the core
functionality and API of the platform. The data and execution caches have been 
unified for simplicity. There have also been large changes to the accounting 
functionality, with 'hooks' added in preparation for accurate calculation and 
handling of margins.

### Breaking Changes
- Renamed `Account.balance()` to `Account.balance_total()`
- Consolidated`TradingStrategy.data` into `TradingStrategy.cache`
- Consolidated `TradingStrategy.execution` into `TradingStrategy.cache`
- Moved `redis` subpackage into `infrastructure`
- Moved some accounting methods back to `Instrument`
- Removed `Instrument.market_value()`
- Renamed `Portfolio.market_values()` to `Portfolio.net_exposures()`
- Renamed `Portfolio.market_value()` to `Portfolio.net_exposure()`
- Renamed `InMemoryExecutionDatabase` to `BypassCacheDatabase`
- Renamed `Position.relative_qty` to `Position.net_qty`
- Renamed `default_currency` to `base_currency`
- Removed `cost_currency` property from `Instrument`

### Enhancements
- `ExecutionClient` now has the option of calculating account state
- Unified data and execution caches into single `Cache`
- Improved configuration options and naming
- Simplified `Portfolio` component registration
- Simplified wiring of `Cache` into components
- Added `repr` to execution messages
- Added `AccountType` enum
- Added `cost_currency` to `Position`
- Added `get_cost_currency()` to `Instrument`
- Added `get_base_currency()` to `Instrument`

### Fixes
- Fixed `Order.is_working` for `PENDING_CANCEL` and `PENDING_REPLACE` states
- Fixed loss of precision for nanosecond timestamps in Redis
- Fixed state reconciliation when uninstantiated client

---

# NautilusTrader 1.121.0 Beta

## Release Notes

Released on 30th May 2021.

In this release there has been a major change to the use of inlines for method
signatures. From the Cython docs:
_"Note that class-level cdef functions are handled via a virtual function table
so the compiler won’t be able to inline them in almost all cases."_.
https://cython.readthedocs.io/en/latest/src/userguide/pyrex_differences.html?highlight=inline.

It has been found that adding `inline` to method signatures makes no difference
to the performance of the system - and so they have been removed to reduce 
'noise' and simplify the codebase. Please note that the use of `inline` for 
module level functions will be passed to the C compiler with the expected 
result of inlining the function.

### Breaking Changes
- `BacktestEngine.add_venue` added `venue_type` to method params
- `ExecutionClient` added `venue_type` to constructor params
- `TraderId` instantiation
- `StrategyId` instantiation
- `Instrument` serialization

### Enhancements
- `Portfolio` pending calculations if data not immediately available
- Added `instruments` subpackage with expanded class definitions
- Added `timestamp_origin_ns` timestamp when originally occurred
- Added `AccountState.is_reported` flagging if reported by exchange or calculated
- Simplified `TraderId` and `StrategyId` identifiers
- Improved `ExecutionEngine` order routing
- Improved `ExecutionEngine` client registration
- Added order routing configuration
- Added `VenueType` enum and parser
- Improved param typing for identifier generators
- Improved log formatting of `Money` and `Quantity` thousands commas

### Fixes
- CCXT `TICK_SIZE` precision mode - size precisions (BitMEX, FTX)
- State reconciliation (various bugs)

---

# NautilusTrader 1.120.0 Beta

## Release Notes

This release focuses on simplifications and enhancements of existing machinery

### Breaking Changes
- `Position` now requires an `Instrument` param
- `is_inverse` removed from `OrderFilled`
- `ClientId` removed from `TradingCommand` and subclasses
- `AccountId` removed from `TradingCommand` and subclasses
- `TradingCommand` serialization

### Enhancements
- Added `Instrument` methods to `ExecutionCache`
- Added `Venue` filter to cache queries
- Moved order validations into `RiskEngine`
- Refactored `RiskEngine`
- Removed routing type information from identifiers

### Fixes
None

---

# NautilusTrader 1.119.0 Beta

## Release Notes

This release applies another major refactoring to the value object API for
`BaseDecimal` and its subclasses `Price` and `Quantity`. Previously a precision
was not explicitly required when passing in a `decimal.Decimal` type which
sometimes resulted in unexpected behavior when a user passed in a decimal with
a very large precision (when wrapping a float with `decimal.Decimal`).

Convenience methods have been added to `Price` and `Quantity` where precision
is implicitly zero for ints, or implied in the number of digits after the '.'
point for strings. Convenience methods have also been added to `Instrument` to
assist the UX.

The serialization of `Money` has been improved with the inclusion of the 
currency code in the string delimited by whitespace. This avoids an additional
field for the currency code.

`RiskEngine` has been rewired ahead of `ExecutionEngine` which clarifies areas
of responsibility and cleans up the registration sequence and allows a more
natural flow of command and event messages.

### Breaking Changes
- Serializations involving `Money`
- Changed usage of `Price` and `Quantity`
- Renamed `BypassExecutionDatabase` to `BypassCacheDatabase`

### Enhancements
- Rewired `RiskEngine` and `ExecutionEngine` sequence
- Added `Instrument` database operations
- Added `MsgPackInstrumentSerializer`
- Added `Price.from_str()`
- Added `Price.from_int()`
- Added `Quantity.zero()`
- Added `Quantity.from_str()`
- Added `Quantity.from_int()`
- Added `Instrument.make_price()`
- Added `Instrument.make_qty()`
- Improved serialization of `Money`

### Fixes
- Handling of precision for `decimal.Decimal` values passed to value objects

---

# NautilusTrader 1.118.0 Beta

## Release Notes

This release simplifies the backtesting workflow by removing the need for the 
intermediate `BacktestDataContainer`. There has also been some simplifications
for `OrderFill` events, as well as additional order states and events.

### Breaking Changes
- Standardized all 'cancelled' references to 'canceled'.
- `SimulatedExchange` no longer generates `OrderAccepted` for `MarketOrder`
- Removed redundant `BacktestDataContainer`
- Removed redundant `OrderFilled.cum_qty`
- Removed redundant `OrderFilled.leaves_qty`
- `BacktestEngine` constructor simplified
- `BacktestMarketDataClient` no longer needs instruments
- Rename `PortfolioAnalyzer.get_realized_pnls` to `.realized_pnls`

### Enhancements
- Re-engineered `BacktestEngine` to take data directly
- Added `OrderState.PENDING_CANCEL`
- Added `OrderState.PENDING_REPLACE`
- Added `OrderPendingUpdate` event
- Added `OrderPendingCancel` event
- Added `OrderFilled.is_buy` property (with corresponding `is_buy_c()` fast method)
- Added `OrderFilled.is_sell` property (with corresponding `is_sell_c()` fast method)
- Added `Position.is_opposite_side(OrderSide side)` convenience method
- Modified the `Order` FSM and event handling for the above
- Consolidated event generation into `ExecutionClient` base class
- Refactored `SimulatedExchange` for greater clarity

### Fixes
- `ExecutionCache` positions open queries
- Exchange accounting for exchange `OMSType.NETTING`
- Position flipping logic for exchange `OMSType.NETTING`
- Multi-currency account terminology
- Windows wheel packaging
- Windows path errors

---

# NautilusTrader 1.117.0 Beta

## Release Notes

The major thrust of this release is added support for order book data in
backtests. The `SimulatedExchange` now maintains order books of each instrument
and will accurately simulate market impact with L2/L3 data. For quote and trade
tick data a L1 order book is used as a proxy. A future release will include 
improved fill modelling assumptions and customizations.

### Breaking Changes
- `OrderBook.create` now takes `Instrument` and `BookLevel`

### Enhancements
- `SimulatedExchange` now maintains order books internally
- `LiveLogger` now exhibits better blocking behavior and logging

### Fixes
- Various patches to the `Betfair` adapter
- Documentation builds

---

# NautilusTrader 1.116.1 Beta

## Release Notes

Announcing official Windows 64-bit support.

Several bugs have been identified and fixed.

### Breaking Changes
None

### Enhancements
- Performance test refactoring
- Remove redundant performance harness
- Add `Queue.peek()` to high-performance queue
- GitHub action refactoring, CI for Windows
- Builds for 32-bit platforms

### Fixes
- `OrderBook.create` for `BookLevel.L3` now returns correct book
- Betfair handling of trade IDs

---

# NautilusTrader 1.116.0 Beta

## Release Notes

**This release includes substantial breaking changes.**

Further fundamental changes to the core API have been made.

### Breaking Changes
- Introduce `ClientId` for data and execution client identification
- Standardize client IDs to upper case
- Rename `OrderBookOperation` to `OrderBookDelta`
- Rename `OrderBookOperations` to `OrderBookDeltas`
- Rename `OrderBookOperationType` to `OrderBookDeltaType`

### Enhancements
None

### Fixes
None

---

# NautilusTrader 1.115.0 Beta

## Release Notes

**This release includes substantial breaking changes.**

Due to recent feedback and much further thought - a major renaming has been carried
out involving order identifiers. The `Order` is the only domain object in the
model which is identified with more than one ID. Due to this, more explicitness
helps to ensure correct logic. Previously the `OrderId` was
implicitly assumed to be the one assigned by the trading venue. This has been
clarified by renaming the identifier to `VenueOrderId`. Following this, it no
longer made sense to refer to it through `Order.id`, and so this was changed to
its full name `Order.venue_order_id`. This naturally resulted in `ClientOrderId`(s)
being renamed in properties and variables from `cl_ord_id` to `client_order_id`.

### Breaking Changes
- Rename `OrderId` to `VenueOrderId`
- Rename `Order.id` to `Order.venue_order_id`
- Rename `Order.cl_ord_id` to `Order.client_order_id`
- Rename `AssetClass.STOCK` to `AssetClass.EQUITY`
- Remove redundant flag `generate_position_ids` (handled by `OMSType`)

### Enhancements
- Introduce integration for Betfair.
- Add `AssetClass.METAL` and `AssetClass.ENERGY`
- Add `VenueStatusEvent`, `InstrumentStatusEvent` and `InstrumentClosePrice`
- Usage of `np.ndarray` to improve function and indicator performance

### Fixes
- LiveLogger log message when blocking.

---

# NautilusTrader 1.114.0 Beta

## Release Notes

**This release includes substantial breaking changes.**

Further standardization of naming conventions along with internal refinements
and fixes.

### Breaking Changes
- Rename `AmendOrder` to `UpdateOrder`
- Rename `OrderAmended` to `OrderUpdated`
- Rename `amend` and `amended` related methods to `update` and `updated`
- Rename `OrderCancelReject` to `OrderCancelRejected` (standardize tense)

### Enhancements
- Improve efficiency of data wrangling
- Simplify `Logger` and general system logging
- Add `stdout` and `stderr` log streams with configuration
- Add `OrderBookData` base class

### Fixes
- Backtest handling of `GenericData` and `OrderBook` related data
- Backtest `DataClient` creation logic prevented client registering

---

# NautilusTrader 1.113.0 Beta

## Release Notes

**This release includes substantial breaking changes.**

Further standardization of naming conventions along with internal refinements
and fixes.

### Breaking Changes
- Rename `AmendOrder` to `UpdateOrder`
- Rename `OrderAmended` to `OrderUpdated`
- Rename `amend` and `amended` related methods to `update` and `updated`
- Rename `OrderCancelReject` to `OrderCancelRejected` (standardize tense)

### Enhancements
- Introduce `OrderUpdateRejected`, event separated for clarity
- Refined LiveLogger: Now runs on event loop with high-performance `Queue`
- Improved flexibility of when strategies are added to a `BacktestEngine`
- Improved checks for `VenueOrderId` equality when applying order events

### Fixes
- Removed `UNDEFINED` enum values. Do not allow invalid values to be represented
in the system (prefer throwing exceptions)

---

# NautilusTrader 1.112.0 Beta

## Release Notes

**This release includes substantial breaking changes.**

The platforms internal timestamping has been standardized to nanoseconds. This
decision was made to increase the accuracy of backtests to nanosecond precision,
improve data handling including order book and custom data for backtesting, and
to future-proof the platform to a more professional standard. The top-level user
API still takes `datetime` and `timedelta` objects for usability.

There has also been some standardization of naming conventions to align more
closely with established financial market terminology with reference to the
FIX5.0 SP2 specification, and CME MDP 3.0.

### Breaking Changes
- Move `BarType` into `Bar` as a property
- Change signature of `Bar` handling methods due to above
- Remove `Instrument.leverage` (incorrect place for concept)
- Change `ExecutionClient.venue` as a `Venue` to `ExecutionClient.name` as a `str`
- Change serialization of timestamp datatype to `int64`
- Extensive changes to serialization constant names
- Rename `OrderFilled.filled_qty` to `OrderFilled.last_qty`
- Rename `OrderFilled.filled_price` to `OrderFilled.last_px`
- Rename `avg_price` to `avg_px` in methods and properties
- Rename `avg_open` to `avg_px_open` in methods and properties
- Rename `avg_close` to `avg_px_close` in methods and properties
- Rename `Position.relative_quantity` to `Position.relative_qty`
- Rename `Position.peak_quantity` to `Position.peak_qty`

### Enhancements
- Standardize nanosecond timestamps
- Add time unit conversion functions as found in `nautilus_trader.core.datetime`
- Add optional `broker` property to `Venue` to assist with routing
- Enhance state reconciliation from both `LiveExecutionEngine` and `LiveExecutionClient`
- Add internal messages to aid state reconciliation

### Fixes
- `DataCache` incorrectly caching bars

---

# NautilusTrader 1.111.0 Beta

## Release Notes

This release adds further enhancements to the platform.

### Breaking Changes
None

### Enhancements
- `RiskEngine` built out including configuration options hook and
  `LiveRiskEngine` implementation
- Add generic `Throttler`
- Add details `dict` to `instrument_id` related requests to cover IB futures
  contracts
- Add missing Fiat currencies
- Add additional Crypto currencies
- Add ISO 4217 codes
- Add currency names

### Fixes
- Queue `put` coroutines in live engines when blocking at `maxlen` was not
  creating a task on the event loop.

---

# NautilusTrader 1.110.0 Beta

## Release Notes

This release applies one more major change to the identifier API. `Security` has
been renamed to `InstrumentId` for greater clarity that the object is an identifier,
and to group the concept of an instrument with its identifier.

Data objects in the framework have been further abstracted to prepare for the
handling of custom data in backtests.

A `RiskEngine` base class has also been scaffolded.

### Breaking Changes
- `Security` renamed to `InstrumentId`
- `Instrument.security` renamed to `Instrument.id`
- `Data` becomes an abstract base class with `timestamp` and `unix_timestamp`
  properties
- `Data` and `DataType` moved to `model.data`
- `on_data` methods now take `GenericData`

### Enhancements
- Add `GenericData`
- Add `Future` instrument

### Fixes
None

---

# NautilusTrader 1.109.0 Beta

## Release Notes

The main thrust of this release is to refine and further bed down the changes
to the identifier model via `InstrumentId`, and fix some bugs.

Errors in the CCXT clients caused by the last release have been addressed.

### Breaking Changes
- `InstrumentId` now takes first class value object `Symbol`
- `InstrumentId` `asset_class` and `asset_type` no longer optional
- `SimulatedExchange.venue` changed to `SimulatedExchange.id`

### Enhancements
- Ensure `TestTimer` advances monotonically increase
- Add `AssetClass.BETTING`

### Fixes
- CCXT data and execution clients regarding `instrument_id` vs `symbol` naming
- `InstrumentId` equality and hashing
- Various docstrings

---

# NautilusTrader 1.108.0 Beta

## Release Notes

This release executes a major refactoring of `Symbol` and how securities are
generally identified within the platform. This will allow a smoother integration
with Interactive Brokers and other exchanges, brokerages and trading
counterparties.

Previously the `Symbol` identifier also included a venue which confused the concept.
The replacement `Security` identifier more clearly expresses the domain with a
symbol string, a primary `Venue`, `AssetClass` and `AssetType` properties.

### Breaking Changes
- All previous serializations
- `Security` replaces `Symbol` with expanded properties
- `AssetClass.EQUITY` changed to `AssetClass.STOCK`
- `from_serializable_string` changed to `from_serializable_str`
- `to_serializable_string` changed to `to_serializable_str`

### Enhancements
- Reports now include full instrument_id name
- Add `AssetType.WARRANT`

### Fixes
- `StopLimitOrder` serialization

---

# NautilusTrader 1.107.1 Beta - Release Notes

This is a patch release which applies various fixes and refactorings.

The behaviour of the `StopLimitOrder` continued to be fixed and refined.
`SimulatedExchange` was refactored further to reduce complexity.

### Breaking Changes
None

### Enhancements
None

### Fixes
- `TRIGGERED` states in order FSM
- `StopLimitOrder` triggering behaviour
- `OrderFactory.stop_limit` missing `post_only` and `hidden`
- `Order` and `StopLimitOrder` `__repr__` string (duplicate id)

---

# NautilusTrader 1.107.0 Beta

## Release Notes

The main thrust of this release is to refine some subtleties relating to order
matching and amendment behaviour for improved realism. This involved a fairly substantial refactoring
of `SimulatedExchange` to manage its complexity, and support extending the order types.

The `post_only` flag for LIMIT orders now results in the expected behaviour regarding
when a marketable limit order will become a liquidity `TAKER` during order placement
and amendment.

Test coverage was moderately increased.

### Breaking Changes
None

### Enhancements
- Refactored `SimulatedExchange` order matching and amendment logic
- Add `risk` subpackage to group risk components

### Fixes
- `StopLimitOrder` triggering behaviour
- All flake8 warnings

---

# NautilusTrader 1.106.0 Beta

## Release Notes

The main thrust of this release is to introduce the Interactive Brokers
integration, and begin adding platform capabilities to support this effort.

### Breaking Changes
- `from_serializable_string` methods changed to `from_serializable_str`

### Enhancements
- Scaffold Interactive Brokers integration in `adapters/ib`
- Add the `Future` instrument type
- Add the `StopLimitOrder` order type
- Add the `Data` and `DataType` types to support custom data handling
- Add the `InstrumentId` identifier types initial implementation to support extending the platforms capabilities

### Fixes
- `BracketOrder` correctness
- CCXT precision parsing bug
- Some log formatting

---
