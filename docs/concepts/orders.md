# Orders

This guide provides further details about the available order types for the platform, along with
the execution instructions supported for each.

Orders are one of the fundamental building blocks of any algorithmic trading strategy.
NautilusTrader has unified a large set of order types and execution instructions
from standard to more advanced, to offer as much of an exchanges available functionality
as possible. This enables traders to define certain conditions and instructions for
order execution and management, facilitating the creation of virtually any type of trading strategy.

## Overview

The two main types of orders are _Market_ orders and _Limit_ orders. All the other order
types are built from these two fundamental types, in terms of liquidity provision they
are exact opposites. _Market_ orders demand liquidity and require immediate trading at the best
price available. Conversely, _Limit_ orders provide liquidity, they act as standing orders in a limit order book 
at a specified limit price.

The core order types available for the platform are (using the enum values):
- `MARKET`
- `LIMIT`
- `STOP_MARKET`
- `STOP_LIMIT`
- `MARKET_TO_LIMIT`
- `MARKET_IF_TOUCHED`
- `LIMIT_IF_TOUCHED`
- `TRAILING_STOP_MARKET`
- `TRAILING_STOP_LIMIT`

:::info
NautilusTrader has unified the API for a large set of order types and execution instructions. 
Not all of these are available for every exchange. If an order is submitted where an instruction or 
option is not available, the system will **NOT** submit the order and an error will be logged with 
a clear explanatory message.
:::

### Terminology

- An order is **aggressive** if the type is `MARKET` or if its executing like a `MARKET` order (taking liquidity).
- An order is **passive** if the type is not `MARKET` (providing liquidity).
- An order is **active local** if it is still within the local system boundary in one of the following three (non-terminal) status:
    - `INITIALIZED`
    - `EMULATED`
    - `RELEASED`
- An order is **in-flight** when in one of the following status:
    - `SUBMITTED`
    - `PENDING_UPDATE`
    - `PENDING_CANCEL`
- An order is **open** when in one of the following (non-terminal) status:
    - `ACCEPTED`
    - `TRIGGERED`
    - `PENDING_UPDATE`
    - `PENDING_CANCEL`
    - `PARTIALLY_FILLED`
- An order is **closed** when in one of the following (terminal) status:
    - `DENIED`
    - `REJECTED`
    - `CANCELED`
    - `EXPIRED`
    - `FILLED`

## Execution instructions

Certain exchanges allow a trader to specify conditions and restrictions on
how an order will be processed and executed. The following is a brief
summary of the different execution instructions available.

### Time in force

The order's time in force specifies how long the order will remain open or active before any
remaining quantity is canceled.

- `GTC` **(Good-Till-Canceled)**: The order remains active until canceled by the trader or the exchange.
- `IOC` **(Immediate-Or-Cancel / Fill-And-Kill)**: The order executes immediately, with any unfilled portion canceled.
- `FOK` **(Fill-Or-Kill)**: The order executes immediately in full or not at all.
- `GTD` **(Good-Till-Date/Time)**: The order remains active until a specified expiration date and time.
- `DAY` **(Good for session/day)**: The order remains active until the end of the current trading session.
- `AT_THE_OPEN` **(OPG)**: The order is only active at the open of the trading session.
- `AT_THE_CLOSE`: The order is only active at the close of the trading session.

### Expire time

This instruction is to be used in conjunction with the `GTD` time in force to specify the time
at which the order will expire and be removed from the exchanges order book (or order management system).

### Post-only

An order which is marked as `post_only` will only ever participate in providing liquidity to the 
limit order book, and never initiating a trade which takes liquidity as an aggressor. This option is
important for market makers, or traders seeking to restrict the order to a liquidity _maker_ fee tier.

### Reduce-only

An order which is set as `reduce_only` will only ever reduce an existing position on an instrument, and
never open a new position (if already flat). The exact behavior of this instruction can vary between
exchanges, however the behavior as per the Nautilus `SimulatedExchange` is typical of a live exchange.

- Order will be canceled if the associated position is closed (becomes flat)
- Order quantity will be reduced as the associated positions size reduces

### Display quantity

The `display_qty` specifies the portion of a _Limit_ order which is displayed on the limit order book.
These are also known as iceberg orders as there is a visible portion to be displayed, with more quantity which is hidden. 
Specifying a display quantity of zero is also equivalent to setting an order as `hidden`.

### Trigger type

