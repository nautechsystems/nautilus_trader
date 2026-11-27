// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Backtests order book imbalance on Polymarket book history from FinFeed.
//!
//! Companion program for the "Backtest with Polymarket Order Book Data (FinFeed)" tutorial.
//! The first run downloads the book history and reads `FINFEED_API_KEY`. It stores the data
//! in a Parquet catalog under `NAUTILUS_DATA_DIR/FinFeed`, or `~/Downloads/Data/FinFeed`
//! when `NAUTILUS_DATA_DIR` is unset. Later runs reuse the catalog.
//!
//! Run with:
//! `cargo run --bin finfeed-polymarket-book-imbalance --package nautilus-tutorials --features examples`

use std::path::PathBuf;

use anyhow::Context;
use aws_lc_rs::digest;
use nautilus_backtest::{
    config::{BacktestDataConfig, BacktestEngineConfig, BacktestRunConfig, BacktestVenueConfig},
    node::BacktestNode,
};
use nautilus_core::{DurationNanos, UnixNanos, hex};
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::models::{
    fee::{FeeModelAny, MakerTakerFeeModel},
    latency::{LatencyModelAny, StaticLatencyModel},
};
use nautilus_finfeed::{common::symbol::to_instrument_id, loaders::FinFeedDataLoader};
use nautilus_model::{
    data::NautilusDataType,
    enums::{AccountType, BookAction, BookType, OmsType, RecordFlag},
    instruments::InstrumentAny,
    orderbook::{OrderBook, analysis::book_check_integrity},
    types::Quantity,
};
use nautilus_persistence::{
    backend::parquet::catalog::ParquetDataCatalog, config::DataCatalogConfig,
};
use nautilus_trading::examples::strategies::{OrderBookImbalance, OrderBookImbalanceConfig};
use rust_decimal_macros::dec;

