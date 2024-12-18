# Instruments

The `Instrument` base class represents the core specification for any tradable asset/contract. There are
currently a number of subclasses representing a range of _asset classes_ and _instrument classes_ which are supported by the platform:
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

:::warning
The correct instrument must be matched to a market dataset such as ticks or order book data for logically sound operation.
An incorrectly specified instrument may truncate data or otherwise produce surprising results.
:::

## Backtesting

Generic test instruments can be instantiated through the `TestInstrumentProvider`:

```python
from nautilus_trader.test_kit.providers import TestInstrumentProvider

audusd = TestInstrumentProvider.default_fx_ccy("AUD/USD")
```

Exchange specific instruments can be discovered from live exchange data using an adapters `InstrumentProvider`:

```python
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.model import InstrumentId

provider = BinanceSpotInstrumentProvider(client=binance_http_client)
await self.provider.load_all_async()

btcusdt = InstrumentId.from_str("BTCUSDT.BINANCE")
instrument = provider.find(btcusdt)
```

Or flexibly defined by the user through an `Instrument` constructor, or one of its more specific subclasses:

```python
from nautilus_trader.model.instruments import Instrument

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
from nautilus_trader.model import InstrumentId

instrument_id = InstrumentId.from_str("ETHUSDT-PERP.BINANCE")
instrument = self.cache.instrument(instrument_id)
```

It's also possible to subscribe to any changes to a particular instrument:
```python
self.subscribe_instrument(instrument_id)
```

Or subscribe to all instrument changes for an entire venue:
```python
from nautilus_trader.model import Venue

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

## Precisions and increments

The instrument objects are a convenient way to organize the specification of an
instrument through _read-only_ properties. Correct price and quantity precisions, as well as
minimum price and size increments, multipliers and standard lot sizes, are available.

:::note
Most of these limits are checked by the Nautilus `RiskEngine`, otherwise invalid
values for prices and quantities _can_ result in the exchange rejecting orders.
:::

## Limits

Certain value limits are optional for instruments and can be `None`, these are exchange
dependent and can include:
- `max_quantity` (maximum quantity for a single order)
- `min_quantity` (minimum quantity for a single order)
- `max_notional` (maximum value of a single order)
- `min_notional` (minimum value of a single order)
- `max_price` (maximum valid quote or order price)
- `min_price` (minimum valid quote or order price)

:::note
Most of these limits are checked by the Nautilus `RiskEngine`, otherwise exceeding
published limits _can_ result in the exchange rejecting orders.
:::

## Prices and quantities

Instrument objects also offer a convenient way to create correct prices
and quantities based on given values.

```python
instrument = self.cache.instrument(instrument_id)

price = instrument.make_price(0.90500)
quantity = instrument.make_qty(150)
```

:::tip
The above is the recommended method for creating valid prices and quantities,
such as when passing them to the order factory to create an order.
:::

## Margins and fees

Margin calculations are handled by the `MarginAccount` class. Let's explore how margins work in our trading system and understand all the key concepts.

### When margins apply?

Each exchange (like CME or Binance) operates with a specific account type that determines whether margin calculations are relevant for your trading. When setting up an exchange connection, you'll specify one of these account types:
- `AccountType.MARGIN`: Uses margin calculations we'll discuss below
- `AccountType.CASH`: Simple accounts where margins don't apply
- `AccountType.BETTING`: Also doesn't use margin calculations

### Vocabulary

Let's start with the key terms you'll need to understand trading on margin:

**Notional Value** is the full contract value in base currency. The instrument's notional value represents the total market value of your position. For example, with EUR/USD futures on CME (symbol 6E):
- Each contract represents 125,000 EUR (EUR is quote currency, USD is base currency)
- When the current market price is 1.1000, then the notional value equals 125,000 EUR × `1.1000` (price of EUR/USD) = 137,500 USD

**Leverage** (`leverage`) is an attribute of your trading account that defines how much market exposure you can control with your account deposit. For example, with 10× leverage, you can control 10,000 USD worth of positions with just 1,000 USD in your account.

**Initial Margin** (`margin_init`) is the initial margin rate required to open positions. It represents the amount of money that must be available in your account to open new positions. This is just an entry check requirement - no money is actually locked in your account.

**Maintenance Margin** (`margin_maint`) is the maintenance margin rate to keep positions open. This is the amount of money that gets locked and must remain in your account to keep positions open. It's always lower than the initial margin, and you can view all blocked funds (sum of maintenance margins) from open positions using `self.portfolio.balances_locked(venue)` in your strategy.

**Maker/Taker Fees** describe how exchanges charge trading fees based on how you interact with the market. When you place an order, you're either a "maker" or a "taker" of liquidity:

- **Maker Fee** (`maker_fee`): This is the lower fee you pay when you "make" liquidity by placing orders that sit on the order book. For example, if you place a limit buy order below the current market price, you're adding liquidity to the market and will pay the maker fee when your order eventually fills.
- **Taker Fee** (`taker_fee`): This is typically a higher fee charged when you "take" liquidity by placing orders that execute immediately against existing orders. For instance, if you place a market buy order or a limit buy above the current price, you're removing liquidity from the market and will pay the taker fee.

Not all exchanges / instruments implement maker/taker fees. In these cases, both `maker_fee` and `taker_fee` should be set to 0 in `Instrument` and all its subclasses (such as `FuturesContract`, `Equity`, `CurrencyPair`, `Commodity`, `Cfd`, `BinaryOption`, `BettingInstrument`, and others).

### Margin calculation formula

When examining the code in `MarginAccount`, you'll find the margin calculations follow these formulas:

```python
# Initial margin calculation
margin_init = (notional_value / leverage * margin_init) + (notional_value / leverage * taker_fee)

