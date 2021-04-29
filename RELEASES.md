# NautilusTrader 1.117.0 Beta - Release Notes

The major thrust of this release is added support for order book data in
backtests. The `SimulatedExchange` now maintains order books of each instrument
and will accurately simulate market impact with L2/L3 data. For quote and trade
tick data a L1 order book is used as a proxy. A future release will include 
improved fill modelling assumptions and customizations.

## Breaking Changes
- `OrderBook.create` now takes `Instrument` and `OrderBookLevel`.

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
- `OrderBook.create` for `OrderBookLevel.L3` now returns correct book.
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
