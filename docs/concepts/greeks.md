# Greeks

Nautilus provides two paths for working with option Greeks
(sensitivities of option prices to changes in market variables):

1. **Venue-provided Greeks (Rust/PyO3)**: real-time Greeks streamed from venues
   like Deribit, Bybit, and OKX via the `OptionGreeks` data type and the option
   chain aggregation system.
2. **Local Greeks calculator (Cython/Python and Rust/PyO3)**: the `GreeksCalculator`
   class computes Black-Scholes Greeks from cached market data, with support for
   portfolio aggregation, shock scenarios, and beta weighting.

Either path works independently or together. Venue-provided Greeks arrive
through the data subscription system and require no local computation. The local
calculator covers venues that do not stream Greeks, backtesting, and custom
adjustments (shocks, beta weighting, percent Greeks).

## Venue-provided Greeks (Rust/PyO3)

### OptionGreeks

The `OptionGreeks` type represents venue-provided sensitivities for a single option
contract. It is a Rust-native type exposed to Python via PyO3.

| Field              | Type               | Description                                         |
|--------------------|--------------------|-----------------------------------------------------|
| `instrument_id`    | `InstrumentId`     | The option contract these Greeks apply to.          |
| `convention`       | `GreeksConvention` | Numeraire convention for the Greeks.                |
| `delta`            | `float`            | Rate of change of option price per unit underlying. |
| `gamma`            | `float`            | Rate of change of delta per unit underlying.        |
| `vega`             | `float`            | Sensitivity to a 1% change in implied volatility.   |
| `theta`            | `float`            | Daily time decay (dV/dt / 365.25).                  |
| `rho`              | `float`            | Sensitivity to a change in interest rate.           |
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

See the [Options](options.md) guide for the full subscription API including option
chain aggregation, strike range filtering, and snapshot modes.

### Persistence and replay

`OptionGreeks` is a native member of the `Data` enum, so it persists to the data catalog
and replays in backtests as built-in market data (not custom data). Writing and querying
use the standard catalog API:

```python
catalog.write_data(greeks)               # greeks: list[OptionGreeks]
greeks = catalog.query(data_cls=OptionGreeks)
```

During replay, persisted Greeks reach a subscribed actor or strategy through the same
`on_option_greeks` handler used for live data. They also feed option-chain aggregation:
when a strategy subscribes to an `OptionChainSlice`, the backtest data engine joins
replayed `OptionGreeks` with replayed `QuoteTick` BBO updates for each option
instrument. The `underlying_price` field seeds ATM, and `delta` supports delta-based
strike selection through `StrikeRange.delta(target, tolerance)`.

### Core schema versus custom data

The native `OptionGreeks` fields are the canonical core schema: the five standard Greeks
(`delta`, `gamma`, `vega`, `theta`, `rho`) plus implied volatility, underlying price, open
interest, and convention. These field names are stable.

There is no single complete Greeks shape, so venue- or model-specific values such as
`vanna`, `volga`, `charm`, calibration inputs, or surface metadata belong in
[custom data](custom_data.md) rather than the native type. Optional venue fields are
nullable. Fields required to interpret the values, such as `convention`, are non-nullable
and carry defaults.

### Underlying Rust types

The core Rust implementation lives in `crates/model/src/data/greeks.rs`:

- `OptionGreekValues`: a plain struct with `delta`, `gamma`, `vega`, `theta`, `rho`
  fields. Implements `Add` and `Mul<f64>` for aggregation.
- `OptionGreeks` (in `crates/model/src/data/option_chain.rs`): wraps
  `OptionGreekValues` with `instrument_id`, `convention`, implied volatility fields, and
  timestamps. Implements `Deref<Target = OptionGreekValues>` so you can access Greeks
  fields directly.
- `HasGreeks` trait: provides a `greeks()` method returning `OptionGreekValues`.
  Implemented by both `OptionGreekValues` and `OptionGreeks`.

### Black-Scholes functions (Rust/PyO3)

Low-level pricing functions exposed to Python from `crates/model/src/data/greeks.rs`:

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

# Refine volatility from a starting vol estimate (faster convergence)
result = refine_vol_and_greeks(s=100.0, r=0.05, b=0.0, is_call=True, k=100.0, t=0.25,
                                target_price=5.0, initial_vol=0.18)
