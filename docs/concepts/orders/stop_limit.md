# Stop-Limit

`FIX OrdType <40>=4` (Stop Limit)

A *Stop-Limit* order releases a *Limit* order at the specified price when its trigger price is
reached.

## Use cases

Use a *Stop-Limit* order when a stop trigger must also enforce a worst acceptable fill price, such as
for a price-protected exit or breakout entry. If the market gaps through both the trigger and limit,
the order may not fill and can leave a position unprotected.

## Example

The following example creates a *Stop-Limit* order on the Currenex FX ECN to BUY 50,000 GBP at a
limit price of 1.3000 USD once the market reaches 1.30010 USD. The order expires one hour after
creation:

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce, TriggerType},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

let expire_time = self.clock().timestamp_ns() + 3_600_000_000_000_u64;
let order = self.order().stop_limit(
    InstrumentId::from("GBP/USD.CURRENEX"),
    OrderSide::Buy,
    Quantity::from(50_000),
    Price::from("1.30000"),
    Price::from("1.30010"),
    Some(TriggerType::BidAsk), // optional (default DEFAULT)
    Some(TimeInForce::Gtd),    // optional (default GTC)
    Some(expire_time),         // one hour from now
    Some(true),                // post_only (default false)
    Some(false),               // reduce_only (default false)
    None,                      // quote_quantity (default false)
    None,                      // display_qty
    None,                      // emulation_trigger
    None,                      // trigger_instrument_id
    None,                      // exec_algorithm_id
    None,                      // exec_algorithm_params
    None,                      // tags
    None,                      // client_order_id
);
```

```python tab="Python"
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StopLimitOrder
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TriggerType

order: StopLimitOrder = self.order_factory.stop_limit(
    instrument_id=InstrumentId.from_str("GBP/USD.CURRENEX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(50_000),
    price=Price.from_str("1.30000"),
    trigger_price=Price.from_str("1.30010"),
    trigger_type=TriggerType.BID_ASK,  # <-- optional (default DEFAULT)
    time_in_force=TimeInForce.GTD,  # <-- optional (default GTC)
    expire_time=self.clock.timestamp_ns() + 3_600_000_000_000,
    post_only=True,  # <-- optional (default False)
    reduce_only=False,  # <-- optional (default False)
    tags=None,  # <-- optional (default None)
)
```

See the
[`StopLimitOrder` API reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.StopLimitOrder)
for further details.

## Related guides

- [Orders](index.md#trigger-type) - Trigger types and other execution instructions.
- [Emulated orders](emulated.md) - Emulating conditional orders on venues without native support.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
