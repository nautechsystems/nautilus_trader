# Market-If-Touched

`FIX OrdType <40>=J` (Market If Touched)

A *Market-If-Touched* order is a conditional order which once triggered will immediately
place a *Market* order. This order type is often used to enter a new position on a stop price,
or to take profits for an existing position, either as a SELL order against LONG positions,
or as a BUY order against SHORT positions.

## Use cases

Use a *Market-If-Touched* order to act with execution certainty when a target price is touched, such
as entering on a pullback to a level or taking profit at a target. It behaves like a stop in the
opposite direction (buying below or selling above the current market) and converts to a *Market* order
on trigger. The tradeoff matches any market execution: the touch price is not the fill price, and the
fill can slip in fast markets.

## Example

In the following example we create a *Market-If-Touched* order on the Binance Futures exchange
to SELL 10 ETHUSDT-PERP Perpetual Futures contracts at a trigger price of 10,000 USDT, active until further notice:

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce, TriggerType},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use ustr::Ustr;

let order = self.order().market_if_touched(
    InstrumentId::from("ETHUSDT-PERP.BINANCE"),
    OrderSide::Sell,
    Quantity::from(10),
    Price::from("10000.00"),
    Some(TriggerType::LastPrice),    // optional (default DEFAULT)
    Some(TimeInForce::Gtc),          // optional (default GTC)
    None,                            // expire_time
    Some(false),                     // reduce_only (default false)
    None,                            // quote_quantity (default false)
    None,                            // emulation_trigger
    None,                            // trigger_instrument_id
    None,                            // exec_algorithm_id
    None,                            // exec_algorithm_params
    Some(vec![Ustr::from("ENTRY")]), // tags
    None,                            // client_order_id
);
```

```python tab="Python"
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model.orders import MarketIfTouchedOrder

order: MarketIfTouchedOrder = self.order_factory.market_if_touched(
    instrument_id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    order_side=OrderSide.SELL,
    quantity=Quantity.from_int(10),
    trigger_price=Price.from_str("10_000.00"),
    trigger_type=TriggerType.LAST_PRICE,  # <-- optional (default DEFAULT)
    time_in_force=TimeInForce.GTC,  # <-- optional (default GTC)
    expire_time=None,  # <-- optional (default None)
    reduce_only=False,  # <-- optional (default False)
    tags=["ENTRY"],  # <-- optional (default None)
)
```

See the [`MarketIfTouchedOrder` API Reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.orders.market_if_touched.MarketIfTouchedOrder) for further details.

## Related guides

- [Orders](index.md#trigger-type) - Trigger types and other execution instructions.
- [Emulated orders](emulated.md) - Emulating conditional orders on venues without native support.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
