# Instruments

The `Instrument` base class represents the core specification for any tradable financial market instrument. There are
currently a number of subclasses representing a range of asset classes and asset types which are supported by the platform:
- `CurrencySpot` (can represent both Fiat FX and Crypto)
- `CryptoPerpetual` (perpetual swap derivative)
- `BettingInstrument`
- `Equity`
- `Future`
- `Option`

All instruments should have a unique `InstrumentId` which is made up of both the native symbol and venue ID, separated by a period e.g. `ETH-PERP.FTX`.

```{warning}
The correct instrument must be matched to a market dataset such as ticks or orderbook data for logically sound operation.
An incorrectly specified instrument may truncate data or otherwise produce surprising results.
```

## Backtesting
Generic test instruments can be instantiated through the `TestInstrumentProvider`:
```python
audusd = TestInstrumentProvider.default_fx_ccy("AUD/USD")
```

Exchange specific instruments can be discovered from live exchange data using an adapters `InstrumentProvider`:
```python
provider = BinanceInstrumentProvider(
    client=binance_http_client,
    logger=live_logger,
)
await self.provider.load_all_async()

btcusdt = InstrumentId.from_str("BTC/USDT.BINANCE")
instrument: Optional[Instrument] = provider.find(btcusdt)
```

Or flexibly defined by the user through an `Instrument` constructor, or one of its more specific subclasses:
```python
instrument = Instrument(...)  # <-- provide all necessary paramaters
```
See the full instrument [API Reference](../api_reference/model/instruments.md).

## Live trading
All the live venue integration adapters have defined `InstrumentProvider` classes which work in an automated way
under the hood to cache the latest instrument details from the exchange. Refer to a particular `Instrument` object by pass the matching `InstrumentId` to data and execution
related methods and classes which require one.

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

Or subscribe to all instrument changes for an entire venue:
```python
from nautilus_trader.model.identifiers import Venue

ftx = Venue("FTX")
self.subscribe_instruments(ftx)
```

When an update to the instrument(s) is received by the `DataEngine`, the object(s) will
be passed to the strategy/actors `on_instrument()` method. A user can override this method with actions
to take upon receiving an instrument update:

```python
def on_instrument(instrument: Instrument) -> None:
    # Take some action on an instrument update
    pass
```