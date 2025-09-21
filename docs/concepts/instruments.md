# Instruments

The `Instrument` base class represents the core specification for any tradable asset/contract. There are
currently a number of subclasses representing a range of *asset classes* and *instrument classes* which are supported by the platform:

- `Equity` (listed shares or ETFs traded on cash markets)
- `FuturesContract` (deliverable futures contract with defined underlying, expiry, and multiplier)
- `FuturesSpread` (exchange-defined multi-leg futures strategy—e.g., calendar or inter-commodity—quoted as one instrument)
- `OptionContract` (exchange-traded option—put or call—on an underlying with strike and expiry)
- `OptionSpread` (exchange-defined multi-leg options strategy—e.g., vertical, calendar, straddle—quoted as one instrument)
- `BinaryOption` (fixed-payout option that settles to 0 or 1 based on a binary outcome)
- `Cfd` (over-the-counter Contract for Difference that tracks an underlying and is cash-settled)
- `Commodity` (spot commodity instrument—e.g., gold or oil—traded in cash markets)
- `CurrencyPair` (spot FX or crypto pair in BASE/QUOTE format traded in cash markets)
- `CryptoOption` (option on a crypto underlying with crypto quote/settlement; supports inverse or quanto styles)
- `CryptoPerpetual` (perpetual futures contract—aka perpetual swap—on crypto with no expiry; can be inverse or quanto-settled)
- `CryptoFuture` (dated, deliverable crypto futures contract with fixed expiry, underlying crypto, and settlement currency)
- `IndexInstrument` (spot index calculated from constituents; used as a reference price and not directly tradable)
- `BettingInstrument` (a sports/gaming market selection—e.g., team or runner—tradable on betting venues)

## Symbology

All instruments should have a unique `InstrumentId`, which is made up of both the native symbol, and venue ID, separated by a period.
For example, on the Binance Futures crypto exchange, the Ethereum Perpetual Futures Contract has the instrument ID `ETHUSDT-PERP.BINANCE`.

All native symbols *should* be unique for a venue (this is not always the case e.g. Binance share native symbols between spot and futures markets),
and the `{symbol.venue}` combination *must* be unique for a Nautilus system.

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
await provider.load_all_async()

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
object by passing the matching `InstrumentId` to data and execution related methods and classes that require one.

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
from nautilus_trader.model.instruments import Instrument

def on_instrument(self, instrument: Instrument) -> None:
    # Take some action on an instrument update
    pass
