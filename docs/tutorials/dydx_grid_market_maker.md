# dYdX - Grid Market Making

This tutorial walks through running a grid market making strategy on dYdX v4 using the
Rust-native `LiveNode`. By the end, you will have a working grid quoter that places symmetric
limit orders around the mid-price, skews the grid to manage inventory, and continuously
requotes as short-term orders cycle through expiry.

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

- **Short-term orders** (~10s expiry) provide low-latency placement without on-chain storage.
- **~0.5s block times** give fast confirmation cycles.
- **No gas fees for cancellations**: short-term order cancels are free (GTB replay protection).
- **On-chain order book**: deterministic matching within each block.
- **Batch cancel**: cancel all short-term orders in a single `MsgBatchCancel` call.

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

1. **`max_position`**: Hard cap on net exposure (long or short). When the projected exposure
   from adding another grid level would exceed this cap, that level is skipped.
2. **Projected exposure tracking**: Before placing each level, the strategy tracks the
   worst-case per-side exposure (current position + all pending orders) to prevent over-committing.

Because `cancel_all_orders()` is asynchronous, pending orders may still fill between the cancel
request and acknowledgement. The strategy accounts for this by tracking worst-case per-side
exposure (current position + all pending buy/sell orders) before placing new grid levels. This
prevents momentary over-exposure during cancel-requote transitions.

### Requote threshold

The `requote_threshold_bps` parameter controls how much the mid-price must move (in basis points)
before the strategy cancels all existing orders and places a fresh grid. This creates a
trade-off:

- **Lower threshold** (e.g., 5 bps): More responsive to price moves, but generates more
  cancel/place transactions.
- **Higher threshold** (e.g., 50 bps): Fewer transactions, but orders may sit further from
  the current price.

## Configuration

| Parameter               | Type           | Default    | Description                                                              |
| ----------------------- | -------------- | ---------- | ------------------------------------------------------------------------ |
| `instrument_id`         | `InstrumentId` | *required* | Instrument to trade (e.g., `ETH-USD-PERP.DYDX`).                         |
| `max_position`          | `Quantity`     | *required* | Maximum net exposure (long or short).                                    |
| `trade_size`            | `Quantity`     | `None`     | Size per grid level. If `None`, uses instrument's `min_quantity` or 1.0. |
| `num_levels`            | `usize`        | `3`        | Number of buy and sell levels.                                           |
| `grid_step_bps`         | `u32`          | `10`       | Grid spacing in basis points (10 = 0.1%).                                |
| `skew_factor`           | `f64`          | `0.0`      | How aggressively to shift the grid based on inventory.                   |
| `requote_threshold_bps` | `u32`          | `5`        | Minimum mid-price move in bps before re-quoting.                         |
| `expire_time_secs`      | `Option<u64>`  | `None`     | Order expiry in seconds. Uses GTD when set, GTC otherwise.               |
| `on_cancel_resubmit`    | `bool`         | `false`    | Resubmit grid on next quote after an unexpected cancel.                  |

### Choosing parameters

**`grid_step_bps`**: Start wider (50-100 bps) in volatile markets, tighter (5-20 bps) in calm
conditions. Wider grids capture more spread per fill but fill less frequently.

**`skew_factor`**: Start at `0.0` (no skew) and increase gradually. A value of `0.5` means
each unit of position shifts the grid by 0.5 price units. Too aggressive a skew can cause
the grid to move entirely above or below the mid-price.

**`expire_time_secs`**: For dYdX short-term orders, set to `8` seconds. This fits within
the 20-block (~10s) short-term window, giving the orders time to rest while keeping them
in the fast short-term path. When `None`, orders use GTC (long-term path).

**`on_cancel_resubmit`**: Resubmits the grid on the next quote tick after any unexpected
cancel (e.g. self-trade prevention, risk limits). Note that short-term order expiry is
silent and does not generate cancel events, so the grid refreshes naturally via continuous
requoting, not through this flag.

## dYdX-specific considerations

### Short-term order expiry

When `expire_time_secs=8`, orders are classified as short-term by the adapter:

