# Grid Market Making on dYdX v4

This tutorial walks through running a grid market making strategy on dYdX v4 using the
Rust-native `LiveNode`. By the end you will have a working grid quoter that places symmetric
limit orders around the mid-price, skews the grid to manage inventory, and automatically
resubmits orders when the protocol cancels expired short-term orders.

## Introduction

### What is grid market making?

A grid market maker maintains a ladder of resting buy and sell limit orders at fixed price
intervals around the current mid-price. When an order fills, the strategy profits from the
spread between buy and sell levels. The approach is conceptually simple but requires careful
inventory management to avoid accumulating a large directional position.

### Inventory skewing (Avellaneda-Stoikov inspired)

The strategy implements inventory-based skewing: when the position grows long, the entire grid
shifts downward (cheaper buys, cheaper sells) to encourage selling. When the position grows
short, the grid shifts upward. This is inspired by the Avellaneda-Stoikov framework for
optimal market making, adapted to a discrete grid.

### Why dYdX v4?

dYdX v4 is well-suited for market making strategies because:

- **Short-term orders** (~10s expiry) provide low-latency placement without on-chain storage
- **~0.5s block times** give fast confirmation cycles
- **No gas fees for cancellations** — short-term order cancels are free (GTB replay protection)
- **On-chain order book** — deterministic matching within each block
- **Batch cancel** — cancel all short-term orders in a single `MsgBatchCancel` call

## Prerequisites

### Funded dYdX account

