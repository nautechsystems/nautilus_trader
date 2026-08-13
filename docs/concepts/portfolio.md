# Portfolio

The Portfolio maintains account and position‑derived state for a trading node or backtest. Strategies
use it to query accounts, PnL, exposure, margin, equity, and performance statistics.

## Currency conversion

The Python `Portfolio` can convert PnL and exposure from native cost currencies to an account base
currency or an explicit target currency. This supports instruments with different cost currencies
and accounts with different base currencies.

### Supported conversions

Currency conversion is available for the following portfolio queries:

- `realized_pnl()` and `realized_pnls()` convert realized PnL.
- `unrealized_pnl()` and `unrealized_pnls()` convert unrealized PnL.
- `total_pnl()` and `total_pnls()` convert total PnL.
- `net_exposure()` and `net_exposures()` convert net exposure.

All eight methods accept an optional `target_currency`. A successful targeted query contains only
that currency. The Portfolio converts each native value directly to the target, even when an
account has a different base currency.

### Single account behavior

With `PortfolioConfig.convert_to_account_base_currency=true` (the default), a query for one account
without `target_currency` converts values to the account's base currency when it has one. Otherwise,
the result remains in its native cost currency.

```python
# Returns exposure in the account's base currency (e.g., USD)
exposure = portfolio.net_exposures(venue=BINANCE, account_id=account_id)
```

### Multi-account behavior

When querying multiple accounts without `target_currency`, the output depends on whether the method
returns a currency map or one `Money` value:

- Collection methods can return a dictionary with one entry per output currency, using account base
  currencies where configured and native cost currencies otherwise. Provide `target_currency` to
  aggregate the complete result into one currency.
- Single‑value methods return `None` if values from different accounts cannot resolve to one output
  currency.

`net_exposures()` also returns `None` if one instrument spans accounts with different base
currencies because its per‑instrument exposure cannot resolve to one output currency.

```python
# Multiple accounts with the same base currency
exposures = portfolio.net_exposures(venue=BINANCE)
# Returns {USD: Money(...)}

# Accounts with different base currencies and separately resolvable instruments
exposures = portfolio.net_exposures(venue=BINANCE)
# Returns {USD: Money(...), EUR: Money(...)}

# Force single currency across accounts
exposures = portfolio.net_exposures(venue=BINANCE, target_currency=USD)
# Returns {USD: Money(...)}
```

### Calculation failures

PnL and exposure queries fail closed when any required price, xrate, or exact arithmetic operation
is unavailable. Their Python behavior depends on the method type:

- Single‑value methods (`realized_pnl`, `unrealized_pnl`, `total_pnl`, and `net_exposure`)
  return `None`.
- `realized_pnls`, `unrealized_pnls`, and `total_pnls` raise `RuntimeError`.
- `net_exposures` returns `None`.

Collection queries fail as one unit. They never return a partial result or combine target and
source currencies. For example, one unpriced instrument invalidates the whole `unrealized_pnls`
or `total_pnls` result. A valid all‑scope `net_exposures()` query returns `{}` when the portfolio
is flat.

:::warning
Exchange rate data must be available when using `target_currency` for cross‑currency
aggregation.
:::

### Conversion price types

Position valuation prefers a current `MARK` price when `use_mark_prices` is enabled. Otherwise, it
uses `BID` for a long position and `ASK` for a short position before trying the remaining price
fallbacks. Currency conversion uses a current `MID` xrate from the cache. If `use_mark_xrates` is
enabled, a current `MARK` xrate takes precedence and `MID` remains the fallback. Explicit
target‑currency queries do not reuse a carried stale xrate.

### Exposure aggregation

`net_exposure()` values each open position for one instrument before adding long notional and
subtracting short notional. It returns the magnitude of that net valued notional, so the result does
not retain direction. A caller‑supplied `price` values every selected position at the same price.
Without an override or common mark price, side‑specific bid and ask prices can leave a valuation
residual for equal opposing quantities.

`net_exposures()` groups and sums nonzero per‑instrument magnitudes by output currency. It does not
net directional exposure between different instruments.

### Price overrides

The Python methods `unrealized_pnl`, `total_pnl`, and `net_exposure` accept an optional `price`.
When supplied, the Portfolio values the selected instrument at that price instead of reading a
cached market price. The calculation is fresh: it does not replace the cached PnL, exposure, or
market price used by later queries.

## Equity and mark-to-market

The Portfolio exposes pull‑style queries for continuous portfolio valuation and
recorded snapshots. Per‑currency results use the relevant account base currency
or native cost currency.

