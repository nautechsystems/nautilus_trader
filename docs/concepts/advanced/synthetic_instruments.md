# Synthetic Instruments

The platform supports the definition of customized synthetic instruments. 
These instruments can generate synthetic quote and trade ticks, which are beneficial for:

- Allowing actors (and strategies) to subscribe to quote or trade feeds (for any purpose)
- Facilitating the triggering of emulated orders
- Constructing bars from synthetic quotes or trades

Synthetic instruments cannot be traded directly, as they are constructs that only exist locally 
within the platform. However, the synthetic instrument serves as an analytical tool providing 
useful metrics based on its component instruments.

In the future, we plan to support order management for synthetic instruments, which would involve 
trading their component instruments based on the behavior of the synthetic instrument.

```{note}
Note that the venue for a synthetic instrument is always designated as `'SYNTH'`.
```

## Formula
A synthetic instrument is composed of a combination of two or more component instruments (which
can include instruments from multiple venues), as well as a "derivation formula". 
Utilizing the dynamic expression engine powered by the [evalexpr](https://github.com/ISibboI/evalexpr) 
Rust library, the platform can evaluate the formula to calculate the latest synthetic price tick 
from the incoming component instrument prices.

See the `evalexpr` documentation for a full description of available features, operators and precedence.

```{warning}
Before defining a new synthetic instrument, ensure that all component instruments are already defined and exist in the cache.
```

## Subscribing
The following example demonstrates the creation of a new synthetic instrument with an actor/strategy. 
This synthetic instrument will represent a simple spread between Bitcoin and 
Ethereum spot prices on Binance. For this example, it is assumed that spot instruments for 
`BTCUSDT.BINANCE` and `ETHUSDT.BINANCE` are already present in the cache.

```python
from nautilus_trader.model.instruments import SyntheticInstrument

btcusdt_binance_id = InstrumentId.from_str("BTCUSDT.BINANCE")
ethusdt_binance_id = InstrumentId.from_str("ETHUSDT.BINANCE")

# Define the synthetic instrument
synthetic = SyntheticInstrument(
    symbol=Symbol("BTC-ETH:BINANCE"),
    price_precision=8,
    components=[
        btcusdt_binance_id,
        ethusdt_binance_id,
    ],
    formula=f"{btcusdt_binance_id} - {ethusdt_binance_id}",
    ts_event=self.clock.timestamp_ns(),
    ts_init=self.clock.timestamp_ns(),
)

# Recommended to store the synthetic instruments ID somewhere
self._synthetic_id = synthetic.id

# Add the synthetic instrument for use by other components
self.add_synthetic(synthetic)

# Subscribe to quote ticks for the synthetic instrument
self.subscribe_quote_ticks(self._synthetic_id)
```

```{note}
The `instrument_id` for the synthetic instrument in the above example will be structured as `{symbol}.{SYNTH}`, resulting in 'BTC-ETH:BINANCE.SYNTH'.
```

## Updating formulas
It's also possible to update a synthetic instruments formula at any time. The following examples
shows up to achieve this with an actor/strategy.

```
# Recover the synthetic instrument from the cache (assuming `synthetic_id` was assigned)
synthetic = self.cache.synthetic(self._synthetic_id)

# Update the formula, here is a simple example of just taking the average
new_formula = "(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2"
synthetic.change_formula(new_formula)

# Now update the synthetic instrument
self.update_synthetic(synthetic)
```

## Trigger instrument IDs
The platform allows for emulated orders to be triggered based on synthetic instrument prices. In 
the following example, we build upon the previous one to submit a new emulated order. 
This order will be retained in the emulator until a trigger from synthetic quote ticks releases it. 
It will then be submitted to Binance as a MARKET order:

```
order = self.strategy.order_factory.limit(
    instrument_id=ETHUSDT_BINANCE.id,
    order_side=OrderSide.BUY,
    quantity=Quantity.from_str("1.5"),
    price=Price.from_str("30000.00000000"),  # <-- Synthetic instrument price
    emulation_trigger=TriggerType.DEFAULT,
    trigger_instrument_id=self._synthetic_id,  # <-- Synthetic instrument identifier
)

self.strategy.submit_order(order)
```

## Error handling
Considerable effort has been made to validate inputs, including the derivation formula for 
synthetic instruments. Despite this, caution is advised as invalid or erroneous inputs may lead to 
undefined behavior. 

Refer to the `SyntheticInstrument` [API reference](https://docs.nautilustrader.io/api_reference/model/instruments.html#nautilus_trader.model.instruments.synthetic.SyntheticInstrument)
for a detailed understanding of input requirements and potential exceptions.
