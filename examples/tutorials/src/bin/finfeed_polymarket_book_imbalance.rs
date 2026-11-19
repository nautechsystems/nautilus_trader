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
use nautilus_backtest::{
    config::{BacktestDataConfig, BacktestEngineConfig, BacktestRunConfig, BacktestVenueConfig},
    node::BacktestNode,
};
use nautilus_core::UnixNanos;
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::models::fee::{FeeModelAny, MakerTakerFeeModel};
use nautilus_finfeed::{common::symbol::to_instrument_id, loaders::FinFeedDataLoader};
use nautilus_model::{
    data::NautilusDataType,
    enums::{AccountType, BookType, OmsType, RecordFlag},
    instruments::{Instrument, InstrumentAny},
    orderbook::{OrderBook, analysis::book_check_integrity},
    types::Quantity,
};
use nautilus_persistence::{
    backend::parquet::catalog::ParquetDataCatalog, config::DataCatalogConfig,
};
use nautilus_trading::examples::strategies::{OrderBookImbalance, OrderBookImbalanceConfig};
use rust_decimal_macros::dec;

const MARKET_ID: &str = "BITCOIN-ABOVE-76K-ON-NOVEMBER-3-2026_YES";
const RUN_ID: &str = "finfeed-polymarket";

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
        .join("btc-above-76k-nov3-20261102-20");

    if !catalog_path.exists() {
        let loader = FinFeedDataLoader::new(None); // Reads FINFEED_API_KEY
        let instrument = loader.request_instrument(instrument_id).await?;
        let deltas = loader
            .request_book_deltas(instrument_id, start, end, None)
            .await?;

        std::fs::create_dir_all(&catalog_path)?;
        let catalog = ParquetDataCatalog::new(&catalog_path, None, None, None, None);
        let instruments = vec![InstrumentAny::BinaryOption(instrument)];
        catalog.write_instruments(instruments)?;
        catalog.write_to_parquet(&deltas, None, None, None)?;
    }

    let mut catalog = ParquetDataCatalog::new(&catalog_path, None, None, None, None);
    let instrument_ids = vec![instrument_id.to_string()];
    let instrument = catalog
        .instruments(Some(&instrument_ids), None, None)?
        .into_iter()
        .next()
        .context("instrument missing from catalog")?;
    let deltas = catalog.order_book_deltas(Some(instrument_ids), None, None)?;
    println!("Catalog: {}", catalog_path.display());

    let mut book = OrderBook::new(instrument_id, book_type);
    let mut batches = 0;

    for delta in &deltas {
        book.apply_delta(delta)?;
        if RecordFlag::F_LAST.matches(delta.flags) {
            book_check_integrity(&book)?;
            batches += 1;
        }
    }

    let top_levels = book.pprint(3, None);
    println!("Loaded {} deltas in {batches} batches", deltas.len());
    println!("{top_levels}");

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
        .base_currency(instrument.quote_currency())
        .starting_balances(vec!["1000 pUSD".to_string()])
        .book_type(book_type)
        .fee_model(FeeModelAny::MakerTaker(fees))
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
