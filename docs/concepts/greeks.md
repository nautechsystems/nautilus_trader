# Greeks

Nautilus provides two paths for working with option Greeks, which measure how option prices respond
to changes in market variables:

1. **Venue-provided Greeks**: real-time Greeks streamed from supported venues through the
   `OptionGreeks` data type and the option chain aggregation system.
1. **Local Greeks calculator**: `GreeksCalculator` computes Black-Scholes Greeks from cached market
   data, with support for portfolio aggregation, shock scenarios, and beta weighting.

Use either path independently or combine them. Venue-provided Greeks arrive through the data
subscription system and require no local computation. The local calculator covers venues that do
not stream Greeks, backtesting, and custom adjustments such as shocks, beta weighting, and percent
Greeks.

## Venue-provided Greeks

### OptionGreeks

The `OptionGreeks` type represents venue-provided sensitivities for a single option contract. It is
a Rust-native type exposed to Python through PyO3.

| Field              | Type               | Description                                         |
| ------------------ | ------------------ | --------------------------------------------------- |
| `instrument_id`    | `InstrumentId`     | The option contract these Greeks apply to.          |
| `convention`       | `GreeksConvention` | Numeraire convention for the Greeks.                |
| `delta`            | `float`            | Rate of change of option price per unit underlying. |
| `gamma`            | `float`            | Rate of change of delta per unit underlying.        |
| `vega`             | `float`            | Venue-reported vega.                                |
| `theta`            | `float`            | Venue-reported theta.                               |
| `rho`              | `float`            | Venue-reported rho, with zero as the default.       |
| `mark_iv`          | `float` or None    | Mark implied volatility.                            |
| `bid_iv`           | `float` or None    | Bid implied volatility.                             |
| `ask_iv`           | `float` or None    | Ask implied volatility.                             |
| `underlying_price` | `float` or None    | Underlying price at time of calculation.            |
| `open_interest`    | `float` or None    | Open interest for the contract.                     |
| `ts_event`         | `int`              | UNIX timestamp (nanoseconds) of the event.          |
| `ts_init`          | `int`              | UNIX timestamp (nanoseconds) when initialized.      |

Subscribe from an actor or strategy:

```python
self.subscribe_option_greeks(instrument_id, client_id=ClientId("DERIBIT"))
```

Handle updates:

```python
def on_option_greeks(self, greeks: OptionGreeks) -> None:
    self.log.info(f"delta={greeks.delta:.4f} gamma={greeks.gamma:.6f}")
```

See the [Options](options.md) guide for the full subscription API, including option chain
aggregation, strike range filtering, and snapshot modes.

### Persistence and replay

`OptionGreeks` is a native member of the `Data` enum. Thus, it persists to the data catalog and replays in
backtests as built-in market data rather than custom data. Use the type-specific catalog methods to write and
query it:

```python
catalog.write_option_greeks(greeks)  # greeks: list[OptionGreeks]
greeks = catalog.query_option_greeks()
```

During replay, persisted Greeks reach a subscribed actor or strategy through the same `on_option_greeks`
handler used for live data. They also feed option-chain aggregation.

When a strategy subscribes to an `OptionChainSlice`, the backtest data engine joins replayed `OptionGreeks`
with replayed `QuoteTick` BBO updates for each option instrument. The `underlying_price` field seeds ATM
selection, and `delta` supports delta-based strike selection through `StrikeRange.delta(target, tolerance)`.

### Core schema versus custom data

The native `OptionGreeks` fields form the core schema: the five standard Greeks (`delta`, `gamma`,
`vega`, `theta`, and `rho`) plus implied volatility, underlying price, open interest, and
convention. These field names are stable.

No single schema covers all Greeks use cases. Put venue-specific or model-specific values such as `vanna`,
`volga`, `charm`, calibration inputs, or surface metadata in [custom data](custom_data.md), not the native
type. Optional venue fields are nullable. `convention` is non-nullable and defaults to
`GreeksConvention.BLACK_SCHOLES` in Python.

### Underlying Rust types

The core Rust implementation spans `crates/model/src/data/greeks.rs` and
`crates/model/src/data/option_chain.rs`:

- `OptionGreekValues`: a plain struct with `delta`, `gamma`, `vega`, `theta`, and `rho`
  fields. Implements `Add` and `Mul<f64>` for aggregation.
