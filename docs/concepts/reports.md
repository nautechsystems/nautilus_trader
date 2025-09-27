# Reports

:::info
We are currently working on this concept guide.
:::

This guide explains the portfolio analysis and reporting capabilities provided by the `ReportProvider`
class, and how these reports are used for PnL accounting and backtest post-run analysis.

## Overview

The `ReportProvider` class in NautilusTrader generates structured analytical reports from
trading data, transforming raw orders, fills, positions, and account states into pandas DataFrames
for analysis and visualization. These reports are essential for understanding strategy performance,
analyzing execution quality, and ensuring accurate PnL accounting.

Reports can be generated using two approaches:

- **Trader helper methods** (recommended): Convenient methods like `trader.generate_orders_report()`.
- **ReportProvider directly**: For more control over data selection and filtering.

Reports provide consistent analytics across both backtesting and live trading environments,
enabling reliable performance evaluation and strategy comparison.

## Available reports

The `ReportProvider` class offers several static methods to generate reports from trading data.
Each report returns a pandas DataFrame with specific columns and indexing for easy analysis.

### Orders report

Generates a comprehensive view of all orders:

```python
# Using Trader helper method (recommended)
orders_report = trader.generate_orders_report()

# Or using ReportProvider directly
from nautilus_trader.analysis.reporter import ReportProvider
orders = cache.orders()
orders_report = ReportProvider.generate_orders_report(orders)
```

**Returns `pd.DataFrame` with:**

| Column             | Description                                   |
|--------------------|-----------------------------------------------|
| `client_order_id`  | Index - unique order identifier.              |
| `instrument_id`    | Trading instrument.                           |
| `strategy_id`      | Strategy that created the order.              |
| `side`             | BUY or SELL.                                  |
| `type`             | MARKET, LIMIT, etc.                           |
| `status`           | Current order status.                         |
| `quantity`         | Original order quantity (string).             |
| `filled_qty`       | Amount filled (string).                       |
| `price`            | Limit price (string if set).                  |
| `avg_px`           | Average fill price (float if set).            |
| `ts_init`          | Order initialization timestamp (nanoseconds). |
| `ts_last`          | Last update timestamp (nanoseconds).          |

### Order fills report

Provides a summary of filled orders (one row per order):

```python
# Using Trader helper method (recommended)
fills_report = trader.generate_order_fills_report()

# Or using ReportProvider directly
orders = cache.orders()
fills_report = ReportProvider.generate_order_fills_report(orders)
```

This report includes only orders with `filled_qty > 0` and contains the same columns as the
orders report, but filtered to executed orders only. Note that `ts_init` and `ts_last` are
converted to datetime objects in this report for easier analysis.

### Fills report

Details individual fill events (one row per fill):

```python
# Using Trader helper method (recommended)
fills_report = trader.generate_fills_report()

# Or using ReportProvider directly
orders = cache.orders()
fills_report = ReportProvider.generate_fills_report(orders)
```

**Returns `pd.DataFrame` with:**

| Column             | Description                          |
|--------------------|--------------------------------------|
| `client_order_id`  | Index - order identifier.            |
| `trade_id`         | Unique trade/fill identifier.        |
| `venue_order_id`   | Venue-assigned order ID.             |
| `last_px`          | Fill execution price (string).       |
| `last_qty`         | Fill execution quantity (string).    |
| `liquidity_side`   | MAKER or TAKER.                      |
| `commission`       | Commission amount and currency.      |
| `ts_event`         | Fill timestamp (datetime).           |
| `ts_init`          | Initialization timestamp (datetime). |

### Positions report

Comprehensive position analysis including snapshots:

```python
# Using Trader helper method (recommended)
# Automatically includes snapshots for NETTING OMS
positions_report = trader.generate_positions_report()

# Or using ReportProvider directly
positions = cache.positions()
snapshots = cache.position_snapshots()  # For NETTING OMS
positions_report = ReportProvider.generate_positions_report(
    positions=positions,
    snapshots=snapshots
)
```

**Returns `pd.DataFrame` with:**

