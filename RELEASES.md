# NautilusTrader 1.133.0 Beta - Release Notes

Released on 8th, November 2021

## Breaking Changes
None

## Enhancements
- Added `LatencyModel` for simulated exchange.
- Added `Clock.timestamp_ms()`.
- Added `depth` param when subscribing to order book deltas.
- Added `TestDataProvider` and consolidate test data.
- Added orjson default serializer for arrow.
- Reorganized example strategies and launch scripts.

## Fixes
- Fixed logic for partial fills in backtests.
- Various Betfair integration fixes.
- Various `BacktestNode` fixes.

---

# NautilusTrader 1.132.0 Beta - Release Notes

Released on 24th, October 2021

## Breaking Changes
- `Actor` constructor now takes `ActorConfig`.

## Enhancements
- Added `ActorConfig`.
- Added `ImportableActorConfig`.
- Added `ActorFactory`.
- Added `actors` to `BacktestRunConfig`.
- Improved network base classes.
- Refine `InstrumentProvider`.

## Fixes
- Fixed persistence config for `BacktestNode`.
- Various Betfair integration fixes.

---

# NautilusTrader 1.131.0 Beta - Release Notes

Released on 10th, October 2021

## Breaking Changes
- Renamed `nanos_to_unix_dt` to `unix_nanos_to_dt` (more accurate name).
- Changed `Clock.set_time_alert(...)` method signature.
- Changed `Clock.set_timer(...)` method signature.
- Removed `pd.Timestamp` from `TimeEvent`.

## Enhancements
- `OrderList` submission and OTO, OCO contingencies now operational.
- Added `Cache.orders_for_position(...)` method.
- Added `Cache.position_for_order(...)` method.
- Added `SimulatedExchange.get_working_bid_orders(...)` method.
- Added `SimulatedExchange.get_working_ask_orders(...)` method.
- Added optional `run_config_id` for backtest runs.
- Added `BacktestResult` object.
- Added `Clock.set_time_alert_ns(...)` method.
- Added `Clock.set_timer_ns(...)` method.
- Added `fill_limit_at_price` simulated exchange option.
- Added `fill_stop_at_price` simulated exchange option.
- Improve timer and time event efficiency.

## Fixes
- Fixed `OrderUpdated` leaves quantity calculation.
- Fixed contingency order logic at the exchange.
- Fixed indexing of orders for a position in the cache.
- Fixed flip logic for zero sized positions (not a flip).

---

# NautilusTrader 1.130.0 Beta - Release Notes

Released on 26th, September 2021

## Breaking Changes
- `BacktestEngine.run` method signature change.
- Renamed `BookLevel` to `BookType`.
- Renamed `FillModel` params.

## Enhancements
- Added streaming backtest machinery.
- Added `quantstats` (removed `empyrical`).
- Added `BacktestEngine.run_streaming()`.
- Added `BacktestEngine.end_streaming()`
- Added `Portfolio.balances_locked(venue)`.
- Improved `DataCatalog` functionality.
- Improved logging for `BacktestEngine`.
- Improved parquet serialization and machinery.

## Fixes
- Fixed `SimulatedExchange` message processing.
- Fixed `BacktestEngine` event ordering in main loop.
- Fixed locked balance calculation for `CASH` accounts.
- Fixed fill dynamics for `reduce-only` orders.
- Fixed `PositionId` handling for `HEDGING` OMS exchanges.
- Fixed parquet `Instrument` serialization.
- Fixed `CASH` account PnL calculations with base currency.

---

# NautilusTrader 1.129.0 Beta - Release Notes

Released on 12th, September 2021

