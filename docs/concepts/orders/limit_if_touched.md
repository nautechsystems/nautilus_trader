# Limit-If-Touched

`FIX OrdType <40>` no dedicated value (commonly `4` Stop Limit with a favorable trigger)

A *Limit-If-Touched* order releases a *Limit* order at the specified price when its trigger price is
reached.

## Use cases

Use a *Limit-If-Touched* order to activate a price-protected order only after a trigger is touched,
for example to place a take-profit *Limit* order as price approaches a target instead of resting it
early. As with a *Stop-Limit*, the order may not fill if the market moves through the limit after the
trigger.

## Example

The following example creates a *Limit-If-Touched* order to BUY 5 BTCUSDT-PERP perpetual futures
contracts on Binance Futures at a limit price of 30,100 USDT once the market reaches 30,150 USDT.
The order expires one hour after creation:

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce, TriggerType},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use ustr::Ustr;

let expire_time = self.clock().timestamp_ns() + 3_600_000_000_000_u64;
let order = self.order().limit_if_touched(
    InstrumentId::from("BTCUSDT-PERP.BINANCE"),
    OrderSide::Buy,
    Quantity::from(5),
    Price::from("30100"),
    Price::from("30150"),
    Some(TriggerType::LastPrice), // optional (default DEFAULT)
    Some(TimeInForce::Gtd),       // optional (default GTC)
    Some(expire_time),            // one hour from now
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
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import LimitIfTouchedOrder
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TriggerType

order: LimitIfTouchedOrder = self.order_factory.limit_if_touched(
    instrument_id=InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(5),
    price=Price.from_str("30_100"),
    trigger_price=Price.from_str("30_150"),
    trigger_type=TriggerType.LAST_PRICE,  # <-- optional (default DEFAULT)
    time_in_force=TimeInForce.GTD,  # <-- optional (default GTC)
    expire_time=self.clock.timestamp_ns() + 3_600_000_000_000,
    post_only=True,  # <-- optional (default False)
    reduce_only=False,  # <-- optional (default False)
    tags=["TAKE_PROFIT"],  # <-- optional (default None)
)
```

See the
[`LimitIfTouchedOrder` API reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.LimitIfTouchedOrder)
for further details.

## Related guides

- [Orders](index.md#trigger-type) - Trigger types and other execution instructions.
- [Emulated orders](emulated.md) - Emulating conditional orders on venues without native support.
- [Execution](../execution/) - How orders reach the venue and fills are handled.