```

The `BlackScholesGreeksResult` returned by these functions contains: `price`, `vol`,
`delta`, `gamma`, `vega`, `theta`, and `itm_prob`.

**Conventions:**

- Vega is scaled by 0.01 (sensitivity to a 1 percentage point vol change).
- Theta is scaled by 1/365.25 (daily decay).
- American-style options are priced as European for Greeks computation.

## Local Greeks calculators

### GreeksCalculator

The legacy Cython `GreeksCalculator` class in `nautilus_trader/model/greeks.pyx` computes
Black-Scholes Greeks from cached market data. A PyO3 calculator is also exposed from
`nautilus_trader.common.GreeksCalculator` for the v2 surface. Both use the cache and clock
and are accessible from actors or strategies.

```python
from nautilus_trader.model.greeks import GreeksCalculator

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
# Returns GreeksData or None
```

The calculator:

1. Looks up the instrument and its underlying in the cache.
2. Retrieves current prices (MID preferred, LAST as fallback).
3. Looks up yield curves from the cache (falls back to `flat_interest_rate`).
4. Implies volatility from the market price using `imply_vol_and_greeks`.
5. Returns a `GreeksData` object with all computed values.

For non-option instruments (futures, equities), the calculator returns a `GreeksData`
with `delta=1` (or beta-weighted delta) and no gamma/vega/theta.

**Shock scenarios**: apply hypothetical changes to spot, volatility, or time:

```python
greeks = calculator.instrument_greeks(
    instrument_id=option_id,
    spot_shock=10.0,            # +10 points on underlying
    vol_shock=0.02,             # +2% absolute vol increase
    time_to_expiry_shock=1/365, # roll forward one day
)
```

**Volatility update**: refine implied vol from a cached starting point for faster
convergence:

```python
greeks = calculator.instrument_greeks(
    instrument_id=option_id,
    update_vol=True,        # use cached vol as starting point
    cache_greeks=True,      # store result for next iteration
)
```

**Beta-weighted Greeks**: express delta and gamma in terms of an index:

```python
greeks = calculator.instrument_greeks(
    instrument_id=option_id,
    index_instrument_id=InstrumentId.from_str("SPX.CBOE"),
    beta_weights={underlying_id: 1.15},
    percent_greeks=True,
)
```

**Time-weighted vega**: normalize vega across different expirations:

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
# Returns PortfolioGreeks: pnl, price, delta, gamma, vega, theta
```

Filters:

- `underlyings`: list of symbol prefixes (e.g., `["AAPL"]` matches AAPL stock and
  all AAPL options).
- `venue`: restrict to a single venue.
- `instrument_id`: restrict to a single instrument.
- `strategy_id`: restrict to a single strategy.
- `side`: filter by position side (LONG, SHORT).
- `greeks_filter`: callable that accepts `PortfolioGreeks` per position; return
  `True` to include.

### GreeksData

On the legacy Python surface, `GreeksData` is a Python custom data class
(`@customdataclass`) that carries the full context of a single instrument's Greeks
computation. It extends `Data` and supports Arrow serialization, cache storage, and
catalog persistence. The v2/PyO3 surface exposes the same core fields from Rust.

| Field               | Type            | Description                                            |
|---------------------|-----------------|--------------------------------------------------------|
| `instrument_id`     | `InstrumentId`  | The instrument.                                        |
| `is_call`           | `bool`          | True for call, False for put.                          |
| `strike`            | `float`         | Strike price.                                          |
| `expiry`            | `int`           | Expiry date as YYYYMMDD integer.                       |
| `expiry_in_days`    | `int`           | Days to expiry.                                        |
| `expiry_in_years`   | `float`         | Years to expiry (days / 365.25).                       |
| `multiplier`        | `float`         | Contract multiplier.                                   |
| `quantity`          | `float`         | Position quantity (always 1 from `instrument_greeks`). |
| `underlying_price`  | `float`         | Underlying price used in calculation.                  |
| `interest_rate`     | `float`         | Interest rate used.                                    |
| `cost_of_carry`     | `float`         | Cost of carry (r - dividend yield; 0 for futures).     |
| `vol`               | `float`         | Implied volatility.                                    |
| `pnl`               | `float`         | PnL relative to position entry (if position provided). |
| `price`             | `float`         | Model price.                                           |
| `delta`             | `float`         | Delta.                                                 |
| `gamma`             | `float`         | Gamma.                                                 |
| `vega`              | `float`         | Vega (dV / 1% vol change).                             |
| `theta`             | `float`         | Theta (daily decay).                                   |
| `itm_prob`          | `float`         | In‑the‑money probability.                              |

