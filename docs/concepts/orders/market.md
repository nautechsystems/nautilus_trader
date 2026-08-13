# Market

`FIX OrdType <40>=1`

A *Market* order instructs the venue to trade a quantity immediately at the best available price.
It can also carry time in force and reduce‑only instructions.

## Use cases

Use a *Market* order when prompt execution matters more than the exact price, such as for urgent
risk reduction or entry into a liquid, fast‑moving market. A *Market* order has no price protection:
it can incur spread costs and slippage, and the venue can still reject it or leave it unfilled when
no market is available.

## Example

In the following example we create a *Market* order on the Interactive Brokers
[IdealPro](https://ibkr.info/node/1708) Forex ECN to BUY 100,000 AUD using USD:

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::InstrumentId,
    types::Quantity,
};
use ustr::Ustr;

let order = self.order().market(
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
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Quantity
from nautilus_trader.model import TimeInForce

order: MarketOrder = self.order_factory.market(
    instrument_id=InstrumentId.from_str("AUD/USD.IDEALPRO"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(100_000),
    time_in_force=TimeInForce.IOC,  # <-- optional (default GTC)
    reduce_only=False,  # <-- optional (default False)
    tags=["ENTRY"],  # <-- optional (default None)
)
```

See the [`MarketOrder` API reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.MarketOrder)
for further details.

## Related guides

- [Orders](index.md) - Order concepts, execution instructions, and the order factory.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
