# Market-To-Limit

`FIX OrdType <40>=K` (Market With Left Over as Limit)

A *Market-To-Limit* order submits as a market order at the current best price.
If the order partially fills, the system cancels the remainder and resubmits it as a *Limit* order at the executed price.

## Use cases

Use a *Market-To-Limit* order to take the liquidity available at the best price immediately, without
sweeping deeper levels at worse prices: helpful in thin books, or for larger orders where you want the
touch price but not the market impact of walking the book. The advantage is an immediate fill at the
best price with any remainder resting there as a *Limit* rather than chasing. The tradeoff is that the
unfilled remainder may sit unexecuted if the market moves away.

## Example

In the following example we create a *Market-To-Limit* order on the Interactive Brokers [IdealPro](https://ibkr.info/node/1708) Forex ECN
to BUY 200,000 USD using JPY:

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::InstrumentId,
    types::Quantity,
};

let order = self.core.order_factory().market_to_limit(
    InstrumentId::from("USD/JPY.IDEALPRO"),
    OrderSide::Buy,
    Quantity::from(200_000),
    Some(TimeInForce::Gtc), // optional (default GTC)
    None,                   // expire_time
    Some(false),            // reduce_only (default false)
    None,                   // quote_quantity (default false)
    None,                   // display_qty (default full display)
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
from nautilus_trader.model import Quantity
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

See the [`MarketToLimitOrder` API Reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.orders.market_to_limit.MarketToLimitOrder) for further details.

## Related guides

- [Orders](index.md) - Order concepts, execution instructions, and the order factory.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