| Column             | Description                            |
|--------------------|----------------------------------------|
| `position_id`      | Index - unique position identifier.    |
| `instrument_id`    | Trading instrument.                    |
| `strategy_id`      | Strategy that managed the position.    |
| `entry`            | Entry side (BUY or SELL).              |
| `side`             | Position side (LONG, SHORT, or FLAT).  |
| `quantity`         | Position size.                         |
| `peak_qty`         | Maximum size reached.                  |
| `avg_px_open`      | Average entry price.                   |
| `avg_px_close`     | Average exit price (if closed).        |
| `realized_pnl`     | Realized profit/loss.                  |
| `realized_return`  | Return percentage.                     |
| `ts_opened`        | Opening timestamp (datetime).          |
| `ts_closed`        | Closing timestamp (datetime or NA).    |
| `duration_ns`      | Position duration in nanoseconds.      |
| `is_snapshot`      | Whether this is a historical snapshot. |

### Account report

Tracks account balance and margin changes over time:

```python
# Using Trader helper method (recommended)
# Requires venue parameter
from nautilus_trader.model.identifiers import Venue
venue = Venue("BINANCE")
account_report = trader.generate_account_report(venue)

# Or using ReportProvider directly
account = cache.account(account_id)
account_report = ReportProvider.generate_account_report(account)
```

**Returns `pd.DataFrame` with:**

| Column             | Description                                |
|--------------------|--------------------------------------------|
| `ts_event`         | Index - timestamp of account state change. |
| `account_id`       | Account identifier.                        |
| `account_type`     | Type of account (e.g., SPOT, MARGIN).      |
| `base_currency`    | Base currency for the account.             |
| `total`            | Total balance amount.                      |
| `free`             | Available balance.                         |
| `locked`           | Balance locked in orders.                  |
| `currency`         | Currency of the balance.                   |
| `reported`         | Whether balance was reported by venue.     |
| `margins`          | Margin information (if applicable).        |
| `info`             | Additional venue-specific information.     |

## PnL accounting considerations

Accurate PnL accounting requires careful consideration of several factors:

### Position-based PnL

- **Realized PnL**: Calculated when positions are partially or fully closed.
- **Unrealized PnL**: Marked-to-market using current prices.
- **Commission impact**: Only included when in settlement currency.

:::warning
PnL calculations depend on the OMS type. In `NETTING` mode, position snapshots
preserve historical PnL when positions reopen. Always include snapshots in
reports for accurate total PnL calculation.
:::

### Multi-currency accounting

When dealing with multiple currencies:

- Each position tracks PnL in its settlement currency.
- Portfolio aggregation requires currency conversion.
- Commission currencies may differ from settlement currency.

```python
# Accessing PnL across positions
for position in positions:
    realized = position.realized_pnl  # In settlement currency
    unrealized = position.unrealized_pnl(last_price)

    # Handle multi-currency aggregation (illustrative)
    # Note: Currency conversion requires user-provided exchange rates
    if position.settlement_currency != base_currency:
        # Apply conversion rate from your data source
        # rate = get_exchange_rate(position.settlement_currency, base_currency)
        # realized_converted = realized.as_double() * rate
        pass
```

### Snapshot considerations

For `NETTING` OMS configurations:

```python
from nautilus_trader.model.objects import Money

# Include snapshots for complete PnL (per currency)
pnl_by_currency = {}

# Add PnL from current positions
for position in cache.positions(instrument_id=instrument_id):
    if position.realized_pnl:
        currency = position.realized_pnl.currency
        if currency not in pnl_by_currency:
            pnl_by_currency[currency] = 0.0
        pnl_by_currency[currency] += position.realized_pnl.as_double()

# Add PnL from historical snapshots
for snapshot in cache.position_snapshots(instrument_id=instrument_id):
    if snapshot.realized_pnl:
        currency = snapshot.realized_pnl.currency
        if currency not in pnl_by_currency:
            pnl_by_currency[currency] = 0.0
        pnl_by_currency[currency] += snapshot.realized_pnl.as_double()

# Create Money objects for each currency
total_pnls = [Money(amount, currency) for currency, amount in pnl_by_currency.items()]
```

## Backtest post-run analysis

After a backtest completes, comprehensive analysis is available through various reports
and the portfolio analyzer.

### Accessing backtest results

