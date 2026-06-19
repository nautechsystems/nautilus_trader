# Stop-Limit

`FIX OrdType <40>=4` (Stop Limit)

A *Stop-Limit* order is a conditional order which once triggered will immediately place
a *Limit* order at the specified price.

## Use cases

Use a *Stop-Limit* order when you want a stop trigger but also a cap on the worst acceptable fill,
such as a protective exit or breakout entry where you refuse to trade beyond a price. The advantage is
price protection on the released *Limit* order. The tradeoff is the central risk versus a
*Stop-Market*: if the market gaps through both the trigger and the limit, the order may not fill at
all, leaving a position unprotected.

## Example

In the following example we create a *Stop-Limit* order on the Currenex FX ECN to BUY 50,000 GBP at a limit price of 1.3000 USD
once the market hits the trigger price of 1.30010 USD, active until midday 6th June, 2022 (UTC):

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, TimeInForce, TriggerType},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

let order = self.order().stop_limit(
    InstrumentId::from("GBP/USD.CURRENEX"),
    OrderSide::Buy,
    Quantity::from(50_000),
    Price::from("1.30000"),
    Price::from("1.30010"),
    Some(TriggerType::BidAsk), // optional (default DEFAULT)
    Some(TimeInForce::Gtd),    // optional (default GTC)
    Some(UnixNanos::from(1_654_516_800_000_000_000_u64)), // 2022-06-06T12:00:00 UTC
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
import pandas as pd
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model.orders import StopLimitOrder

order: StopLimitOrder = self.order_factory.stop_limit(
    instrument_id=InstrumentId.from_str("GBP/USD.CURRENEX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(50_000),
    price=Price.from_str("1.30000"),
    trigger_price=Price.from_str("1.30010"),
    trigger_type=TriggerType.BID_ASK,  # <-- optional (default DEFAULT)
    time_in_force=TimeInForce.GTD,  # <-- optional (default GTC)
    expire_time=pd.Timestamp("2022-06-06T12:00"),
    post_only=True,  # <-- optional (default False)
    reduce_only=False,  # <-- optional (default False)
    tags=None,  # <-- optional (default None)
)
```

See the [`StopLimitOrder` API Reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.orders.stop_limit.StopLimitOrder) for further details.

## Related guides

- [Orders](index.md#trigger-type) - Trigger types and other execution instructions.
- [Emulated orders](emulated.md) - Emulating conditional orders on venues without native support.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
