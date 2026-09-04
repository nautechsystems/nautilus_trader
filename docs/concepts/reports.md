# Reports

This guide explains the portfolio analysis and reporting capabilities provided by the `ReportProvider`
class, and how these reports are used for PnL accounting and backtest post-run analysis.

## Overview

`ReportProvider` turns cached orders, fills, positions, and account states into pandas DataFrames
for analysis and visualization. These reports help you evaluate strategy performance, analyze
execution quality, and verify PnL accounting. The same reports are available in backtesting and live
trading, which keeps performance evaluation and strategy comparison consistent across both.

Reports can be generated using two approaches:

- **Backtest methods**: `BacktestEngine.generate_orders_report()` and its siblings read the engine's
  own cache. `BacktestNode` exposes the same methods, taking the run config ID as the first argument.
- **`ReportProvider` directly**: pass any collection of orders or positions, such as a live node's
  cache or a filtered cache query.

Every method returns an empty DataFrame when no matching data exists.

Report generation requires pandas, which `nautilus_trader.analysis` imports lazily: the module
imports without pandas installed, and the `ImportError` surfaces when you generate a report. The
`visualization` extra installs it.

## Available reports

The `ReportProvider` class offers several static methods to generate reports from trading data.
Each report returns a pandas DataFrame with specific columns and indexing for easy analysis.

### Orders report

Generates a full view of all orders:

```python
from nautilus_trader.analysis import ReportProvider

# From a completed backtest run
orders_report = engine.generate_orders_report()

# Or from any cache, such as a live node's
orders_report = ReportProvider.generate_orders_report(cache.orders())
```

**Returns `pd.DataFrame`. Columns include:**

| Column            | Description                                        |
| ----------------- | -------------------------------------------------- |
| `client_order_id` | Index - unique order identifier.                   |
| `instrument_id`   | Trading instrument.                                |
| `strategy_id`     | Strategy that created the order.                   |
| `trader_id`       | Trader identifier.                                 |
| `account_id`      | Account identifier (if assigned).                  |
| `venue_order_id`  | Venue-assigned order ID (if accepted).             |
| `side`            | BUY or SELL.                                       |
| `type`            | MARKET, LIMIT, etc.                                |
| `status`          | Current order status.                              |
| `quantity`        | Original order quantity (string).                  |
| `filled_qty`      | Amount filled (string).                            |
| `price`           | Limit price (string, order-type dependent).        |
| `avg_px`          | Average fill price (string, if filled).            |
| `time_in_force`   | Time-in-force instruction.                         |
| `ts_init`         | Order initialization timestamp (Unix nanoseconds). |
| `ts_last`         | Last update timestamp (Unix nanoseconds).          |

Additional columns vary by order type, such as `trigger_price` for stop orders and `expire_time_ns`
for GTD orders. See `Order.to_dict()` for the complete field list.

### Order fills report

Provides a summary of filled orders (one row per order):

```python
# From a completed backtest run
fills_report = engine.generate_order_fills_report()

# Or from any cache
fills_report = ReportProvider.generate_order_fills_report(cache.orders())
```

This report includes only orders with `filled_qty > 0` and contains the same columns as the
orders report, but filtered to executed orders only. Note that `ts_init` and `ts_last` are
converted to datetime objects in this report for easier analysis.

### Fills report

Details individual fill events (one row per fill):

```python
# From a completed backtest run
fills_report = engine.generate_fills_report()

# Or from any cache
fills_report = ReportProvider.generate_fills_report(cache.orders())
```

**Returns `pd.DataFrame`. Columns include:**

| Column            | Description                              |
| ----------------- | ---------------------------------------- |
| `client_order_id` | Index - order identifier.                |
| `trade_id`        | Unique trade/fill identifier.            |
| `venue_order_id`  | Venue-assigned order ID.                 |
| `instrument_id`   | Trading instrument.                      |
| `strategy_id`     | Strategy that created the order.         |
| `account_id`      | Account identifier.                      |
| `position_id`     | Associated position ID (if applicable).  |
| `order_side`      | BUY or SELL.                             |
| `order_type`      | Order type (MARKET, LIMIT, etc.).        |
| `last_px`         | Fill execution price (string).           |
| `last_qty`        | Fill execution quantity (string).        |
| `currency`        | Currency of the fill.                    |
| `liquidity_side`  | MAKER or TAKER.                          |
| `commission`      | Commission amount and currency (string). |
| `ts_event`        | Fill timestamp (datetime).               |
| `ts_init`         | Initialization timestamp (datetime).     |

See `OrderFilled.to_dict()` for the complete field list; the report drops its `type` column.

### Positions report

Position analysis including snapshots:

```python
# From a completed backtest run, which includes snapshots automatically
positions_report = engine.generate_positions_report()

# Or from any cache
positions_report = ReportProvider.generate_positions_report(
    positions=cache.positions(),
    snapshots=cache.position_snapshots(),  # Needed for NETTING OMS totals
)
```

