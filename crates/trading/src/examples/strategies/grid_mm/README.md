# Grid Market Maker

Inventory-aware grid market making strategy with configurable skew and
position limits.

## Strategy overview

Places a symmetric grid of limit buy and sell orders around the current
mid-price. Orders persist across ticks and are only replaced when the
mid-price moves beyond a configurable threshold, reducing unnecessary
cancel/replace churn. The grid is shifted by a skew proportional to the
current net position to discourage inventory buildup (Avellaneda-Stoikov
inspired).

### Order placement

On each requote the strategy:

1. Computes the mid-price from the latest quote.
2. Checks whether the mid has moved beyond `requote_threshold_bps` since
   the last grid placement.
3. Cancels all existing orders.
4. Calculates grid prices using geometric spacing:
   - Buy level N: `mid * (1 - grid_step_bps / 10000) ^ N - skew`
   - Sell level N: `mid * (1 + grid_step_bps / 10000) ^ N - skew`
5. Enforces `max_position` per side before placing each level, accounting
   for worst-case exposure from pending orders.

### Inventory skew

The skew shifts the entire grid to discourage further accumulation in the
direction of the current position:

```
skew = skew_factor * net_position
```

A positive net position shifts sell levels lower (more aggressive) and buy
levels lower (less aggressive), encouraging the market to take the long
inventory.

### Position limits

Before placing each order the strategy projects the worst-case per-side
exposure: current position plus all pending buy or sell orders. Orders that
would breach `max_position` are skipped.

## Configuration

| Parameter               | Type           | Default     | Description                                                                          |
|-------------------------|----------------|-------------|--------------------------------------------------------------------------------------|
| `instrument_id`         | `InstrumentId` | *required*  | Instrument to trade.                                                                 |
| `max_position`          | `Quantity`     | *required*  | Hard cap on net exposure (long or short).                                            |
| `trade_size`            | `Option<Qty>`  | `None`      | Size per grid level. When `None`, resolves from the instrument's `min_quantity`.      |
| `num_levels`            | `usize`        | `3`         | Number of price levels on each side (buy and sell).                                   |
| `grid_step_bps`         | `u32`          | `10`        | Grid spacing in basis points of mid-price. 10 bps = 0.1%.                            |
| `skew_factor`           | `f64`          | `0.0`       | Inventory skew multiplier. Higher values skew more aggressively against position.     |
| `requote_threshold_bps` | `u32`          | `5`         | Minimum mid-price move in bps before re-quoting. Reduces cancel/replace frequency.   |
| `expire_time_secs`      | `Option<u64>`  | `None`      | Order expiry in seconds. When set, orders use GTD time-in-force.                      |
| `on_cancel_resubmit`    | `bool`         | `false`     | Resubmit the grid on the next quote after an order cancel event.                      |

### Tuning guidelines

- **Tight spreads, high volume**: use `grid_step_bps=5`, `num_levels=5`,
  `requote_threshold_bps=2`. Captures more ticks but generates higher
  order traffic.
- **Wide spreads, low volume**: use `grid_step_bps=20`, `num_levels=3`,
  `requote_threshold_bps=10`. Reduces unnecessary requotes on illiquid
  instruments.
- **Inventory control**: start with `skew_factor=0.5` and increase if the
  strategy accumulates directional inventory. Set `max_position` to the
  maximum tolerable exposure.

## Rust usage

```rust
use nautilus_trading::examples::strategies::{GridMarketMaker, GridMarketMakerConfig};

let config = GridMarketMakerConfig::new(
    InstrumentId::from("BTC-USDT-SWAP.OKX"),
    Quantity::from("10.0"),
)
.with_trade_size(Quantity::from("0.1"))
.with_num_levels(5)
.with_grid_step_bps(15)
.with_skew_factor(0.5)
.with_requote_threshold_bps(5);

let strategy = GridMarketMaker::new(config);
node.add_strategy(strategy)?;
```

## Python usage (v2)

Pass the config to `add_native_strategy` on a `LiveNode` or
`BacktestEngine`. Python provides the configuration; the strategy
runs entirely in Rust.

```python
from nautilus_trader.core.nautilus_pyo3.trading import GridMarketMakerConfig

config = GridMarketMakerConfig(
    instrument_id=InstrumentId.from_str("BTC-USDT-SWAP.OKX"),
    max_position=Quantity.from_str("10.0"),
    trade_size=Quantity.from_str("0.1"),
    num_levels=5,
    grid_step_bps=15,
    skew_factor=0.5,
    requote_threshold_bps=5,
)

node.add_native_strategy(config)
```