1. The adapter checks: `8 seconds < max_short_term_secs (20 blocks × ~0.5s = ~10s)`.
2. Since it fits, the order is submitted as short-term with `GoodTilBlock = current_height + N`.
3. The order expires silently after ~8 seconds if not filled.

This is the recommended configuration for market making because:

- Short-term orders have lower latency.
- No gas fees for expiry (GTB replay protection handles it).
- Continuous requoting naturally replaces expired orders.

See the [Order classification](../integrations/dydx.md#order-classification) section
in the integration guide for full details.

### Unexpected cancels and `on_cancel_resubmit`

The `pending_self_cancels` set distinguishes between self-initiated and unexpected cancels:

1. When the strategy calls `cancel_all_orders()`, it records all open order IDs in
   `pending_self_cancels`.
2. When `on_order_canceled` fires:
   - If the order ID is in `pending_self_cancels`, it's a self-cancel and no action is needed.
   - If not, it's an unexpected cancel (e.g. self-trade prevention or risk limits).
     Reset `last_quoted_mid` to trigger a full grid resubmission on the next quote.

This prevents the strategy from re-quoting unnecessarily during its own cancel waves while
still responding to unexpected cancels.

`on_order_filled` also removes the order from `pending_self_cancels`. If an order fills
before the cancel acknowledgement arrives, this prevents stale entries from accumulating
in the set.

### Order quantization

All price and size quantization for dYdX markets is handled automatically by the adapter's
`OrderMessageBuilder`. No manual rounding or conversion is needed. See
[Price and size quantization](../integrations/dydx.md#price-and-size-quantization) for details.

### Post-only orders

All grid orders are submitted with `post_only=true`. This ensures every order enters the
book as a maker order (never crosses the spread). If a grid price has moved through the
book by the time it reaches the matching engine, the order is rejected rather than filling
as a taker. This guarantees maker fee rates and prevents unintended crossing during
requote transitions.

## Running and stopping

### Environment setup

Credentials can be set via environment variables or a `.env` file in the project root
(loaded automatically via `dotenvy`):

```bash
# Export directly
export DYDX_PRIVATE_KEY="0x..."
export DYDX_WALLET_ADDRESS="dydx1..."
```

```bash
# Or use a .env file (alternative to shell exports)
DYDX_PRIVATE_KEY=0x...
DYDX_WALLET_ADDRESS=dydx1...
```

### Run the example

```bash
cargo run --example dydx-grid-mm --package nautilus-dydx
```

### Graceful shutdown

Press **Ctrl+C** to stop the node. The shutdown sequence:

1. SIGINT received, trader stops, `on_stop()` fires.
2. Strategy cancels all orders and closes positions.
3. 5-second grace period (`delay_post_stop_secs`) processes residual events.
4. Clients disconnect, node exits.

## Code walkthrough

### Node setup

The complete `main()` function from the example (`node_grid_mm.rs`):

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok(); // Load .env file if present

    // Configuration
    let is_testnet = false;
    let network = if is_testnet {
        DydxNetwork::Testnet
    } else {
        DydxNetwork::Mainnet
    };

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("DYDX-001");
    let node_name = "DYDX-GRID-MM-001".to_string();
    let instrument_id = InstrumentId::from("ETH-USD-PERP.DYDX");

    // Load credentials from environment (testnet/mainnet-aware)
    let private_key_env = if is_testnet {
        "DYDX_TESTNET_PRIVATE_KEY"
    } else {
        "DYDX_PRIVATE_KEY"
    };
    let private_key = get_env_option(private_key_env);
    let wallet_env = if is_testnet {
        "DYDX_TESTNET_WALLET_ADDRESS"
    } else {
        "DYDX_WALLET_ADDRESS"
    };
    let wallet_address = get_env_option(wallet_env);

    if private_key.is_none() && wallet_address.is_none() {
        return Err(
            format!("Set {private_key_env} or {wallet_env} environment variable").into(),
        );
    }

    // Minimal data client config: is_testnet selects the correct endpoints
    let data_config = DydxDataClientConfig {
        is_testnet,
        ..Default::default()
    };

    // Execution client with trader ID, network, credentials, and rate limiting
    let exec_config = DYDXExecClientConfig {
        trader_id,
        account_id,
        network,
        private_key,
        wallet_address,
        subaccount_number: 0,
        grpc_endpoint: None,
        grpc_urls: vec![],
        ws_endpoint: None,
        http_endpoint: None,
        authenticator_ids: vec![],
        http_timeout_secs: Some(30),
        max_retries: Some(3),
        retry_delay_initial_ms: Some(1000),
        retry_delay_max_ms: Some(10000),
        grpc_rate_limit_per_second: Some(4), // Conservative for public providers
    };

    let data_factory = DydxDataClientFactory::new();
    let exec_factory = DydxExecutionClientFactory::new();

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    // Builder pattern wires up logging, data/execution clients, and node options
    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(false)   // Disabled for simplicity; enable in production
                                      // to resume state across restarts
        .with_delay_post_stop_secs(5) // Grace period for pending cancel/close events
        .build()?;

    // Strategy configuration and registration
    let config = GridMarketMakerConfig::new(instrument_id, Quantity::from("0.10"))
        .with_num_levels(3)
        .with_grid_step_bps(100)
        .with_skew_factor(0.5)
        .with_requote_threshold_bps(10)
        .with_expire_time_secs(8)
        .with_on_cancel_resubmit(true);
    let strategy = GridMarketMaker::new(config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}
```

Key configuration points:

- **`dotenvy::dotenv().ok()`**: loads a `.env` file from the project root (if present).
- **`with_reconciliation(false)`**: disabled for simplicity; enable in production to resume
  state across restarts.
- **`with_delay_post_stop_secs(5)`**: grace period for pending cancel/close events to finalize
  during shutdown.

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
  │     ├── Check should_requote(): skip if within threshold
  │     ├── cancel_all_orders(): record IDs in pending_self_cancels
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

## Strategy internals

This section shows the key Rust code from `grid_mm.rs` so you can see exactly how the
strategy works without reading the full source.

### Trade size resolution (`on_start`)

When the strategy starts, it resolves the trade size from the instrument cache. The fallback
chain is: config value → instrument `min_quantity` → `1.0`:

```rust
fn on_start(&mut self) -> anyhow::Result<()> {
    let instrument_id = self.config.instrument_id;
    let (price_precision, size_precision, min_quantity) = {
        let cache = self.cache();
        let instrument = cache
            .instrument(&instrument_id)
            .expect("Instrument should be in cache");
        (
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.min_quantity(),
        )
    };
    self.price_precision = price_precision;

    // Resolve trade_size from instrument when not explicitly provided
    if self.trade_size.is_none() {
        self.trade_size =
            Some(min_quantity.unwrap_or_else(|| Quantity::new(1.0, size_precision)));
    }

    self.subscribe_quotes(instrument_id, None, None);
    Ok(())
}
```

### Quote handler (`on_quote`, abbreviated)

This is the heart of the strategy. On each quote tick it computes the mid-price,
checks whether a requote is needed, cancels stale orders, computes worst-case exposure,
and places new grid orders with GTD + post_only:

```rust
fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
    let mid_f64 = (quote.bid_price.as_f64() + quote.ask_price.as_f64()) / 2.0;
    let mid = Price::new(mid_f64, self.price_precision);

    if !self.should_requote(mid) {
        return Ok(()); // Mid hasn't moved enough, keep existing grid
    }

    // ... record open order IDs in pending_self_cancels (for on_cancel_resubmit) ...

    self.cancel_all_orders(instrument_id, None, None)?;

    // Compute worst-case per-side exposure (position + all pending orders)
    // since cancels are async and pending orders may still fill
    let (net_position, worst_long, worst_short) = { /* ... */ };

    let grid = self.grid_orders(mid, net_position, worst_long, worst_short);

    if grid.is_empty() {
        return Ok(()); // Don't advance requote anchor when fully constrained
    }

    // Compute time-in-force from config
    let (tif, expire_time) = match self.config.expire_time_secs {
        Some(secs) => {
            let now_ns = self.core.clock().timestamp_ns();
            let expire_ns = now_ns + secs * 1_000_000_000;
            (Some(TimeInForce::Gtd), Some(expire_ns))
        }
        None => (None, None),
    };

    for (side, price) in grid {
        let order = self.core.order_factory().limit(
            instrument_id,
            side,
            trade_size,
            price,
            tif,
            expire_time,
            Some(true), // post_only
            // ... remaining None fields ...
        );
        self.submit_order(order, None, None)?;
    }

    self.last_quoted_mid = Some(mid);
    Ok(())
}
```

### Grid pricing (`grid_orders`)

Computes geometric grid prices and enforces max_position per-level. This is the function
behind the ASCII diagrams in the [Strategy overview](#geometric-grid-pricing) section:

```rust
fn grid_orders(
    &self,
    mid: Price,
    net_position: f64,
    worst_long: Decimal,
    worst_short: Decimal,
) -> Vec<(OrderSide, Price)> {
    let mid_f64 = mid.as_f64();
    let skew_f64 = self.config.skew_factor * net_position;
    let pct = self.config.grid_step_bps as f64 / 10_000.0;
    let trade_size = self.trade_size
        .expect("trade_size should be resolved in on_start")
        .as_decimal();
    let max_pos = self.config.max_position.as_decimal();
    let mut projected_long = worst_long;
    let mut projected_short = worst_short;
    let mut orders = Vec::new();

    for level in 1..=self.config.num_levels {
        let buy_price = Price::new(
            mid_f64 * (1.0 - pct).powi(level as i32) - skew_f64,
            precision,
        );
        let sell_price = Price::new(
            mid_f64 * (1.0 + pct).powi(level as i32) - skew_f64,
            precision,
        );

        // Only place buy if projected long exposure stays within max_position
        if projected_long + trade_size <= max_pos {
            orders.push((OrderSide::Buy, buy_price));
            projected_long += trade_size;
        }

        // Only place sell if projected short exposure stays within max_position
        if projected_short - trade_size >= -max_pos {
            orders.push((OrderSide::Sell, sell_price));
            projected_short -= trade_size;
        }
    }

    orders
}
```

## Monitoring and understanding output

### Key log messages

| Log message                                         | Meaning                                            |
| --------------------------------------------------- | -------------------------------------------------- |
| `Requoting grid: mid=X, last_mid=Y`                 | Mid-price moved beyond threshold, refreshing grid. |
| `Submit short-term order N`                         | Order submitted via short-term broadcast path.     |
| `BatchCancel N short-term orders`                   | Batch cancel executed for expired/stale orders.    |
| `benign cancel error, treating as success`          | Cancel for already-filled/expired order (normal).  |
| `Sequence mismatch detected, will resync and retry` | Cosmos SDK sequence error, auto-recovering.        |

### Expected behavior patterns

1. **Startup**: Instruments load, WebSocket connects, first quote triggers initial grid.
2. **Steady state**: Grid persists across ticks; requotes only on significant mid-price moves.
3. **Fills**: Position updates, skew adjusts, next requote shifts grid.
4. **Expiry**: Short-term orders expire silently after ~8s; grid naturally refreshes on the next requote.
5. **Shutdown**: All orders cancelled, positions closed, WebSocket disconnected.

## Customization tips

### High vs low volatility

| Condition       | Adjustment                                                                |
| --------------- | ------------------------------------------------------------------------- |
| High volatility | Wider `grid_step_bps` (100-200), fewer `num_levels`, lower `skew_factor`. |
| Low volatility  | Tighter `grid_step_bps` (10-30), more `num_levels`, higher `skew_factor`. |
| Thin liquidity  | Increase `requote_threshold_bps` to reduce cancel frequency.              |

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

- [dYdX v4 Integration Guide](../integrations/dydx.md): full adapter reference.
- [dYdX Protocol Documentation](https://docs.dydx.exchange/): official protocol docs.
- [Short-term vs Stateful Orders](https://docs.dydx.exchange/api_integration-trading/short_term_vs_stateful):
  protocol-level order mechanics.
