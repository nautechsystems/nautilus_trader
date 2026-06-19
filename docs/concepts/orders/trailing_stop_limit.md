# Trailing-Stop-Limit

`FIX OrdType <40>=4` (Stop Limit) + trailing peg

A *Trailing-Stop-Limit* order is a conditional order which trails a stop trigger price
a fixed offset away from the defined market price. Once triggered a *Limit* order will
immediately be placed at the defined price (which is also updated as the market moves until triggered).

## Use cases

Use a *Trailing-Stop-Limit* order when you want the dynamic trail of a trailing stop but also a cap on
the fill price. The advantage is trailing protection combined with price control. The tradeoff is the
trailing analogue of a *Stop-Limit*: in a fast reversal the released *Limit* may not fill, leaving the
position open.

## Example

In the following example we create a *Trailing-Stop-Limit* order on the Currenex FX ECN to BUY 1,250,000 AUD using USD
at a limit price of 0.71000 USD, activating at 0.72000 USD then trailing at a stop offset of 0.00100 USD
away from the current ask price, active until further notice:

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use rust_decimal_macros::dec;
use ustr::Ustr;

let order = self.order().trailing_stop_limit(
    InstrumentId::from("AUD/USD.CURRENEX"),
    OrderSide::Buy,
    Quantity::from(1_250_000),
    Price::from("0.71000"),          // limit price
    dec!(0.00050),                   // limit_offset
    dec!(0.00100),                   // trailing_offset
    Some(TrailingOffsetType::Price), // optional (default PRICE)
    Some(Price::from("0.72000")),    // activation_price
    None,                            // trigger_price (falls back to activation_price)
    Some(TriggerType::BidAsk),       // optional (default DEFAULT)
    Some(TimeInForce::Gtc),          // optional (default GTC)
    None,                            // expire_time
    Some(false),                     // post_only (default false)
    Some(true),                      // reduce_only (default false)
    None,                            // quote_quantity (default false)
    None,                            // display_qty
    None,                            // emulation_trigger
    None,                            // trigger_instrument_id
    None,                            // exec_algorithm_id
    None,                            // exec_algorithm_params
    Some(vec![Ustr::from("TRAILING_STOP")]), // tags
    None,                            // client_order_id
);
```

```python tab="Python"
import pandas as pd
from decimal import Decimal
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model.orders import TrailingStopLimitOrder

order: TrailingStopLimitOrder = self.order_factory.trailing_stop_limit(
    instrument_id=InstrumentId.from_str("AUD/USD.CURRENEX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(1_250_000),
    price=Price.from_str("0.71000"),
    activation_price=Price.from_str("0.72000"),
    trigger_type=TriggerType.BID_ASK,  # <-- optional (default DEFAULT)
    limit_offset=Decimal("0.00050"),
    trailing_offset=Decimal("0.00100"),
    trailing_offset_type=TrailingOffsetType.PRICE,
    time_in_force=TimeInForce.GTC,  # <-- optional (default GTC)
    expire_time=None,  # <-- optional (default None)
    reduce_only=True,  # <-- optional (default False)
    tags=["TRAILING_STOP"],  # <-- optional (default None)
)
```

See the [`TrailingStopLimitOrder` API Reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.orders.trailing_stop_limit.TrailingStopLimitOrder) for further details.

## Related guides

- [Orders](index.md#trigger-offset-type) - Trigger and trailing offset types.
- [Emulated orders](emulated.md) - Emulating trailing stops on venues without native support.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