| Method                                         | Returns                                                |
| ---------------------------------------------- | ------------------------------------------------------ |
| `mark_values(venue, account_id)`               | Signed MTM totals for open positions.                  |
| `equity(venue, account_id)`                    | Total equity combining balance and position valuation. |
| `build_snapshot(account_id)`                   | Account‑wide MTM totals and valuation metadata.        |
| `snapshots(account_id)`                        | Recorded account snapshots in emission order.          |
| `missing_price_instruments(venue, account_id)` | Instruments currently flagged as unpriceable.          |

Longs contribute positive notional, shorts contribute negative notional. Flat
positions are skipped.

An account‑scoped `equity()` query returns `{}` for an unknown account. For a known account, it
raises `RuntimeError` if exact snapshot valuation fails instead of presenting the failure as empty
equity.

### Equity formula

Equity combines the account balance with open‑position valuation, using a different
second term depending on account type:

- **Cash accounts without a base currency and Wallet accounts**: Start with `balances_total`. For
  positions owned by that account, do not add a base‑asset mark value when the balance already holds
  that asset and the instrument's cost currency differs from its base currency. Add mark values for
  inverse instruments and positions not represented by a credited balance asset.
- **Cash accounts with a base currency and betting accounts**:
  `balances_total + Σ mark_value(open positions)`.
- **Margin accounts**: `balances_total + Σ unrealized_pnl(open positions)`.

`mark_values()` always returns gross open‑position values, including assets already present in a
multi‑currency Cash or Wallet balance. The value‑once rule means `equity()` and equity snapshots
count each non‑inverse base asset either as a balance or a mark value, not both. The margin path uses
the same cached unrealized PnL pipeline that powers `unrealized_pnls()`.

### Price fallback

Valuation asks `Cache` for a price in this order, stopping at the first match:

1. Mark price, if `use_mark_prices=true` (the default) in `PortfolioConfig` and a mark price is cached.
2. Side‑appropriate quote: `BID` for longs, `ASK` for shorts.
3. Last trade price.
4. Most recent cached bar close (populated when `bar_updates=true`).

Set `use_mark_prices=false` to skip the mark tier and begin with the side‑appropriate quote.

If none of the four yield a current price, the Portfolio carries the last valid price
for that instrument and position side. The next snapshot lists the instrument in
`stale_instruments`. If the position has never had a valid price, it goes into the
missing‑price tracker, is listed in `unpriced_instruments`, and is excluded from the sum.

### Base currency conversion

When `convert_to_account_base_currency=true` (the default) and the account has a
`base_currency` set, cost‑currency values are converted to the base currency
using `MID` xrates from `Cache.get_xrate()`. With `use_mark_xrates=true`, the cached
mark xrate from `Cache.get_mark_xrate()` is used first and falls back to `MID` if
unavailable. The output dictionary then has a single key matching the base currency.

When `convert_to_account_base_currency=false`, or the account has no `base_currency`,
results are keyed by each position's native cost currency and no xrate
conversion is applied.

If no current xrate is available for a required conversion, the Portfolio carries the
last valid rate and lists its source currency in `stale_currencies`. If no valid rate
has ever been available, the position is treated as unpriceable and flagged through
the missing‑price tracker rather than silently valued at a 1.0 rate.

### Snapshot valuation metadata

`PortfolioSnapshot.total_equity` always provides the per‑currency MTM breakdown.
When base‑currency conversion is enabled and the account has a base currency,
`base_currency_equity` provides the headline scalar in that currency. It is `None`
when conversion is disabled or the account has no base currency.

`is_stale` is true when the snapshot uses a carried price or xrate, or excludes a
position that has never had all required valuation inputs. The related fields identify
the cause:

- `stale_instruments`: Instruments valued with carried prices.
- `stale_currencies`: Source currencies converted with carried xrates.
- `unpriced_instruments`: Instruments excluded because no complete valid valuation has
  ever been available.

Call `build_snapshot(account_id)` for an on‑demand sample. Call `snapshots(account_id)`
to read the bounded recorded sequence. The methods are available from the Rust
Portfolio and Strategy API and from the Python Portfolio binding. Building a snapshot does
not add it to the recorded sequence; only the configured lifecycle emission records snapshots.

### Automatic equity curve

`PortfolioConfig.equity_curve=true` (the default) records and publishes a
mark‑to‑market snapshot when each account registers, at every UTC midnight even while
the account is flat, and when the backtest or live node shuts down. Set
`equity_curve=false` for workloads such as optimizer runs that do not consume an equity
curve. On‑demand `equity()` and `build_snapshot()` calculations remain available.

The separate `snapshot_interval_ms` setting remains opt‑in. When set, it adds
fine‑grained snapshots only while the account has an open position.

### Missing-price tracking

The tracker keeps the latest missing set for each account‑filtered query scope
and the unfiltered venue scope. `missing_price_instruments(venue)` returns their
venue‑wide union. Pass `account_id` to return only that account's current set. Each observation
remains authoritative until the same scope runs again; a filtered result does not declare an
earlier unfiltered result resolved. It has two observable behaviors:

