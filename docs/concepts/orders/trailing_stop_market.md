# Trailing-Stop-Market

`FIX OrdType <40>=3` (Stop) + trailing peg

A *Trailing-Stop-Market* order keeps its stop trigger a fixed offset from the specified market
price as the market moves favorably. It releases a *Market* order when triggered.

## Use cases

Use a *Trailing-Stop-Market* order to protect gains while allowing a position to continue through
favorable moves. A tight offset can trigger on ordinary volatility, while a wide offset can give
back more profit. The released *Market* order can also slip, be rejected, or remain unfilled on a
sharp reversal.

## Example

In the following example we create a *Trailing-Stop-Market* order on the Binance Futures exchange
to SELL 10 ETHUSD-PERP COIN_M margined Perpetual Futures Contracts. It activates at a price of
5,000 USD, then trails at an offset of 1% (in basis points) from the current last traded price:

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

let order = self.order().trailing_stop_market(
    InstrumentId::from("ETHUSD-PERP.BINANCE"),
    OrderSide::Sell,
    Quantity::from(10),
    Decimal::from(100),                    // trailing_offset
    Some(TrailingOffsetType::BasisPoints), // optional (default PRICE)
    Some(Price::from("5000")),             // activation_price
    None,                                  // trigger_price (materializes from the offset on the first trail)
    Some(TriggerType::LastPrice),          // optional (default DEFAULT)
    Some(TimeInForce::Gtc),                // optional (default GTC)
    None,                                  // expire_time
    Some(true),                            // reduce_only (default false)
    None,                                  // quote_quantity (default false)
    None,                                  // display_qty
    None,                                  // emulation_trigger
    None,                                  // trigger_instrument_id
    None,                                  // exec_algorithm_id
    None,                                  // exec_algorithm_params
    Some(vec![Ustr::from("TRAILING_STOP-1")]), // tags
    None,                                  // client_order_id
);
```

```python tab="Python"
from decimal import Decimal

from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TrailingOffsetType
from nautilus_trader.model import TrailingStopMarketOrder
from nautilus_trader.model import TriggerType

order: TrailingStopMarketOrder = self.order_factory.trailing_stop_market(
    instrument_id=InstrumentId.from_str("ETHUSD-PERP.BINANCE"),
    order_side=OrderSide.SELL,
    quantity=Quantity.from_int(10),
    activation_price=Price.from_str("5_000"),
    trigger_type=TriggerType.LAST_PRICE,  # <-- optional (default DEFAULT)
    trailing_offset=Decimal(100),
    trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
    time_in_force=TimeInForce.GTC,  # <-- optional (default GTC)
    expire_time=None,  # <-- optional (default None)
    reduce_only=True,  # <-- optional (default False)
    tags=["TRAILING_STOP-1"],  # <-- optional (default None)
)
```

If both `activation_price` and `trigger_price` are omitted, the order activates immediately at the
current market and its trigger price materializes from `trailing_offset` on the first update.

See the
[`TrailingStopMarketOrder` API reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.TrailingStopMarketOrder)
for further details.

## Related guides

- [Orders](index.md#trailing-offset-type) - Trigger and trailing offset types.
- [Emulated orders](emulated.md) - Emulating trailing stops on venues without native support.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
