# Orders

This guide provides more details on the available order types for the platform, along with
the optional execution instructions available for each.

Orders are one of the fundamental building blocks of any algorithmic trading strategy.
NautilusTrader has unified a large set of order types and execution instructions
from standard to more advanced, to offer as much of an exchanges available functionality
as possible. This allows traders to define certain conditions and instructions for
order execution and management, which allows essentially any type of trading strategy to be created.

## Types
The two main types of orders are _market_ orders and _limit_ orders. All the other order
types are built from these two fundamental types, in terms of liquidity provision they
are exact opposites. Market orders demand liquidity and require immediate trading at the best
price available. Conversely, limit orders provide liquidity, they act as standing orders in a limit order book 
at a specified price limit.

The order types available for the platform are (using the enum values):
- `MARKET`
- `LIMIT`
- `STOP_MARKET`
- `STOP_LIMIT`
- `MARKET_TO_LIMIT`
- `MARKET_IF_TOUCHED`
- `LIMIT_IF_TOUCHED`
- `TRAILING_STOP_MARKET`
- `TRAILING_STOP_LIMIT`

```{warning}
NautilusTrader has unified the API for a large set of order types and execution instructions, however
not all of these are available for every exchange. If an order is submitted where an instruction or option
is not available, then the system will not submit the order and an error will be logged with
a clear explanatory message.
```

## Order Factory
The easiest way to create new orders is by using the built-in `OrderFactory`, which is
automatically attached to every `TradingStrategy` class. This factory will take care
of lower level details - such as ensuring the correct trader ID and strategy ID is assigned, generation
of a necessary initialization ID and timestamp, and abstracts away parameters which don't necessarily
apply to the order type being created, or are only needed to specify more advanced execution instructions. 

This leaves the factory with simpler order creation methods to work with, all the
examples will leverage an `OrderFactory` from within a `TradingStrategy` context.

```{note}
For clarity, any optional parameters will be clearly marked with a comment which includes the default value.
```

### Market
The vanilla market order is an instruction by the trader to immediately trade
the given quantity at the best price available. You can also specify several
time in force options, and indicate whether this order is only intended to reduce
a position.

In the following example we create a market order to buy 100,000 AUD on the
Interactive Brokers [IdealPro](https://ibkr.info/node/1708) Forex ECN:

```python
order: MarketOrder = self.order_factory.market(
        instrument_id=InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO")),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100000),
        time_in_force=TimeInForce.IOC,  # <-- optional (default GTC)
        reduce_only=False,  # <-- optional (default False)
        tags="ENTRY",  # <-- optional (default None)
)
```
[API Reference](../api_reference/model/orders.md#market)

### Limit

[API Reference](../api_reference/model/orders.md#limit)

### Stop-Market

[API Reference](../api_reference/model/orders.md#stop-market)

### Stop-Limit

[API Reference](../api_reference/model/orders.md#stop-limit)

### Market-To-Limit

[API Reference](../api_reference/model/orders.md#market-to-limit)

### Market-If-Touched

[API Reference](../api_reference/model/orders.md#market-if-touched)

### Limit-If-Touched

[API Reference](../api_reference/model/orders.md#limit-if-touched)

### Order Lists

[API Reference](../api_reference/model/orders.md#order-list)

### Bracket Orders

[API Reference](../api_reference/common.md#factories)