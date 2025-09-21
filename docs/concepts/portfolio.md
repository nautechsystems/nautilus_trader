# Portfolio

:::info
We are currently working on this concept guide.
:::

The Portfolio serves as the central hub for managing and tracking all positions across active strategies for the trading node or backtest.
It consolidates position data from multiple instruments, providing a unified view of your holdings, risk exposure, and overall performance.
Explore this section to understand how NautilusTrader aggregates and updates portfolio state to support effective trading and risk management.

## Portfolio statistics

There are a variety of [built-in portfolio statistics](https://github.com/nautechsystems/nautilus_trader/tree/develop/nautilus_trader/analysis/statistics)
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

:::info
See the `PortfolioAnalyzer` [API Reference](../api_reference/analysis.md#class-portfolioanalyzer) for details on available methods.
:::
```

:::tip
Ensure your statistic is robust to degenerate inputs such as ``None``, empty series, or insufficient data.

The expectation is that you would then return ``None``, NaN or a reasonable default.
:::

## Backtest analysis

Following a backtest run, a performance analysis will be carried out by passing realized PnLs, returns, positions and orders data to each registered
statistic in turn, calculating their values (with a default configuration). Any output is then displayed in the tear sheet
under the `Portfolio Performance` heading, grouped as.

- Realized PnL statistics (per currency)
- Returns statistics (for the entire portfolio)
- General statistics derived from position and order data (for the entire portfolio)
