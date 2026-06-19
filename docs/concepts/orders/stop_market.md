# Stop-Market

`FIX OrdType <40>=3` (Stop)

A *Stop-Market* order is a conditional order which once triggered, will immediately
place a *Market* order. This order type is often used as a stop-loss to limit losses, either
as a SELL order against LONG positions, or as a BUY order against SHORT positions.

## Use cases

Use a *Stop-Market* order when you need execution certainty once a price level is breached, such as a
protective stop-loss or a breakout entry. Because it converts to a *Market* order on trigger, the
position is almost always opened or closed. The tradeoff is that the trigger price is not the fill
price: in fast or gapping markets the fill can land well beyond the stop, so it trades price certainty
for execution certainty (the opposite of a *Stop-Limit*).

## Example

In the following example we create a *Stop-Market* order on the Binance Spot/Margin exchange
to SELL 1 BTC at a trigger price of 100,000 USDT, active until further notice:

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce, TriggerType},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

let order = self.order().stop_market(
    InstrumentId::from("BTCUSDT.BINANCE"),
    OrderSide::Sell,
    Quantity::from(1),
    Price::from("100000"),
    Some(TriggerType::LastPrice), // optional (default DEFAULT)
    Some(TimeInForce::Gtc),       // optional (default GTC)
    None,                         // expire_time
    Some(false),                  // reduce_only (default false)
    None,                         // quote_quantity (default false)
    None,                         // display_qty
    None,                         // emulation_trigger
    None,                         // trigger_instrument_id
    None,                         // exec_algorithm_id
    None,                         // exec_algorithm_params
    None,                         // tags
    None,                         // client_order_id
);
```

```python tab="Python"
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model.orders import StopMarketOrder

order: StopMarketOrder = self.order_factory.stop_market(
    instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE"),
    order_side=OrderSide.SELL,
    quantity=Quantity.from_int(1),
    trigger_price=Price.from_int(100_000),
    trigger_type=TriggerType.LAST_PRICE,  # <-- optional (default DEFAULT)
    time_in_force=TimeInForce.GTC,  # <-- optional (default GTC)
    expire_time=None,  # <-- optional (default None)
    reduce_only=False,  # <-- optional (default False)
    tags=None,  # <-- optional (default None)
)
```

See the [`StopMarketOrder` API Reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.orders.stop_market.StopMarketOrder) for further details.

## Related guides

- [Orders](index.md#trigger-type) - Trigger types and other execution instructions.
- [Emulated orders](emulated.md) - Emulating conditional orders on venues without native support.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
