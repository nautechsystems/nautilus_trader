# Issue #3505 Contribution Notes

## Issue
- Upstream issue: https://github.com/nautechsystems/nautilus_trader/issues/3505

## My implementation
- Fork repository: https://github.com/<你的用户名>/nautilus_trader
- Branch: `fix/issue-3505-custom-order-id-generator`

## Summary
This change allows strategies to override the `OrderFactory` client order ID generator.

Implemented changes:
- added optional `client_order_id_generator` injection to `OrderFactory`
- validated the custom generator interface
- used the custom generator for generation, count synchronization, and reset
- preserved cache collision retry behavior
- added a strategy hook to provide the generator
- added unit tests for `OrderFactory` and `Strategy`

## Files changed
- `nautilus_trader/common/factories.pxd`
- `nautilus_trader/common/factories.pyx`
- `nautilus_trader/trading/strategy.pxd`
- `nautilus_trader/trading/strategy.pyx`
- `tests/unit_tests/common/test_factories.py`
- `tests/unit_tests/trading/test_strategy.py`

## Testing
```bash
uv run pytest tests/unit_tests/common/test_factories.py tests/unit_tests/trading/test_strategy.py -q
