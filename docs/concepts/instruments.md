# Instruments

The `Instrument` base class represents the core specification for any tradable asset/contract. There are
currently a number of subclasses representing a range of _asset classes_ and _asset types_ which are supported by the platform:
- `Equity` (generic Equity)
- `Future` (generic Futures Contract)
- `Option` (generic Options Contract)
- `CurrencyPair` (represents a Fiat FX or Cryptocurrency pair in a spot/cash market)
- `CryptoPerpetual` (Perpetual Futures Contract a.k.a. Perpetual Swap)
- `CryptoFuture` (Deliverable Futures Contract with Crypto assets as underlying, and for price quotes and settlement)
- `BettingInstrument` (Sports, gaming, or other betting)

## Symbology
All instruments should have a unique `InstrumentId`, which is made up of both the native symbol, and venue ID, separated by a period.
For example, on the Binance Futures crypto exchange, the Ethereum Perpetual Futures Contract has the instrument ID `ETHUSDT-PERP.BINANCE`.

All native symbols _should_ be unique for a venue (this is not always the case e.g. Binance share native symbols between spot and futures markets), 
and the `{symbol.venue}` combination _must_ be unique for a Nautilus system.

```{warning}
The correct instrument must be matched to a market dataset such as ticks or order book data for logically sound operation.
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

btcusdt = InstrumentId.from_str("BTCUSDT.BINANCE")
instrument = provider.find(btcusdt)
```

Or flexibly defined by the user through an `Instrument` constructor, or one of its more specific subclasses:

```python
instrument = Instrument(...)  # <-- provide all necessary parameters
```
See the full instrument [API Reference](../api_reference/model/instruments.md).

## Live trading
Live integration adapters have defined `InstrumentProvider` classes which work in an automated way to cache the 
latest instrument definitions for the exchange. Refer to a particular `Instrument` 
object by pass the matching `InstrumentId` to data and execution related methods, and classes which require one.

## Finding instruments
Since the same actor/strategy classes can be used for both backtest and live trading, you can
get instruments in exactly the same way through the central cache:

```python
from nautilus_trader.model.identifiers import InstrumentId

instrument_id = InstrumentId.from_str("ETHUSDT-PERP.BINANCE")
instrument = self.cache.instrument(instrument_id)
```

It's also possible to subscribe to any changes to a particular instrument:
```python
self.subscribe_instrument(instrument_id)
```

Or subscribe to all instrument changes for an entire venue:
```python
from nautilus_trader.model.identifiers import Venue

binance = Venue("BINANCE")
self.subscribe_instruments(binance)
```

When an update to the instrument(s) is received by the `DataEngine`, the object(s) will
be passed to the actors/strategies `on_instrument()` method. A user can override this method with actions
to take upon receiving an instrument update:

```python
def on_instrument(instrument: Instrument) -> None:
    # Take some action on an instrument update
    pass
```

## Precisions and Increments
The instrument objects are a convenient way to organize the specification of an
instrument through _read-only_ properties. Correct price and quantity precisions, as well as 
minimum price and size increments, multipliers and standard lot sizes, are available.

```{note}
Most of these limits are checked by the Nautilus `RiskEngine`, otherwise invalid
values for prices and quantities _can_ result in the exchange rejecting orders.
```

## Limits
Certain value limits are optional for instruments and can be `None`, these are exchange
dependent and can include:
- `max_quantity` (maximum quantity for a single order)
- `min_quantity` (minimum quantity for a single order)
- `max_notional` (maximum value of a single order)
- `min_notional` (minimum value of a single order)
- `max_price` (maximum valid quote or order price)
- `min_price` (minimum valid quote or order price)

```{note}
Most of these limits are checked by the Nautilus `RiskEngine`, otherwise exceeding
published limits _can_ result in the exchange rejecting orders.
```

## Prices and Quantities
Instrument objects also offer a convenient way to create correct prices
and quantities based on given values.

```python
instrument = self.cache.instrument(instrument_id)

price = instrument.make_price(0.90500)
quantity = instrument.make_qty(150)
```

```{tip}
The above is the recommended method for creating valid prices and quantities, 
such as when passing them to the order factory to create an order.
```

## Margins and Fees
The current initial and maintenance margin requirements, as well as any trading 
fees are also available from an instrument:
- `margin_init` (initial/order margin rate)
- `margin_maint` (maintenance/position margin rate)
- `maker_fee` (the fee percentage applied to notional order values when providing liquidity)
- `taker_fee` (the fee percentage applied to notional order values when demanding liquidity)

## Additional Info
The raw instrument definition as provided by the exchange (typically from JSON serialized data) is also
included as a generic Python dictionary. This is to retain all information
which is not necessarily part of the unified Nautilus API, and is available to the user
at runtime by calling the `.info` property.
