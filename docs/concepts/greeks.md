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
| `rho`              | `float`            | Venue-reported rho; defaults to zero.               |
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

`OptionGreeks` is a native member of the `Data` enum, so it persists to the data catalog and replays
in backtests as built-in market data rather than custom data. Use the type-specific catalog methods
to write and query it:

```python
catalog.write_option_greeks(greeks)  # greeks: list[OptionGreeks]
greeks = catalog.query_option_greeks()
```

During replay, persisted Greeks reach a subscribed actor or strategy through the same
`on_option_greeks` handler used for live data. They also feed option-chain aggregation. When a
strategy subscribes to an `OptionChainSlice`, the backtest data engine joins replayed
`OptionGreeks` with replayed `QuoteTick` BBO updates for each option instrument. The
`underlying_price` field seeds ATM selection, and `delta` supports delta-based strike selection
through `StrikeRange.delta(target, tolerance)`.

### Core schema versus custom data

The native `OptionGreeks` fields form the core schema: the five standard Greeks (`delta`, `gamma`,
`vega`, `theta`, and `rho`) plus implied volatility, underlying price, open interest, and
convention. These field names are stable.

No single schema covers every Greeks use case. Put venue-specific or model-specific values such as
`vanna`, `volga`, `charm`, calibration inputs, or surface metadata in
[custom data](custom_data.md), not the native type. Optional venue fields are nullable.
`convention` is non-nullable and defaults to `GreeksConvention.BLACK_SCHOLES` in Python.

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
# result.delta, result.gamma, result.vega, result.theta, result.price, result.vol

# Imply volatility from market price, then compute Greeks
result = imply_vol_and_greeks(s=100.0, r=0.05, b=0.0, is_call=True, k=100.0, t=0.25, price=5.0)

