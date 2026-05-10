# BitMEX - Grid Market Making with Deadman's Switch

This tutorial walks through backtesting a grid market making strategy on BitMEX using free
historical quote tick data from [Tardis.dev](https://tardis.dev), then running it live
using the Rust-native `LiveNode`. The key differentiator covered here is BitMEX's
**deadman's switch**, a server-side safety mechanism that automatically cancels all open
orders if your client loses connectivity.

## Introduction

### Why BitMEX for grid market making?

BitMEX is one of the deepest and most liquid Bitcoin derivatives venues, with a live order
book going back to 2014. The `XBTUSD` inverse perpetual swap is among the most-traded
instruments in crypto derivatives. Its thick order book and predictable spread behaviour
make it a natural venue for grid market making.

Two features make BitMEX particularly well-suited for automated market making:

1. **Deadman's switch** (`cancelAllAfter`): BitMEX maintains a server-side countdown timer.
   Your client refreshes it periodically. If the connection drops and the timer expires,
   BitMEX cancels all open orders on your behalf, protecting you from stranded quotes.

2. **Submit/cancel broadcaster**: The adapter can fan out order submissions and
   cancellations across multiple independent HTTP connections simultaneously, with the first
   successful response short-circuiting the rest. This provides redundancy against transient
   network failures.

### Deadman's switch mechanics

When `deadmans_switch_timeout_secs` is set in the execution client config, a background
task runs continuously:

```
timeout = 60s → refresh interval = timeout / 4 = 15s

 t=0s   Strategy starts, cancelAllAfter(60000ms) sent
 t=15s  Refresh: cancelAllAfter(60000ms) sent  (resets timer)
 t=30s  Refresh: cancelAllAfter(60000ms) sent
 t=45s  Refresh: cancelAllAfter(60000ms) sent
    ↓
 Connectivity lost at t=50s (last refresh was at t=45s)
    ↓
 t=105s  Server timer fires → BitMEX cancels all open orders
```

For market making specifically, stranded quotes are a serious risk: if your software crashes
while holding grid orders around the mid-price, price can move against those orders before
they are cancelled. The deadman's switch caps the window of exposure at `timeout` seconds,
regardless of why connectivity was lost.

## Prerequisites

- **NautilusTrader** installed (see the [installation guide](../getting_started/installation.md)).
- **Rust toolchain** (`cargo`) for the live example. Install from [rustup.rs](https://rustup.rs/).
- **BitMEX account**: sign up at [bitmex.com](https://www.bitmex.com/) and generate an API key
  with order management permissions. For testing, use the
  [BitMEX testnet](https://testnet.bitmex.com/).

### Environment variables

```bash
# Mainnet
export BITMEX_API_KEY="your-api-key"
export BITMEX_API_SECRET="your-api-secret"

# Testnet
export BITMEX_TESTNET_API_KEY="your-testnet-api-key"
export BITMEX_TESTNET_API_SECRET="your-testnet-api-secret"
```

Alternatively, place these in a `.env` file in the project root (loaded automatically via `dotenvy`).

## Backtesting with Tardis free quote data

BitMEX does not offer historical market data via its own API beyond recent trade history.
[Tardis.dev](https://tardis.dev) captures and archives tick-level BitMEX data from March 2019
onward in its native WebSocket format. The **first day of each month is freely downloadable**
without an API key (enough for a representative backtest run).

The grid market maker subscribes to best-bid/ask quotes, so the `quotes` dataset is the
right source: it records every change to the top of book.

### Download the data

```bash
curl -LO https://datasets.tardis.dev/v1/bitmex/quotes/2024/01/01/XBTUSD.csv.gz
```

This downloads January 1 2024 XBTUSD quote data. No API key required.

:::tip
Full historical data (all dates) requires a paid Tardis API key. Use the
[Tardis download utility](https://docs.tardis.dev/downloadable-csv-files) for bulk fetches.
:::

### Load the data

`TardisCSVDataLoader` parses the `.csv.gz` file directly (no decompression needed) and
returns a list of `QuoteTick` objects:

```python
from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.model.identifiers import InstrumentId

instrument_id = InstrumentId.from_str("XBTUSD.BITMEX")

loader = TardisCSVDataLoader(instrument_id=instrument_id)
quotes = loader.load_quotes("XBTUSD.csv.gz")
```

The `instrument_id` override ensures every tick is tagged `XBTUSD.BITMEX` regardless of
what appears in the CSV.

### Instrument definition

Since we are loading the data directly (not through the live BitMEX adapter), we define the
`XBTUSD` instrument manually. XBTUSD is an **inverse perpetual**: prices are quoted in USD
but the contract is margined and settled in BTC. One contract equals 1 USD of notional
exposure.

```python
from decimal import Decimal

from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import PerpetualContract
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

XBTUSD = PerpetualContract(
    instrument_id=instrument_id,
    raw_symbol=Symbol("XBTUSD"),
    underlying="XBT",
    asset_class=AssetClass.CRYPTOCURRENCY,
    base_currency=BTC,
    quote_currency=USD,
    settlement_currency=BTC,
    is_inverse=True,
    price_precision=1,        # $0.5 tick → one decimal place
    size_precision=0,         # integer contracts
    price_increment=Price.from_str("0.5"),
    size_increment=Quantity.from_int(1),
    multiplier=Quantity.from_int(1),  # 1 USD per contract
    lot_size=Quantity.from_int(1),
    margin_init=Decimal("0.01"),      # 1% initial margin = 100x max leverage
    margin_maint=Decimal("0.005"),
    maker_fee=Decimal("-0.00025"),    # maker rebate
    taker_fee=Decimal("0.00075"),
    ts_event=0,
    ts_init=0,
)
```

Fee rates are explicit backtest assumptions. Check
[bitmex.com/app/fees](https://www.bitmex.com/app/fees) for current rates.

### Backtest engine setup

XBTUSD is BTC-margined, so the starting balance is in BTC:

```python
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money

config = BacktestEngineConfig(
    trader_id=TraderId("BACKTESTER-001"),
    logging=LoggingConfig(log_level="INFO"),
)
engine = BacktestEngine(config=config)

BITMEX = Venue("BITMEX")
engine.add_venue(
    venue=BITMEX,
    oms_type=OmsType.NETTING,
    account_type=AccountType.MARGIN,
    base_currency=BTC,
    starting_balances=[Money(1, BTC)],
)

engine.add_instrument(XBTUSD)
engine.add_data(quotes)
```

### Strategy configuration

```python
from nautilus_trader.examples.strategies.grid_market_maker import GridMarketMaker
from nautilus_trader.examples.strategies.grid_market_maker import GridMarketMakerConfig

strategy_config = GridMarketMakerConfig(
    instrument_id=instrument_id,
    max_position=Quantity.from_int(300),   # 300 USD contracts max exposure
    trade_size=Quantity.from_int(100),     # 100 USD contracts per level
    num_levels=3,
    grid_step_bps=100,                     # 1% between levels
    skew_factor=0.5,
    requote_threshold_bps=10,
)
strategy = GridMarketMaker(config=strategy_config)
engine.add_strategy(strategy)
```

### Run and review results

```python
import pandas as pd

engine.run()

with pd.option_context("display.max_rows", 100, "display.max_columns", None, "display.width", 300):
    print(engine.trader.generate_account_report(BITMEX))
    print(engine.trader.generate_order_fills_report())
    print(engine.trader.generate_positions_report())

engine.reset()
engine.dispose()
```

The complete script is available as
[`bitmex_grid_market_maker.py`](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/backtest/bitmex_grid_market_maker.py)
in the examples directory.

## Live trading: GridMarketMaker with deadman's switch

Once you have validated data loading and strategy mechanics in the backtest, the same
configuration runs live using the Rust `LiveNode`. The `GridMarketMaker` strategy is
implemented natively in Rust for maximum throughput.

### Environment setup

Credentials are loaded automatically from environment variables when not set explicitly in
the config:

```bash
# Testnet (recommended for initial setup)
export BITMEX_TESTNET_API_KEY="your-key"
export BITMEX_TESTNET_API_SECRET="your-secret"
```

```ini
# Or use a .env file at the project root
BITMEX_TESTNET_API_KEY=your-key
BITMEX_TESTNET_API_SECRET=your-secret
```

### Code walkthrough

The complete `main()` function from
[`node_grid_mm.rs`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/bitmex/examples/node_grid_mm.rs):

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok(); // Load .env file if present

    let use_testnet = true; // Set false for mainnet

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let instrument_id = InstrumentId::from("XBTUSD.BITMEX");

    // Minimal data client: just selects testnet or mainnet endpoints
    let data_config = BitmexDataClientConfig {
        use_testnet,
        ..Default::default()
    };

    // Execution client with deadman's switch enabled
    let exec_config = BitmexExecFactoryConfig::new(
        trader_id,
        BitmexExecClientConfig {
            use_testnet,
            deadmans_switch_timeout_secs: Some(60), // Cancels all orders after 60s without refresh
            ..Default::default()
        },
    );

    let data_factory = BitmexDataClientFactory::new();
    let exec_factory = BitmexExecutionClientFactory::new();

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    // Builder wires up logging, data and execution clients, and node options
    let mut node = LiveNode::builder(trader_id, environment)?
        .with_logging(log_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)            // Resume state across restarts
        .with_reconciliation_lookback_mins(2880) // Look back 2 days (2880 min)
        .with_delay_post_stop_secs(5)         // Grace period for pending cancel/close events
        .build()?;

    // Grid configuration: XBTUSD, 300 USD max position, 3 levels at 100 bps each
    let config = GridMarketMakerConfig::new(instrument_id, Quantity::from("300"))
        .with_num_levels(3)
        .with_grid_step_bps(100)      // 1% between levels
        .with_skew_factor(0.5)        // shift grid 0.5 price units per unit of inventory
        .with_requote_threshold_bps(10); // requote when mid moves more than 0.1%
    let strategy = GridMarketMaker::new(config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}
```

Configuration points:

- **`deadmans_switch_timeout_secs: Some(60)`**: enables the deadman's switch with a 60-second
  timeout. The background task refreshes every 15 seconds (`timeout / 4`).
- **`with_reconciliation(true)`**: reconciles open orders and positions on startup by
  querying the BitMEX REST API, allowing the strategy to resume correctly after a restart.
- **`with_reconciliation_lookback_mins(2880)`**: looks back 2 days when reconciling order history.
- **`with_delay_post_stop_secs(5)`**: allows 5 seconds after strategy stop for pending
  cancel/fill events to arrive before the node exits.

### Deadman's switch in context

During normal operation the deadman's switch is invisible: the background task silently
refreshes the server-side timer. Its value becomes apparent in failure scenarios:

```
Normal operation:
  ┌─────────────────────────────────────────────────────────┐
  │  Strategy running                                       │
  │  t=0s   cancelAllAfter(60_000ms) ──────────────► BitMEX │
  │  t=15s  cancelAllAfter(60_000ms) ──────────────► BitMEX │
  │  t=30s  cancelAllAfter(60_000ms) ──────────────► BitMEX │
  └─────────────────────────────────────────────────────────┘

Connectivity loss:
  ┌──────────────────────────────────────────────────────────┐
  │  t=40s  Network failure, no more refreshes sent          │
  │  t=100s BitMEX timer fires → all open orders cancelled   │
  │         (60s after the last successful refresh at t=40s) │
  └──────────────────────────────────────────────────────────┘
```

Unlike dYdX where short-term order expiry provides a similar automatic cleanup, BitMEX
uses GTC orders (no expiry). Without the deadman's switch, a crashed client could leave
grid orders resting in the book indefinitely.

### BitMEX-specific considerations

#### GTC orders and post-only

BitMEX grid orders use `GTC` (Good-Till-Cancelled) time-in-force combined with
`ParticipateDoNotInitiate` (post-only). Post-only ensures every order enters the book as
a maker order; if a grid price has moved through the book by the time it reaches the
matching engine, the order is rejected rather than filling as a taker.

This differs from the dYdX setup where short-term orders provide automatic expiry every
~8 seconds. On BitMEX, the requote cycle is driven entirely by mid-price movement
(`requote_threshold_bps`) rather than order expiry.

#### Order quantization

All price and size quantization for BitMEX instruments is handled automatically by the
adapter. No manual rounding or conversion is needed in strategy code.

#### Inverse perpetual accounting

Because XBTUSD is inverse (BTC-margined), PnL is in BTC. A grid that captures a $1 spread
on a $42,000 BTC price earns approximately 1/42,000 BTC per fill. Account for this when
sizing `max_position` and `trade_size`.

### Run the example

```bash
cargo run --example bitmex-grid-mm --package nautilus-bitmex
```

### Graceful shutdown

Press **Ctrl+C** to stop the node. The shutdown sequence:

1. SIGINT received, trader stops, `on_stop()` fires.
2. Strategy cancels all orders and closes positions.
3. 5-second grace period (`delay_post_stop_secs`) processes residual events.
4. Deadman's switch background task stops.
5. Clients disconnect, node exits.

## Configuration

### GridMarketMaker parameters

| Parameter               | Type           | Default    | Description                                                              |
| ----------------------- | -------------- | ---------- | ------------------------------------------------------------------------ |
| `instrument_id`         | `InstrumentId` | *required* | Instrument to trade (e.g., `XBTUSD.BITMEX`).                            |
| `max_position`          | `Quantity`     | *required* | Maximum net exposure in contracts (long or short).                       |
| `trade_size`            | `Quantity`     | `None`     | Size per grid level. If `None`, uses instrument's `min_quantity` or 1.0. |
| `num_levels`            | `usize`        | `3`        | Number of buy and sell levels.                                           |
| `grid_step_bps`         | `u32`          | `10`       | Grid spacing in basis points (100 = 1%).                                 |
| `skew_factor`           | `f64`          | `0.0`      | How aggressively to shift the grid based on net inventory.               |
| `requote_threshold_bps` | `u32`          | `5`        | Minimum mid-price move (bps) before re-quoting.                          |
| `expire_time_secs`      | `Option<u64>`  | `None`     | Order expiry in seconds. Use `None` for GTC on BitMEX.                   |
| `on_cancel_resubmit`    | `bool`         | `false`    | Resubmit grid on next quote after an unexpected cancel.                  |

### Deadman's switch parameter

| Parameter                     | Type              | Description                                                                                       |
| ----------------------------- | ----------------- | ------------------------------------------------------------------------------------------------- |
| `deadmans_switch_timeout_secs`| `Option<u64>`     | Server-side cancel timer in seconds. Refresh interval = `timeout / 4` (minimum 1s). `None` disables the feature. |

**Recommended value**: `60`. This gives a 15-second refresh interval and a 60-second window
before BitMEX fires the timer. Lower values reduce the exposure window but increase API
call frequency; higher values reduce overhead but extend the window.

### Choosing grid parameters

**`grid_step_bps`**: XBTUSD has tight spreads. Start wider (50–100 bps) to ensure fills
before tightening. Each level captures half the step as spread (buy fills $1 below mid,
sells $1 above on a 200 bps total spread).

**`skew_factor`**: Start at `0.0` (no skew). A value of `0.5` shifts the grid by 0.5 price
units per unit of net position. For XBTUSD, this is 0.5 USD per contract; with max_position
of 300, full skew is ±150 price units.

**`requote_threshold_bps`**: 10 bps (0.1%) is a reasonable starting point for XBTUSD.
Too low causes excessive cancel/replace churn; too high leaves orders stale during fast moves.

## Event flow

```
LiveNode starts
  │
  ├── connect() → REST: load instruments; WebSocket: subscribe channels
  │
  ├── deadman's switch task starts
  │     └── cancelAllAfter(timeout_ms) sent every timeout/4 seconds
  │
  ├── on_start()
  │     └── subscribe_quotes(XBTUSD.BITMEX)
  │
  ├── on_quote() [repeated]
  │     ├── Calculate mid-price
  │     ├── Check should_requote(): skip if within threshold
  │     ├── cancel_all_orders(): record IDs in pending_self_cancels
  │     ├── Compute grid with inventory skew
  │     └── Submit GTC post-only limit orders
  │
  ├── on_order_filled()
  │     └── Remove from pending_self_cancels; position/skew update
  │
  ├── on_order_canceled()
  │     ├── Self-cancel? → no action
  │     └── Unexpected cancel? → reset last_quoted_mid (triggers requote)
  │
  └── on_stop()
        ├── cancel_all_orders()
        ├── close_all_positions()
        ├── unsubscribe_quotes()
        └── deadman's switch task stops
```

## Monitoring and understanding output

### Key log messages

| Log message                                                             | Meaning                                                   |
| ----------------------------------------------------------------------- | --------------------------------------------------------- |
| `Requoting grid: mid=X, last_mid=Y`                                     | Mid-price moved beyond threshold, refreshing grid.        |
| `Starting dead man's switch: timeout=60s, refresh_interval=15s`         | Deadman's switch armed at node start.                     |
| `Dead man's switch heartbeat failed: ...`                               | Transient network issue; switch will retry next interval. |
| `Disarming dead man's switch`                                           | Switch stopped cleanly during shutdown.                   |
| `benign cancel error, treating as success`                              | Cancel for an already-filled or cancelled order (normal). |
| `Reconciling orders from last 2880 minutes`                             | Startup reconciliation loading prior state.               |

### Expected behaviour patterns

1. **Startup**: Instruments load, reconciliation queries prior orders, WebSocket connects,
   first quote triggers initial grid.
2. **Steady state**: Grid persists across ticks; requotes only when mid moves beyond threshold.
3. **Fills**: Position updates, skew adjusts on next requote.
4. **Shutdown**: All orders cancelled, positions closed, deadman's switch stops.
5. **Restart**: Reconciliation restores open order state; strategy resumes from prior grid.

## Customization tips

### High vs low volatility

| Condition       | Adjustment                                                                 |
| --------------- | -------------------------------------------------------------------------- |
| High volatility | Wider `grid_step_bps` (100–200), fewer `num_levels`, lower `skew_factor`.  |
| Low volatility  | Tighter `grid_step_bps` (20–50), more `num_levels`, higher `skew_factor`.  |
| Thin liquidity  | Increase `requote_threshold_bps` to reduce cancel frequency.               |

### Enabling the submit broadcaster

For production deployments, enable the submit broadcaster to provide redundant order
submission across multiple HTTP connections:

```rust
let exec_config = BitmexExecFactoryConfig::new(
    trader_id,
    BitmexExecClientConfig {
        use_testnet: false,
        deadmans_switch_timeout_secs: Some(60),
        submitter_pool_size: Some(2), // two parallel submission paths
        canceller_pool_size: Some(2), // two parallel cancel paths
        ..Default::default()
    },
);
```

With `submitter_pool_size=2`, each order submission fans out to two HTTP clients in
parallel; the first successful response wins. This reduces the probability of a missed
submission due to a transient network failure on a single path.

### Mainnet toggle

Change a single flag to switch networks:

```rust
let use_testnet = false; // true for testnet
```

All endpoints and credential environment variables are resolved automatically.

## Further reading

- [BitMEX integration guide](../integrations/bitmex.md): full adapter reference.
- [dYdX grid market maker tutorial](./dydx_grid_market_maker.md): comparison with
  short-term order expiry as an alternative to the deadman's switch.
- [Tardis downloadable CSV files](https://docs.tardis.dev/downloadable-csv-files): full
  schema documentation for `incremental_book_L2` and other data types.
- [BitMEX API documentation](https://www.bitmex.com/app/apiOverview): `cancelAllAfter`
  endpoint and order management reference.