```

## Precisions and increments

The instrument objects are a convenient way to organize the specification of an
instrument through *read-only* properties. Correct price and quantity precisions, as well as
minimum price and size increments, multipliers and standard lot sizes, are available.

:::note
Most of these limits are checked by the Nautilus `RiskEngine`, otherwise invalid
values for prices and quantities *can* result in the exchange rejecting orders.
:::

## Limits

Certain value limits are optional for instruments and can be `None`, these are exchange
dependent and can include:

- `max_quantity` (maximum quantity for a single order).
- `min_quantity` (minimum quantity for a single order).
- `max_notional` (maximum value of a single order).
- `min_notional` (minimum value of a single order).
- `max_price` (maximum valid quote or order price).
- `min_price` (minimum valid quote or order price).

:::note
Most of these limits are checked by the Nautilus `RiskEngine`, otherwise exceeding
published limits *can* result in the exchange rejecting orders.
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

Margin calculations are handled by the `MarginAccount` class. This section explains how margins work and introduces key concepts you need to know.

### When margins apply?

Each exchange (e.g., CME or Binance) operates with a specific account type that determines whether margin calculations are applicable.
When setting up an exchange venue, you'll specify one of these account types:

- `AccountType.MARGIN`: Accounts that use margin calculations, which are explained below.
- `AccountType.CASH`: Simple accounts where margin calculations do not apply.
- `AccountType.BETTING`: Accounts designed for betting, which also do not involve margin calculations.

### Vocabulary

To understand trading on margin, let’s start with some key terms:

**Notional Value**: The total contract value in the quote currency. It represents the full market value of your position. For example, with EUR/USD futures on CME (symbol 6E).

- Each contract represents 125,000 EUR (EUR is base currency, USD is quote currency).
- If the current market price is 1.1000, the notional value equals 125,000 EUR × 1.1000 (price of EUR/USD) = 137,500 USD.

**Leverage** (`leverage`): The ratio that determines how much market exposure you can control relative to your account deposit. For example, with 10× leverage, you can control 10,000 USD worth of positions with just 1,000 USD in your account.

**Initial Margin** (`margin_init`): The margin rate required to open a position. It represents the minimum amount of funds that must be available in your account to open new positions. This is only a pre-check — no funds are actually locked.

**Maintenance Margin** (`margin_maint`): The margin rate required to keep a position open. This amount is locked in your account to maintain the position. It is always lower than the initial margin. You can view the total blocked funds (sum of maintenance margins for open positions) using the following in your strategy:

```python
self.portfolio.balances_locked(venue)
```

**Maker/Taker Fees**: The fees charged by exchanges based on your order's interaction with the market:

- Maker Fee (`maker_fee`): A fee (typically lower) charged when you "make" liquidity by placing an order that remains on the order book. For example, a limit buy order below the current price adds liquidity, and the *maker* fee applies when it fills.
- Taker Fee (`taker_fee`): A fee (typically higher) charged when you "take" liquidity by placing an order that executes immediately. For instance, a market buy order or a limit buy above the current price removes liquidity, and the *taker* fee applies.

:::tip
Not all exchanges or instruments implement maker/taker fees. If absent, set both `maker_fee` and `taker_fee` to 0 for the `Instrument` (e.g., `FuturesContract`, `Equity`, `CurrencyPair`, `Commodity`, `Cfd`, `BinaryOption`, `BettingInstrument`).
:::

### Margin calculation formula

The `MarginAccount` class calculates margins using the following formulas:

```python
# Initial margin calculation
margin_init = (notional_value / leverage * margin_init) + (notional_value / leverage * taker_fee)

# Maintenance margin calculation
margin_maint = (notional_value / leverage * margin_maint) + (notional_value / leverage * taker_fee)
```

**Key Points**:

- Both formulas follow the same structure but use their respective margin rates (`margin_init` and `margin_maint`).
- Each formula consists of two parts:
  - **Primary margin calculation**: Based on notional value, leverage, and margin rate.
  - **Fee Adjustment**: Accounts for the maker/taker fee.

### Implementation details

For those interested in exploring the technical implementation:

- [nautilus_trader/accounting/accounts/margin.pyx](https://github.com/nautechsystems/nautilus_trader/blob/develop/nautilus_trader/accounting/accounts/margin.pyx)
- Key methods: `calculate_margin_init(self, ...)` and `calculate_margin_maint(self, ...)`

## Commissions

Trading commissions represent the fees charged by exchanges or brokers for executing trades.
While maker/taker fees are common in cryptocurrency markets, traditional exchanges like CME often
employ other fee structures, such as per-contract commissions.
NautilusTrader supports multiple commission models to accommodate diverse fee structures across different markets.

### Built-in fee models

The framework provides two built-in fee model implementations:

1. `MakerTakerFeeModel`: Implements the maker/taker fee structure common in cryptocurrency exchanges, where fees are
    calculated as a percentage of the trade value.
2. `FixedFeeModel`: Applies a fixed commission per trade, regardless of the trade size.

### Creating custom fee models

While the built-in fee models cover common scenarios, you might encounter situations requiring specific commission structures.
NautilusTrader's flexible architecture allows you to implement custom fee models by inheriting from the base `FeeModel` class.

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

- `Order_order`: The order object, with type prefix `Order_`.
- `Quantity_fill_qty`: The fill quantity, with type prefix `Quantity_`.
- `Price_fill_px`: The fill price, with type prefix `Price_`.
- `Instrument_instrument`: The instrument object, with type prefix `Instrument_`.

These parameter names follow NautilusTrader's Cython naming conventions, where the prefix indicates the expected type.
While this might seem verbose compared to typical Python naming conventions, it ensures type safety and consistency
with the framework's Cython codebase.

### Using fee models in practice

To use any fee model in your trading system, whether built-in or custom, you specify it when setting up the venue.
Here's an example using the custom per-contract fee model:

```python
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.objects import Money, Currency