`GreeksData` scales to portfolio level via its `to_portfolio_greeks()` method, which
multiplies all values by the contract `multiplier`. The `*` operator applies position
quantity:

```python
position_greeks = signed_qty * instrument_greeks  # returns PortfolioGreeks
```

### PortfolioGreeks

`PortfolioGreeks` is the aggregated result from `portfolio_greeks()`. It supports
addition (`+`) for combining positions and scalar multiplication (`*`) for scaling:

| Field   | Type    | Description            |
|---------|---------|------------------------|
| `pnl`   | `float` | Aggregate PnL.         |
| `price` | `float` | Aggregate model value. |
| `delta` | `float` | Portfolio delta.       |
| `gamma` | `float` | Portfolio gamma.       |
| `vega`  | `float` | Portfolio vega.        |
| `theta` | `float` | Portfolio theta.       |

### YieldCurveData

`YieldCurveData` stores an interest rate or dividend yield curve. The `GreeksCalculator`
looks up curves from the cache by currency code (for interest rates) or by underlying
instrument ID (for dividend yields).

```python
from nautilus_trader.model.greeks_data import YieldCurveData
import numpy as np

curve = YieldCurveData(
    ts_event=0,
    ts_init=0,
    curve_name="USD",
    tenors=np.array([0.25, 0.5, 1.0, 2.0]),
    interest_rates=np.array([0.04, 0.042, 0.045, 0.048]),
)

# Callable: interpolates rate for a given tenor
rate = curve(0.75)  # quadratic interpolation
```

## Choosing between the two paths

| Criterion                    | Venue‑provided (`OptionGreeks`)        | Local calculator (`GreeksCalculator`)    |
|------------------------------|----------------------------------------|------------------------------------------|
| Computation                  | Done by the venue                      | Local Black‑Scholes                      |
| Latency                      | Arrives with market data               | Computed on demand                       |
| Venues                       | Deribit, Bybit, OKX                    | Any venue with option instruments        |
| Shock scenarios              | Not supported                          | Spot, vol, and time shocks               |
| Portfolio aggregation        | Manual (iterate `OptionChainSlice`)    | Built‑in via `portfolio_greeks()`        |
| Beta weighting               | Not supported                          | Built‑in                                 |
| Backtest support             | Via recorded `OptionGreeks` data       | From cached prices at any point in time  |
| Greeks available             | delta, gamma, vega, theta, rho, IV, OI | delta, gamma, vega, theta, itm_prob, vol |
| Data type                    | `OptionGreeks` (Rust/PyO3)             | `GreeksData` / `PortfolioGreeks`         |

## Greek definitions

For reference, the Greeks that Nautilus computes:

| Greek      | Symbol | Definition                                                                    |
|------------|--------|-------------------------------------------------------------------------------|
| Delta      | `d`    | First derivative of option price with respect to underlying price (dV/dS).    |
| Gamma      | `g`    | Second derivative of option price with respect to underlying price (d2V/dS2). |
| Vega       | `v`    | Sensitivity to a 1 percentage point change in implied volatility (dV/dVol).   |
| Theta      | `t`    | Daily time decay: change in option price per calendar day (dV/dt / 365.25).   |
| Rho        | `r`    | Sensitivity to a change in the risk‑free interest rate (dV/dr).               |
| ITM prob   | -      | Probability that the option finishes in the money.                            |

For calls, ITM probability is `P(S_T > K)`. For puts, it is `P(S_T < K)`.

## Examples

Complete working examples are available in the repository:

- `examples/live/bybit/bybit_option_greeks.py`: subscribe to Bybit venue-provided Greeks.
- `examples/live/deribit/deribit_option_greeks.py`: subscribe to Deribit venue-provided Greeks.
- `examples/live/okx/okx_option_greeks.py`: subscribe to OKX venue-provided Greeks.

## Related guides

- [Options](options.md) - Option instruments, chain subscriptions, and strike filtering.
- [Data](data.md) - Built-in data types, custom data, and the subscription model.
- [Actors](actors.md) - Subscription and handler reference.
- [Strategies](strategies.md) - Strategy implementation and handler methods.