```python
# After backtest run
engine.run(start=start_time, end=end_time)

# Generate reports using Trader helper methods
orders_report = engine.trader.generate_orders_report()
positions_report = engine.trader.generate_positions_report()
fills_report = engine.trader.generate_fills_report()

# Or access data directly for custom analysis
orders = engine.cache.orders()
positions = engine.cache.positions()
snapshots = engine.cache.position_snapshots()
```

### Portfolio statistics

The portfolio analyzer provides comprehensive performance metrics:

```python
# Access portfolio analyzer
portfolio = engine.portfolio

# Get different categories of statistics
stats_pnls = portfolio.analyzer.get_performance_stats_pnls()
stats_returns = portfolio.analyzer.get_performance_stats_returns()
stats_general = portfolio.analyzer.get_performance_stats_general()
```

:::info
For detailed information about available statistics and creating custom metrics,
see the [Portfolio guide](portfolio.md#portfolio-statistics). The Portfolio guide covers:

- Built-in statistics categories (PnLs, returns, positions, orders based).
- Creating custom statistics with `PortfolioStatistic`.
- Registering and using custom metrics.

:::

### Visualization

Reports integrate well with visualization tools:

```python
import matplotlib.pyplot as plt

# Plot cumulative returns
returns = positions_report["realized_return"].cumsum()
returns.plot(title="Cumulative Returns")
plt.show()

# Analyze fill quality (commission is a Money string e.g. "0.50 USD")
# Extract numeric values and currency
fills_report["commission_value"] = fills_report["commission"].str.split().str[0].astype(float)
fills_report["commission_currency"] = fills_report["commission"].str.split().str[1]

# Group by liquidity side and currency
commission_by_side = fills_report.groupby(["liquidity_side", "commission_currency"])["commission_value"].sum()
commission_by_side.plot.bar()
plt.title("Commission by Liquidity Side and Currency")
plt.show()
```

## Report generation patterns

### Live trading

During live trading, generate reports periodically:

```python
import pandas as pd

class ReportingActor(Actor):
    def on_start(self):
        # Schedule periodic reporting
        self.clock.set_timer(
            name="generate_reports",
            interval=pd.Timedelta(minutes=30),
            callback=self.generate_reports
        )

    def generate_reports(self, event):
        # Generate and log reports
        positions_report = self.trader.generate_positions_report()

        # Save or transmit report
        positions_report.to_csv(f"positions_{event.ts_event}.csv")
```

### Performance analysis

For backtest analysis:

```python
import pandas as pd

# Run the backtest
engine.run(start=start_time, end=end_time)

# Collect comprehensive results
positions_closed = engine.cache.positions_closed()
stats_pnls = engine.portfolio.analyzer.get_performance_stats_pnls()
stats_returns = engine.portfolio.analyzer.get_performance_stats_returns()
stats_general = engine.portfolio.analyzer.get_performance_stats_general()

# Create summary dictionary
results = {
    "total_positions": len(positions_closed),
    "pnl_total": stats_pnls.get("PnL (total)"),
    "sharpe_ratio": stats_returns.get("Sharpe Ratio (252 days)"),
    "profit_factor": stats_general.get("Profit Factor"),
    "win_rate": stats_general.get("Win Rate"),
}

# Display results
results_df = pd.DataFrame([results])
print(results_df.T)  # Transpose for vertical display
```

:::info
Reports are generated from in-memory data structures. For large-scale analysis
or long-running systems, consider persisting reports to a database for efficient
querying. See the [Cache guide](cache.md) for persistence options.
:::

## Integration with other components

The `ReportProvider` works with several system components:

- **Cache**: Source of all trading data (orders, positions, accounts) for reports.
- **Portfolio**: Uses reports for performance analysis and metrics calculation.
- **BacktestEngine**: Leverages reports for post-run analysis and visualization.
- **Position snapshots**: Critical for accurate PnL reporting in `NETTING` OMS mode.

## Summary

The `ReportProvider` class offers a comprehensive suite of analytical reports for evaluating
trading performance. These reports transform raw trading data into structured DataFrames,
enabling detailed analysis of orders, fills, positions, and account states. Understanding
how to generate and interpret these reports is essential for strategy development,
performance evaluation, and accurate PnL accounting, particularly when dealing with
position snapshots in `NETTING` OMS configurations.