Also known as [trigger method](https://guides.interactivebrokers.com/tws/usersguidebook/configuretws/modify_the_stop_trigger_method.htm) 
which is applicable to conditional trigger orders, specifying the method of triggering the stop price.

- `DEFAULT`: The default trigger type for the exchange (typically `LAST` or `BID_ASK`)
- `LAST`: The trigger price will be based on the last traded price
- `BID_ASK`: The trigger price will be based on the `BID` for buy orders and `ASK` for sell orders
- `DOUBLE_LAST`: The trigger price will be based on the last two consecutive `LAST` prices
- `DOUBLE_BID_ASK`: The trigger price will be based on the last two consecutive `BID` or `ASK` prices as applicable
- `LAST_OR_BID_ASK`: The trigger price will be based on the `LAST` or `BID`/`ASK`
- `MID_POINT`: The trigger price will be based on the mid-point between the `BID` and `ASK`
- `MARK`: The trigger price will be based on the exchanges mark price for the instrument
- `INDEX`: The trigger price will be based on the exchanges index price for the instrument

### Trigger offset type

Applicable to conditional trailing-stop trigger orders, specifies the method of triggering modification
of the stop price based on the offset from the 'market' (bid, ask or last price as applicable).

- `DEFAULT`: The default offset type for the exchange (typically `PRICE`)
- `PRICE`: The offset is based on a price difference
- `BASIS_POINTS`: The offset is based on a price percentage difference expressed in basis points (100bp = 1%)
- `TICKS`: The offset is based on a number of ticks
- `PRICE_TIER`: The offset is based on an exchange specific price tier

### Contingent orders

More advanced relationships can be specified between orders such as assigning child order(s) which will only
trigger when the parent order is activated or filled, or linking orders together which will cancel or reduce in quantity
contingent on each other. More documentation for these options can be found in the [advanced order guide](advanced/advanced_orders.md).

## Order factory

The easiest way to create new orders is by using the built-in `OrderFactory`, which is
automatically attached to every `Strategy` class. This factory will take care
of lower level details - such as ensuring the correct trader ID and strategy ID are assigned, generation
of a necessary initialization ID and timestamp, and abstracts away parameters which don't necessarily
apply to the order type being created, or are only needed to specify more advanced execution instructions. 

This leaves the factory with simpler order creation methods to work with, all the
examples will leverage an `OrderFactory` from within a `Strategy` context.

:::info
See the `OrderFactory` [API Reference](../api_reference/common.md#class-orderfactory) for further details.
:::

## Order Types

The following describes the order types which are available for the platform with a code example.
Any optional parameters will be clearly marked with a comment which includes the default value.

### Market

A _Market_ order is an instruction by the trader to immediately trade
the given quantity at the best price available. You can also specify several
time in force options, and indicate whether this order is only intended to reduce
a position.

In the following example we create a _Market_ order on the Interactive Brokers [IdealPro](https://ibkr.info/node/1708) Forex ECN
to BUY 100,000 AUD using USD:

```python
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import MarketOrder

order: MarketOrder = self.order_factory.market(
    instrument_id=InstrumentId.from_str("AUD/USD.IDEALPRO"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(100_000),
    time_in_force=TimeInForce.IOC,  # <-- optional (default GTC)
    reduce_only=False,  # <-- optional (default False)
    tags=["ENTRY"],  # <-- optional (default None)
)
```

:::info
See the `MarketOrder` [API Reference](../api_reference/model/orders.md#class-marketorder) for further details.
:::

### Limit

A _Limit_ order is placed on the limit order book at a specific price, and will only
execute at that price (or better).

In the following example we create a _Limit_ order on the Binance Futures Crypto exchange to SELL 20 ETHUSDT-PERP Perpetual Futures
contracts at a limit price of 5000 USDT, as a market maker.

```python
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder

order: LimitOrder = self.order_factory.limit(
    instrument_id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    order_side=OrderSide.SELL,
    quantity=Quantity.from_int(20),
    price=Price.from_str("5_000.00"),
    time_in_force=TimeInForce.GTC,  # <-- optional (default GTC)
    expire_time=None,  # <-- optional (default None)
    post_only=True,  # <-- optional (default False)
    reduce_only=False,  # <-- optional (default False)
    display_qty=None,  # <-- optional (default None which indicates full display)
    tags=None,  # <-- optional (default None)
)
```

:::info
See the `LimitOrder` [API Reference](../api_reference/model/orders.md#class-limitorder) for further details.
:::

### Stop-Market

A _Stop-Market_ order is a conditional order which once triggered, will immediately
place a _Market_ order. This order type is often used as a stop-loss to limit losses, either
as a SELL order against LONG positions, or as a BUY order against SHORT positions.

In the following example we create a _Stop-Market_ order on the Binance Spot/Margin exchange 
to SELL 1 BTC at a trigger price of 100,000 USDT, active until further notice:

```python
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import StopMarketOrder

order: StopMarketOrder = self.order_factory.stop_market(
    instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE"),
    order_side=OrderSide.SELL,
    quantity=Quantity.from_int(1),
    trigger_price=Price.from_int(100_000),
    trigger_type=TriggerType.LAST_TRADE,  # <-- optional (default DEFAULT)
    time_in_force=TimeInForce.GTC,  # <-- optional (default GTC)
    expire_time=None,  # <-- optional (default None)
    reduce_only=False,  # <-- optional (default False)
    tags=None,  # <-- optional (default None)
)
```

:::info
See the `StopMarketOrder` [API Reference](../api_reference/model/orders.md#class-stopmarketorder) for further details.
:::

### Stop-Limit

A _Stop-Limit_ order is a conditional order which once triggered will immediately place
a _Limit_ order at the specified price. 

In the following example we create a _Stop-Limit_ order on the Currenex FX ECN to BUY 50,000 GBP at a limit price of 1.3000 USD
once the market hits the trigger price of 1.30010 USD, active until midday 6th June, 2022 (UTC):

```python
import pandas as pd
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import StopLimitOrder

order: StopLimitOrder = self.order_factory.stop_limit(
    instrument_id=InstrumentId.from_str("GBP/USD.CURRENEX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(50_000),
    price=Price.from_str("1.30000"),
    trigger_price=Price.from_str("1.30010"),
    trigger_type=TriggerType.BID,  # <-- optional (default DEFAULT)
    time_in_force=TimeInForce.GTD,  # <-- optional (default GTC)
    expire_time=pd.Timestamp("2022-06-06T12:00"),
    post_only=True,  # <-- optional (default False)
    reduce_only=False,  # <-- optional (default False)
    tags=None,  # <-- optional (default None)
)
```

:::info
See the `StopLimitOrder` [API Reference](../api_reference/model/orders.md#class-stoplimitorder) for further details.
:::

### Market-To-Limit

A _Market-To-Limit_ order is submitted as a market order to execute at the current best market price. 
If the order is only partially filled, the remainder of the order is canceled and re-submitted as a _Limit_ order with 
the limit price equal to the price at which the filled portion of the order executed.

In the following example we create a _Market-To-Limit_ order on the Interactive Brokers [IdealPro](https://ibkr.info/node/1708) Forex ECN
to BUY 200,000 USD using JPY:

```python
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import MarketToLimitOrder

order: MarketToLimitOrder = self.order_factory.market_to_limit(
    instrument_id=InstrumentId.from_str("USD/JPY.IDEALPRO"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(200_000),
    time_in_force=TimeInForce.GTC,  # <-- optional (default GTC)
    reduce_only=False,  # <-- optional (default False)
    display_qty=None,  # <-- optional (default None which indicates full display)
    tags=None,  # <-- optional (default None)
)
```

:::info
See the `MarketToLimitOrder` [API Reference](../api_reference/model/orders.md#class-markettolimitorder) for further details.
:::

### Market-If-Touched

A _Market-If-Touched_ order is a conditional order which once triggered will immediately
place a _Market_ order. This order type is often used to enter a new position on a stop price in the market orders direction,
or to take profits for an existing position, either as a SELL order against LONG positions, 
or as a BUY order against SHORT positions.

In the following example we create a _Market-If-Touched_ order on the Binance Futures exchange
to SELL 10 ETHUSDT-PERP Perpetual Futures contracts at a trigger price of 10,000 USDT, active until further notice:

```python
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import MarketIfTouchedOrder

order: MarketIfTouchedOrder = self.order_factory.market_if_touched(
    instrument_id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    order_side=OrderSide.SELL,
    quantity=Quantity.from_int(10),
    trigger_price=Price.from_str("10_000.00"),
    trigger_type=TriggerType.LAST_TRADE,  # <-- optional (default DEFAULT)
    time_in_force=TimeInForce.GTC,  # <-- optional (default GTC)
    expire_time=None,  # <-- optional (default None)
    reduce_only=False,  # <-- optional (default False)
    tags=["ENTRY"],  # <-- optional (default None)
)
```

:::info
See the `MarketIfTouchedOrder` [API Reference](../api_reference/model/orders.md#class-marketiftouchedorder) for further details.
:::

### Limit-If-Touched

A _Limit-If-Touched_ order is a conditional order which once triggered will immediately place
a _Limit_ order at the specified price. 

In the following example we create a _Stop-Limit_ order to BUY 5 BTCUSDT-PERP Perpetual Futures contracts on the
Binance Futures exchange at a limit price of 30,100 USDT (once the market hits the trigger price of 30,150 USDT),
active until midday 6th June, 2022 (UTC):

```python
import pandas as pd
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import StopLimitOrder

order: StopLimitOrder = self.order_factory.limit_if_touched(
    instrument_id=InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(5),
    price=Price.from_str("30_100"),
    trigger_price=Price.from_str("30_150"),
    trigger_type=TriggerType.LAST_TRADE,  # <-- optional (default DEFAULT)
    time_in_force=TimeInForce.GTD,  # <-- optional (default GTC)
    expire_time=pd.Timestamp("2022-06-06T12:00"),
    post_only=True,  # <-- optional (default False)
    reduce_only=False,  # <-- optional (default False)
    tags=["TAKE_PROFIT"],  # <-- optional (default None)
)
```

:::info
See the `StopLimitOrder` [API Reference](../api_reference/model/orders.md#class-stoplimitorder-1) for further details.
:::

### Trailing-Stop-Market

A _Trailing-Stop-Market_ order is a conditional order which trails a stop trigger price
a fixed offset away from the defined market price. Once triggered a _Market_ order will
immediately be placed.

In the following example we create a _Trailing-Stop-Market_ order on the Binance Futures exchange to SELL 10 ETHUSD-PERP COIN_M margined
Perpetual Futures Contracts activating at a trigger price of 5,000 USD, then trailing at an offset of 1% (in basis points) away from the current last traded price:

```python
import pandas as pd
from decimal import Decimal
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import TrailingStopMarketOrder

order: TrailingStopMarketOrder = self.order_factory.trailing_stop_market(
    instrument_id=InstrumentId.from_str("ETHUSD-PERP.BINANCE"),
    order_side=OrderSide.SELL,
    quantity=Quantity.from_int(10),
    trigger_price=Price.from_str("5_000"),
    trigger_type=TriggerType.LAST_TRADE,  # <-- optional (default DEFAULT)
    trailing_offset=Decimal(100),
    trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
    time_in_force=TimeInForce.GTC,  # <-- optional (default GTC)
    expire_time=None,  # <-- optional (default None)
    reduce_only=True,  # <-- optional (default False)
    tags=["TRAILING_STOP-1"],  # <-- optional (default None)
)
```

:::info
See the `TrailingStopMarketOrder` [API Reference](../api_reference/model/orders.md#class-trailingstopmarketorder-1) for further details.
:::

### Trailing-Stop-Limit

A _Trailing-Stop-Limit_ order is a conditional order which trails a stop trigger price
a fixed offset away from the defined market price. Once triggered a _Limit_ order will
immediately be placed at the defined price (which is also updated as the market moves until triggered).

In the following example we create a _Trailing-Stop-Limit_ order on the Currenex FX ECN to BUY 1,250,000 AUD using USD 
at a limit price of 0.71000 USD, activating at 0.72000 USD then trailing at a stop offset of 0.00100 USD
away from the current ask price, active until further notice:

```python
import pandas as pd
from decimal import Decimal
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import TrailingStopLimitOrder

order: TrailingStopLimitOrder = self.order_factory.trailing_stop_limit(
    instrument_id=InstrumentId.from_str("AUD/USD.CURRENEX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(1_250_000),
    price=Price.from_str("0.71000"),
    trigger_price=Price.from_str("0.72000"),
    trigger_type=TriggerType.BID_ASK,  # <-- optional (default DEFAULT)
    limit_offset=Decimal("0.00050"),
    trailing_offset=Decimal("0.00100"),
    trailing_offset_type=TrailingOffsetType.PRICE,
    time_in_force=TimeInForce.GTC,  # <-- optional (default GTC)
    expire_time=None,  # <-- optional (default None)
    reduce_only=True,  # <-- optional (default False)
    tags=["TRAILING_STOP"],  # <-- optional (default None)
)
```

:::info
See the `TrailingStopLimitOrder` [API Reference](../api_reference/model/orders.md#class-trailingstoplimitorder-1) for further details.
:::
