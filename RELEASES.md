# NautilusTrader 1.112.0 Beta - Release Notes

**This release includes substantial breaking changes.**

The platforms internal timestamping has been standardized to nanoseconds. This
decision was made to increase the accuracy of backtests to nanosecond precision,
improve data handling including custom data for backtesting and to future proof
the platform to a professional standard.

There has also been some renaming to align more closely with established
financial market terminology with reference to the FIX5.0 SP2 spec.

## Breaking Changes
- Remove `Instrument.leverage` (incorrect place for concept).
- Change `ExecutionClient.venue` as a `Venue` to `ExecutionClient.name` as a `str`.
- Change serialization of timestamps to ints.
- Rename `COMMISSION` constant to `COMMISSION_AMOUNT`.
- Rename `filled_qty` to `last_qty`.
- Rename `filled_price` to `last_px`.
- Rename `avg_price` to `avg_px` in methods and properties.
- Rename `avg_open` to `avg_px_open` in methods and properties.
- Rename `avg_close` to `avg_px_close` in methods and properties.
- Rename `Position.relative_quantity` to `Position.relative_qty`.
- Rename `Position.peak_quantity` to `Position.peak_qty`.

## Enhancements
- Add Betfair adapter.
- Add optional `broker` property to `Venue` to assist with routing.
- Enhance state reconciliation from both `LiveExecutionEngine` and `LiveExecutionClient`.

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
- Add `risk` sub-package to group risk components.

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