You need a dYdX account with USDC collateral. See the
[Testnet setup](../integrations/dydx.md#testnet-setup) section in the integration guide
for instructions on creating and funding a testnet account.

### Environment variables

```bash
# For mainnet
export DYDX_PRIVATE_KEY="0x..."
export DYDX_WALLET_ADDRESS="dydx1..."

# For testnet
export DYDX_TESTNET_PRIVATE_KEY="0x..."
export DYDX_TESTNET_WALLET_ADDRESS="dydx1..."
```

## Strategy overview

### Geometric grid pricing

The strategy uses a **geometric grid** where each level is a fixed percentage (basis points)
away from the mid-price:

```
Buy level N:  mid × (1 - bps/10000)^N  - skew
Sell level N: mid × (1 + bps/10000)^N  - skew
```

Where `skew = skew_factor × net_position`.

For a 3-level grid with `grid_step_bps=100` (1%) around a mid of 1000.00:

```
                        Sell 3: 1030.30
                    Sell 2: 1020.10
                Sell 1: 1010.00
            ─── Mid: 1000.00 ───
                Buy 1:  990.00
                    Buy 2:  980.10
                        Buy 3:  970.30
```

With inventory skew (long 2 units, `skew_factor=1.0`), the entire grid shifts down by 2.0:

```
                        Sell 3: 1028.30
                    Sell 2: 1018.10
                Sell 1: 1008.00
            ─── Mid: 1000.00 ───
                Buy 1:  988.00
                    Buy 2:  978.10
                        Buy 3:  968.30
```

### Inventory management

The strategy enforces position limits through two mechanisms:

1. **`max_position`** — Hard cap on net exposure (long or short). When the projected exposure
   from adding another grid level would exceed this cap, that level is skipped.
2. **Projected exposure tracking** — Before placing each level, the strategy tracks the
   worst-case per-side exposure (current position + all pending orders) to prevent over-committing.

### Requote threshold

The `requote_threshold_bps` parameter controls how much the mid-price must move (in basis points)
before the strategy cancels all existing orders and places a fresh grid. This creates a
trade-off:

- **Lower threshold** (e.g., 5 bps): More responsive to price moves, but generates more
  cancel/place transactions
- **Higher threshold** (e.g., 50 bps): Fewer transactions, but orders may sit further from
  the current price

## Configuration walkthrough

### Parameter reference

| Parameter                | Type          | Default     | Description                                                         |
|--------------------------|---------------|-------------|---------------------------------------------------------------------|
| `instrument_id`          | `InstrumentId`| *required*  | Instrument to trade (e.g., `ETH-USD-PERP.DYDX`)                   |
| `max_position`           | `Quantity`    | *required*  | Maximum net exposure (long or short)                                |
| `trade_size`             | `Quantity`    | `None`      | Size per grid level. If `None`, uses instrument's `min_quantity`   |
| `num_levels`             | `usize`       | `3`         | Number of buy and sell levels                                       |
| `grid_step_bps`          | `u32`         | `10`        | Grid spacing in basis points (10 = 0.1%)                           |
| `skew_factor`            | `f64`         | `0.0`       | How aggressively to shift the grid based on inventory               |
| `requote_threshold_bps`  | `u32`         | `5`         | Minimum mid-price move in bps before re-quoting                    |
| `expire_time_secs`       | `Option<u64>` | `None`      | Order expiry in seconds. Uses GTD when set, GTC otherwise          |
| `on_cancel_resubmit`     | `bool`        | `false`     | Resubmit grid on next quote after protocol cancel                  |

### Choosing parameters

**`grid_step_bps`**: Start wider (50-100 bps) in volatile markets, tighter (5-20 bps) in calm
conditions. Wider grids capture more spread per fill but fill less frequently.

**`skew_factor`**: Start at `0.0` (no skew) and increase gradually. A value of `0.5` means
each unit of position shifts the grid by 0.5 price units. Too aggressive a skew can cause
the grid to move entirely above or below the mid-price.

**`expire_time_secs`**: For dYdX short-term orders, set to `8` seconds. This fits within
the 20-block (~10s) short-term window, giving the orders time to rest while keeping them
in the fast short-term path. When `None`, orders use GTC (long-term path).

**`on_cancel_resubmit`**: Set to `true` when using short-term orders with `expire_time_secs`.
Short-term orders are cancelled by the protocol when they expire (without generating cancel
events visible to the strategy). This flag ensures the grid refreshes on the next quote tick
after any protocol-initiated cancel.

## dYdX-specific considerations

### Short-term order expiry

When `expire_time_secs=8`, orders are classified as short-term by the adapter:

1. The adapter checks: `8 seconds < max_short_term_secs (20 blocks × ~0.5s = ~10s)`
2. Since it fits, the order is submitted as short-term with `GoodTilBlock = current_height + N`
3. The order expires silently after ~8 seconds if not filled

This is the recommended configuration for market making because:
- Short-term orders have lower latency
- No gas fees for expiry (GTB replay protection handles it)
- The `on_cancel_resubmit` mechanism keeps the grid fresh

See the [Order classification](../integrations/dydx.md#order-classification) section
in the integration guide for full details.

### Protocol cancellations and `on_cancel_resubmit`

The `pending_self_cancels` set distinguishes between self-initiated and protocol-initiated
cancels:

1. When the strategy calls `cancel_all_orders()`, it records all open order IDs in
   `pending_self_cancels`
2. When `on_order_canceled` fires:
   - If the order ID is in `pending_self_cancels` → self-cancel, no action needed
   - If not → protocol-initiated cancel (expiry), reset `last_quoted_mid` to trigger
     a full grid resubmission on the next quote

This prevents the strategy from re-quoting unnecessarily during its own cancel waves while
still responding to protocol expiry events.

### Order quantization

All price and size quantization for dYdX markets is handled automatically by the adapter's
`OrderMessageBuilder`. No manual rounding or conversion is needed. See
[Price and size quantization](../integrations/dydx.md#price-and-size-quantization) for details.

## Running the example

### Environment setup

```bash
# Load credentials (create a .env file or export directly)
export DYDX_PRIVATE_KEY="0x..."
export DYDX_WALLET_ADDRESS="dydx1..."
```

### Run the example

```bash
cargo run --example dydx-grid-mm --package nautilus-dydx
```

## Code walkthrough

### Node setup

The example configures a `LiveNode` with dYdX data and execution clients:

1. **`DydxDataClientConfig`** — Minimal config; `is_testnet` selects the correct endpoints
2. **`DydxExecClientConfig`** — Includes trader ID, account ID, network, credentials, and
   rate limiting (`grpc_rate_limit_per_second: Some(4)`)
3. **`LiveNode::builder()`** — The builder pattern wires up logging, data/execution clients,
   and optional features like reconciliation

### Strategy registration

```rust
let config = GridMarketMakerConfig::new(instrument_id, Quantity::from("0.10"))
    .with_num_levels(3)           // 3 buy + 3 sell levels
    .with_grid_step_bps(100)      // 1% spacing
    .with_skew_factor(0.5)        // Moderate inventory skew
    .with_requote_threshold_bps(10) // Requote on 10bps mid move
    .with_expire_time_secs(8)     // Short-term orders (~8s)
    .with_on_cancel_resubmit(true); // Refresh grid on protocol cancel
```

### Event flow

```
LiveNode starts
  │
  ├── connect() → HTTP: load instruments, WebSocket: subscribe channels
  │
  ├── on_start()
  │     └── subscribe_quotes(ETH-USD-PERP.DYDX)
  │
  ├── on_quote() [repeated]
  │     ├── Calculate mid-price
  │     ├── Check should_requote() — skip if within threshold
  │     ├── cancel_all_orders() — record IDs in pending_self_cancels
  │     ├── Compute grid with inventory skew
  │     └── Submit limit orders (GTD, expire in 8s)
  │
  ├── on_order_filled()
  │     └── Remove from pending_self_cancels
  │
  ├── on_order_canceled()
  │     ├── Self-cancel? → no action
  │     └── Protocol cancel? → reset last_quoted_mid (triggers requote)
  │
  └── on_stop()
        ├── cancel_all_orders()
        ├── close_all_positions()
        └── unsubscribe_quotes()
```

## Monitoring and understanding output

### Key log messages

| Log message | Meaning |
|---|---|
| `Requoting grid: mid=X, last_mid=Y` | Mid-price moved beyond threshold, refreshing grid |
| `Submit short-term order N` | Order submitted via short-term broadcast path |
| `BatchCancel N short-term orders` | Batch cancel executed for expired/stale orders |
| `benign cancel error, treating as success` | Cancel for already-filled/expired order (normal) |
| `Sequence mismatch detected, will resync and retry` | Cosmos SDK sequence error, auto-recovering |

### Expected behavior patterns

1. **Startup**: Instruments load, WebSocket connects, first quote triggers initial grid
2. **Steady state**: Grid persists across ticks; requotes only on significant mid-price moves
3. **Fills**: Position updates, skew adjusts, next requote shifts grid
4. **Expiry**: Short-term orders expire after ~8s, `on_cancel_resubmit` triggers fresh grid
5. **Shutdown**: All orders cancelled, positions closed, WebSocket disconnected

## Customization tips

### High vs low volatility

| Condition      | Adjustment                                                         |
|----------------|--------------------------------------------------------------------|
| High volatility | Wider `grid_step_bps` (100-200), fewer `num_levels`, lower `skew_factor` |
| Low volatility  | Tighter `grid_step_bps` (10-30), more `num_levels`, higher `skew_factor` |
| Thin liquidity  | Increase `requote_threshold_bps` to reduce cancel frequency        |

### Multiple instruments

Run separate `GridMarketMaker` instances for each instrument. Each instance manages its own
grid, position tracking, and cancel state independently:

```rust
let btc_config = GridMarketMakerConfig::new(
    InstrumentId::from("BTC-USD-PERP.DYDX"),
    Quantity::from("0.001"),
)
.with_strategy_id(StrategyId::from("GRID_MM-BTC"))
.with_order_id_tag("BTC".to_string())
.with_grid_step_bps(50);

let eth_config = GridMarketMakerConfig::new(
    InstrumentId::from("ETH-USD-PERP.DYDX"),
    Quantity::from("0.10"),
)
.with_strategy_id(StrategyId::from("GRID_MM-ETH"))
.with_order_id_tag("ETH".to_string())
.with_grid_step_bps(100);

node.add_strategy(GridMarketMaker::new(btc_config))?;
node.add_strategy(GridMarketMaker::new(eth_config))?;
```

### Mainnet vs testnet toggle

Change a single flag to switch networks:

```rust
let is_testnet = true;  // false for mainnet
let network = if is_testnet { DydxNetwork::Testnet } else { DydxNetwork::Mainnet };
```

All endpoints, chain IDs, and credential environment variables are resolved automatically
based on this flag.

## Further reading

- [dYdX v4 Integration Guide](../integrations/dydx.md) — Full adapter reference
- [dYdX Protocol Documentation](https://docs.dydx.exchange/) — Official protocol docs
- [Short-term vs Stateful Orders](https://docs.dydx.exchange/api_integration-trading/short_term_vs_stateful) — Protocol-level order mechanics
