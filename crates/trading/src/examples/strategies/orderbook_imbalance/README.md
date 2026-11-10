# Order Book Imbalance

`OrderBookImbalance` is one Rust strategy exposed to Python through the built-in strategy registry.
Each instance trades one instrument. It is a teaching example and has no demonstrated trading edge.

## Signal and orders

A managed book subscription maintains the current book. On each delta batch, the strategy requires:

- A positive spread and positive best bid/ask sizes.
- A larger best size strictly above `trigger_min_size` (default `100`).
- A smaller/larger size ratio strictly below `trigger_imbalance_ratio` (default `0.20`).
- At least `min_seconds_between_triggers` since the last signal (default `1.0`).
- No in-flight order belonging to this strategy.

When the bid is larger, the strategy buys at the best ask. When the ask is larger, it sells at the
best bid. Each order is a non-post-only limit order, capped at the smaller size and
`max_trade_size`. `time_in_force` defaults to FOK, which fills the full quantity or cancels the order.
Set it to IOC for venues such as Lighter and Hyperliquid: IOC fills the available quantity and
cancels any remainder. Other time-in-force values are rejected so orders cannot rest on the book.
Size truncates to instrument precision; a result of zero produces no order.
Prices, sizes, and ratio thresholds use exact decimal arithmetic.

L2 MBP best sizes represent price levels. For L3 MBO, the book API returns the first order at each
best price, rather than the aggregate level size. The [Databento notebook](../../../../../../examples/backtest/notebooks/databento_test_order_book_deltas.py)
retains these semantics with an L3 MBO subscription in dry-run mode, sending no order commands.

## Rust usage

After adding the instrument and simulated venue to a `BacktestEngine`:

```rust
use nautilus_model::{enums::TimeInForce, identifiers::InstrumentId, types::Quantity};
use nautilus_trading::examples::strategies::{OrderBookImbalance, OrderBookImbalanceConfig};

let config = OrderBookImbalanceConfig::builder()
    .instrument_id(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
    .max_trade_size(Quantity::from("1.000"))
    .time_in_force(TimeInForce::Ioc)
    .build();
engine.add_strategy(OrderBookImbalance::from_config(config)?)?;
```

The [Betfair example](../../../../../adapters/betfair/examples/betfair_backtest.rs) registers a separate
instance for each runner, with unique strategy IDs and order ID tags.

## Python usage

With the same instrument and simulated venue registered:

```python
from decimal import Decimal

from nautilus_trader.model import BookType, InstrumentId, Quantity, TimeInForce
from nautilus_trader.trading import OrderBookImbalanceConfig

config = OrderBookImbalanceConfig(
    instrument_id=InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID"),
    max_trade_size=Quantity.from_str("1.000"),
    trigger_min_size=Decimal("100"),
    trigger_imbalance_ratio=Decimal("0.20"),
    book_type=BookType.L2_MBP,
    time_in_force=TimeInForce.IOC,
)
engine.add_builtin_strategy("OrderBookImbalance", config)
```

`LiveNode` uses the same method and configuration. For `BacktestNode`, pass the run ID first:
`node.add_builtin_strategy(run_id, "OrderBookImbalance", config)`.

## Lifecycle and dry-run mode

Stop unsubscribes from deltas, cancels this strategy's open orders, and attempts to close its positions.
Resume restores the subscription and retains trigger spacing and counts. Reset clears runtime state.

On Lighter, an IOC fill below the minimum quantity or notional can leave a position that the
close-on-stop path rejects. Check residual positions after stopping; shutdown does not guarantee a flat account.

Set `dry_run=True` in Python or `.dry_run(true)` in Rust to report eligible signals without sending
order commands, including during shutdown. Dry-run retains thresholds, spacing, and the in-flight
guard. It rejects base configuration that enables managed stop, GTD expiry, or contingent orders.
`log_interval` controls best-size snapshot output; zero disables snapshots. Stop prints update and
signal counts in both modes.
