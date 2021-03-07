# NautilusTrader 1.107.0 Beta Release Notes

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
- Refactored `SimulatedExchange` order matching and amendment logic.
- Add `risk` sub-package to group risk components.

### Fixes
- `StopLimitOrder` triggering behaviour.
- All flake8 warnings.

# NautilusTrader 1.106.0 Beta Release Notes

The main thrust of this release is to introduce the Interactive Brokers
integration, and begin adding platform capabilities to support this effort.

### Breaking Changes
- `from_serializable_string` methods changed to `from_serializable_str`.

### Enhancements
- Scaffold Interactive Brokers integration in `adapters/ib`.
- Add the `Future` instrument type.
- Add the `StopLimitOrder` order type.
- Add the `Data` and `DataType` types to support custom data handling.
- Add the `Security` identifier types initial implementation to support extending the platforms capabilities.

### Fixes
- `BracketOrder` correctness.
- CCXT precision parsing bug.
- Some log formatting.
