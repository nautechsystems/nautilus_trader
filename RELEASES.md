# NautilusTrader 1.190.0 Beta

Released on 22nd March 2024 (UTC).

### Enhancements
- Added Databento adapter `continuous`, `parent` and `instrument_id` symbology support (will infer from symbols)
- Added `DatabaseConfig.timeout` config option for timeout seconds to wait for a new connection
- Added CSV tick and bar data loader params, thanks @rterbush
- Implemented `LogGuard` to ensure global logger is flushed on termination, thanks @ayush-sb and @twitu
- Improved Interactive Brokers client connectivity resilience and component lifecycle, thanks @benjaminsingleton
- Improved Binance execution client ping listen key error handling and logging
- Improved Redis cache adapter and message bus error handling and logging
- Improved Redis port parsing (`DatabaseConfig.port` can now be either a string or integer)
- Refactored `InteractiveBrokersEWrapper`, thanks @rsmb7z
- Redact Redis passwords in strings and logs
- Upgraded `redis` crate to 0.25.2 which bumps up TLS dependencies, and turned on `tls-rustls-webpki-roots` feature flag

### Breaking Changes
None

### Fixes
- Fixed JSON format for log file output (was missing `timestamp` and `trader\_id`)
- Fixed `DatabaseConfig` port JSON parsing for Redis (was always defaulting to 6379)
- Fixed `ChandeMomentumOscillator` indicator divide by zero error (both Rust and Cython versions)

---

# NautilusTrader 1.189.0 Beta

Released on 15th March 2024 (UTC).

### Enhancements
- Implemented Binance order book snapshot rebuilds on websocket reconnect (see integration guide)
- Added additional validations for `OrderMatchingEngine` (will now raise a `RuntimeError` when a price or size precision for `OrderFilled` does not match the instruments precisions)
- Added `LoggingConfig.use_pyo3` config option for pyo3 based logging initialization (worse performance but allows visibility into logs originating from Rust)
- Added `exchange` field to `FuturesContract`, `FuturesSpread`, `OptionsContract` and `OptionsSpread` (optional)

### Breaking Changes
- Changed Arrow schema adding `exchange` field for `FuturesContract`, `FuturesSpread`, `OptionsContract` and `OptionsSpread`