# Refine volatility from a starting estimate with one Halley iteration
result = refine_vol_and_greeks(
    s=100.0, r=0.05, b=0.0, is_call=True, k=100.0, t=0.25, target_price=5.0, initial_vol=0.18
)
```

`refine_vol_and_greeks()` performs one refinement step, not a full convergence loop. Use it with a
good starting estimate; use `imply_vol_and_greeks()` when a full implied-volatility solve is needed.

The `BlackScholesGreeksResult` returned by these functions contains: `price`, `vol`,
`delta`, `gamma`, `vega`, `theta`, and `itm_prob`.

Conventions:

- Vega is scaled by 0.01 (sensitivity to a 1 percentage point vol change).
- Theta is scaled by 1/365.25 (daily decay).
- American-style options are priced as European for Greeks computation.

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

1. Look up the instrument and its underlying in the cache.
1. Retrieve prices from the cache. Standard instruments prefer `MID` and fall back to `LAST`; true
   index instruments prefer the cached index price.
1. Look up yield curves from the cache, falling back to `flat_interest_rate`.
1. Imply volatility from the market price with `imply_vol_and_greeks`.
1. Return a `GreeksData` object with the computed values.

Missing prices return `None`, which lets strategies treat warm-up as a normal no-op path.
Setup errors such as a missing instrument definition raise a Python exception instead.

For non-option instruments such as futures and equities, the calculator returns `GreeksData` with
`delta=1` or beta-weighted delta and zero gamma, vega, theta, and rho. Option-specific fields retain
their default values.

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

With cached Greeks, `update_vol=True` uses the single-iteration refiner described above. If the cache
has no prior Greeks for the instrument, the calculator performs a full implied-volatility solve.

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
- `greeks_filter`: callable that receives per-position `GreeksData` after `pnl`, `price`, and the
  Greek values are scaled by signed position quantity; return `True` to include it.

### GreeksData

`GreeksData` carries the context of a single instrument's Greeks computation and is exposed from
`nautilus_trader.model`. Passing `cache_greeks=True` stores the result in the cache. The Rust
`GreeksCalculator` can also publish it to the `data.GreeksData.instrument_id={symbol}` topic; the
Python surface does not expose that flag.

| Field              | Type           | Description                                                         |
| ------------------ | -------------- | ------------------------------------------------------------------- |
| `ts_init`          | `int`          | Initialization timestamp in nanoseconds.                            |
| `ts_event`         | `int`          | Event timestamp in nanoseconds.                                     |
| `instrument_id`    | `InstrumentId` | Instrument for the calculation.                                     |
| `is_call`          | `bool`         | `True` for a call or non-option result; `False` for a put.          |
| `strike`           | `float`        | Strike price.                                                       |
| `expiry`           | `int`          | Expiry date as a `YYYYMMDD` integer.                                |
| `expiry_in_days`   | `int`          | Days to expiry.                                                     |
| `expiry_in_years`  | `float`        | Years to expiry (`expiry_in_days / 365.25`).                        |
| `multiplier`       | `float`        | Contract multiplier.                                                |
| `quantity`         | `float`        | Quantity, set to 1 by `instrument_greeks()`.                        |
| `underlying_price` | `float`        | Underlying price used in the calculation.                           |
| `interest_rate`    | `float`        | Interest rate used in the calculation.                              |
| `cost_of_carry`    | `float`        | Cost of carry (`r - dividend yield` when supplied; otherwise zero). |
| `vol`              | `float`        | Implied volatility.                                                 |
| `pnl`              | `float`        | PnL relative to the position entry, when a position is provided.    |
| `price`            | `float`        | Option model price; non-option position PnL when supplied.          |
| `delta`            | `float`        | Delta.                                                              |
| `gamma`            | `float`        | Gamma.                                                              |
| `vega`             | `float`        | Vega per one percentage point of volatility.                        |
| `theta`            | `float`        | Daily theta.                                                        |
| `rho`              | `float`        | Rho, set to zero by the local calculator.                           |
| `itm_prob`         | `float`        | In-the-money probability.                                           |

Internally, `portfolio_greeks()` multiplies `pnl`, `price`, and the Greek values by each position's
signed quantity before adding them to the portfolio result. The intermediate `quantity` field
remains `1` and is not part of `PortfolioGreeks`. The calculation does not apply the `multiplier`
field, and the public Python types do not expose arithmetic operators for this aggregation.
Rust callers can apply the same scaling with `quantity * &greeks_data`, which returns `GreeksData`
with scaled `pnl`, `price`, and Greek values.

### PortfolioGreeks

`PortfolioGreeks` is the aggregated result from `portfolio_greeks()`:

The Rust type implements `Add` to combine portfolio results. The Python type does not expose this
operator.

| Field      | Type    | Description                                          |
| ---------- | ------- | ---------------------------------------------------- |
| `ts_init`  | `int`   | Initialization timestamp in nanoseconds.             |
| `ts_event` | `int`   | Event timestamp in nanoseconds.                      |
| `pnl`      | `float` | Aggregate PnL after signed-quantity scaling.         |
| `price`    | `float` | Aggregate model value after signed-quantity scaling. |
| `delta`    | `float` | Portfolio delta.                                     |
| `gamma`    | `float` | Portfolio gamma.                                     |
| `vega`     | `float` | Portfolio vega.                                      |
| `theta`    | `float` | Portfolio theta.                                     |
| `rho`      | `float` | Portfolio rho, zero for local calculator results.    |

### Yield curves

The Python API does not expose the Rust `YieldCurveData` type. Pass `flat_interest_rate` and
`flat_dividend_yield` to `GreeksCalculator` methods when Python calculations need rates that differ
from the defaults. Rust callers can use `YieldCurveData` for interpolated interest rate or dividend
yield curves.

## Choosing between the two paths

| Criterion             | Venue-provided (`OptionGreeks`)                       | Local calculator (`GreeksCalculator`)                     |
| --------------------- | ----------------------------------------------------- | --------------------------------------------------------- |
| Computation           | Done by the venue or broker                           | Local Black-Scholes                                       |
| Latency               | Arrives with market data                              | Computed on demand                                        |
| Venues                | Bybit, Deribit, Derive, Interactive Brokers, and OKX  | Any cached option with required prices                    |
| Shock scenarios       | Not supported                                         | Spot, vol, and time shocks                                |
| Portfolio aggregation | Manual, such as iterating an `OptionChainSlice`       | Built-in via `portfolio_greeks()`                         |
| Beta weighting        | Not supported                                         | Built-in                                                  |
| Backtest support      | Via recorded `OptionGreeks` data                      | From cached prices at any point in time                   |
| Values                | delta, gamma, vega, theta, rho, IV, and open interest | delta, gamma, vega, theta, itm_prob, and vol; rho is zero |
| Data type             | `OptionGreeks`                                        | `GreeksData` and `PortfolioGreeks`                        |

## Greek definitions

These terms appear across both paths. The local Black-Scholes functions scale vega and theta as
described above. `OptionGreeks` retains the values reported by each venue or broker and records their
`convention`.

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