- A warning log fires once per instrument on the transition from no scope reporting
  it to at least one scope reporting it, not on every subsequent call. Once every
  reporting scope observes recovery, a future drop re‑warns.
- When a venue goes flat (no open positions), its tracker entry is cleared so stale
  instruments do not remain flagged.

Call `missing_price_instruments(venue)` to inspect the current set.

:::tip
If `equity()` understates what you expect, check `missing_price_instruments(venue)`
before investigating the math. An instrument without a usable mark, quote, trade, or bar
price is excluded from the total and appears in the missing‑price tracker.
:::

### Venue and account scope

Python collection queries accept optional `venue` and `account_id` scopes. If both are provided,
they must resolve to the same account or the query raises `ValueError`. With `account_id=None`, a
venue query aggregates across every account on that venue.

An account‑filtered valuation reconciles only that account's observation, so
flags raised by other accounts on the same venue survive.

### Python query boundary

The Python Portfolio is a read‑only query facade. It does not expose initialization, reset, or
update commands; the Rust engine remains responsible for authoritative mutation. It also does not
expose the internal recorded realized‑PnL cache. `account()` returns a detached, point‑in‑time copy.
The copy does not reflect later account updates, and changing it does not affect the Portfolio.
Call `account()` again to obtain the latest account state.

## Portfolio statistics

`Portfolio.statistics()` computes a new `PortfolioStatistics` value from all accounts, cached
positions, position snapshots, recorded close‑time PnLs, and portfolio snapshots. It recomputes the
statistics on every call, so invoke it sparingly on hot paths.

The result contains:

- PnL statistics for each currency.
- Return statistics from the preferred return series described below.
- General statistics derived from positions.

The default set includes `WinRate`, `ProfitFactor`, `SharpeRatio`, and `LongRatio`. See the
[Analysis API Reference](/docs/python-api-latest/analysis.html) for all built‑in statistic types. A
standalone `PortfolioAnalyzer` can register other built‑in types such as `MaxDrawdown`, but this does
not change the default set used by `Portfolio.statistics()`, backtest results, or post‑run logs.

After a backtest, `engine.get_result()` exposes these categories through `stats_pnls`,
`stats_returns`, and `stats_general`, plus the selected `returns_series`. When `run_analysis=true`,
the engine also logs the three statistic categories under `PORTFOLIO PERFORMANCE` after the run.

For metrics outside the built‑in set, calculate them from reports, snapshots, or position data and
add them to compatible offline tearsheet inputs. See [Visualization](visualization.md). Define the
result for empty or insufficient data: return `None` when the metric is unknown, or use a
domain‑appropriate default such as `0.0`.

## Returns: position vs portfolio

The analyzer tracks two distinct return series:

- **Position returns** (`analyzer.position_returns()`) measure realized return per position
  as a side‑aware price return relative to the average open price. This reflects the
  instrument's price movement between entry and exit, independent of account size or
  leverage.
- **Portfolio returns** (`analyzer.portfolio_returns()`) measure daily percentage change in
  mark‑to‑market account equity. A $900 gain on a $100,000 account reports roughly 0.9%
  for that day.

When complete portfolio snapshots span at least two distinct UTC dates, the analyzer
uses the final snapshot from each date and computes portfolio returns automatically.
It uses them as the primary series for statistics, tearsheets, and the monthly returns
heatmap. A snapshot emitted exactly at UTC midnight closes the preceding date, keeping
the daily tier consistent with fine‑grained samples. The first valid registration sample
anchors the opening value for a partial first date. Missing or unpriced account dates are
forward‑filled after every account has an initial valid sample. Multiple snapshots on the
same date count as one date, so intra‑day trading alone does not produce portfolio returns.
When portfolio returns are unavailable, the analyzer falls back to position returns;
Python tearsheets can also fall back to account reports.

The convenience accessor `analyzer.returns()` resolves this preference: portfolio returns
when present, position returns otherwise.

### Multi-currency accounts

Portfolio returns require every account's snapshot equity to resolve to one common
currency. Base‑currency conversion normally provides that scalar. When snapshots expose
multiple currencies or accounts resolve to different currencies, the analyzer falls back
to position returns. An explicit tearsheet currency can select matching per‑currency
equity where available.

If you need portfolio‑level returns for a multi‑currency account, compute them externally
by converting balances to a common currency before calculating percentage changes.

### Multi-account calculation

Backtest analysis aggregates all cached accounts after resolving them to a common
currency. The tearsheet follows the same account‑wide aggregation rule for multi‑venue
backtests.

## Related guides

- [Positions](positions.md): Position tracking within portfolios.
- [Reports](reports.md): Generate portfolio analysis reports.
- [Visualization](visualization.md): Visualize portfolio performance.