# Maintenance margin calculation
margin_maint = (notional_value / leverage * margin_maint) + (notional_value / leverage * taker_fee)
```

Notes:
- Notice that both formulas follow the same structure but use their respective margin rates (`margin_init` / `margin_maint`).
- Each formula consists of two parts: First is the main margin calculation ; Second is an addition for maker/taker fees

### Implementation details

For those interested in exploring the technical implementation:
- File location: `nautilus_trader/accounting/accounts/margin.pyx`
- Key methods: `calculate_margin_init(self, ...)` and `calculate_margin_maint(self, ...)`

## Commissions

Trading commissions represent the fees charged by exchanges or brokers for executing trades. While maker/taker fees
are common in cryptocurrency markets, traditional exchanges like CME often use different fee structures,
such as per-contract commissions. Nautilus Trader supports multiple commission models to accommodate various
fee structures across different markets.

### Built-in Fee Models

The framework provides two official fee model implementations:

1. `MakerTakerFeeModel`: Implements the maker/taker fee structure common in cryptocurrency exchanges, where fees are
    calculated as a percentage of the trade value.
2. `FixedFeeModel`: Applies a fixed commission per trade, regardless of the trade size.

### Creating Custom Fee Models

While the built-in fee models cover common scenarios, you might encounter situations requiring specific commission structures.
Nautilus Trader's flexible architecture allows you to implement custom fee models by inheriting from the base `FeeModel` class.

For example, if you're trading futures on exchanges that charge per-contract commissions (like CME), you can implement
a custom fee model. When creating custom fee models, we inherit from the `FeeModel` base class, which is implemented
in Cython for performance reasons. This Cython implementation is reflected in the parameter naming convention,
where type information is incorporated into parameter names using underscores (like `Order_order` or `Quantity_fill_qty`).

While these parameter names might look unusual to Python developers, they're a result of Cython's type system and help
maintain consistency with the framework's core components. Here's how you could create a per-contract commission model:

```python
class PerContractFeeModel(FeeModel):
    def __init__(self, commission: Money):
        super().__init__()
        self.commission = commission

    def get_commission(self, Order_order, Quantity_fill_qty, Price_fill_px, Instrument_instrument):
        total_commission = Money(self.commission * Quantity_fill_qty, self.commission.currency)
        return total_commission
```

This custom implementation calculates the total commission by multiplying a `fixed per-contract fee` by the `number
of contracts` traded. The `get_commission(...)` method receives information about the order, fill quantity, fill price
and instrument, allowing for flexible commission calculations based on these parameters.

Our new class `PerContractFeeModel` inherits class `FeeModel`, which is implemented in Cython,
so notice the Cython-style parameter names in the method signature:

- `Order_order`: The order object, with type prefix `Order_`
- `Quantity_fill_qty`: The fill quantity, with type prefix `Quantity_`
- `Price_fill_px`: The fill price, with type prefix `Price_`
- `Instrument_instrument`: The instrument object, with type prefix `Instrument_`

These parameter names follow Nautilus Trader's Cython naming conventions, where the prefix indicates the expected type.
While this might seem verbose compared to typical Python naming conventions, it ensures type safety and consistency
with the framework's Cython codebase.

### Using Fee Models in Practice

To use any fee model in your trading system, whether built-in or custom, you specify it when setting up the venue.
Here's an example using the custom per-contract fee model:

```python
engine.add_venue(
    venue=venue,
    oms_type=OmsType.NETTING,
    account_type=AccountType.MARGIN,
    base_currency=USD,
    fee_model=PerContractFeeModel(Money(2.50, USD)),  # Our custom fee-model injected here: 2.50 USD / per 1 filled contract
    starting_balances=[Money(1_000_000, USD)],
)
```

:::tip
When implementing custom fee models, ensure they accurately reflect the fee structure of your target exchange.
Even small discrepancies in commission calculations can significantly impact strategy performance metrics during backtesting.
:::


## Additional info
The raw instrument definition as provided by the exchange (typically from JSON serialized data) is also
included as a generic Python dictionary. This is to retain all information
which is not necessarily part of the unified Nautilus API, and is available to the user
at runtime by calling the `.info` property.