- `OptionGreeks`: wraps `OptionGreekValues` with `instrument_id`, `convention`, implied volatility
  fields, and timestamps. Implements `Deref<Target = OptionGreekValues>` so Rust callers can access
  Greek fields directly.
- `HasGreeks` trait: provides a `greeks()` method returning `OptionGreekValues`.
  Implemented by `OptionGreeks`, `GreeksData`, `PortfolioGreeks`, and
  `BlackScholesGreeksResult`.

### Black-Scholes functions

Low-level pricing functions from `crates/model/src/data/greeks.rs` are also exposed to Python:

```python
from nautilus_trader.model import (
    black_scholes_greeks,
    imply_vol,
    imply_vol_and_greeks,
    refine_vol_and_greeks,
)

# Compute Greeks given known volatility
result = black_scholes_greeks(s=100.0, r=0.05, b=0.0, vol=0.20, is_call=True, k=100.0, t=0.25)
# result.delta, result.gamma, result.vega, result.theta, result.rho, result.price, result.vol

# Imply volatility from market price, then compute Greeks
result = imply_vol_and_greeks(s=100.0, r=0.05, b=0.0, is_call=True, k=100.0, t=0.25, price=5.0)

# Refine volatility from a starting estimate, with a full-solver fallback
result = refine_vol_and_greeks(
    s=100.0, r=0.05, b=0.0, is_call=True, k=100.0, t=0.25, target_price=5.0, initial_vol=0.18
)
```

`refine_vol_and_greeks()` first performs one Halley refinement step. It accepts the result only when
volatility stays inside the refinement bounds and the model price matches the premium within
0.01 percent. Otherwise, it performs a full implied-volatility solve.

The Python pricing functions raise `ValueError` for invalid inputs or results. IV functions also
reject premiums with no finite positive implied volatility. `imply_vol()` returns the scalar solution
without repricing. The functions solving IV and returning Greeks require repricing within
0.01 percent and use the exact implementation when fast-kernel precision is insufficient.
Known-volatility pricing also uses the exact implementation when the fast result is nonfinite or
inconsistent. If the exact result still fails validation, the functions raise `ValueError`.

The `BlackScholesGreeksResult` returned by these functions contains: `price`, `vol`,
`delta`, `gamma`, `vega`, `theta`, `itm_prob`, and `rho`.

Conventions:

- Vega is scaled by 0.01 (sensitivity to a 1 percentage point vol change).
- Theta is scaled by 1/365 (daily decay).
- Rho is scaled by 0.01 (sensitivity to a 1 percentage point rate change).

For the Python pricing functions, `db_dr=0.0` holds carry fixed when the interest rate changes.
Use `db_dr=1.0` when carry is `r - q` and the dividend yield `q` stays fixed, including when `r == q`.
This optional final parameter preserves existing positional calls.

## Local Greeks calculator

### GreeksCalculator

`GreeksCalculator` computes Black-Scholes Greeks from cached market data. It is exposed from
`nautilus_trader.common`, uses the cache and clock, and is accessible from actors and strategies.

```python
from nautilus_trader.common import GreeksCalculator

# Typically created in on_start()
calculator = GreeksCalculator(cache=self.cache, clock=self.clock)
```

#### Instrument Greeks

Compute Greeks for a single instrument (option or underlying) with quantity of 1:

```python
greeks = calculator.instrument_greeks(
    instrument_id=option_id,
    flat_interest_rate=0.0425,  # used if no yield curve in cache
)
# Returns GreeksData or None while market data is warming up.
```

For a fresh option valuation, the calculator solves implied volatility from the market premium and
returns checked `GreeksData`. For futures, equities, and other non-option instruments, it returns a
delta of 1 or a beta-weighted delta, with zero gamma, vega, theta, and rho. Option-specific fields
keep their default values.

##### Underlying and prices

For a fresh valuation or a shock, the calculator resolves the underlying in this order.

1. The option definition's `underlying_instrument_id`, when set.
1. The underlying symbol on the option's venue, if the cache holds its definition or an implied futures spread.
1. A cached cross-venue instrument with that symbol. Multiple matches on discovery raise an error.

The calculator prefers `MID` prices and falls back to `LAST`. True index instruments prefer the
cached index price.

##### Pricing conventions

