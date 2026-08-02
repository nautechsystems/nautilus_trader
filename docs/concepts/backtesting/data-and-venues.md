# Backtest Data and Venues

## Data

Historical data advances the backtest clock, updates simulated market state, and drives strategy
callbacks. The venue's `book_type` determines which data can update the matching book, so the
configuration must match the available data.

Order book data exposes more execution detail than quotes, trades, or bars, but even a recorded
book cannot show how a simulated order would have changed the market. NautilusTrader supports the
following data in descending order of detail:

```mermaid
flowchart LR
    L3["L3 Order Book<br/>(market-by-order)"]
    L2["L2 Order Book<br/>(market-by-price)"]
    L1["L1 Quotes<br/>(top of book)"]
    T["Trades"]
    B["Bars"]

    L3 --> L2 --> L1 --> T --> B

    style L3 fill:#2d5a3d,color:#fff
    style L2 fill:#3d6a4d,color:#fff
    style L1 fill:#4d7a5d,color:#fff
    style T fill:#5d8a6d,color:#fff
    style B fill:#6d9a7d,color:#fff
```

More granular data exposes more of the recorded queue and depth, while less granular data requires
more simulation assumptions.

- **L3 order book data (market-by-order)**: Individual orders at each recorded price level.
- **L2 order book data (market-by-price)**: Aggregate size at each recorded price level.
- **L1 quote ticks (market-by-price)**: Best bid and ask prices and sizes.
- **Trade ticks**: Recorded executions.
- **Bars**: Aggregated price and volume over fixed intervals.

### Choosing data: cost vs. accuracy

Bar data can be sufficient for early strategy development and is often cheaper and easier to
obtain than tick or order book data. It cannot establish intrabar price order, spread, depth, or
queue position, so execution-sensitive strategies need more granular validation.

:::tip
Start with bars to test the core signal when appropriate. Move to quotes, trades, or depth data
before relying on results that depend on spread, exact intrabar order, tight exits, or queue
position.
:::

## Venues

When initializing a venue for backtesting, you must specify its internal order `book_type` for
execution processing from the following options:

- `L1_MBP`: Level 1 market-by-price (default). Only the top level of the order book is maintained.
- `L2_MBP`: Level 2 market-by-price. Order book depth is maintained, with a single order aggregated
  per price level.
- `L3_MBO`: Level 3 market-by-order. Order book depth is maintained, with all individual orders
  tracked as provided by the data.

The `book_type` determines which data updates book state and drives matching. Data that does not
apply to the selected book is ignored for book and price updates, but the outer backtest clock still
advances. Strategies continue to receive subscribed data through the data engine. Precision
validation depends on the matching path; for example, a bar ignored by an L2 or L3 venue returns
before executable-bar precision checks.

| Data type          | L1_MBP            | L2_MBP            | L3_MBO            |
| ------------------ | ----------------- | ----------------- | ----------------- |
| `QuoteTick`        | Updates book      | *Ignored*         | *Ignored*         |
| `TradeTick`        | Triggers matching | Triggers matching | Triggers matching |
| `Bar`              | Updates book      | *Ignored*         | *Ignored*         |
| `OrderBookDelta`   | *Ignored*         | Updates book      | Updates book      |
| `OrderBookDeltas`  | *Ignored*         | Updates book      | Updates book      |
| `OrderBookDepth10` | Updates book      | Updates book      | Updates book      |

:::note
The granularity of the data must match the specified order `book_type`. Nautilus cannot generate
higher granularity data (L2 or L3) from lower-level data such as quotes, trades, or bars.
:::

:::warning
If you specify `L2_MBP` or `L3_MBO` as the venue's `book_type`, quotes and bars will not update the
book. Ensure you provide order book delta data, otherwise orders may appear as though they are never
filled.
:::

:::warning
When using `L1_MBP` (the default), order book deltas are ignored by the matching engine. If you
subscribe to order book deltas, set the venue `book_type` to `L2_MBP` or `L3_MBO`. This also applies
to sandbox execution, where the matching engine uses the same `book_type` configuration.
:::
