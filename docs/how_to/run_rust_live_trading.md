# Run Live Trading (Rust)

The `LiveNode` connects to real venues through adapter clients. This guide
walks through a complete live trading setup using OKX as an example.

For background on live trading architecture and reconciliation, see the
[Live trading](../concepts/live.md) concept guide. For project setup and
feature flags, see the [Rust](../concepts/rust.md#project-setup) concept
guide.

## Dependencies

Add the live crate, your venue adapter, and supporting crates to
`Cargo.toml`:

```toml
[dependencies]
nautilus-common = "0.55"
nautilus-live = "0.55"
nautilus-model = "0.55"
nautilus-okx = "0.55"
nautilus-trading = { version = "0.55", features = ["examples"] }

anyhow = "1"
dotenvy = "0.15"
log = "0.4"
tokio = { version = "1", features = ["full"] }
```

## Build the node

The `LiveNode` uses a builder pattern. Add data and execution client
factories for your venue, configure logging, and build.

```rust
use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_okx::{
    common::enums::OKXInstrumentType,
    config::{OKXDataClientConfig, OKXExecClientConfig},
    factories::{OKXDataClientFactory, OKXExecutionClientFactory},
};

let trader_id = TraderId::from("TESTER-001");
let account_id = AccountId::from("OKX-001");

let data_config = OKXDataClientConfig {
    instrument_types: vec![OKXInstrumentType::Swap],
    ..Default::default()
};

let exec_config = OKXExecClientConfig {
    trader_id,
    account_id,
    instrument_types: vec![OKXInstrumentType::Swap],
    ..Default::default()
};

let log_config = LoggerConfig {
    stdout_level: LevelFilter::Info,
    ..Default::default()
};

let mut node = LiveNode::builder(trader_id, Environment::Live)?
    .with_name("MY-NODE-001".to_string())
    .with_logging(log_config)
    .add_data_client(
        None,
        Box::new(OKXDataClientFactory::new()),
        Box::new(data_config),
    )?
    .add_exec_client(
        None,
        Box::new(OKXExecutionClientFactory::new()),
        Box::new(exec_config),
    )?
    .with_reconciliation(false) // Simplified; enable in production
    .with_delay_post_stop_secs(5)
    .build()?;
```

:::warning
This example disables reconciliation for simplicity. In production, remove
`.with_reconciliation(false)` so the engine aligns cached state with the
venue on startup. See [Execution reconciliation](../concepts/live.md#execution-reconciliation).
:::

## Add strategies and run

```rust
use nautilus_model::{identifiers::InstrumentId, types::Quantity};
use nautilus_trading::examples::strategies::{
    GridMarketMaker, GridMarketMakerConfig,
};

let mut config = GridMarketMakerConfig::new(
    InstrumentId::from("ETH-USDT-SWAP.OKX"),
    Quantity::from("0.10"),
)
    .with_num_levels(3)
    .with_grid_step_bps(100)
    .with_skew_factor(0.5)
    .with_requote_threshold_bps(10)
    .with_expire_time_secs(8)
    .with_on_cancel_resubmit(true);

// OKX rejects hyphens in client order IDs
config.base.use_hyphens_in_client_order_ids = false;

let strategy = GridMarketMaker::new(config);

node.add_strategy(strategy)?;
node.run().await?;
```

The node runs until interrupted (Ctrl+C) or shut down programmatically.

## Environment variables

OKX reads API credentials from environment variables. Use a `.env` file
with `dotenvy` or set them in your shell:

```bash
export OKX_API_KEY="your_api_key"
export OKX_API_SECRET="your_api_secret"
export OKX_API_PASSPHRASE="your_passphrase"
```

For demo trading, set `is_demo: true` in both config structs and use demo
API credentials from OKX.

Each adapter documents its required variables in the
[integration guide](../integrations/) for that venue.

## Async runtime

`LiveNode::run()` is async and requires a Tokio runtime. Use `#[tokio::main]`
on your main function:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // ... node setup ...

    node.run().await?;
    Ok(())
}
```

## Adapter examples

Most adapters include runnable examples with data testers and execution
testers:

| Adapter      | Example directory                          |
|--------------|--------------------------------------------|
| Architect AX | `crates/adapters/architect_ax/examples/`   |
| Betfair      | `crates/adapters/betfair/examples/`        |
| Binance      | `crates/adapters/binance/examples/`        |
| BitMEX       | `crates/adapters/bitmex/examples/`         |
| Bybit        | `crates/adapters/bybit/examples/`          |
| Databento    | `crates/adapters/databento/examples/`      |
| Deribit      | `crates/adapters/deribit/examples/`        |
| dYdX         | `crates/adapters/dydx/examples/`           |
| Hyperliquid  | `crates/adapters/hyperliquid/examples/`    |
| Kraken       | `crates/adapters/kraken/examples/`         |
| OKX          | `crates/adapters/okx/examples/`            |
| Polymarket   | `crates/adapters/polymarket/examples/`     |
| Sandbox      | `crates/adapters/sandbox/examples/`        |
| Tardis       | `crates/adapters/tardis/examples/`         |