engine.add_venue(
    venue=venue,
    oms_type=OmsType.NETTING,
    account_type=AccountType.MARGIN,
    base_currency=USD,
    fee_model=PerContractFeeModel(Money(2.50, USD)),  # 2.50 USD per contract
    starting_balances=[Money(1_000_000, USD)],  # Starting with 1,000,000 USD balance
)
```

:::tip
When implementing custom fee models, ensure they accurately reflect the fee structure of your target exchange.
Even small discrepancies in commission calculations can significantly impact strategy performance metrics during backtesting.
:::

### Additional info

The raw instrument definition as provided by the exchange (typically from JSON serialized data) is also
included as a generic Python dictionary. This is to retain all information
which is not necessarily part of the unified Nautilus API, and is available to the user
at runtime by calling the `.info` property.

## Synthetic instruments

The platform supports creating customized synthetic instruments, which can generate synthetic quote
and trades. These are useful for:

- Enabling `Actor` and `Strategy` components to subscribe to quote or trade feeds.
- Triggering emulated orders.
- Constructing bars from synthetic quotes or trades.

Synthetic instruments cannot be traded directly, as they are constructs that only exist locally
within the platform. They serve as analytical tools, providing useful metrics based on their component
instruments.

In the future, we plan to support order management for synthetic instruments, enabling trading of
their component instruments based on the synthetic instrument's behavior.

:::info
The venue for a synthetic instrument is always designated as `'SYNTH'`.
:::

### Formula

A synthetic instrument is composed of a combination of two or more component instruments (which
can include instruments from multiple venues), as well as a "derivation formula".
Utilizing the dynamic expression engine powered by the [evalexpr](https://github.com/ISibboI/evalexpr)
Rust crate, the platform can evaluate the formula to calculate the latest synthetic price tick
from the incoming component instrument prices.

See the `evalexpr` documentation for a full description of available features, operators and precedence.

:::tip
Before defining a new synthetic instrument, ensure that all component instruments are already defined and exist in the cache.
:::

### Subscribing

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

# Subscribe to quotes for the synthetic instrument
self.subscribe_quote_ticks(self._synthetic_id)
```

:::note
The `instrument_id` for the synthetic instrument in the above example will be structured as `{symbol}.{SYNTH}`, resulting in `'BTC-ETH:BINANCE.SYNTH'`.
:::

### Updating formulas

It's also possible to update a synthetic instrument formulas at any time. The following example
shows how to achieve this with an actor/strategy.

```
# Recover the synthetic instrument from the cache (assuming `synthetic_id` was assigned)
synthetic = self.cache.synthetic(self._synthetic_id)

# Update the formula, here is a simple example of just taking the average
new_formula = "(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2"
synthetic.change_formula(new_formula)

# Now update the synthetic instrument
self.update_synthetic(synthetic)
```

### Trigger instrument IDs

The platform allows for emulated orders to be triggered based on synthetic instrument prices. In
the following example, we build upon the previous one to submit a new emulated order.
This order will be retained in the emulator until a trigger from synthetic quotes releases it.
It will then be submitted to Binance as a MARKET order:

```python
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

### Error handling

Considerable effort has been made to validate inputs, including the derivation formula for
synthetic instruments. Despite this, caution is advised as invalid or erroneous inputs may lead to
undefined behavior.

:::info
See the `SyntheticInstrument` [API reference](../api_reference/model/instruments.md#class-syntheticinstrument-1)
for a detailed understanding of input requirements and potential exceptions.
:::