## Breaking Changes
- Removed CCXT adapter (#428).
- Backtest configuration changes.
- Renamed `UpdateOrder` to `ModifyOrder` (terminology standardization).
- Renamed `DeltaType` to `BookAction` (terminology standardization).

## Enhancements
- Added `BacktestNode`.
- Added `BookIntegrityError` with improved integrity checks for order books.
- Added order custom user tags.
- Added `Actor.register_warning_event` (also applicable to `TradingStrategy`).
- Added `Actor.deregister_warning_event` (also applicable to `TradingStrategy`).
- Added `ContingencyType` enum (for contingency orders in an `OrderList`).
- All order types can now be `reduce_only` (#437).
- Refined backtest configuration options.
- Improved efficiency of `UUID4` using the `fastuuid` Rust bindings.

## Fixes
- Fixed Redis loss of precision for `int64_t` nanosecond timestamps (#363).
- Fixed behavior of `reduce_only` orders for both submission and filling (#437).
- Fixed PnL calculation for `CASH` accounts when commission negative (#436).

---

# NautilusTrader 1.128.0 Beta - Release Notes

Released on 30th, August 2021

This release continues the focus on the core system, with upgrades and cleanups
to the component base class. The concept of an `active` order has been introduced, 
which is an order whose state can change (is not a `completed` order).

## Breaking Changes
- All configuration due `pydantic` upgrade.
- Throttling config now takes string e.g. "100/00:00:01" which is 100 / second.
- Renamed `DataProducerFacade` to `DataProducer`.
- Renamed `fill.side` to `fill.order_side` (clarity and standardization).
- Renamed `fill.type` to `fill.order_type` (clarity and standardization).

## Enhancements
- Added serializable configuration classes leveraging `pydantic`.
- Improved adding bar data to `BacktestEngine`.
- Added `BacktestEngine.add_bar_objects()`.
- Added `BacktestEngine.add_bars_as_ticks()`.
- Added order `active` concept, with `order.is_active` and cache methods.
- Added `ComponentStateChanged` event.
- Added `Component.degrade()` and `Component.fault()` command methods.
- Added `Component.on_degrade()` and `Component.on_fault()` handler methods.
- Added `ComponentState.PRE_INITIALIZED`.
- Added `ComponentState.DEGRADING`.
- Added `ComponentState.DEGRADED`.
- Added `ComponentState.FAULTING`.
- Added `ComponentState.FAULTED`.
- Added `ComponentTrigger.INITIALIZE`.
- Added `ComponentTrigger.DEGRADE`.
- Added `ComponentTrigger.DEGRADED`.
- Added `ComponentTrigger.FAULT`.
- Added `ComponentTrigger.FAULTED`.
- Wired up `Ticker` data type.

## Fixes
- `DataEngine.subscribed_bars()` now reports internally aggregated bars also.

---

# NautilusTrader 1.127.0 Beta - Release Notes

Released on 17th, August 2021

This release has again focused on core areas of the platform, including a 
significant overhaul of accounting and portfolio components. The wiring between 
the `DataEngine` and `DataClient`(s) has also received attention, and should now 
exhibit correct subscription mechanics.

The Betfair adapter has been completely re-written, providing various fixes and
enhancements, increased performance, and full async support.

There has also been some further renaming to continue to align the platform
as closely as possible with established terminology in the domain.

## Breaking Changes
- Moved margin calculation methods from `Instrument` to `Account`.
- Removed redundant `Portfolio.register_account`.
- Renamed `OrderState` to `OrderStatus`.
- Renamed `Order.state` to `Order.status`.
- Renamed `msgbus.message_bus` to `msgbus.bus`.

## Enhancements
- Betfair adapter re-write.
- Extracted `accounting` subpackage.
- Extracted `portfolio` subpackage.
- Subclassed `Account` with `CashAccount` and `MarginAccount`.
- Added `AccountsManager`.
- Added `AccountFactory`.
- Moved registration of custom account classes to `AccountFactory`.
- Moved registration of calculated account to `AccountFactory`.
- Added registration of OMS type per trading strategy.
- Added `ExecutionClient.create_account` for custom account classes.
- Separate `PortfolioFacade` from `Portfolio`.

## Fixes
- Data subscription handling in `DataEngine`.
- `Cash` accounts no longer generate spurious margins.
- Fix `TimeBarAggregator._stored_close_ns` property name.

---

# NautilusTrader 1.126.1 Beta - Release Notes

Released on 3rd, August 2021

This is a patch release which fixes a bug involving `NotImplementedError` 
exception handling when subscribing to order book deltas when not supported by 
a client. This bug affected CCXT order book subscriptions.

## Breaking Changes
None

## Enhancements
None

## Fixes
- Fix `DataEngine` order book subscription handling.

---

# NautilusTrader 1.126.0 Beta - Release Notes

Released on 2nd, August 2021

This release sees the completion of the initial implementation of the 
`MessageBus`, with data now being handled by PUB/SUB patterns, along with the 
additions of point-to-point and REQ/REP messaging functionality.

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

## Breaking Changes
- Renamed `timestamp_ns` to `ts_init`.
- Renamed `ts_recv_ns` to `ts_event`.
- Renamed various event timestamp parameters to `ts_event`.
- Removed null object methods on identifiers.

## Enhancements
- Added `Actor` component base class.
- Added `MessageBus.register()`.
- Added `MessageBus.send()`.
- Added `MessageBus.request()`.
- Added `MessageBus.response()`.
- Added `Trader.add_component()`.
- Added `Trader.add_components()`.
- Added `Trader.add_log_sink()`.

## Fixes
- Various Betfair adapter patches and fixes.
- `ExecutionEngine` position flip logic in certain edge cases.

---

# NautilusTrader 1.125.0 Beta - Release Notes

Released on 18th, July 2021

This release introduces a major re-architecture of the internal messaging system.
A common message bus has been implemented which now handles all events via a 
PUB/SUB messaging pattern. The next release will see all data being handled by 
the message bus, please see the related issue for further details on this enhancement.

Another notable feature is the introduction of the order 'in-flight' concept, 
which is a submitted order which has not yet been acknowledged by the 
trading venue. Several properties on `Order`, and methods on `Cache`, now exist
to support this.

The `Throttler` has been refactored and optimized further. There has also been
extensive reorganization of the model sub-package, standardization of identifiers
on events, along with numerous 'under the hood' cleanups and two bug fixes.

## Breaking Changes
- Renamed `MessageType` enum to `MessageCategory`.
- Renamed `fill.order_side` to `fill.side`.
- Renamed `fill.order_type` to `fill.type`.
- All `Event` serialization due to domain refactorings.

## Enhancements
- Added `MessageBus` class.
- Added `TraderId` to `Order` and `Position`.
- Added `OrderType` to OrderFilled.
- Added unrealized PnL to position events.
- Added order inflight concept to `Order` and `Cache`.
- Improved efficiency of `Throttler`.
- Standardized events `str` and `repr`.
- Standardized commands `str` and `repr`.
- Standardized identifiers on events and objects.
- Improved `Account` `str` and `repr`.
- Using `orjson` over `json` for efficiency.
- Removed redundant `BypassCacheDatabase`.
- Introduced `mypy` to the codebase.

## Fixes
- Fixed backtest log timestamping.
- Fixed backtest duplicate initial account event.

---

# NautilusTrader 1.124.0 Beta - Release Notes

Released on 6th, July 2021

This release sees the expansion of pre-trade risk check options (see 
`RiskEngine` class documentation). There has also been extensive 'under the 
hood' code cleanup and consolidation.

## Breaking Changes
- Renamed `Position.opened_timestamp_ns` to `ts_opened_ns`.
- Renamed `Position.closed_timestamp_ns` to `ts_closed_ns`.
- Renamed `Position.open_duration_ns` to `duration_ns`.
- Renamed Loggers `bypass_logging` to `bypass`.
- Refactored `PositionEvent` types.

## Enhancements
- Add pre-trade risk checks to `RiskEngine` iteration 2.
- Improve `Throttler` functionality and performance.
- Removed redundant `OrderInvalid` state and associated code.
- Improve analysis reports.

## Fixes
- PnL calculations for `CASH` account types.
- Various event serializations.

---

# NautilusTrader 1.123.0 Beta - Release Notes

Released on 20th, June 2021

A major feature of this release is a complete re-design of serialization for the
platform, along with initial support for the [Parquet](https://parquet.apache.org/) format.
The MessagePack serialization functionality has been refined and retained.

In the interests of explicitness there is now a convention that timestamps are 
named either `timestamp_ns`, or prepended with `ts`. Timestamps which are 
represented with an `int64` are always in nanosecond resolution, and appended 
with `_ns` accordingly.

Initial scaffolding for new backtest data tooling has been added.

## Breaking Changes
- Renamed `OrderState.PENDING_REPLACE` to `OrderState.PENDING_UPDATE`
- Renamed `timestamp_origin_ns` to `ts_event_ns`.
- Renamed `timestamp_ns` for data to `ts_recv_ns`.
- Renamed `updated_ns` to `ts_updated_ns`.
- Renamed `submitted_ns` to `ts_submitted_ns`.
- Renamed `rejected_ns` to `ts_rejected_ns`.
- Renamed `accepted_ns` to `ts_accepted_ns`.
- Renamed `pending_ns` to `ts_pending_ns`.
- Renamed `canceled_ns` to `ts_canceled_ns`.
- Renamed `triggered_ns` to `ts_triggered_ns`.
- Renamed `expired_ns` to `ts_expired_ns`.
- Renamed `execution_ns` to `ts_filled_ns`.
- Renamed `OrderBookLevel` to `BookLevel`.
- Renamed `Order.volume` to `Order.size`.

## Enhancements
- Adapter dependencies are now optional extras at installation.
- Added arrow/parquet serialization.
- Added object `to_dict()` and `from_dict()` methods.
- Added `Order.is_pending_update`.
- Added `Order.is_pending_cancel`.
- Added `run_analysis` config option for `BacktestEngine`.
- Removed `TradeMatchId` in favour of bare string.
- Removed redundant conversion to `pd.Timestamp` when checking timestamps.
- Removed redundant data `to_serializable_str` methods.
- Removed redundant data `from_serializable_str` methods.
- Removed redundant `__ne__` implementations.
- Removed redundant `MsgPackSerializer` cruft.
- Removed redundant `ObjectCache` and `IdentifierCache`.
- Removed redundant string constants.

## Fixes
- Fixed millis to nanos in `CCXTExecutionClient`.
- Added missing trigger to `UpdateOrder` handling.
- Removed all `import *`.

---

# NautilusTrader 1.122.0 Beta - Release Notes

Released on 6th, June 2021

This release includes numerous breaking changes with a view to enhancing the core
functionality and API of the platform. The data and execution caches have been 
unified for simplicity. There have also been large changes to the accounting 
functionality, with 'hooks' added in preparation for accurate calculation and 
handling of margins.

## Breaking Changes
- Renamed `Account.balance()` to `Account.balance_total()`.
- Consolidated`TradingStrategy.data` into `TradingStrategy.cache`.
- Consolidated `TradingStrategy.execution` into `TradingStrategy.cache`.
- Moved `redis` subpackage into `infrastructure`.
- Moved some accounting methods back to `Instrument`.
- Removed `Instrument.market_value()`.
- Renamed `Portfolio.market_values()` to `Portfolio.net_exposures()`.
- Renamed `Portfolio.market_value()` to `Portfolio.net_exposure()`.
- Renamed `InMemoryExecutionDatabase` to `BypassCacheDatabase`.
- Renamed `Position.relative_qty` to `Position.net_qty`.
- Renamed `default_currency` to `base_currency`.
- Removed `cost_currency` property from `Instrument`.

## Enhancements
- `ExecutionClient` now has the option of calculating account state.
- Unified data and execution caches into single `Cache`.
- Improved configuration options and naming.
- Simplified `Portfolio` component registration.
- Simplified wiring of `Cache` into components.
- Added `repr` to execution messages.
- Added `AccountType` enum.
- Added `cost_currency` to `Position`.
- Added `get_cost_currency()` to `Instrument`.
- Added `get_base_currency()` to `Instrument`.

## Fixes
- Fixed `Order.is_working` for `PENDING_CANCEL` and `PENDING_REPLACE` states.
- Fixed loss of precision for nanosecond timestamps in Redis.
- Fixed state reconciliation when uninstantiated client.

---

# NautilusTrader 1.121.0 Beta - Release Notes

Released on 30th, May 2021

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

## Breaking Changes
- `BacktestEngine.add_venue` added `venue_type` to method params.
- `ExecutionClient` added `venue_type` to constructor params.
- `TraderId` instantiation.
- `StrategyId` instantiation.
- `Instrument` serialization.

## Enhancements
- `Portfolio` pending calculations if data not immediately available.
- Added `instruments` subpackage with expanded class definitions.
- Added `timestamp_origin_ns` timestamp when originally occurred.
- Added `AccountState.is_reported` flagging if reported by exchange or calculated.
- Simplified `TraderId` and `StrategyId` identifiers.
- Improved `ExecutionEngine` order routing.
- Improved `ExecutionEngine` client registration.
- Added order routing configuration.
- Added `VenueType` enum and parser.
- Improved param typing for identifier generators.
- Improved log formatting of `Money` and `Quantity` thousands commas.

## Fixes
- CCXT `TICK_SIZE` precision mode - size precisions (BitMEX, FTX).
- State reconciliation (various bugs).

---

# NautilusTrader 1.120.0 Beta - Release Notes

This release focuses on simplifications and enhancements of existing machinery.

## Breaking Changes
- `Position` now requires an `Instrument` param.
- `is_inverse` removed from `OrderFilled`.
- `ClientId` removed from `TradingCommand` and subclasses.
- `AccountId` removed from `TradingCommand` and subclasses.
- `TradingCommand` serialization.

## Enhancements
- Added `Instrument` methods to `ExecutionCache`.
- Added `Venue` filter to cache queries.
- Moved order validations into `RiskEngine`.
- Refactored `RiskEngine`.
- Removed routing type information from identifiers.

## Fixes
None

---

# NautilusTrader 1.119.0 Beta - Release Notes

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

## Breaking Changes
- Serializations involving `Money`.
- Changed usage of `Price` and `Quantity`.
- Renamed `BypassExecutionDatabase` to `BypassCacheDatabase`.

## Enhancements
- Rewired `RiskEngine` and `ExecutionEngine` sequence.
- Added `Instrument` database operations.
- Added `MsgPackInstrumentSerializer`.
- Added `Price.from_str()`.
- Added `Price.from_int()`.
- Added `Quantity.zero()`.
- Added `Quantity.from_str()`.
- Added `Quantity.from_int()`.
- Added `Instrument.make_price()`.
- Added `Instrument.make_qty()`.
- Improved serialization of `Money`.

## Fixes
- Handling of precision for `decimal.Decimal` values passed to value objects.

---

# NautilusTrader 1.118.0 Beta - Release Notes

This release simplifies the backtesting workflow by removing the need for the 
intermediate `BacktestDataContainer`. There has also been some simplifications
for `OrderFill` events, as well as additional order states and events.

## Breaking Changes
- Standardized all 'cancelled' references to 'canceled'.
- `SimulatedExchange` no longer generates `OrderAccepted` for `MarketOrder`.
- Removed redundant `BacktestDataContainer`.
- Removed redundant `OrderFilled.cum_qty`.
- Removed redundant `OrderFilled.leaves_qty`.
- `BacktestEngine` constructor simplified.
- `BacktestMarketDataClient` no longer needs instruments.
- Rename `PerformanceAnalyzer.get_realized_pnls` to `.realized_pnls`.

## Enhancements
- Re-engineered `BacktestEngine` to take data directly.
- Added `OrderState.PENDING_CANCEL`.
- Added `OrderState.PENDING_REPLACE`.
- Added `OrderPendingUpdate` event.
- Added `OrderPendingCancel` event.
- Added `OrderFilled.is_buy` property (with corresponding `is_buy_c()` fast method).
- Added `OrderFilled.is_sell` property (with corresponding `is_sell_c()` fast method).
- Added `Position.is_opposite_side(OrderSide side)` convenience method.
- Modified the `Order` FSM and event handling for the above.
- Consolidated event generation into `ExecutionClient` base class.
- Refactored `SimulatedExchange` for greater clarity.

## Fixes
- `ExecutionCache` positions open queries.
- Exchange accounting for exchange `OMSType.NETTING`.
- Position flipping logic for exchange `OMSType.NETTING`.
- Multi-currency account terminology.
- Windows wheel packaging.
- Windows path errors.

---

# NautilusTrader 1.117.0 Beta - Release Notes

The major thrust of this release is added support for order book data in
backtests. The `SimulatedExchange` now maintains order books of each instrument
and will accurately simulate market impact with L2/L3 data. For quote and trade
tick data a L1 order book is used as a proxy. A future release will include 
improved fill modelling assumptions and customizations.

## Breaking Changes
- `OrderBook.create` now takes `Instrument` and `BookLevel`.

## Enhancements
- `SimulatedExchange` now maintains order books internally.
- `LiveLogger` now exhibits better blocking behavior and logging.

## Fixes
- Various patches to the `Betfair` adapter.
- Documentation builds.

---

# NautilusTrader 1.116.1 Beta - Release Notes

Announcing official Windows 64-bit support.

Several bugs have been identified and fixed.

## Breaking Changes
None

## Enhancements
- Performance test refactoring.
- Remove redundant performance harness.
- Add `Queue.peek()` to high-performance queue.
- GitHub action refactoring, CI for Windows.
- Builds for 32-bit platforms.

## Fixes
- `OrderBook.create` for `BookLevel.L3` now returns correct book.
- Betfair handling of execution IDs.

---

# NautilusTrader 1.116.0 Beta - Release Notes

**This release includes substantial breaking changes.**

Further fundamental changes to the core API have been made.

## Breaking Changes
- Introduce `ClientId` for data and execution client identification.
- Standardize client IDs to upper case.
- Rename `OrderBookOperation` to `OrderBookDelta`.
- Rename `OrderBookOperations` to `OrderBookDeltas`.
- Rename `OrderBookOperationType` to `OrderBookDeltaType`.

## Enhancements
None

## Fixes
None

---

# NautilusTrader 1.115.0 Beta - Release Notes

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

## Breaking Changes
- Rename `OrderId` to `VenueOrderId`.
- Rename `Order.id` to `Order.venue_order_id`.
- Rename `Order.cl_ord_id` to `Order.client_order_id`.
- Rename `AssetClass.STOCK` to `AssetClass.EQUITY`.
- Remove redundant flag `generate_position_ids` (handled by `OMSType`).

## Enhancements
- Introduce integration for Betfair.
- Add `AssetClass.METAL` and `AssetClass.ENERGY`.
- Add `VenueStatusEvent`, `InstrumentStatusEvent` and `InstrumentClosePrice`.
- Usage of `np.ndarray` to improve function and indicator performance.

## Fixes
- LiveLogger log message when blocking.

---

# NautilusTrader 1.114.0 Beta - Release Notes

**This release includes substantial breaking changes.**

Further standardization of naming conventions along with internal refinements
and fixes.

## Breaking Changes
- Rename `AmendOrder` to `UpdateOrder`.
- Rename `OrderAmended` to `OrderUpdated`.
- Rename `amend` and `amended` related methods to `update` and `updated`.
- Rename `OrderCancelReject` to `OrderCancelRejected` (standardize tense).

## Enhancements
- Improve efficiency of data wrangling.
- Simplify `Logger` and general system logging.
- Add `stdout` and `stderr` log streams with configuration.
- Add `OrderBookData` base class.

## Fixes
- Backtest handling of `GenericData` and `OrderBook` related data.
- Backtest `DataClient` creation logic prevented client registering.

---

# NautilusTrader 1.113.0 Beta - Release Notes

**This release includes substantial breaking changes.**

Further standardization of naming conventions along with internal refinements
and fixes.

## Breaking Changes
- Rename `AmendOrder` to `UpdateOrder`.
- Rename `OrderAmended` to `OrderUpdated`.
- Rename `amend` and `amended` related methods to `update` and `updated`.
- Rename `OrderCancelReject` to `OrderCancelRejected` (standardize tense).

## Enhancements
- Introduce `OrderUpdateRejected`, event separated for clarity.
- Refined LiveLogger: Now runs on event loop with high-performance `Queue`.
- Improved flexibility of when strategies are added to a `BacktestEngine`.
- Improved checks for `VenueOrderId` equality when applying order events.

## Fixes
- Removed `UNDEFINED` enum values. Do not allow invalid values to be represented
in the system (prefer throwing exceptions).

---

# NautilusTrader 1.112.0 Beta - Release Notes

**This release includes substantial breaking changes.**

The platforms internal timestamping has been standardized to nanoseconds. This
decision was made to increase the accuracy of backtests to nanosecond precision,
improve data handling including order book and custom data for backtesting, and
to future-proof the platform to a more professional standard. The top-level user
API still takes `datetime` and `timedelta` objects for usability.

There has also been some standardization of naming conventions to align more
closely with established financial market terminology with reference to the
FIX5.0 SP2 specification, and CME MDP 3.0.

## Breaking Changes
- Move `BarType` into `Bar` as a property.
- Change signature of `Bar` handling methods due to above.
- Remove `Instrument.leverage` (incorrect place for concept).
- Change `ExecutionClient.venue` as a `Venue` to `ExecutionClient.name` as a `str`.
- Change serialization of timestamp datatype to `int64`.
- Extensive changes to serialization constant names.
- Rename `OrderFilled.filled_qty` to `OrderFilled.last_qty`.
- Rename `OrderFilled.filled_price` to `OrderFilled.last_px`.
- Rename `avg_price` to `avg_px` in methods and properties.
- Rename `avg_open` to `avg_px_open` in methods and properties.
- Rename `avg_close` to `avg_px_close` in methods and properties.
- Rename `Position.relative_quantity` to `Position.relative_qty`.
- Rename `Position.peak_quantity` to `Position.peak_qty`.

## Enhancements
- Standardize nanosecond timestamps.
- Add time unit conversion functions as found in `nautilus_trader.core.datetime`.
- Add optional `broker` property to `Venue` to assist with routing.
- Enhance state reconciliation from both `LiveExecutionEngine` and `LiveExecutionClient`.
- Add internal messages to aid state reconciliation.

## Fixes
- `DataCache` incorrectly caching bars.

---

# NautilusTrader 1.111.0 Beta - Release Notes

This release adds further enhancements to the platform.

## Breaking Changes
None

## Enhancements
- `RiskEngine` built out including configuration options hook and
  `LiveRiskEngine` implementation.
- Add generic `Throttler`.
- Add details `dict` to `instrument_id` related requests to cover IB futures
  contracts.
- Add missing Fiat currencies.
- Add additional Crypto currencies.
- Add ISO 4217 codes.
- Add currency names.

## Fixes
- Queue `put` coroutines in live engines when blocking at `maxlen` was not
  creating a task on the event loop.

---

# NautilusTrader 1.110.0 Beta - Release Notes

This release applies one more major change to the identifier API. `Security` has
been renamed to `InstrumentId` for greater clarity that the object is an identifier,
and to group the concept of an instrument with its identifier.

Data objects in the framework have been further abstracted to prepare for the
handling of custom data in backtests.

A `RiskEngine` base class has also been scaffolded.

## Breaking Changes
- `Security` renamed to `InstrumentId`.
- `Instrument.security` renamed to `Instrument.id`.
- `Data` becomes an abstract base class with `timestamp` and `unix_timestamp`
  properties.
- `Data` and `DataType` moved to `model.data`.
- `on_data` methods now take `GenericData`.

## Enhancements
- Add `GenericData`.
- Add `Future` instrument.

## Fixes
None

---

# NautilusTrader 1.109.0 Beta - Release Notes

The main thrust of this release is to refine and further bed down the changes
to the identifier model via `InstrumentId`, and fix some bugs.

Errors in the CCXT clients caused by the last release have been addressed.

## Breaking Changes
- `InstrumentId` now takes first class value object `Symbol`.
- `InstrumentId` `asset_class` and `asset_type` no longer optional.
- `SimulatedExchange.venue` changed to `SimulatedExchange.id`.

## Enhancements
- Ensure `TestTimer` advances monotonically increase.
- Add `AssetClass.BETTING`.

## Fixes
- CCXT data and execution clients regarding `instrument_id` vs `symbol` naming.
- `InstrumentId` equality and hashing.
- Various docstrings.

---

# NautilusTrader 1.108.0 Beta - Release Notes

This release executes a major refactoring of `Symbol` and how securities are
generally identified within the platform. This will allow a smoother integration
with Interactive Brokers and other exchanges, brokerages and trading
counterparties.

Previously the `Symbol` identifier also included a venue which confused the concept.
The replacement `Security` identifier more clearly expresses the domain with a
symbol string, a primary `Venue`, `AssetClass` and `AssetType` properties.

## Breaking Changes
- All previous serializations.
- `Security` replaces `Symbol` with expanded properties.
- `AssetClass.EQUITY` changed to `AssetClass.STOCK`.
- `from_serializable_string` changed to `from_serializable_str`.
- `to_serializable_string` changed to `to_serializable_str`.

## Enhancements
- Reports now include full instrument_id name.
- Add `AssetType.WARRANT`.

## Fixes
- `StopLimitOrder` serialization.

---

# NautilusTrader 1.107.1 Beta - Release Notes

This is a patch release which applies various fixes and refactorings.

The behaviour of the `StopLimitOrder` continued to be fixed and refined.
`SimulatedExchange` was refactored further to reduce complexity.

## Breaking Changes
None

## Enhancements
None

## Fixes
- `TRIGGERED` states in order FSM.
- `StopLimitOrder` triggering behaviour.
- `OrderFactory.stop_limit` missing `post_only` and `hidden`.
- `Order` and `StopLimitOrder` `__repr__` string (duplicate id).

---

# NautilusTrader 1.107.0 Beta - Release Notes

The main thrust of this release is to refine some subtleties relating to order
matching and amendment behaviour for improved realism. This involved a fairly substantial refactoring
of `SimulatedExchange` to manage its complexity, and support extending the order types.

The `post_only` flag for LIMIT orders now results in the expected behaviour regarding
when a marketable limit order will become a liquidity `TAKER` during order placement
and amendment.

Test coverage was moderately increased.

## Breaking Changes
None

## Enhancements
- Refactored `SimulatedExchange` order matching and amendment logic.
- Add `risk` subpackage to group risk components.

## Fixes
- `StopLimitOrder` triggering behaviour.
- All flake8 warnings.

---

# NautilusTrader 1.106.0 Beta - Release Notes

The main thrust of this release is to introduce the Interactive Brokers
integration, and begin adding platform capabilities to support this effort.

## Breaking Changes
- `from_serializable_string` methods changed to `from_serializable_str`.

## Enhancements
- Scaffold Interactive Brokers integration in `adapters/ib`.
- Add the `Future` instrument type.
- Add the `StopLimitOrder` order type.
- Add the `Data` and `DataType` types to support custom data handling.
- Add the `InstrumentId` identifier types initial implementation to support extending the platforms capabilities.

## Fixes
- `BracketOrder` correctness.
- CCXT precision parsing bug.
- Some log formatting.

---