The calculator uses generalized European Black-Scholes for all exercise styles. For American options,
the model approximates the value without accounting for early exercise. It takes the strike, market
premium, and underlying price per unit of the underlying. It returns price and PnL per unit and
carries the contract multiplier separately.

The `strike_currency`, `quote_currency` (premium), and `settlement_currency` must match the currency
of the underlying price. Load the underlying definition or use a cached implied futures spread to
establish that currency.

The calculator uses the strike-currency interest rate and the underlying's dividend yield. With
a dividend yield or curve, carry is `r - q`. Otherwise, carry stays fixed at zero. See
[Yield curves](#yield-curves).

Fresh valuations use the exact time to expiry for pricing and curve lookups, measured from `ts_event`
or the clock time if unset, and expressed as a fraction of a 365-day year.
Cached valuations use the saved expiry horizon as their starting point. Older cached records can
use a 365.25-day basis.

##### Missing data and errors

Python returns `None` when a price is missing during warm-up and raises `ValueError` for valuation
errors. Rust returns an error for missing prices, missing required definitions, invalid shocked inputs
or results, and failed implied-volatility solves.

The calculator reports these `OptionValuationError` variants in Rust.

- `Expired` for options at or past expiry.
- `UnsupportedSpread` for option spreads.
- `InvalidInput` for invalid valuation inputs, including nonfinite or nonpositive strikes, underlying
  prices, or premiums.
- `ImpliedVolatilityUnavailable` when a positive premium is at or outside the model's no-arbitrage
  bounds or has no finite, positive implied volatility.
- `InvalidResult` for nonfinite or inconsistent valuation results, including failure to reprice
  the premium within the model's tolerance.
- `UnsupportedConvention` for inverse pricing or mixed or unknown price currencies, such as USD
  strikes with BTC or USDC premiums.

The calculator checks expiration, instrument type, and pricing conventions before using cached Greeks.

#### Implied futures prices

`cache_futures_spread()` uses put-call parity to derive an implied futures price from a call/put pair.

```text
implied_price = strike + exp(rate * years) * (call_price - put_price)
```

`years` is the exact time from the calculator's clock to expiry on a 365-day basis. The formula
assumes European exercise. For American-style pairs, the result is an approximation.

- Both legs must resolve to the same underlying ID. For a same-venue pair with both IDs absent, the
  calculator uses each underlying symbol on that venue. Other pairs use the instrument Greeks lookup.
- The pair must be before expiry.
- Use premiums per underlying unit and the same pricing conventions as for instrument Greeks.
- Use the strike-currency yield curve. When it is missing, the calculation uses its own flat rate of `0.0425`.

The cache stores the difference between the implied price and the reference futures price.
`get_cached_futures_spread_price()` adds that difference to the latest reference price. The result
can stand in for a future whose own price or definition is missing.

At creation and reuse, the reference and any loaded underlying must be futures contracts quoted in
the spread's strike currency.

For an option without an explicit ID or a same-venue underlying definition, the cached spread key
takes precedence over cross-venue lookup. If a fresh valuation cannot use the spread, refresh it or
supply an explicit underlying ID.

#### Shock scenarios

Apply hypothetical changes to spot, volatility, or time:

```python
greeks = calculator.instrument_greeks(
    instrument_id=option_id,
    spot_shock=10.0,  # +10 points on underlying
    vol_shock=0.02,  # +2 percentage points of volatility
    time_to_expiry_shock=1 / 365,  # roll forward one calendar day
)
```

The floating-point year fraction limits the precision of time shocks at expiry and whole-day
boundaries, especially for long horizons.

Cached shocks reuse the saved numeric inputs. Carry rate sensitivity comes from the current call's
dividend yield or the available dividend curve. After correcting an option's `underlying_instrument_id`,
recompute and cache its Greeks before reusing them.

#### Volatility update

Refine implied volatility from a cached starting point:

```python
greeks = calculator.instrument_greeks(
    instrument_id=option_id,
    update_vol=True,  # use cached vol as starting point
    cache_greeks=True,  # store result for next iteration
)
```

With cached Greeks, `update_vol=True` uses the cached volatility as a refinement seed, with the
validation and full-solver fallback described above. If the cache has no prior Greeks for the
instrument, the calculator performs a full implied-volatility solve.

#### Beta-weighted Greeks

Express delta and gamma in terms of an index:

```python
greeks = calculator.instrument_greeks(
    instrument_id=option_id,
    index_instrument_id=InstrumentId.from_str("SPX.CBOE"),
    beta_weights={underlying_id: 1.15},
    percent_greeks=True,
)
```

#### Time-weighted vega

Vega time weighting uses `expiry_in_days`. Fresh valuations have a minimum of one whole day.
A time shock can leave zero whole days, in which case the calculator skips the weighting.

```python
greeks = calculator.instrument_greeks(
    instrument_id=option_id,
    vega_time_weight_base=30,  # normalize to 30-day vega
)
```

#### Portfolio Greeks

Aggregate Greeks across all open positions matching filter criteria:

```python
portfolio = calculator.portfolio_greeks(
    underlyings=["AAPL", "MSFT"],
    venue=Venue("CBOE"),
    strategy_id=StrategyId("DELTA_HEDGE-001"),
    flat_interest_rate=0.0425,
    index_instrument_id=InstrumentId.from_str("SPX.CBOE"),
    beta_weights=beta_dict,
    percent_greeks=True,
)
# Returns PortfolioGreeks.
```

Filters:

- `underlyings`: list of symbol prefixes. For example, `["AAPL"]` matches AAPL stock and all AAPL
  options.
- `venue`: restrict to a single venue.
- `instrument_id`: restrict to a single instrument.
- `strategy_id`: restrict to a single strategy.
- `side`: filter by position side, such as `LONG` or `SHORT`.
- `greeks_filter`: callable that receives per-position `GreeksData` after `pnl`, `price`, and the Greek values
  are scaled by the contract multiplier, then signed position quantity. Return `True` to include it.
  Instrument context, including `multiplier` and `quantity`, stays unchanged. Use these values directly:
  `to_portfolio_greeks()` or Python scalar multiplication applies the multiplier again.

A failed instrument valuation or a nonfinite aggregate raises `ValueError`. The calculation does not return a
partial portfolio result.

### GreeksData

`GreeksData` carries the context of a single instrument's Greeks computation and is exposed from
`nautilus_trader.model`. Requested caching and publication use the unshocked option valuation only after
validation succeeds. Cached valuations bypass those operations, including when shocks are applied.

The calculator exposes `publish_greeks` only in Rust. It publishes to
`data.GreeksData.underlying={underlying}.instrument_id={instrument_id}`, using the resolved underlying's symbol
and the full option instrument ID.

| Field              | Type           | Description                                                             |
| ------------------ | -------------- | ----------------------------------------------------------------------- |
| `instrument_id`    | `InstrumentId` | Instrument for the calculation.                                         |
| `is_call`          | `bool`         | `True` for a call or non-option result, `False` for a put.              |
| `strike`           | `float`        | Strike price.                                                           |
| `expiry`           | `int`          | Expiry date as a `YYYYMMDD` integer.                                    |
| `expiry_in_days`   | `int`          | Whole days to expiry.                                                   |
| `expiry_in_years`  | `float`        | Fractional years to expiry.                                             |
| `multiplier`       | `float`        | Contract multiplier.                                                    |
| `quantity`         | `float`        | Quantity, set to 1 by `instrument_greeks()`.                            |
| `underlying_price` | `float`        | Underlying price used in the calculation.                               |
| `interest_rate`    | `float`        | Interest rate used in the calculation.                                  |
| `cost_of_carry`    | `float`        | Cost of carry (`r - dividend yield` when supplied, or zero without it). |
| `vol`              | `float`        | Implied volatility.                                                     |
| `pnl`              | `float`        | PnL relative to the position entry, when a position is provided.        |
| `price`            | `float`        | Option model price, or non-option position PnL when supplied.           |
| `delta`            | `float`        | Delta.                                                                  |
| `gamma`            | `float`        | Gamma.                                                                  |
| `vega`             | `float`        | Vega per one percentage point of volatility.                            |
| `theta`            | `float`        | Daily theta.                                                            |
| `rho`              | `float`        | Rho per one percentage point of interest rate.                          |
| `itm_prob`         | `float`        | In-the-money probability.                                               |
| `ts_event`         | `int`          | Event timestamp in nanoseconds.                                         |
| `ts_init`          | `int`          | Initialization timestamp in nanoseconds.                                |

`GreeksData.to_portfolio_greeks()` applies the contract multiplier once to `pnl`, `price`, and all
five Greek values, including `rho`, and returns `PortfolioGreeks` with the same timestamps.
It leaves position quantity scaling to the caller and preserves the per-unit `GreeksData`.
In Python, `scalar * greeks_data` converts and scales the result, returning `PortfolioGreeks`.

`portfolio_greeks()` uses this multiplier-first, quantity-second scaling before filtering and aggregation. The
intermediate `quantity` field stays `1` and is not part of `PortfolioGreeks`. Rust callers use
`quantity * &greeks_data.to_portfolio_greeks()` for the same result. The Rust expression
`quantity * &greeks_data` keeps quantity-only scaling and returns `GreeksData`.

### PortfolioGreeks

`PortfolioGreeks` is the result of conversion and portfolio aggregation.

Rust and Python support addition of portfolio results, preserving the left operand's timestamps.
Scalar-left multiplication scales `pnl`, `price`, and all five Greek values while preserving timestamps.
Use `scalar * &portfolio_greeks` in Rust and `scalar * portfolio_greeks` in Python.
The Python `GreeksData` and `PortfolioGreeks` types support scalar-left multiplication only.

| Field      | Type    | Description                                                         |
| ---------- | ------- | ------------------------------------------------------------------- |
| `pnl`      | `float` | Aggregate PnL after multiplier and signed-quantity scaling.         |
| `price`    | `float` | Aggregate model value after multiplier and signed-quantity scaling. |
| `delta`    | `float` | Portfolio delta.                                                    |
| `gamma`    | `float` | Portfolio gamma.                                                    |
| `vega`     | `float` | Portfolio vega.                                                     |
| `theta`    | `float` | Portfolio theta.                                                    |
| `rho`      | `float` | Portfolio rho per one percentage point of interest rate.            |
| `ts_event` | `int`   | Event timestamp in nanoseconds.                                     |
| `ts_init`  | `int`   | Initialization timestamp in nanoseconds.                            |

### Yield curves

`YieldCurve` holds annual interest rates or dividend yields by tenor in years. Enter rates as decimals,
such as `0.0425` for 4.25%. The pricing model uses continuous compounding. A one-rate curve is flat,
and a curve with three or more points interpolates between tenors.

`instrument_greeks()` looks up each curve by `curve_name`.

- Use the strike currency code, such as `USD`, for the interest rate. The fallback is `flat_interest_rate`.
- Use the resolved underlying ID for the dividend yield. The fallback is `flat_dividend_yield`.

`YieldCurve` is custom data. Wrap each curve in `CustomData` to write it to the catalog, replay it
in a backtest, or publish it to subscribed actors. The data engine does not cache curves, so the actor
that receives a curve must call `cache.add_yield_curve()` before the calculator can use it:

```python
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType

YIELD_CURVES = DataType("YieldCurve", {"curve_name": "USD"})


def on_start(self) -> None:
    self.subscribe_data(YIELD_CURVES)


def on_data(self, data: CustomData) -> None:
    self.cache.add_yield_curve(data.data)
```

The Python constructor is `YieldCurve(curve_name, tenors, interest_rates, ts_event, ts_init)`.
It raises `ValueError` when `tenors` and `interest_rates` differ in length, hold two points or none, or
hold a non-finite value, and when `tenors` does not strictly increase with each step wide enough to
interpolate. `cache.add_yield_curve()` rejects the same curves, including curves decoded from the catalog
or JSON, so the calculator never evaluates them.
Python registers the type on import. In Rust, call
`nautilus_serialization::ensure_custom_data_registered::<YieldCurve>()` before catalog, streaming,
or external message bus use.

### Recording calculated values

`GreeksData` and `PortfolioGreeks` are custom data. Wrap a calculated value in `CustomData`
to publish it with `publish_data()`, receive it through `subscribe_data()` and `on_data()`, or store
it with `ParquetDataCatalog.write_custom_data()`:

```python
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import NautilusDataType

data_type = DataType("GreeksData", identifier=str(greeks.instrument_id))
data = CustomData(data_type, greeks)
catalog.write_custom_data([data])
loaded = catalog.query_custom_data(
    NautilusDataType.Custom("GreeksData"),
    identifiers=[str(greeks.instrument_id)],
)
```

Pass the loaded `CustomData` objects to `BacktestEngine.add_data()` to replay them through `on_data()`.
Use the `DataType` identifier to separate instrument, portfolio, or scenario streams in the catalog.
Metadata is preserved and distinguishes message bus topics. For stream conversion, omit
`instrument_id` and `bar_type` from `DataType` metadata: these keys override the identifier when
selecting the catalog directory. Other metadata does not separate catalog storage.
Custom-data delivery does not populate the calculator's caches. Rust consumers can cache
received instrument Greeks with `Cache::add_greeks()`; Python's cache has no equivalent method.
In Rust, register each type with `nautilus_serialization::ensure_custom_data_registered::<T>()`
before custom-data catalog, streaming, or external message bus use. Python registers these types on import.
JSON and MsgPack custom-data transport require finite numeric values.

`StreamingFeatherWriter.write()` accepts these values wrapped in `CustomData`. A subscribed writer
also captures the Rust calculator's typed `GreeksData` publications as custom data, using the
instrument ID as the catalog identifier. Typed `PortfolioGreeks` messages published through the
dynamic message bus are recorded as custom data without an identifier.

Calculated Greeks retain their native Arrow schemas. Query them through the custom-data APIs;
they have no `NautilusRecordType` selectors. They remain separate from venue-reported `OptionGreeks`.
Yield curves use the same custom-data path; see [Yield curves](#yield-curves).

## Choosing between the two paths

| Criterion             | Venue-provided (`OptionGreeks`)                       | Local calculator (`GreeksCalculator`)             |
| --------------------- | ----------------------------------------------------- | ------------------------------------------------- |
| Computation           | Done by the venue or broker                           | Local Black-Scholes                               |
| Latency               | Arrives with market data                              | Computed on demand                                |
| Venues                | Bybit, Deribit, Derive, Interactive Brokers, and OKX  | Any venue with supported cached pricing inputs    |
| Shock scenarios       | Not supported                                         | Spot, vol, and time shocks                        |
| Portfolio aggregation | Manual, such as iterating an `OptionChainSlice`       | Built-in through `portfolio_greeks()`             |
| Beta weighting        | Not supported                                         | Built-in                                          |
| Backtest support      | From recorded `OptionGreeks` data                     | From cached prices or recorded custom data        |
| Values                | delta, gamma, vega, theta, rho, IV, and open interest | delta, gamma, vega, theta, rho, itm_prob, and vol |
| Data type             | `OptionGreeks`                                        | `GreeksData` and `PortfolioGreeks`                |

## Greek definitions

These terms appear across both paths. The local Black-Scholes functions scale vega and theta as described
above. `OptionGreeks` keeps the values reported by each venue or broker and records their `convention`.

| Greek    | Field      | Definition                                                                                                        |
| -------- | ---------- | ----------------------------------------------------------------------------------------------------------------- |
| Delta    | `delta`    | First derivative of option price with respect to underlying price (`dV/dS`).                                      |
| Gamma    | `gamma`    | Second derivative of option price with respect to underlying price (`d²V/dS²`).                                   |
| Vega     | `vega`     | Sensitivity to a change in implied volatility (`dV/dVol`).                                                        |
| Theta    | `theta`    | Sensitivity to the passage of time (`dV/dt`).                                                                     |
| Rho      | `rho`      | Sensitivity to a change in the risk-free interest rate (`dV/dr`).                                                 |
| ITM prob | `itm_prob` | Probability that the option finishes in the money: `P(ϕS_T > ϕK)`, where `ϕ = 1` for calls and `ϕ = -1` for puts. |

## Examples

Complete working examples are available in the repository:

- `examples/live/bybit/bybit_option_greeks.py`: subscribe to Bybit venue-provided Greeks.
- `examples/live/deribit/deribit_option_greeks.py`: subscribe to Deribit venue-provided Greeks.
- `examples/live/okx/okx_option_greeks.py`: subscribe to OKX venue-provided Greeks.

## Related guides

- [Options](options.md): option instruments, chain subscriptions, and strike filtering.
- [Data](data/): built-in data types, custom data, and the subscription model.
- [Actors](actors.md): subscription and handler reference.
- [Strategies](strategies.md): strategy implementation and handler methods.