### Fixes
- Fixed `MessageBus` handling of subscriptions after a topic has been published on (was previously dropping messages for these late subscribers)
- Fixed `MessageBus` handling of subscriptions under certain edge cases (subscriptions list could be resized on iteration causing a `RuntimeError`)
- Fixed `Throttler` handling of sending messages after messages have been dropped, thanks @davidsblom
- Fixed `OrderBookDelta.to_pyo3_list` using zero precision from clear delta
- Fixed `DataTransformer.pyo3_order_book_deltas_to_record_batch_bytes` using zero precision from clear delta
- Fixed `OrderBookMbo` and `OrderBookMbp` integrity check when crossed book
- Fixed `OrderBookMbp` error when attempting to add to a L1\_MBP book type (now raises `RuntimeError` rather than panicking)
- Fixed Interactive Brokers connection error logging (#1524), thanks @benjaminsingleton
- Fixed `SimulationModuleConfig` location and missing re-export from `config` subpackage
- Fixed logging `StdoutWriter` from also writing error logs (writers were duplicating error logs)
- Fixed `BinanceWebSocketClient` to [new specification](https://binance-docs.github.io/apidocs/futures/en/#websocket-market-streams) which requires responding to pings with a pong containing the pings payload
- Fixed Binance Futures `AccountBalance` calculations based on wallet and available balance
- Fixed `ExecAlgorithm` circular import issue for installed wheels (importing from `execution.algorithm` was a circular import)

---

# NautilusTrader 1.188.0 Beta

Released on 25th February 2024 (UTC).

### Enhancements
- Added `FuturesSpread` instrument type
- Added `OptionsSpread` instrument type
- Added `InstrumentClass.FUTURE_SPREAD`
- Added `InstrumentClass.OPTION_SPREAD`
- Added `managed` parameter to `subscribe_order_book_deltas`, default true to retain current behavior (if false then the data engine will not automatically manage a book)
- Added `managed` parameter to `subscribe_order_book_snapshots`, default true to retain current behavior (if false then the data engine will not automatically manage a book)
- Added additional validations for `OrderMatchingEngine` (will now reject orders with incorrect price or quantity precisions)
- Removed `interval_ms` 20 millisecond limitation for `subscribe_order_book_snapshots` (i.e. just needs to be positive), although we recommend you consider subscribing to deltas below 100 milliseconds
- Ported `LiveClock` and `LiveTimer` implementations to Rust
- Implemented `OrderBookDeltas` pickling
- Implemented `AverageTrueRange` in Rust, thanks @rsmb7z

### Breaking Changes
- Changed `TradeId` value maximum length to 36 characters (will raise a `ValueError` if value exceeds the maximum)

### Fixes
- Fixed `TradeId` memory leak due assigning unique values to the `Ustr` global string cache (which are never freed for the lifetime of the program)
- Fixed `TradeTick` size precision for pyo3 conversion (size precision was incorrectly price precision)
- Fixed `RiskEngine` cash value check when selling (would previously divide quantity by price which is too much), thanks for reporting@AnthonyVince
- Fixed FOK time in force behavior (allows fills beyond the top level, will cancel if cannot fill full size)
- Fixed IOC time in force behavior (allows fills beyond the top level, will cancel any remaining after all fills are applied)
- Fixed `LiveClock` timer behavior for small intervals causing next time to be less than now (timer then would not run)
- Fixed log level filtering for `log_level_file` (bug introduced in v1.187.0), thanks @twitu
- Fixed logging `print_config` config option (was not being passed through to the logging system)
- Fixed logging timestamps for backtesting (static clock was not being incrementally set to individual `TimeEvent` timestamps)
- Fixed account balance updates (fills from zero quantity `NETTING` positions will generate account balance updates)
- Fixed `MessageBus` publishable types collection type (needed to be `tuple` not `set`)
- Fixed `Controller` registration of components to ensure all active clocks are iterated correctly during backtests
- Fixed `Equity` short selling for `CASH` accounts (will now reject)
- Fixed `ActorFactory.create` JSON encoding (was missing the encoding hook)
- Fixed `ImportableConfig.create` JSON encoding (was missing the encoding hook)
- Fixed `ImportableStrategyConfig.create` JSON encoding (was missing the encoding hook)
- Fixed `ExecAlgorithmFactory.create` JSON encoding (was missing the encoding hook)
- Fixed `ControllerConfig` base class and docstring
- Fixed Interactive Brokers historical bar data bug, thanks @benjaminsingleton
- Fixed persistence `freeze_dict` function to handle `fs_storage_options`, thanks @dimitar-petrov

---

# NautilusTrader 1.187.0 Beta

Released on 9th February 2024 (UTC).

### Enhancements
- Refined logging system module and writers in Rust, thanks @ayush-sb and @twitu
- Improved Interactive Brokers adapter symbology and parsing with a `strict_symbology` option, thanks @rsmb7z and @fhill2

### Breaking Changes
- Reorganized configuration objects (separated into a `config` module per subpackage, with re-exports from `nautilus_trader.config`)

### Fixes
- Fixed `BacktestEngine` and `Trader` disposal (now properly releasing resources), thanks for reporting @davidsblom
- Fixed circular import issues from configuration objects, thanks for reporting @cuberone
- Fixed unnecessary creation of log files when file logging off

---

# NautilusTrader 1.186.0 Beta

Released on 2nd February 2024 (UTC).

### Enhancements
None

### Breaking Changes
None

### Fixes
- Fixed Interactive Brokers get account positions bug (#1475), thanks @benjaminsingleton
- Fixed `TimeBarAggregator` handling of interval types on build
- Fixed `BinanceSpotExecutionClient` non-existent method name, thanks @sunlei
- Fixed unused `psutil` import, thanks @sunlei

---

# NautilusTrader 1.185.0 Beta

Released on 26th January 2024 (UTC).

### Enhancements
- Add warning log when `bypass_logging` is set true for a `LIVE` context
- Improved `register_serializable object` to also add type to internal `_EXTERNAL_PUBLIHSABLE_TYPES`
- Improved Interactive Brokers expiration contract parsing, thanks @fhill2

### Breaking Changes
- Changed `StreamingConfig.include_types` type from `tuple[str]` to `list[type]` (better alignment with other type filters)
- Consolidated `clock` module into `component` module (reduce binary wheel size)
- Consolidated `logging` module into `component` module (reduce binary wheel size)

### Fixes
- Fixed Arrow serialization of `OrderUpdated` (`trigger_price` type was incorrect), thanks @benjaminsingleton
- Fixed `StreamingConfig.include_types` behavior (was not being honored for instrument writers), thanks for reporting @doublier1
- Fixed `ImportableStrategyConfig` type assignment in `StrategyFactory` (#1470), thanks @rsmb7z

---

# NautilusTrader 1.184.0 Beta

Released on 22nd January 2024 (UTC).

### Enhancements
- Added `LogLevel.OFF` (matches the Rust `tracing` log levels)
- Added `init_logging` function with sensible defaults to initialize the Rust implemented logging system
- Updated Binance Futures enum members for `BinanceFuturesContractType` and `BinanceFuturesPositionUpdateReason`
- Improved log header using the `sysinfo` crate (adds swap space metrics and a PID identifier)
- Removed Python dependency on `psutil`

### Breaking Changes
- Removed `clock` parameter from `Logger` (no dependency on `Clock` anymore)
- Renamed `LoggerAdapter` to `Logger` (and removed old `Logger` class)
- Renamed `Logger` `component_name` parameter to `name` (matches Python built-in `logging` API)
- Renamed `OptionKind` `kind` parameter and property to `option_kind` (better clarity)
- Renamed `OptionsContract` Arrow schema field `kind` to `option_kind`
- Changed `level_file` log level to `OFF` (file logging is off by default)

### Fixes
- Fixed memory leak for catalog queries (#1430), thanks @twitu
- Fixed `DataEngine` order book snapshot timer names (could not parse instrument IDs with hyphens), thanks for reporting @x-zho14 and @dimitar-petrov
- Fixed `LoggingConfig` parsing of `WARNING` log level (was not being recognized), thanks for reporting @davidsblom
- Fixed Binance Futures `QuoteTick` parsing to capture event time for `ts_event`, thanks for reporting @x-zho14

---

# NautilusTrader 1.183.0 Beta

Released on 12th January 2024 (UTC).

### Enhancements
- Added `NautilusConfig.json_primitives` to convert object to Python dictionary with JSON primitive values
- Added `InstrumentClass.BOND`
- Added `MessageBusConfig` `use_trader_prefix` and `use_trader_id` options (provides more control over stream names)
- Added `CacheConfig.drop_instruments_on_reset` (default true to retain current behavior)
- Implemented core logging interface via the `log` crate, thanks @twitu
- Implemented global atomic clock in Rust (improves performance and ensures properly monotonic timestamps in real-time), thanks @twitu
- Improved Interactive Brokers adapter raising docker `RuntimeError` only when needed (not when using TWS), thanks @rsmb7z
- Upgraded core HTTP client to latest `hyper` and `reqwest`, thanks @ayush-sb
- Optimized Arrow encoding (resulting in ~100x faster writes for the Parquet data catalog)

### Breaking Changes
- Changed `ParquetDataCatalog` custom data prefix from `geneticdata_` to `custom_` (you will need to rename any catalog subdirs)
- Changed `ComponentStateChanged` Arrow schema for `config` from `string` to `binary`
- Changed `OrderInitialized` Arrow schema for `options` from `string` to `binary`
- Changed `OrderBookDeltas` dictionary representation of `deltas` field from JSON `bytes` to a list of `dict` (standardize with all other data types)
- Changed external message publishing stream name keys to be `trader-{trader_id}-{instance_id}-streams` (with options allows many traders to publish to the same streams)
- Renamed all version 2 data wrangler classes with a `V2` suffix for clarity
- Renamed `GenericData` to `CustomData` (more accurately reflects the nature of the type)
- Renamed `DataClient.subscribed_generic_data` to `.subscribed_custom_data`
- Renamed `MessageBusConfig.stream` to `.streams_prefix` (more accurate)
- Renamed `ParquetDataCatalog.generic_data` to `.custom_data`
- Renamed `TradeReport` to `FillReport` (more conventional terminology, and more clearly separates market data from user execution reports)
- Renamed `asset_type` to `instrument_class` across the codebase (more conventional terminology)
- Renamed `AssetType` enum to `InstrumentClass` (more conventional terminology)
- Renamed `AssetClass.BOND` to `AssetClass.DEBT` (more conventional terminology)
- Removed `AssetClass.METAL` (not strictly an asset class, more a futures category)
- Removed `AssetClass.ENERGY` (not strictly an asset class, more a futures category)
- Removed `multiplier` param from `Equity` constructor (not applicable)
- Removed `size_precision`, `size_increment`, and `multiplier` fields from `Equity` dictionary representation (not applicable)
- Removed `TracingConfig` (now redundant with new logging implementation)
- Removed `Ticker` data type and associated methods (not a type which can be practically normalized and so becomes adapter specific generic data)
- Moved `AssetClass.SPORTS_BETTING` to `InstrumentClass.SPORTS_BETTING`

### Fixes
- Fixed logger thread leak, thanks @twitu
- Fixed handling of configuration objects to work with `StreamingFeatherWriter`
- Fixed `BinanceSpotInstrumentProvider` fee loading key error for partial instruments load, thanks for reporting @doublier1
- Fixed Binance API key configuration parsing for testnet (was falling through to non-testnet env vars)
- Fixed TWAP execution algorithm scheduled size handling when first order should be for the entire size, thanks for reporting @pcgm-team
- Added `BinanceErrorCode.SERVER_BUSY` (-1008), also added to the retry error codes
- Added `BinanceOrderStatus.EXPIRED_IN_MATCH` which is when an order was canceled by the exchange due self-trade prevention (STP), thanks for reporting @doublier1

---

# NautilusTrader 1.182.0 Beta

Released on 23rd December 2023 (UTC).

### Enhancements
- Added `CacheDatabaseFacade` and `CacheDatabaseAdapter` to abstract backing technology from Python codebase
- Added `RedisCacheDatabase` implemented in Rust with separate MPSC channel thread for insert, update and delete operations
- Added TA-Lib integration, thanks @rsmb7z
- Added `OrderBookDelta` and `OrderBookDeltas` to serializable and publishable types
- Moved `PortfolioFacade` to `Actor`
- Improved `Actor` and `Strategy` usability to be more lenient to mistaken calls to `clock` and `logger` from the constructor (warnings also added to docs)
- Removed `redis` and `hiredis` dependencies from Python codebase

### Breaking Changes
- Changed configuration objects to take stronger types as these are now serializable when registered (rather than primitives)
- Changed `NautilusKernelConfig.trader_id` to type `TraderId`
- Changed `BacktestDataConfig.instrument_id` to type `InstrumentId`
- Changed `ActorConfig.component_id` to type `ComponentId | None`
- Changed `StrategyConfig.strategy_id` to type `StrategyId | None`
- Changed `Instrument`, `OrderFilled` and `AccountState` `info` field serialization due below fix (you'll need to flush your cache)
- Changed `CacheConfig` to take a `DatabaseConfig` (better symmetry with `MessageBusConfig`)
- Changed `RedisCacheDatabase` data structure for currencies from hashset to simpler key-value (you'll need to clear cache or delete all curreny keys)
- Changed `Actor` state loading to now use the standard `Serializer`
- Renamed `register_json_encoding` to `register_config_encoding`
- Renamed `register_json_decoding` to `register_config_decoding`
- Removed `CacheDatabaseConfig` (due above config change)
- Removed `infrastructure` subpackage (now redundant with new Rust implementation)

### Fixes
- Fixed `json` encoding for `CacheDatabaseAdapter` from `info` field serialization fix below
- Fixed `Instrument`, `OrderFilled` and `AccountState` `info` field serialization to retain JSON serializable dicts (rather than double encoding and losing information)
- Fixed Binance Futures `good_till_date` value when `time_in_force` not GTD, such as when strategy is managing the GTD (was incorrectly passing through UNIX milliseconds)
- Fixed `Executor` handling of queued task IDs (was not discarding from queued tasks on completion)
- Fixed `DataEngine` handling of order book snapshots with very small intervals (now handles as short as 20 milliseconds)
- Fixed `BacktestEngine.clear_actors()`, `BacktestEngine.clear_strategies()` and `BacktestEngine.clear_exec_algorithms()`, thanks for reporting @davidsblom
- Fixed `BacktestEngine` OrderEmulator reset, thanks @davidsblom
- Fixed `Throttler.reset` and reset of `RiskEngine` throttlers, thanks @davidsblom

---

# NautilusTrader 1.181.0 Beta

Released on 2nd December (UTC).

This release adds support for Python 3.12.

### Enhancements
- Rewrote Interactive Brokers integration documentation, many thanks @benjaminsingleton
- Added Interactive Brokers adapter support for crypto instruments with cash quantity, thanks @benjaminsingleton
- Added `HistoricInteractiveBrokerClient`, thanks @benjaminsingleton and @limx0
- Added `DataEngineConfig.time_bars_interval_type` (determines the type of interval used for time aggregation `left-open` or `right-open`)
- Added `LoggingConfig.log_colors` to optionally use ANSI codes to produce colored logs (default true to retain current behavior)
- Added `QuoteTickDataWrangler.process_bar_data` options for `offset_interval_ms` and `timestamp_is_close`
- Added identifier generators in Rust, thanks @filipmacek
- Added `OrderFactory` in Rust, thanks @filipmacek
- Added `WilderMovingAverage` in Rust, thanks @ayush-sb
- Added `HullMovingAverage` in Rust, thanks @ayush-sb
- Added all common identifier generators in Rust, thanks @filipmacek
- Added generic SQL database support with `sqlx` in Rust, thanks @filipmacek

### Breaking Changes
- Consolidated all `data` submodules into one `data` module (reduce binary wheel size)
- Moved `OrderBook` from `model.orderbook.book` to `model.book` (subpackage only had this single module)
- Moved `Currency` from `model.currency` to `model.objects` (consolidating modules to reduce binary wheel size)
- Moved `MessageBus` from `common.msgbus` to `common.component` (consolidating modules to reduce binary wheel size)
- Moved `MsgSpecSerializer` from `serialization.msgpack.serializer` to `serialization.serializer`
- Moved `CacheConfig` `snapshot_orders`, `snapshot_positions`, `snapshot_positions_interval` to `NautilusKernelConfig` (logical applicability)
- Renamed `MsgPackSerializer` to `MsgSpecSeralizer` (now handles both JSON and MsgPack formats)

### Fixes
- Fixed missing `trader_id` in `Position` dictionary representation, thanks @filipmacek
- Fixed conversion of fixed precision integers to floats (should be dividing to avoid rounding errors), thanks for reporting @filipmacek
- Fixed daily timestamp parsing for Interactive Brokers, thanks @benjaminsingleton
- Fixed live reconciliation trade processing for partially filled then canceled orders
- Fixed `RiskEngine` cumulative notional risk check for `CurrencyPair` SELL orders on multi-currency cash accounts

---

# NautilusTrader 1.180.0 Beta

Released on 3rd November 2023 (UTC).

### Enhancements
- Improved internal latency for live engines by using `loop.call_soon_threadsafe(...)`
- Improved `RedisCacheDatabase` client connection error handling with retries
- Added `WebSocketClient` connection headers, thanks @ruthvik125 and @twitu
- Added `support_contingent_orders` option for venues (to simulate venues which do not support contingent orders)
- Added `StrategyConfig.manage_contingent_orders` option (to automatically manage **open** contingent orders)
- Added `FuturesContract.activation_utc` property which returns a `pd.Timestamp` tz-aware (UTC)
- Added `OptionsContract.activation_utc` property which returns a `pd.Timestamp` tz-aware (UTC)
- Added `CryptoFuture.activation_utc` property which returns a `pd.Timestamp` tz-aware (UTC)
- Added `FuturesContract.expiration_utc` property which returns a `pd.Timestamp` tz-aware (UTC)
- Added `OptionsContract.expiration_utc` property which returns a `pd.Timestamp` tz-aware (UTC)
- Added `CryptoFuture.expiration_utc` property which returns a `pd.Timestamp` tz-aware (UTC)

### Breaking Changes
- Renamed `FuturesContract.expiry_date` to `expiration_ns` (and associated params) as `uint64_t` UNIX nanoseconds
- Renamed `OptionsContract.expiry_date` to `expiration_ns` (and associated params) as `uint64_t` UNIX nanoseconds
- Renamed `CryptoFuture.expiry_date` to `expiration_ns` (and associated params) as `uint64_t` UNIX nanoseconds
- Changed `FuturesContract` Arrow schema
- Changed `OptionsContract` Arrow schema
- Changed `CryptoFuture` Arrow schema
- Transformed orders will now retain the original `ts_init` timestamp
- Removed unimplemented `batch_more` option for `Strategy.modify_order`
- Removed `InstrumentProvider.venue` property (redundant as a provider may have many venues)
- Dropped support for Python 3.9

### Fixes
- Fixed `ParquetDataCatalog` file writing template, thanks @limx0
- Fixed `Binance` all orders requests which would omit order reports when using a `start` param
- Fixed managed GTD orders past expiry cancellation on restart (orders were not being canceled)
- Fixed managed GTD orders cancel timer on order cancel (timers were not being canceled)
- Fixed `BacktestEngine` logging error with immediate stop (caused by certain timestamps being `None`)
- Fixed `BacktestNode` exceptions during backtest runs preventing next sequential run, thanks for reporting @cavan-black
- Fixed `BinanceSpotPersmission` value error by relaxing typing for `BinanceSpotSymbolInfo.permissions`
- Interactive Brokers adapter various fixes, thanks @rsmb7z

---

# NautilusTrader 1.179.0 Beta

Released on 22nd October 2023 (UTC).

A major feature of this release is the `ParquetDataCatalog` version 2, which represents months of
collective effort thanks to contributions from Brad @limx0, @twitu, @ghill2 and @davidsblom.

This will be the final release with support for Python 3.9.

### Enhancements
- Added `ParquetDataCatalog` v2 supporting built-in data types `OrderBookDelta`, `QuoteTick`, `TradeTick` and `Bar`
- Added `Strategy` specific order and position event handlers
- Added `ExecAlgorithm` specific order and position event handlers
- Added `Cache.is_order_pending_cancel_local(...)` (tracks local orders in cancel transition)
- Added `BinanceTimeInForce.GTD` enum member (futures only)
- Added Binance Futures support for GTD orders
- Added Binance internal bar aggregation inference from aggregated trade ticks or 1-MINUTE bars (depending on lookback window)
- Added `BinanceExecClientConfig.use_gtd` option (to remap to GTC and locally manage GTD orders)
- Added package version check for `nautilus_ibapi`, thanks @rsmb7z
- Added `RiskEngine` min/max instrument notional limit checks
- Added `Controller` for dynamically controlling actor and strategy instances for a `Trader`
- Added `ReportProvider.generate_fills_report(...)` which provides a row per individual fill event, thanks @r3k4mn14r
- Moved indicator registration and data handling down to `Actor` (now available for `Actor`)
- Implemented Binance `WebSocketClient` live subscribe and unsubscribe
- Implemented `BinanceCommonDataClient` retries for `update_instruments`
- Decythonized `Trader`

### Breaking Changes
- Renamed `BookType.L1_TBBO` to `BookType.L1_MBP` (more accurate definition, as L1 is the top-level price either side)
- Renamed `VenueStatusUpdate` -> `VenueStatus`
- Renamed `InstrumentStatusUpdate` -> `InstrumentStatus`
- Renamed `Actor.subscribe_venue_status_updates(...)` to `Actor.subscribe_venue_status(...)`
- Renamed `Actor.subscribe_instrument_status_updates(...)` to `Actor.subscribe_instrument_status(...)`
- Renamed `Actor.unsubscribe_venue_status_updates(...)` to `Actor.unsubscribe_venue_status(...)`
- Renamed `Actor.unsubscribe_instrument_status_updates(...)` to `Actor.unsubscribe_instrument_status(...)`
- Renamed `Actor.on_venue_status_update(...)` to `Actor.on_venue_status(...)`
- Renamed `Actor.on_instrument_status_update(...)` to `Actor.on_instrument_status(...)`
- Changed `InstrumentStatus` fields/schema and constructor
- Moved `manage_gtd_expiry` from `Strategy.submit_order(...)` and `Strategy.submit_order_list(...)` to `StrategyConfig` (simpler and allows re-activiting any GTD timers on start)

### Fixes
- Fixed `LimitIfTouchedOrder.create` (`exec_algorithm_params` were not being passed in)
- Fixed `OrderEmulator` start-up processing of OTO contingent orders (when position from parent is open)
- Fixed `SandboxExecutionClientConfig` `kw_only=True` to allow importing without initializing
- Fixed `OrderBook` pickling (did not include all attributes), thanks @limx0
- Fixed open position snapshots race condition (added `open_only` flag)
- Fixed `Strategy.cancel_order` for orders in `INITIALIZED` state and with an `emulation_trigger` (was not sending command to `OrderEmulator`)
- Fixed `BinanceWebSocketClient` reconnect behavior (reconnect handler was not being called due event loop issue from Rust)
- Fixed Binance instruments missing max notional values, thanks for reporting @AnthonyVince and thanks for fixing @filipmacek
- Fixed Binance Futures fee rates for backtesting
- Fixed `Timer` missing condition check for non-positive intervals
- Fixed `Condition` checks involving integers, was previously defaulting to 32-bit and overflowing
- Fixed `ReportProvider.generate_order_fills_report(...)` which was missing partial fills for orders not in a final `FILLED` status, thanks @r3k4mn14r

---

# NautilusTrader 1.178.0 Beta

Released on 2nd September 2023 (UTC).

### Enhancements
None

### Breaking Changes
None

### Fixes
- Fixed `OrderBookDelta.clear` method (where the `sequence` field was swapped with `flags` causing an overflow)
- Fixed `OrderManager` OTO contingency handling on fills
- Fixed `OrderManager` duplicate order canceled events (race condition when processing contingencies)
- Fixed `Cache` loading of initialized emulated orders (were not being correctly indexed as emulated)
- Fixed Binance order book subscriptions for deltas at full depth (was not requesting initial snapshot), thanks for reporting @doublier1

---

# NautilusTrader 1.177.0 Beta

Released on 26th August 2023 (UTC).

This release includes a large breaking change to quote tick bid and ask price property and 
parameter naming. This was done in the interest of maintaining our generally explicit naming 
standards, and has caused confusion for some users in the past. Data using 'bid' and 'ask' columns should
still work with the legacy data wranglers, as columns are renamed under the hood to accommodate
this change.

### Enhancements
- Added `ActorExecutor` with `Actor` API for creating and running threaded tasks in live environments
- Added `OrderEmulated` event and associated `OrderStatus.EMULATED` enum variant
- Added `OrderReleased` event and associated `OrderStatus.RELEASED` enum variant
- Added `BacktestVenueConfig.use_position_ids` option (default true to retain current behavior)
- Added `Cache.exec_spawn_total_quantity(...)` convenience method
- Added `Cache.exec_spawn_total_filled_qty(...)` convenience method
- Added `Cache.exec_spawn_total_leaves_qty(...)` convenience method
- Added `WebSocketClient.send_text`, thanks @twitu
- Implemented string interning for `TimeEvent`

### Breaking Changes
- Renamed `QuoteTick.bid` to `bid_price` including all associated parameters (for explicit naming standards)
- Renamed `QuoteTick.ask` to `ask_price` including all associated parameters (for explicit naming standards)

### Fixes
- Fixed execution algorithm `position_id` assignment in `HEDGING` mode
- Fixed `OrderMatchingEngine` processing of emulated orders
- Fixed `OrderEmulator` processing of exec algorithm orders
- Fixed `ExecutionEngine` processing of exec algorithm orders (exec spawn IDs)
- Fixed `Cache` emulated order indexing (were not being properly discarded from the set when closed)
- Fixed `RedisCacheDatabase` loading of transformed `LIMIT` orders
- Fixed a connection issue with the IB client, thanks @dkharrat and @rsmb7z

---

# NautilusTrader 1.176.0 Beta

Released on 31st July 2023 (UTC).

### Enhancements
- Implemented string interning with the [ustr](https://github.com/anderslanglands/ustr) crate, thanks @twitu
- Added `SyntheticInstrument` capability, including dynamic derivation formulas
- Added `Order.commissions()` convenience method (also added to state snapshot dictionaries)
- Added `Cache` position and order state snapshots (configure via `CacheConfig`)
- Added `CacheDatabaseConfig.timestamps_as_iso8601` to persist timestamps as ISO 8601 strings
- Added `LiveExecEngineConfig.filter_position_reports` to filter position reports from reconciliation
- Added `Strategy.cancel_gtd_expiry` to cancel managed GTD order expiration
- Added Binance Futures support for modifying `LIMIT` orders
- Added `BinanceExecClientConfig.max_retries` option (for retrying order submit and cancel requests)
- Added `BinanceExecClientConfig.retry_delay` option (the delay between retry attempts)
- Added `BinanceExecClientConfig.use_reduce_only` option (default true to retain current behavior)
- Added `BinanceExecClientConfig.use_position_ids` option (default true to retain current behavior)
- Added `BinanceExecClientConfig.treat_expired_as_canceled` option (default false to retain current behavior)
- Added `BacktestVenueConfig.use_reduce_only` option (default true to retain current behavior)
- Added `MessageBus.is_pending_request(...)` method
- Added `Level` API for core `OrderBook` (exposes the bid and ask levels for the order book)
- Added `Actor.is_pending_request(...)` convenience method
- Added `Actor.has_pending_requests()` convenience method
- Added `Actor.pending_requests()` convenience method
- Added `USDP` (Pax Dollar) and `TUSD` (TrueUSD) stablecoins
- Improved `OrderMatchingEngine` handling when no fills (an error is now logged)
- Improved `Binance` live clients logging
- Upgraded Cython to 3.0.0 stable

### Breaking Changes
- Moved `filter_unclaimed_external_orders` from `ExecEngineConfig` to `LiveExecEngineConfig`
- All `Actor.request_*` methods no longer take a `request_id`, but now return a `UUID4` request ID
- Removed `BinanceExecClientConfig.warn_gtd_to_gtd` (now always an `INFO` level log)
- Renamed `Instrument.native_symbol` to `raw_symbol` (you must manually migrate or flush your cached instruments)
- Renamed `Position.cost_currency` to `settlement_currency` (standardize terminology)
- Renamed `CacheDatabaseConfig.flush` to `flush_on_start` (for clarity)
- Changed `Order.ts_last` to represent the UNIX nanoseconds timestamp of the last _event_ (rather than fill)

### Fixes
- Fixed `Portfolio.net_position` calculation to use `Decimal` rather than `float` to avoid rounding errors
- Fixed race condition on `OrderFactory` order identifiers generation
- Fixed dictionary representation of orders for `venue_order_id` (for three order types)
- Fixed `Currency` registration with core global map on creation
- Fixed serialization of `OrderInitialized.exec_algorithm_params` to spec (bytes rather than string)
- Fixed assignment of position IDs for contingent orders (when parent filled)
- Fixed `PENDING_CANCEL` -> `EXPIRED` as valid state transition (real world possibility)
- Fixed fill handling of `reduce_only` orders when partially filled
- Fixed Binance reconciliation which was requesting reports for the same symbol multiple times
- Fixed Binance Futures native symbol parsing (was actually Nautilus symbol values)
- Fixed Binance Futures `PositionStatusReport` parsing of position side
- Fixed Binance Futures `TradeReport` assignment of position ID (was hardcoded to hedging mode)
- Fixed Binance execution submitting of order lists
- Fixed Binance commission rates requests for `InstrumentProvider`
- Fixed Binance `TriggerType` parsing #1154, thanks for reporting @davidblom603
- Fixed Binance order parsing of invalid orders in execution reports #1157, thanks for reporting @graceyangfan
- Extended `BinanceOrderType` enum members to include undocumented `INSURANCE_FUND`, thanks for reporting @Tzumx
- Extended `BinanceSpotPermissions` enum members #1161, thanks for reporting @davidblom603

---

# NautilusTrader 1.175.0 Beta

Released on 16th June 2023 (UTC).

The Betfair adapter is broken for this release pending integration with the new Rust order book.
We recommend you do not upgrade to this version if you're using the Betfair adapter.

### Enhancements
- Integrated Interactive Brokers adapter v2 into platform, thanks @rsmb7z
- Integrated core Rust `OrderBook` into platform
- Integrated core Rust `OrderBookDelta` data type
- Added core Rust `HttpClient` based on `hyper`, thanks @twitu
- Added core Rust `WebSocketClient` based on `tokio-tungstenite`, thanks @twitu
- Added core Rust `SocketClient` based on `tokio` `TcpStream`, thanks @twitu
- Added `quote_quantity` parameter to determine if order quantity is denominated in quote currency
- Added `trigger_instrument_id` parameter to trigger emulated orders from alternative instrument prices
- Added `use_random_ids` to `add_venue(...)` method, controls whether venue order, position and trade IDs will be random UUID4s (no change to current behavior)
- Added `ExecEngineConfig.filter_unclaimed_external_orders` options, if unclaimed order events with an `EXTERNAL` strategy ID should be filtered/dropped
- Changed `BinanceHttpClient` to use new core HTTP client
- Defined public API for data, can now import directly from `nautilus_trader.model.data` (denest namespace)
- Defined public API for events, can now import directly from `nautilus_trader.model.events` (denest namespace)

### Breaking Changes
- Upgraded `pandas` to v2
- Removed `OrderBookSnapshot` (redundant as can be represented as an initial CLEAR followed by deltas)
- Removed `OrderBookData` (redundant)
- Renamed `Actor.handle_order_book_delta` to `handle_order_book_deltas` (to more clearly reflect the `OrderBookDeltas` data type)
- Renamed `Actor.on_order_book_delta` to `on_order_book_deltas` (to more clearly reflect the `OrderBookDeltas` data type)
- Renamed `inverse_as_quote` to `use_quote_for_inverse` (ambiguous name, only applicable for notional calcs on inverse instruments)
- Changed `Data` contract (custom data), [see docs](https://docs.nautilustrader.io/develop/concepts/advanced/data.html)
- Renamed core `LogMessage` to `LogEvent` to more clearly distinguish between the `message` field and the event struct itself (aligns with [vector](https://vector.dev/docs/about/under-the-hood/architecture/data-model/log/) language)
- Renamed core `LogEvent.timestamp_ns` to `LogEvent.timestamp` (affects field name for JSON format)
- Renamed core `LogEvent.msg` to `LogEvent.message` (affects field name for JSON format)

### Fixes
- Updated `BinanceAccountType` enum members and associated docs
- Fixed `BinanceCommonExecutionClient` iteration of `OrderList` orders
- Fixed heartbeats for `BinanceWebSocketClient` (new Rust client now responds with `pong` frames)
- Fixed Binance adapter typing for `orderId`, `fromId`, `startTime` and `endTime` (all are ints), thanks for reporting @davidsblom
- Fixed `Currency` equality to be based on the `code` field (avoiding equality issues over FFI), thanks for reporting @Otlk
- Fixed `BinanceInstrumentProvider` parsing of initial and maintenance margin values

---

# NautilusTrader 1.174.0 Beta

Released on 19th May 2023 (UTC).

### Breaking Changes
- Parquet schemas are now shifting towards catalog v2 (we recommend you don't upgrade if using legacy catalog)
- Moved order book data from `model.orderbook.data` into the `model.data.book` namespace

### Enhancements
- Improved handling for backtest account blow-up scenarios (balance negative or margin exceeded)
- Added `AccountMarginExceeded` exception and refined `AccountBalanceNegative`
- Various improvements to `Binance` clients error handling and logging
- Improve Binance HTTP error messages

### Fixes
- Fixed handling of emulated order contingencies (not based on status of spawned algorithm orders)
- Fixed sending execution algorithm commands from strategy
- Fixed `OrderEmulator` releasing of already closed orders
- Fixed `MatchingEngine` processing of reduce only for child contingent orders
- Fixed `MatchingEngine` position ID assignment for child contingent orders
- Fixed `Actor` handling of historical data from requests (will now call `on_historical_data` regardless of state), thanks for reporting @miller-moore
- Fixed `pyarrow` schema dictionary index keys being too narrow (int8 -> int16), thanks for reporting @rterbush

---

# NautilusTrader 1.173.0 Beta

Released on 5th May 2023 (UTC).

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed `BacktestEngine` processing of venue(s) message queue based off time event `ts_init`
- Fixed `Position.signed_decimal_qty` (incorrect format precision in f-string), thanks for reporting @rsmb7z
- Fixed trailing stop type order updates for `reduce_only` instruction, thanks for reporting @Otlk
- Fixed updating of active execution algorithm orders (events weren't being cached)
- Fixed condition check for applying pending events (do not apply to orders at `INITIALIZED` status)

---

# NautilusTrader 1.172.0 Beta

Released on 30th April 2023 (UTC).

### Breaking Changes
- Removed legacy Rust parquet data catalog backend (based on arrow2)
- Removed Binance config for `clock_sync_interval_secs` (redundant/unused and should be handled at system level)
- Removed redundant rate limiting from Rust logger (and associated `rate_limit` config params)
- Renamed `Future` instrument to `FuturesContract` (avoids ambiguity)
- Renamed `Option` instrument to `OptionsContract` (avoids ambiguity and naming conflicts in Rust)
- Reinstate hours and minutes time component for default order and position identifiers (easier debugging, less collisions)
- Setting time alerts for in the past or current time will generate an immediate `TimeEvent` (rather than being invalid)

### Enhancements
- Added new DataFusion Rust parquet data catalog backend (yet to be integrated into Python)
- Added `external_order_claims` config option for `StrategyConfig` (for claiming external orders per instrument)
- Added `Order.signed_decimal_qty()`
- Added `Cache.orders_for_exec_algorithm(...)`
- Added `Cache.orders_for_exec_spawn(...)`
- Added `TWAPExecAlgorithm` and `TWAPExecAlgorithmConfig` to examples
- Build out `ExecAlgorithm` base class for implementing 'first class' execution algorithms
- Rewired execution for improved flow flexibility between emulated orders, execution algorithms and the `RiskEngine`
- Improved handling for `OrderEmulator` updating of contingent orders from execution algorithms
- Defined public API for instruments, can now import directly from `nautilus_trader.model.instruments` (denest namespace)
- Defined public API for orders, can now import directly from `nautilus_trader.model.orders` (denest namespace)
- Defined public API for order book, can now import directly from `nautilus_trader.model.orderbook` (denest namespace)
- Now stripping debug symbols after build (reduced binary wheel size)
- Refined build and added additional `debug` Makefile convenience targets

### Fixes
- Fixed processing of contingent orders when in a pending update state
- Fixed calculation of PnL for flipped positions (only book realized PnL against open position)
- Fixed `WebSocketClient` session disconnect, thanks for reporting @miller-moore
- Added missing `BinanceSymbolFilterType.NOTIONAL`
- Fixed incorrect `Mul` trait for `Price` and `Quantity` (not being used in Cython/Python layer)

---

# NautilusTrader 1.171.0 Beta

Released on 30th March 2023 (UTC).

### Breaking Changes
- Renamed all position `net_qty` fields and parameters to `signed_qty` (more accurate naming)
- `NautilusKernelConfig` removed all `log_*` config options (replaced by `logging` with `LoggingConfig`)
- Trading `CurrencyPair` instruments with a _single-currency_ `CASH` account type no longer permitted (unrealistic)
- Changed `PositionEvent` parquet schemas (renamed `net_qty` field to `signed_qty`)

### Enhancements
- Added `LoggingConfig` to consolidate logging configs, offering various file options and per component level filters
- Added `BacktestVenueConfig.bar_execution` to control whether bar data moves the matching engine markets (reinstated)
- Added optional `request_id` for actor data requests (aids processing responses), thanks @rsmb7z
- Added `Position.signed_decimal_qty()`
- Now using above signed quantity for `Portfolio` net position calculation, and `LiveExecutionEngine` reconciliation comparisons

### Fixes
- Fixed `BacktestEngine` clock and logger handling (had a redundant extra logger and not swapping live clock in post run)
- Fixed `close_position` order event publishing and cache persistence for `MarketOrder` and `SubmitOrder`, thanks for reporting @rsmb7z

---

# NautilusTrader 1.170.0 Beta

Released on 11th March 2023 (UTC).

### Breaking Changes
- Moved `backtest.data.providers` to `test_kit.providers`
- Moved `backtest.data.wranglers` to `persistence.wranglers` (to be consolidated)
- Moved `backtest.data.loaders` to `persistence.loaders` (to be consolidated)
- Renamed `from_datetime` to `start` across data request methods and properties
- Renamed `to_datetime` to `end` across data request methods and properties
- Removed `RiskEngineConfig.deny_modify_pending_update` (as now redundant with new pending event sequencing)
- Removed redundant log sink machinery
- Changed parquet catalog schema dictionary integer key widths/types
- Invalidated all pickled data due to Cython 3.0.0b1 upgrade

### Enhancements
- Added logging to file at core Rust level
- Added `DataCatalogConfig` for more cohesive data catalog configuration
- Added `DataEngine.register_catalog` to support historical data requests
- Added `catalog_config` field to base `NautilusKernelConfig`
- Changed to immediately caching orders and order lists in `Strategy`
- Changed to checking duplicate `client_order_id` and `order_list_id` in `Strategy`
- Changed generating and applying `OrderPendingUpdate` and `OrderPendingCancel` in `Strategy`
- `PortfolioAnalyzer` PnL statistics now take optional `unrealized_pnl`
- Backtest performance statistics now include unrealized PnL in total PnL

### Fixes
- Fixed Binance Futures trigger type parsing
- Fixed `DataEngine` bar subscribe and unsubscribe logic, thanks for reporting @rsmb7z
- Fixed `Actor` handling of bars, thanks @limx0
- Fixed `CancelAllOrders` command handling for contingent orders not yet in matching core
- Fixed `TrailingStopMarketOrder` slippage calculation when no `trigger_price`, thanks for reporting @rsmb7z
- Fixed `BinanceSpotInstrumentProvider` parsing of quote asset (was using base), thanks for reporting @logogin
- Fixed undocumented Binance time in force 'GTE\_GTC', thanks for reporting @graceyangfan
- Fixed `Position` calculation of `last_qty` when commission currency was equal to base currency, thanks for reporting @rsmb7z
- Fixed `BacktestEngine` post backtest run PnL performance statistics for currencies traded per venue, thanks for reporting @rsmb7z

---

# NautilusTrader 1.169.0 Beta

Released on 18th February 2023 (UTC).

### Breaking Changes
- `NautilusConfig` objects now _pseudo-immutable_ from new msgspec 0.13.0
- Renamed `OrderFactory.bracket` param `post_only_entry` -> `entry_post_only` (consistency with other params)
- Renamed `OrderFactory.bracket` param `post_only_tp` -> `tp_post_only` (consistency with other params)
- Renamed `build_time_bars_with_no_updates` -> `time_bars_build_with_no_updates` (consistency with new param) 
- Renamed `OrderFactory.set_order_count()` -> `set_client_order_id_count()` (clarity)
- Renamed `TradingNode.start()` to `TradingNode.run()`

### Enhancements
- Complete overhaul and improvements to Binance adapter(s), thanks @poshcoe
- Added Binance aggregated trades functionality with `use_agg_trade_ticks`, thanks @poshcoe
- Added `time_bars_timestamp_on_close` option for configurable bar timestamping (true by default)
- Added `OrderFactory.generate_client_order_id()` (calls internal generator)
- Added `OrderFactory.generate_order_list_id()` (calls internal generator)
- Added `OrderFactory.create_list(...)` as easier method for creating order lists
- Added `__len__` implementation for `OrderList` (returns length of orders)
- Implemented optimized logger using Rust MPSC channel and separate thread
- Expose and improve `MatchingEngine` public API for custom functionality
- Exposed `TradingNode.run_async()` for easier running from async context
- Exposed `TradingNode.stop_async()` for easier stopping from async context

### Fixes
- Fixed registration of `SimulationModule` (and refine `Actor` base registration)
- Fixed loading of previously emulated and transformed orders (handles transforming `OrderInitialized` event)
- Fixed handling of `MARKET_TO_LIMIT` orders in matching and risk engines, thanks for reporting @martinsaip

---

# NautilusTrader 1.168.0 Beta

Released on 29th January 2023 (UTC).

### Breaking Changes
- Removed `Cache.clear_cache()` (redundant with the `.reset()` method)

### Enhancements
- Added `Cache` `.add(...)` and `.get(...)` for general 'user/custom' objects (as bytes)
- Added `CacheDatabase` `.add(...)` and `.load()` for general cache objects (as bytes)
- Added `RedisCacheDatabase` `.add(...) `and `.load()` for general Redis persisted bytes objects (as bytes)
- Added `Cache.actor_ids()`
- Added `Actor` cached state saving and loading functionality
- Improved logging for called action handlers when not overridden

### Fixes
- Fixed configuration of loading and saving actor and strategy state

---

# NautilusTrader 1.167.0 Beta

Released on 28th January 2023 (UTC).

### Breaking Changes
- Renamed `OrderBookData.update_id` to `sequence`
- Renamed `BookOrder.id` to `order_id`

### Enhancements
- Introduced Rust pyo3 based `ParquetReader` and `ParquetWriter`, thanks @twitu
- Added `msgbus.is_subscribed` (to check if topic and handler already subscribed)
- Simplified message type model and introduce CQRS-ish live messaging architecture

### Fixes
- Fixed Binance data clients order book startup buffer handling
- Fixed `NautilusKernel` redundant initialization of event loop for backtesting, thanks @limx0
- Fixed `BacktestNode` disposal sequence
- Fixed quick start docs and notebook

---

# NautilusTrader 1.166.0 Beta

Released on 17th January 2023 (UTC).

### Breaking Changes
- `Position.unrealized_pnl` now `None` until any realized PnL is generated (to reduce ambiguity)

### Enhancements
- Added instrument status update subscription handlers, thanks @limx0
- Improvements to InteractiveBrokers `DataClient`, thanks @rsmb7z
- Improvements to async task handling for live clients
- Various improvements to Betfair adapter, thanks @limx0

### Fixes
- Fixed netted `Position` `realized_pnl` and `realized_return` fields, which were incorrectly cumulative
- Fixed netted `Position` flip logic (now correctly 'resets' position)
- Various fixes for Betfair adapter, thanks @limx0
- InteractiveBrokers integration docs fixes

---

# NautilusTrader 1.165.0 Beta

Released on 14th January 2023 (UTC).

A number of enum variant names have been changed in favour of explicitness, 
and also to avoid C naming collisions.

### Breaking Changes
- Renamed `AggressorSide.NONE` to `NO_AGGRESSOR`
- Renamed `AggressorSide.BUY` to `BUYER`
- Renamed `AggressorSide.SELL` to `SELLER`
- Renamed `AssetClass.CRYPTO` to `CRYPTOCURRENCY`
- Renamed `LiquiditySide.NONE` to `NO_LIQUIDITY_SIDE`
- Renamed `OMSType` to `OmsType`
- Renamed `OmsType.NONE` to `UNSPECIFIED`
- Renamed `OrderSide.NONE` to `NO_ORDER_SIDE`
- Renamed `PositionSide.NONE` to `NO_POSITION_SIDE`
- Renamed `TrailingOffsetType.NONE` to `NO_TRAILING_OFFSET`
- Removed `TrailingOffsetType.DEFAULT`
- Renamed `TriggerType.NONE` to `NO_TRIGGER`
- Renamed `TriggerType.LAST` to `LAST_TRADE`
- Renamed `TriggerType.MARK` to `MARK_PRICE`
- Renamed `TriggerType.INDEX` to `INDEX_PRICE`
- Renamed `ComponentState.INITIALIZED` to `READY`
- Renamed `OrderFactory.bracket(post_only)` to `post_only_entry`
- Moved `manage_gtd_expiry` to `Strategy.submit_order(...)` and `Strategy.submit_order_list(...)`

### Enhancements
- Added `BarSpecification.timedelta` property, thanks @rsmb7z
- Added `DataEngineConfig.build_time_bars_with_no_updates` option
- Added `OrderFactory.bracket(post_only_tp)` param
- Added `OrderListIdGenerator` and integrate with `OrderFactory`
- Added `Cache.add_order_list(...)`
- Added `Cache.order_list(...)`
- Added `Cache.order_lists(...)`
- Added `Cache.order_list_exists(...)`
- Added `Cache.order_list_ids(...)`
- Improved generation of `OrderListId` from factory to ensure uniqueness
- Added auction matches for backtests, thanks @limx0
- Added `.timedelta` property to `BarSpecification`, thanks @rsmb7z
- Numerous improvements to the Betfair adapter, thanks @limx0
- Improvements to Interactive Brokers data subscriptions, thanks @rsmb7z
- Added `DataEngineConfig.validate_data_sequence` (False by default and currently only for `Bar` data), thanks @rsmb7z

### Fixes
- Added `TRD_GRP_*` enum variants for Binance spot permissions
- Fixed `PARTIALLY_FILLED` -> `EXPIRED` order state transition, thanks @bb01100100

---

# NautilusTrader 1.164.0 Beta

Released on 23rd December 2022 (UTC).

### Breaking Changes
None

### Enhancements
- Added managed GTD order expiry (experimental feature, config may change)
- Added Rust `ParquetReader` and `ParquetWriter` (for `QuoteTick` and `TradeTick` only)

### Fixes
- Fixed `MARKET_IF_TOUCHED` orders for `OrderFactory.bracket(..)`
- Fixed `OrderEmulator` trigger event handling for live trading
- Fixed `OrderEmulator` transformation to market orders which had a GTD time in force
- Fixed serialization of `OrderUpdated` events
- Fixed typing and edge cases for new `msgspec`, thanks @limx0 
- Fixed data wrangler processing with missing data, thanks @rsmb7z

---

# NautilusTrader 1.163.0 Beta

Released on 17th December 2022 (UTC).

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed `MARKET_IF_TOUCHED` and `LIMIT_IF_TOUCHED` trigger and modify behavior
- Fixed `MatchingEngine` updates of stop order types
- Fixed combinations of passive or immediate trigger vs passive or immediate fill behavior
- Fixed memory leaks from passing string pointers from Rust, thanks @twitu

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
- Fixed `MARKET_TO_LIMIT` order initial fill behavior
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
- Added emulated contingent orders capability to `OrderEmulator`
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
- Fixed OCO contingent orders which were actually implemented as OUO for backtests
- Fixed various bugs for Interactive Brokers integration, thanks @limx0 and @rsmb7z
- Fixed pyarrow version parsing, thanks @ghill2
- Fixed returning venue from InstrumentId, thanks @rsmb7z

---

# NautilusTrader 1.158.0 Beta

Released on 3rd November 2022 (UTC).

### Breaking Changes
- Added `LiveExecEngineConfig.reconciliation` boolean flag to control if reconciliation is active
- Removed `LiveExecEngineConfig.reconciliation_auto` (unclear naming and concept)
- All Redis keys have changed to a lowercase convention (either migrate or flush your Redis)
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
- Added `PositionSide.NO_POSITION_SIDE` enum variant
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
- Fixed limit order `IOC` and `FOK` behavior, thanks @limx0 for identifying
- Fixed FTX `CryptoFuture` instrument parsing, thanks @limx0
- Fixed missing imports in data catalog example notebook, thanks @gaugau3000
- Fixed order update behavior, affected orders:
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
- Improved component state transition behavior and logging
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
- Fixed behavior of `IOC` and `FOK` time in force instructions
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
- Added `ContingencyType` enum (for contingent orders in an `OrderList`)
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
the message bus, see the related issue for further details on this enhancement.

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
'noise' and simplify the codebase. Note that the use of `inline` for 
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
- Exchange accounting for exchange `OmsType.NETTING`
- Position flipping logic for exchange `OmsType.NETTING`
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
- Remove redundant flag `generate_position_ids` (handled by `OmsType`)

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

The behavior of the `StopLimitOrder` continued to be fixed and refined.
`SimulatedExchange` was refactored further to reduce complexity.

### Breaking Changes
None

### Enhancements
None

### Fixes
- `TRIGGERED` states in order FSM
- `StopLimitOrder` triggering behavior
- `OrderFactory.stop_limit` missing `post_only` and `hidden`
- `Order` and `StopLimitOrder` `__repr__` string (duplicate id)

---

# NautilusTrader 1.107.0 Beta

## Release Notes

The main thrust of this release is to refine some subtleties relating to order
matching and amendment behavior for improved realism. This involved a fairly substantial refactoring
of `SimulatedExchange` to manage its complexity, and support extending the order types.

The `post_only` flag for LIMIT orders now results in the expected behavior regarding
when a marketable limit order will become a liquidity `TAKER` during order placement
and amendment.

Test coverage was moderately increased.

### Breaking Changes
None

### Enhancements
- Refactored `SimulatedExchange` order matching and amendment logic
- Add `risk` subpackage to group risk components

### Fixes
- `StopLimitOrder` triggering behavior
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
