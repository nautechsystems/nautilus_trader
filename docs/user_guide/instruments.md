# Instruments

The `Instrument` base class represents the core specification for any tradable financial market instrument. There are
currently a number of subclasses representing a range of asset classes which are supported by the platform:
- `CurrencySpot` (can represent both Fiat FX and Crypto)
- `CryptoPerpetual` (perpetual swap derivative)
- `BettingInstrument`
- `Equity`
- `Future`
- `Option`

All instruments should have a unique `InstrumentId` which is made up of both the native symbol and venue ID separated by a period e.g. `ETH-PERP.FTX`.

## Backtesting
Exchange specific concrete implementations can be instantiated through: 
- The `TestInstrumentProvider`
- Discovered from live exchange data using an adapters `InstrumentProvider`
- Flexibly defined by the user through the constructor

```{warning}
The correct instrument must be matched to a market dataset such as ticks or orderbook data for logically sound operation.
An incorrectly specified instrument may truncate data or otherwise produce surprising results.
```

## Live trading
All the live venue integration adapters have defined `InstrumentProvider` classes which work in an automated way
under the hood to cache the latest instrument details from the exchange. All that is requirement
then to get a particular `Instrument` object is to use the matching `InstrumentId` by passing it as a parameter to data and execution
related methods and classes.

## Getting instruments
Since the same strategy/actor classes can be used for both backtests and live trading, you can
get instruments in exactly the same way through the central cache:

```python
from nautilus_trader.model.identifiers import InstrumentId

instrument_id = InstrumentId.from_str("ETH/USD.FTX")
instrument = self.cache.instrument(instrument_id)
```

It's also possible to subscribe to any changes to a particular instrument:
```python
self.subscribe_instrument(instrument_id)
```

or for changes to any instrument for an entire venue:
```python
from nautilus_trader.model.identifiers import Venue

ftx = Venue("FTX")
self.subscribe_instruments(ftx)
```

When an update to the instrument(s) is received by the `DataEngine`, the object(s) will eventually
be passed to the strategy/actors `self.on_instrument()` method. A user can override this method with actions
to take upon receiving an instrument update:

```python
def on_instrument(instrument: Instrument) -> None:
    # Take some action on an instrument update
    pass
```