**Returns `pd.DataFrame`. Columns include:**

| Column             | Description                                           |
| ------------------ | ----------------------------------------------------- |
| `position_id`      | Index - unique position identifier.                   |
| `instrument_id`    | Trading instrument.                                   |
| `strategy_id`      | Strategy that managed the position.                   |
| `trader_id`        | Trader identifier.                                    |
| `account_id`       | Account identifier.                                   |
| `opening_order_id` | Order ID that opened the position.                    |
| `closing_order_id` | Order ID that closed the position.                    |
| `entry`            | Entry side (BUY or SELL).                             |
| `side`             | Position side (LONG, SHORT, or FLAT).                 |
| `quantity`         | Current position size (string).                       |
| `peak_qty`         | Maximum size reached (string).                        |
| `avg_px_open`      | Average entry price (float).                          |
| `avg_px_close`     | Average exit price (float, if closed).                |
| `commissions`      | Commissions paid, one entry per currency (list).      |
| `realized_pnl`     | Realized profit/loss in the cost currency (string).   |
| `realized_return`  | Realized return as a ratio (float), so `0.05` is 5%.  |
| `ts_init`          | Position initialization timestamp (Unix nanoseconds). |
| `ts_opened`        | Opening timestamp (datetime).                         |
| `ts_last`          | Last update timestamp (Unix nanoseconds).             |
| `ts_closed`        | Closing timestamp (datetime or NA).                   |
| `duration_ns`      | Position duration in nanoseconds.                     |
| `is_snapshot`      | Whether this is a historical snapshot.                |

Snapshot rows are indexed by a generated ID derived from the original position ID, so use
`is_snapshot` rather than the index to separate archived cycles from live positions. See
`Position.to_dict()` for the complete field list; the report drops `signed_qty`, `base_currency`,
`quote_currency`, and `settlement_currency`.

### Account report

Tracks account balance and margin changes over time:

```python
from nautilus_trader.model import Venue

venue = Venue("BINANCE")

# From a completed backtest run
account_report = engine.generate_account_report(venue=venue)

# Or from any cache
account_report = ReportProvider.generate_account_report(cache.account_for_venue(venue))
```

`BacktestEngine.generate_account_report()` requires `venue` or `account_id` and raises `ValueError`
when both are omitted. `account_id` takes precedence when both are supplied, and an unknown account
yields an empty DataFrame.

**Returns `pd.DataFrame`. Columns include:**

| Column          | Description                                |
| --------------- | ------------------------------------------ |
| `ts_event`      | Index - timestamp of account state change. |
| `account_id`    | Account identifier.                        |
| `account_type`  | Type of account (e.g., SPOT, MARGIN).      |
| `base_currency` | Base currency for the account.             |
| `total`         | Total balance amount (string).             |
| `free`          | Available balance (string).                |
| `locked`        | Balance locked in orders (string).         |
| `currency`      | Currency of the balance.                   |
| `reported`      | Whether balance was reported by venue.     |
| `margins`       | Margin information (list, if applicable).  |
| `info`          | Additional venue-specific information.     |

Each row represents a balance entry; accounts with multiple currencies produce multiple rows
per account state event.

## PnL accounting considerations

Accurate PnL accounting requires careful consideration of several factors:

### Position-based PnL

- **Realized PnL**: Calculated when positions are partially or fully closed.
- **Unrealized PnL**: Marked-to-market using current prices. `Position.unrealized_pnl(last)` marks
  an open position at a given `Price`.
- **Commission impact**: Only included when in the position's cost currency. See
  [Positions](positions.md) for how base-currency commissions on spot pairs adjust position size
  instead.

:::warning
PnL calculations depend on the OMS type. In `NETTING` OMS, position snapshots
preserve historical PnL when positions reopen. Always include snapshots in
reports for accurate total PnL calculation. In `HEDGING` OMS, snapshots are
not used since each position has a unique ID and is never reopened.
:::

### Multi-currency accounting

When dealing with multiple currencies:

- Each position tracks PnL in its cost currency: quote for linear contracts, base for inverse
  contracts, and settlement for quanto contracts.
