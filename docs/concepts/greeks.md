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
- Theta is scaled by 1/365.25 (daily decay).
- Rho is scaled by 0.01 (sensitivity to a 1 percentage point rate change).
- American-style options are priced as European for Greeks computation.

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

For option instruments, the calculator performs these steps:

1. Look up the instrument and reject expired options or unsupported option spreads before using cached Greeks.
1. Resolve the underlying when a fresh or shocked valuation needs it.
1. Retrieve prices from the cache. Standard instruments prefer `MID` and fall back to `LAST`. True index
   instruments prefer the cached index price.
1. Look up yield curves from the cache, falling back to `flat_interest_rate`.
1. Solve implied volatility from a valid market premium and compute checked Greeks.
1. Return a `GreeksData` object with the computed values.

Missing prices return `None`, which lets strategies treat warm-up as a normal no-op path.
Missing instrument definitions, expired options, option spreads, invalid shocked inputs or results,
and failed implied-volatility solves raise `ValueError`. Expiration is checked before returning cached
option Greeks.

The calculator derives carry as `r - q` when a dividend yield or curve is available and holds carry fixed at
zero otherwise. Shocked cached valuations keep their cached numeric inputs and choose carry rate sensitivity
from the current call's dividend yield or available dividend curve.

For non-option instruments such as futures and equities, the calculator returns `GreeksData` with `delta=1` or
beta-weighted delta and zero gamma, vega, theta, and rho. Option-specific fields keep their default values.

#### Shock scenarios

Apply hypothetical changes to spot, volatility, or time:

```python
greeks = calculator.instrument_greeks(
    instrument_id=option_id,
    spot_shock=10.0,  # +10 points on underlying
    vol_shock=0.02,  # +2 percentage points of volatility
    time_to_expiry_shock=1 / 365.25,  # roll forward one calendar day
)
```

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

Normalize vega across different expirations:

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
validation succeeds. Cached valuations bypass those operations, including when shocks are applied. The Rust
`GreeksCalculator` can publish to the `data.GreeksData.underlying={underlying}.instrument_id={instrument_id}` topic,
keyed by the underlying symbol and the full option instrument ID. The Python surface does not expose that flag.

| Field              | Type           | Description                                                             |
| ------------------ | -------------- | ----------------------------------------------------------------------- |
| `instrument_id`    | `InstrumentId` | Instrument for the calculation.                                         |
| `is_call`          | `bool`         | `True` for a call or non-option result, `False` for a put.              |
| `strike`           | `float`        | Strike price.                                                           |
| `expiry`           | `int`          | Expiry date as a `YYYYMMDD` integer.                                    |
| `expiry_in_days`   | `int`          | Days to expiry.                                                         |
| `expiry_in_years`  | `float`        | Years to expiry (`expiry_in_days / 365.25`).                            |
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

`YieldCurve` holds interest rates or dividend yields by tenor in years. A curve with one rate is
flat, and a curve with three or more points is interpolated. The calculator looks up cached curves by
`curve_name`: the option's quote currency code (such as `USD`) for the interest rate, and the
underlying instrument ID for the dividend yield. Without a matching curve, it uses
`flat_interest_rate` and `flat_dividend_yield`.

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
`nautilus_serialization::ensure_custom_data_registered::<YieldCurve>()` before catalog or
streaming writes and queries.

### Recording calculated values

`GreeksData` and `PortfolioGreeks` support native Arrow recording under their matching
`NautilusRecordType` selectors. Rust streaming writers accept these records directly.
A subscribed writer also captures the Rust calculator's typed `GreeksData` publications.

In Python, `StreamingFeatherWriter.write()` accepts `GreeksData` and `PortfolioGreeks`.
Use `nautilus_trader.serialization.pyobjects_to_arrow_record_batch_bytes()` to encode these objects, then
`ParquetDataCatalog.write_record_arrow_bytes()` with the matching record selector to store them.

Read recorded values with `query_record_arrow_bytes()` or `query_record_arrow_stream()`.
These queries return Arrow data. Calculated records do not participate in `OptionGreeks`
market-data replay or populate the calculator's caches. Yield curves are custom data, not records;
see [Yield curves](#yield-curves).

## Choosing between the two paths

| Criterion             | Venue-provided (`OptionGreeks`)                       | Local calculator (`GreeksCalculator`)             |
| --------------------- | ----------------------------------------------------- | ------------------------------------------------- |
| Computation           | Done by the venue or broker                           | Local Black-Scholes                               |
| Latency               | Arrives with market data                              | Computed on demand                                |
| Venues                | Bybit, Deribit, Derive, Interactive Brokers, and OKX  | Any cached option with required prices            |
| Shock scenarios       | Not supported                                         | Spot, vol, and time shocks                        |
| Portfolio aggregation | Manual, such as iterating an `OptionChainSlice`       | Built-in through `portfolio_greeks()`             |
| Beta weighting        | Not supported                                         | Built-in                                          |
| Backtest support      | From recorded `OptionGreeks` data                     | From cached prices at any point in time           |
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