const MARKET_ID: &str = "BITCOIN-ABOVE-76K-ON-NOVEMBER-3-2026_NO";
const RUN_ID: &str = "finfeed-polymarket";
const ORDER_DELAY_MS: u64 = 0;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let instrument_id = to_instrument_id("POLYMARKET", MARKET_ID)?;
    let start = UnixNanos::from("2026-11-02T20:00:05Z");
    let end = UnixNanos::from("2026-11-02T20:02:00Z");
    let book_type = BookType::L2_MBP;

    let data_dir = match std::env::var_os("NAUTILUS_DATA_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::home_dir()
            .context("home directory not found, set NAUTILUS_DATA_DIR")?
            .join("Downloads")
            .join("Data"),
    };

    let catalog_path = data_dir
        .join("FinFeed")
        .join("btc-above-76k-nov3-no-20261102-20");

    if !catalog_path.exists() {
        let loader = FinFeedDataLoader::new(None); // Reads FINFEED_API_KEY
        let instrument = loader.request_instrument(instrument_id).await?;
        let deltas = loader
            .request_book_deltas(instrument_id, start, end, None)
            .await?;

        std::fs::create_dir_all(&catalog_path)?;
        let catalog = ParquetDataCatalog::builder().uri(&catalog_path).build()?;
        let instruments = vec![InstrumentAny::BinaryOption(instrument)];
        catalog.write_instruments(instruments)?;
        catalog.write_to_parquet(&deltas, None, None, None)?;
    }

    let mut catalog = ParquetDataCatalog::builder().uri(&catalog_path).build()?;
    let instrument_ids = vec![instrument_id.to_string()];

    let Some(InstrumentAny::BinaryOption(instrument)) = catalog
        .instruments(Some(&instrument_ids), None, None)?
        .into_iter()
        .next()
    else {
        anyhow::bail!("binary option missing from catalog");
    };

    let deltas = catalog
        .order_book_deltas()
        .identifiers(instrument_ids)
        .execute()?;
    println!("Catalog: {}", catalog_path.display());

    let first = deltas.first().context("catalog has no book deltas")?;
    let last = deltas.last().context("catalog has no book deltas")?;
    anyhow::ensure!(
        first.action == BookAction::Clear && RecordFlag::F_SNAPSHOT.matches(first.flags),
        "book deltas do not open with a snapshot"
    );
    anyhow::ensure!(
        first.ts_init == start && last.ts_init <= end,
        "catalog book deltas do not match the selected window"
    );
    anyhow::ensure!(
        RecordFlag::F_LAST.matches(last.flags),
        "catalog ends with an incomplete book batch"
    );

    let snapshot_end = deltas
        .iter()
        .position(|delta| RecordFlag::F_LAST.matches(delta.flags))
        .context("catalog has no complete book batch")?;
    anyhow::ensure!(
        deltas[1..=snapshot_end].iter().all(|delta| {
            delta.action == BookAction::Add
                && RecordFlag::F_SNAPSHOT.matches(delta.flags)
                && delta.ts_init == first.ts_init
        }),
        "catalog opens with an incomplete book snapshot"
    );

    let definition = format!(
        "{},{},{},{},{},{}",
        instrument.id,
        instrument.currency,
        instrument.activation_ns,
        instrument.expiration_ns,
        instrument.price_increment,
        instrument.size_increment
    );
    let mut fingerprint = digest::Context::new(&digest::SHA256);
    fingerprint.update(definition.as_bytes());
    for constraint in [
        instrument.min_quantity.map(|value| value.to_string()),
        instrument.max_quantity.map(|value| value.to_string()),
        instrument.min_notional.map(|value| value.to_string()),
        instrument.max_notional.map(|value| value.to_string()),
        instrument.min_price.map(|value| value.to_string()),
        instrument.max_price.map(|value| value.to_string()),
        instrument.tick_scheme.map(|value| value.to_string()),
    ] {
        fingerprint.update(b",");
        fingerprint.update(constraint.as_deref().unwrap_or_default().as_bytes());
    }

    fingerprint.update(b"\n");
    let mut book = OrderBook::new(instrument_id, book_type);
    let mut batches = 0;

    for delta in &deltas {
        fingerprint.update(format!("{delta}\n").as_bytes());
        book.apply_delta(delta)?;
        if RecordFlag::F_LAST.matches(delta.flags) {
            book_check_integrity(&book)?;
            batches += 1;
        }
    }

    let top_levels = book.pprint(3, None);
    let checksum = hex::encode(fingerprint.finish());
    println!("Loaded {} deltas in {batches} batches", deltas.len());
    println!("Replay input SHA-256: {checksum}");
    println!("{top_levels}");

    let order_delay = DurationNanos::from_millis(ORDER_DELAY_MS);
    let latency =
        StaticLatencyModel::new(DurationNanos::ZERO, order_delay, order_delay, order_delay);
    println!("Order delay: {ORDER_DELAY_MS} ms");

    let data_engine = DataEngineConfig::builder()
        .emit_quotes_from_book(true)
        .build();
    let engine = BacktestEngineConfig::builder()
        .data_engine(data_engine)
        .build();

    let fees = MakerTakerFeeModel::zero();
    let venue = BacktestVenueConfig::builder()
        .name(instrument_id.venue.as_str())
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Cash)
        .base_currency(instrument.currency)
        .starting_balances(vec!["1000 pUSD".to_string()])
        .book_type(book_type)
        .fee_model(FeeModelAny::MakerTaker(fees))
        .latency_model(LatencyModelAny::Static(latency))
        .build()?;

    let catalog_location = catalog_path.display().to_string();
    let catalog_config = DataCatalogConfig::new(catalog_location, None, None);
    let data = BacktestDataConfig::builder()
        .catalog(catalog_config)
        .data_type(NautilusDataType::OrderBookDelta)
        .instrument_id(instrument_id)
        .build()?;

    let config = BacktestRunConfig::builder()
        .id(RUN_ID.to_string())
        .engine(engine)
        .venues(vec![venue])
        .data(vec![data])
        .raise_exception(true)
        .build()?;

    let max_trade_size = Quantity::from("1.000000");
    let strategy_config = OrderBookImbalanceConfig::builder()
        .instrument_id(instrument_id)
        .book_type(book_type)
        .max_trade_size(max_trade_size)
        .trigger_min_size(dec!(100))
        .trigger_imbalance_ratio(dec!(0.20))
        .min_seconds_between_triggers(1.0)
        .build();

    let mut node = BacktestNode::new(vec![config])?;
    node.build()?;

    let strategy = OrderBookImbalance::from_config(strategy_config)?;
    node.get_engine_mut(RUN_ID)
        .context("engine not built")?
        .add_strategy(strategy)?;

    let results = node.run()?;
    for result in &results {
        println!(
            "Orders: {}, positions: {}",
            result.total_orders, result.total_positions
        );
        println!("PnL statistics: {:?}", result.stats_pnls);
    }

    Ok(())
}
