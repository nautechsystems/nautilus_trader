# EMA Crossover

Dual exponential moving average crossover strategy for trend following.

## Strategy overview

Subscribes to quotes for a single instrument, maintains fast and slow
exponential moving averages of the mid-price, and submits market orders
when the fast EMA crosses the slow EMA.

### Signal generation

The strategy computes the mid-price from each incoming quote and feeds it
to both EMAs. Once both EMAs are initialized (enough data points received),
the strategy tracks whether the fast EMA is above or below the slow EMA:

- **Buy signal**: fast EMA crosses above slow EMA (bullish crossover).
- **Sell signal**: fast EMA crosses below slow EMA (bearish crossover).

Each crossover triggers a single market order for `trade_size`. The
strategy does not manage position sizing, stop losses, or take profits.
It serves as a minimal template for building momentum or trend-following
strategies.

### EMA behavior

The exponential moving average gives more weight to recent prices. The
smoothing factor is `2 / (period + 1)`. A shorter `fast_period` reacts
faster to price changes; a longer `slow_period` smooths out noise.

Common period pairs:

| Style        | Fast | Slow | Crossover frequency |
|--------------|------|------|---------------------|
| Scalping     | 5    | 20   | High                |
| Intraday     | 10   | 50   | Medium              |
| Swing        | 20   | 100  | Low                 |

## Parameters

The `EmaCross` strategy takes constructor parameters directly (no separate
config struct):

| Parameter        | Type           | Description                                         |
|------------------|----------------|-----------------------------------------------------|
| `instrument_id`  | `InstrumentId` | Instrument to subscribe to and trade.               |
| `trade_size`     | `Quantity`     | Order quantity for each crossover signal.            |
| `fast_period`    | `usize`        | Fast EMA period. Shorter periods react faster.       |
| `slow_period`    | `usize`        | Slow EMA period. Longer periods filter noise.        |

## Rust usage

```rust
use nautilus_trading::examples::strategies::EmaCross;

let strategy = EmaCross::new(
    InstrumentId::from("BTC-USDT-SWAP.OKX"),
    Quantity::from("0.01"),
    10,  // fast_period
    50,  // slow_period
);

node.add_strategy(strategy)?;
```

## Extending this strategy

This strategy is intentionally minimal. Common extensions:

- **Position awareness**: check existing positions before submitting orders
  to avoid doubling up or to add to winners.
- **Risk management**: add stop-loss or take-profit logic in
  `on_order_filled`.
- **Multiple timeframes**: use bar data instead of raw quotes for the EMA
  calculation, reducing noise on lower timeframes.
- **Filters**: add volume, volatility, or time-of-day filters to suppress
  signals during unfavorable conditions.
