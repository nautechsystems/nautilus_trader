# Trailing-Stop-Market

`FIX OrdType <40>=3` (Stop) + trailing peg

A *Trailing-Stop-Market* order is a conditional order which trails a stop trigger price
a fixed offset away from the defined market price. Once triggered a *Market* order will
immediately be placed.

## Use cases

Use a *Trailing-Stop-Market* order to lock in gains while letting a position run: the trigger trails
favorable moves by a fixed offset and only fires on a reversal, with no manual adjustment. The
advantage is dynamic protection plus execution certainty on trigger. The tradeoff is choosing the
offset, which balances whipsaw risk (too tight) against giving back profit (too wide), and the market
fill can still slip on a sharp reversal.

## Example

In the following example we create a *Trailing-Stop-Market* order on the Binance Futures exchange to SELL 10 ETHUSD-PERP COIN_M margined
Perpetual Futures Contracts activating at a price of 5,000 USD, then trailing at an offset of 1% (in basis points) away from the current last traded price:

```rust tab="Rust"
use nautilus_model::{
    enums::{OrderSide, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

let order = self.core.order_factory().trailing_stop_market(
    InstrumentId::from("ETHUSD-PERP.BINANCE"),
    OrderSide::Sell,
    Quantity::from(10),
    Decimal::from(100),                    // trailing_offset
    Some(TrailingOffsetType::BasisPoints), // optional (default PRICE)
    Some(Price::from("5000")),             // activation_price
    None,                                  // trigger_price (falls back to activation_price)
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
import pandas as pd
from decimal import Decimal
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model.orders import TrailingStopMarketOrder

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

See the [`TrailingStopMarketOrder` API Reference](/docs/python-api-latest/model/orders.html#nautilus_trader.model.orders.trailing_stop_market.TrailingStopMarketOrder) for further details.

## Related guides

- [Orders](index.md#trigger-offset-type) - Trigger and trailing offset types.
- [Emulated orders](emulated.md) - Emulating trailing stops on venues without native support.
- [Execution](../execution.md) - How orders reach the venue and fills are handled.
