# Limit

`FIX OrdType <40>=2`

A *Limit* order is placed on the limit order book at a specific price, and will only
execute at that price (or better).

## Use cases

Use a *Limit* order when you want to control the execution price, and optionally provide liquidity:
market making, scaling into or out of a position at chosen levels, or capturing maker fee tiers with
`post_only`. The advantage is that it never fills worse than your price. The tradeoff is no execution
guarantee: the order may rest unfilled, or only partially fill, if the market never reaches or holds
your price.

## Example

In the following example we create a *Limit* order on the Binance Futures Crypto exchange to SELL 20 ETHUSDT-PERP Perpetual Futures
contracts at a limit price of 5000 USDT, as a market maker.

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

let order = self.core.order_factory().limit(
    InstrumentId::from("ETHUSDT-PERP.BINANCE"),
    OrderSide::Sell,
    Quantity::from(20),
    Price::from("5000.00"),
    Some(TimeInForce::Gtc), // optional (default GTC)
    None,                   // expire_time
    Some(true),             // post_only (default false)
    Some(false),            // reduce_only (default false)
    None,                   // quote_quantity (default false)
    None,                   // display_qty (default full display)
    None,                   // emulation_trigger
    None,                   // trigger_instrument_id
    None,                   // exec_algorithm_id
    None,                   // exec_algorithm_params
    None,                   // tags
    None,                   // client_order_id
);
```

```python tab="Python"
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
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

See the [`LimitOrder` API Reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.orders.limit.LimitOrder) for further details.

## Related guides

- [Orders](index.md) - Order concepts, execution instructions, and the order factory.
- [Emulated orders](emulated.md) - Emulating *Limit* orders, released as *Market* orders on trigger.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