- Portfolio aggregation requires currency conversion. `Portfolio.realized_pnls(target_currency=...)`
  does this with cached exchange rates; see
  [Supported conversions](portfolio.md#supported-conversions).
- Commission currencies may differ from the position's cost currency.

```python
from decimal import Decimal

# Accessing PnL across positions
for position in cache.positions_closed():
    realized = position.realized_pnl  # Money in the position's cost currency, or None

    if realized is None or realized.currency == base_currency:
        continue

    # Converting by hand: cache.get_xrate() returns a float, so wrap rates as Decimal
    rate = Decimal(str(my_fx_rates[(realized.currency, base_currency)]))
    converted = realized.as_decimal() * rate
```

### Snapshot considerations

For `NETTING` OMS, an accurate instrument total adds the realized PnL of every archived cycle to the
live position. See [Position snapshotting](positions.md#position-snapshotting) for how the execution
engine archives a closed cycle.

```python
from decimal import Decimal

from nautilus_trader.model import Money

pnl_by_currency = {}

for position in cache.positions(instrument_id=instrument_id):
    # Archived cycles are stored under the live position's ID
    snapshots = cache.position_snapshots(position_id=position.id)

    for pnl in (position.realized_pnl, *(s.realized_pnl for s in snapshots)):
        if pnl is None:
            continue

        running = pnl_by_currency.get(pnl.currency, Decimal(0))
        pnl_by_currency[pnl.currency] = running + pnl.as_decimal()

# Create Money objects for each currency
total_pnls = [Money.from_decimal(amount, currency) for currency, amount in pnl_by_currency.items()]
```

## Backtest post-run analysis

After a backtest completes, analysis is available through result statistics
and generated reports.

### Accessing backtest results

```python
# After backtest run
engine.run()

# Access result statistics
result = engine.get_result()

# Generate reports from the backtest engine
fills_report = engine.generate_fills_report()
venue = engine.list_venues()[0]
account_report = engine.generate_account_report(venue=venue)

# Or access data directly for custom analysis
orders = engine.cache.orders()
positions = engine.cache.positions()
snapshots = engine.cache.position_snapshots()
```

### Portfolio statistics

The backtest result provides performance metrics:

```python
# Access backtest result statistics
result = engine.get_result()

# Get different categories of statistics
stats_pnls = result.stats_pnls  # Keyed by currency code, then statistic name
stats_returns = result.stats_returns  # Keyed by statistic name
stats_general = result.stats_general  # Keyed by statistic name
```

Each statistic appears in exactly one category, determined by the input it consumes: realized PnLs,
returns, or positions.

:::info
See the [Portfolio guide](portfolio.md#portfolio-statistics) for the default statistic set, how each
category is derived, and the difference between position returns and portfolio returns.
:::

### Visualization

NautilusTrader provides interactive tearsheets and plots via Plotly:

```python
from nautilus_trader.analysis import create_tearsheet

# After backtest run
engine.run()

# Generate interactive HTML tearsheet
create_tearsheet(engine, output_path="tearsheet.html")
```

This creates an interactive HTML report with:

- Equity curve
- Drawdown analysis
- Monthly returns heatmap
- Performance statistics table
- Returns distribution

For more control, generate individual plots:

```python
import pandas as pd

from nautilus_trader.analysis import create_equity_curve

returns = pd.Series(
    [0.01, -0.005, 0.002],
    index=pd.date_range("2024-01-01", periods=3, tz="UTC"),
)
fig = create_equity_curve(returns, title="My Strategy Equity")
fig.show()  # Display in browser
fig.write_image("equity.png")  # Export to PNG (requires kaleido)
```

Install visualization dependencies:

```bash
uv pip install --pre "nautilus_trader[visualization]"
```

## Report generation patterns

### Live trading

During live trading, generate reports periodically:

```python
from datetime import timedelta

from nautilus_trader.analysis import ReportProvider
from nautilus_trader.common import DataActor
from nautilus_trader.common import TimeEvent


class ReportingActor(DataActor):
    def on_start(self) -> None:
        # Schedule periodic reporting
        self.clock.set_timer(
            name="generate_reports",
            interval=timedelta(minutes=30),
            callback=self.generate_reports,
        )

    def generate_reports(self, event: TimeEvent) -> None:
        # Generate and log reports
        positions_report = ReportProvider.generate_positions_report(
            positions=self.cache.positions(),
            snapshots=self.cache.position_snapshots(),
        )

        # Save or transmit report
        positions_report.to_csv(f"positions_{event.ts_event}.csv")
```

### Performance analysis

For backtest analysis:

```python
import pandas as pd

# Run the backtest
engine.run()

# Collect results
positions_closed = engine.cache.positions_closed()
result = engine.get_result()
stats_pnls = result.stats_pnls
stats_returns = result.stats_returns
stats_general = result.stats_general

# Create summary dictionary
results = {
    "total_positions": len(positions_closed),
    "pnl_total": stats_pnls.get("USD", {}).get("PnL (total)"),
    "win_rate": stats_pnls.get("USD", {}).get("Win Rate"),
    "sharpe_ratio": stats_returns.get("Sharpe Ratio (252 days)"),
    "profit_factor": stats_returns.get("Profit Factor"),
    "long_ratio": stats_general.get("Long Ratio"),
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
- **Portfolio**: Computes its statistics from the same cache data independently, not from these
  reports.
- **BacktestEngine**: Exposes the report methods used for post-run analysis and visualization.
- **Position snapshots**: Required for accurate PnL reporting in `NETTING` OMS.

## Related guides

- [Visualization](visualization.md) - Interactive tearsheets and charts from backtest results.
- [Portfolio](portfolio.md) - Portfolio statistics and performance metrics.
- [Backtesting](backtesting/) - Running backtests that generate reports.
- [Cache](cache.md) - Cache system that stores data for reports.
