# Market

`FIX OrdType <40>=1`

A *Market* order is an instruction by the trader to immediately trade
the given quantity at the best price available. You can also specify several
time in force options, and indicate whether this order is only intended to reduce
a position.

## Use cases

Use a *Market* order when filling matters more than the exact price: urgent risk reduction,
entering a fast-moving liquid market, or crossing a tight spread where waiting costs more than the
spread. The advantage is near-certain, immediate execution. The tradeoff is no price protection:
you pay the spread and risk slippage in thin or fast markets, so it suits liquid instruments far
more than illiquid ones.

## Example

In the following example we create a *Market* order on the Interactive Brokers [IdealPro](https://ibkr.info/node/1708) Forex ECN
to BUY 100,000 AUD using USD:

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::InstrumentId,
    types::Quantity,
};
use ustr::Ustr;

let order = self.core.order_factory().market(
    InstrumentId::from("AUD/USD.IDEALPRO"),
    OrderSide::Buy,
    Quantity::from(100_000),
    Some(TimeInForce::Ioc),          // optional (default GTC)
    Some(false),                     // reduce_only (default false)
    None,                            // quote_quantity (default false)
    None,                            // exec_algorithm_id
    None,                            // exec_algorithm_params
    Some(vec![Ustr::from("ENTRY")]), // tags
    None,                            // client_order_id (auto-generated if None)
);
```

```python tab="Python"
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
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

See the [`MarketOrder` API Reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.orders.market.MarketOrder) for further details.

## Related guides

- [Orders](index.md) - Order concepts, execution instructions, and the order factory.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
