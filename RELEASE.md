# NautilusTrader 1.107.0 Beta Release Notes

### Breaking Changes
None

### Enhancements
- Add `risk` sub-package to group risk components.
- Refactored `SimulatedExchange` order matching and amendment logic.

### Fixes
- `StopLimitOrder` behaviour in `SimulatedExchange`.
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
