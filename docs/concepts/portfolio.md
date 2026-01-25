# Portfolio

The Portfolio serves as the central hub for managing and tracking all positions across active strategies for the trading node or backtest.
It consolidates position data from multiple instruments, providing a unified view of your holdings, risk exposure, and overall performance.
Explore this section to understand how NautilusTrader aggregates and updates portfolio state to support effective trading and risk management.

## Currency conversion

The Portfolio supports automatic currency conversion for PnL and exposure calculations,
allowing you to view results in your preferred currency. This is particularly useful when
trading across multiple instruments with different settlement currencies or managing multiple
accounts with different base currencies.

### Supported conversions

Currency conversion is available for the following portfolio queries:

- `realized_pnl()` / `realized_pnls()` - Convert realized PnL to target currency.
- `unrealized_pnl()` / `unrealized_pnls()` - Convert unrealized PnL to target currency.
- `total_pnl()` / `total_pnls()` - Convert total PnL to target currency.
- `net_exposure()` / `net_exposures()` - Convert net exposure to target currency.

All methods accept an optional `target_currency` parameter to specify the desired output
currency.

### Single account behavior

When querying a single account without specifying `target_currency`, the Portfolio
automatically converts values to that account's base currency:

```python
# Returns exposure in the account's base currency (e.g., USD)
exposure = portfolio.net_exposures(venue=BINANCE, account_id=account_id)
```

### Multi-account behavior

When querying multiple accounts simultaneously, behavior depends on whether you query
all instruments (`net_exposures()`) or a single instrument (`net_exposure()`):

**For `net_exposures()` (all instruments):**

- **Same base currency**: Automatically converts to the common base currency.
- **Different base currencies**: Returns a dict with multiple currencies, each converted
  to its account's base currency. Provide `target_currency` for single-currency results.

**For `net_exposure()` (single instrument across accounts):**

- **Different base currencies**: Returns `None` unless you provide `target_currency`.

```python
# Scenario 1: Multiple accounts, all with USD base currency
exposures = portfolio.net_exposures(venue=BINANCE)
# Returns {USD: Money(...)}

# Scenario 2: Multiple accounts with different base currencies (USD and EUR)
exposures = portfolio.net_exposures(venue=BINANCE)
# Returns {USD: Money(...), EUR: Money(...)}

# Force single currency across accounts
exposures = portfolio.net_exposures(venue=BINANCE, target_currency=USD)
# Returns {USD: Money(...)}
```

### Conversion failures

When `target_currency` is provided and currency conversion fails, behavior depends on
the method type:

- **Single-value methods** (`realized_pnl`, `unrealized_pnl`, `total_pnl`, `net_exposure`):
  Return `None` and log an error to prevent incorrect values.
- **Dict-returning methods** (`realized_pnls`, `unrealized_pnls`, `total_pnls`, `net_exposures`):
  Omit instruments that fail conversion but return results for successful conversions.

:::warning
Ensure exchange rate data is available when using `target_currency` for cross-currency
aggregation.
:::

### Conversion price types

When converting exposures to a target currency, the Portfolio uses different price types
depending on the position composition:

- **All long positions**: Uses `BID` prices (conservative for long exposure).
- **All short positions**: Uses `ASK` prices (conservative for short exposure).
- **Mixed positions**: Uses `MID` prices (neutral when both long and short exist).

This ensures conversions reflect realistic market conditions where you would liquidate
long positions at bid and cover short positions at ask. For mixed positions, mid-pricing
provides a neutral valuation.

If `use_mark_xrates` is enabled in the portfolio configuration, `MARK` prices replace
`MID` prices for mixed positions and general conversions.

## Portfolio statistics

There are a variety of [built-in portfolio statistics](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/analysis/src/statistics)
which are used to analyse a trading portfolios performance for both backtests and live trading.

The statistics are generally categorized as follows.

- PnLs based statistics (per currency)
- Returns based statistics
- Positions based statistics
- Orders based statistics

It's also possible to call a traders `PortfolioAnalyzer` and calculate statistics at any arbitrary
time, including *during* a backtest, or live trading session.

## Custom statistics

Custom portfolio statistics can be defined by inheriting from the `PortfolioStatistic`
base class, and implementing any of the `calculate_` methods.

For example, the following is the implementation for the built-in `WinRate` statistic:

```python
import pandas as pd
from typing import Any
from nautilus_trader.analysis.statistic import PortfolioStatistic


class WinRate(PortfolioStatistic):
    """
    Calculates the win rate from a realized PnLs series.
    """

    def calculate_from_realized_pnls(self, realized_pnls: pd.Series) -> Any | None:
        # Preconditions
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        # Calculate statistic
        winners = [x for x in realized_pnls if x > 0.0]
        losers = [x for x in realized_pnls if x <= 0.0]

        return len(winners) / float(max(1, (len(winners) + len(losers))))
```

These statistics can then be registered with a traders `PortfolioAnalyzer`.

```python
stat = WinRate()

# Register with the portfolio analyzer
engine.portfolio.analyzer.register_statistic(stat)
```

:::info
See the `PortfolioAnalyzer` [API Reference](../api_reference/analysis.md#class-portfolioanalyzer) for details on available methods.
:::

:::tip
Ensure your statistic is robust to degenerate inputs such as ``None``, empty series, or insufficient data.
Return ``None`` for unknown/incalculable values, or a reasonable default like ``0.0`` when semantically appropriate (e.g., win rate with no trades).
:::

## Backtest analysis

Following a backtest run, a performance analysis will be carried out by passing realized PnLs, returns, positions and orders data to each registered
statistic in turn. Any output is then displayed in the tear sheet under the `Portfolio Performance` heading, grouped as:

- Realized PnL statistics (per currency)
- Returns statistics (for the entire portfolio)
- General statistics derived from position and order data (for the entire portfolio)

## Related guides

- [Positions](positions.md) - Position tracking within portfolios.
- [Reports](reports.md) - Generate portfolio analysis reports.
- [Visualization](visualization.md) - Visualize portfolio performance.
