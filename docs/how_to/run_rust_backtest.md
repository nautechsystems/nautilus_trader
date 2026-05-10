# Run a Backtest (Rust)

Nautilus provides two Rust APIs for backtesting: `BacktestEngine`
(low-level) and `BacktestNode` (high-level with catalog streaming). This
guide covers both.

For background on backtesting concepts, fill models, and matching engine
behavior, see the [Backtesting](../concepts/backtesting.md) concept guide.
For project setup and feature flags, see the
[Rust](../concepts/rust.md#project-setup) concept guide.

## Dependencies

Add the following to your `Cargo.toml`. The `streaming` and
`nautilus-persistence` entries are only needed for the high-level
`BacktestNode` API.

```toml
[dependencies]
nautilus-backtest = { version = "0.55", features = ["streaming"] }
nautilus-execution = "0.55"
nautilus-model = { version = "0.55", features = ["stubs"] }
nautilus-persistence = "0.55"
nautilus-trading = { version = "0.55", features = ["examples"] }

ahash = "0.8"
anyhow = "1"
tempfile = "3"
ustr = "1"
```

If you only need the low-level `BacktestEngine`, drop `streaming`,
`nautilus-persistence`, `tempfile`, and `ustr`.

## BacktestEngine (low-level API)

The low-level API gives direct control: you build the engine, add venues and
instruments, load data in memory, register strategies, and run.

### 1. Create the engine

```rust
use nautilus_backtest::{config::BacktestEngineConfig, engine::BacktestEngine};

let mut engine = BacktestEngine::new(BacktestEngineConfig::default())?;
```

### 2. Add a venue

`SimulatedVenueConfig` uses a `bon::Builder`: only required fields must be set,
every other setting falls back to a documented default.

```rust
use nautilus_backtest::config::SimulatedVenueConfig;
use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    identifiers::Venue,
    types::Money,
};

engine.add_venue(
    SimulatedVenueConfig::builder()
        .venue(Venue::from("SIM"))
        .oms_type(OmsType::Hedging)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![Money::from("1_000_000 USD")])
        .build(),
)?;
```

Override any default by chaining setters, e.g. `.reject_stop_orders(false)` or
`.allow_cash_borrowing(true)`.

### 3. Add instruments and data

```rust
use nautilus_model::instruments::{
    Instrument, InstrumentAny, stubs::audusd_sim,
};

let instrument = InstrumentAny::CurrencyPair(audusd_sim());
let instrument_id = instrument.id();
engine.add_instrument(&instrument)?;

let quotes = generate_quotes(instrument_id); // Your data loading function
engine.add_data(quotes, None, true, true)?;
```

### 4. Register a strategy and run

```rust
use nautilus_model::types::Quantity;
use nautilus_trading::examples::strategies::EmaCross;

let strategy = EmaCross::new(
    instrument_id,
    Quantity::from("100000"),
    10, // fast EMA period
    20, // slow EMA period
);

engine.add_strategy(strategy)?;
engine.run(None, None, None, false)?;
```

### Run the full example

```bash
cargo run -p nautilus-backtest --features examples --example engine-ema-cross
```

Source:
[`crates/backtest/examples/engine_ema_cross.rs`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/backtest/examples/engine_ema_cross.rs)

## BacktestNode (high-level API)

The high-level API loads data from a `ParquetDataCatalog` and streams in
configurable chunk sizes. Requires the `streaming` feature on
`nautilus-backtest`.

### 1. Write data to a catalog

```rust
use nautilus_model::instruments::{
    Instrument, InstrumentAny, stubs::audusd_sim,
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use tempfile::TempDir;

let instrument = InstrumentAny::CurrencyPair(audusd_sim());
let instrument_id = instrument.id();
let quotes = generate_quotes(instrument_id);

let temp_dir = TempDir::new()?;
let catalog_path = temp_dir.path().to_str()
    .context("temp dir path is not valid UTF-8")?
    .to_string();
let catalog = ParquetDataCatalog::new(
    temp_dir.path(), None, None, None, None,
);

catalog.write_instruments(vec![instrument])?;
catalog.write_to_parquet(quotes, None, None, None)?;
```

### 2. Configure the run

```rust
use nautilus_backtest::config::{
    BacktestDataConfig, BacktestEngineConfig,
    BacktestRunConfig, BacktestVenueConfig, NautilusDataType,
};
use nautilus_model::enums::{AccountType, BookType, OmsType};
use ustr::Ustr;

let venue_config = BacktestVenueConfig::new(
    Ustr::from("SIM"),
    OmsType::Hedging,
    AccountType::Margin,
    BookType::L1_MBP,
    None, // routing
    None, // frozen_account
    None, // reject_stop_orders
    None, // support_gtd_orders
    None, // support_contingent_orders
    None, // use_position_ids
    None, // use_random_ids
    None, // use_reduce_only
    None, // bar_execution
    None, // bar_adaptive_high_low_ordering
    None, // trade_execution
    None, // use_market_order_acks
    None, // liquidity_consumption
    None, // allow_cash_borrowing
    None, // queue_position
    None, // oto_trigger_mode
    vec!["1_000_000 USD".to_string()],
    None, // base_currency
    None, // default_leverage
    None, // leverages
    None, // price_protection_points
);

let data_config = BacktestDataConfig::new(
    NautilusDataType::QuoteTick,
    catalog_path,
    None, // catalog_fs_protocol
    None, // catalog_fs_storage_options
    Some(instrument_id),
    None, // instrument_ids
    None, // start_time
    None, // end_time
    None, // filter_expr
    None, // client_id
    None, // metadata
    None, // bar_spec
    None, // bar_types
    None, // optimize_file_loading
);

let run_config = BacktestRunConfig::new(
    Some("ema-cross-run".to_string()),
    vec![venue_config],
    vec![data_config],
    BacktestEngineConfig::default(),
    Some(100), // Stream in chunks of 100
    None,      // dispose_on_completion
    None,      // start
    None,      // end
);
```

### 3. Build, add strategies, and run

```rust
use nautilus_backtest::node::BacktestNode;
use nautilus_model::types::Quantity;
use nautilus_trading::examples::strategies::EmaCross;

let mut node = BacktestNode::new(vec![run_config])?;
node.build()?;

let engine = node.get_engine_mut("ema-cross-run")
    .context("engine not found for run config ID")?;
let strategy = EmaCross::new(
    instrument_id,
    Quantity::from("100000"),
    10,
    20,
);
engine.add_strategy(strategy)?;

node.run()?;
```

### Run the full example

```bash
cargo run -p nautilus-backtest --features examples,streaming --example node-ema-cross
```

Source:
[`crates/backtest/examples/node_ema_cross.rs`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/backtest/examples/node_ema_cross.rs)
