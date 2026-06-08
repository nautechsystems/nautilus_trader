# Limit-If-Touched

`FIX OrdType <40>` no dedicated value (commonly `4` Stop Limit with a favorable trigger)

A *Limit-If-Touched* order is a conditional order which once triggered will immediately place
a *Limit* order at the specified price.

## Use cases

Use a *Limit-If-Touched* order to arm a price-protected order only once a trigger is touched, for
example activating a take-profit *Limit* as price approaches a target rather than resting it early.
The advantage is conditional activation combined with a capped fill price. The tradeoff, as with a
*Stop-Limit*, is that the order may not fill if price moves through the limit after the trigger.

## Example

In the following example we create a *Limit-If-Touched* order to BUY 5 BTCUSDT-PERP Perpetual Futures contracts on the
Binance Futures exchange at a limit price of 30,100 USDT (once the market hits the trigger price of 30,150 USDT),
active until midday 6th June, 2022 (UTC):

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, TimeInForce, TriggerType},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use ustr::Ustr;

let order = self.core.order_factory().limit_if_touched(
    InstrumentId::from("BTCUSDT-PERP.BINANCE"),
    OrderSide::Buy,
    Quantity::from(5),
    Price::from("30100"),
    Price::from("30150"),
    Some(TriggerType::LastPrice), // optional (default DEFAULT)
    Some(TimeInForce::Gtd),       // optional (default GTC)
    Some(UnixNanos::from(1_654_516_800_000_000_000_u64)), // 2022-06-06T12:00:00 UTC
    Some(true),                   // post_only (default false)
    Some(false),                  // reduce_only (default false)
    None,                         // quote_quantity (default false)
    None,                         // display_qty
    None,                         // emulation_trigger
    None,                         // trigger_instrument_id
    None,                         // exec_algorithm_id
    None,                         // exec_algorithm_params
    Some(vec![Ustr::from("TAKE_PROFIT")]), // tags
    None,                         // client_order_id
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
from nautilus_trader.model.orders import LimitIfTouchedOrder

order: LimitIfTouchedOrder = self.order_factory.limit_if_touched(
    instrument_id=InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(5),
    price=Price.from_str("30_100"),
    trigger_price=Price.from_str("30_150"),
    trigger_type=TriggerType.LAST_PRICE,  # <-- optional (default DEFAULT)
    time_in_force=TimeInForce.GTD,  # <-- optional (default GTC)
    expire_time=pd.Timestamp("2022-06-06T12:00"),
    post_only=True,  # <-- optional (default False)
    reduce_only=False,  # <-- optional (default False)
    tags=["TAKE_PROFIT"],  # <-- optional (default None)
)
```

See the [`LimitIfTouchedOrder` API Reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.orders.limit_if_touched.LimitIfTouchedOrder) for further details.

## Related guides

- [Orders](index.md#trigger-type) - Trigger types and other execution instructions.
- [Emulated orders](emulated.md) - Emulating conditional orders on venues without native support.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
