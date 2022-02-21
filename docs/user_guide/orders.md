# Orders

This guide focuses on how to use the available order functionality for the platform in the best way.

Orders are one of the fundamental building blocks of any algorithmic trading strategy.
NautilusTrader has unified a large set of order types and execution instructions
from standard to more advanced, to offer as much of an exchanges available functionality
as possible. This allows traders to define certain conditions and directions for
order execution and management, which allows essentially any type of trading strategy to be created.

## Types
The two main order types as _market_ orders and _limit_ orders. All the other order
types are built from these two fundamental types, in terms of liquidity provision they
are exact opposites. Market orders demand liquidity and require immediate trading at the best
price available. Whereas limit orders provide liquidity, they act as standing orders in a limit order book 
at a specified price limit.

The order types available within the platform are (using the enum values):
- `MARKET`
- `LIMIT`
- `STOP_MARKET`
- `STOP_LIMIT`
- `MARKET_TO_LIMIT`
- `MARKET_IF_TOUCHED`
- `LIMIT_IF_TOUCHED`
- `TRAILING_STOP_MARKET`
- `TRAILING_STOP_LIMIT`

### Market

[API Reference](../api_reference/model/orders.md#market)

### Limit

[API Reference](../api_reference/model/orders.md#limit)

### Stop-Market

[API Reference](../api_reference/model/orders.md#stop-market)

### Stop-Limit

[API Reference](../api_reference/model/orders.md#stop-limit)

### Market-To-Limit

API Reference TBD

### Market-If-Touched

[API Reference](../api_reference/model/orders.md#market-if-touched)

### Limit-If-Touched

[API Reference](../api_reference/model/orders.md#limit-if-touched)

## Order Factory

[API Reference](../api_reference/common.md#factories)
