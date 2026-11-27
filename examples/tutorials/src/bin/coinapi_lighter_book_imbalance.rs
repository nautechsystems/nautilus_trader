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

//! Backtests order book imbalance on Lighter ANT equity perpetual book data from CoinAPI.
//!
//! Companion program for the "Backtest with Equity Perpetual Order Book Data (CoinAPI)" tutorial.
//! The first run downloads one Flat Files export and reads `COINAPI_API_KEY`. It stores the
//! file and a Parquet catalog under `NAUTILUS_DATA_DIR/CoinAPI`, or `~/Downloads/Data/CoinAPI`
//! when `NAUTILUS_DATA_DIR` is unset. Later runs reuse the catalog.
//!
//! Run with:
//! `cargo run --bin coinapi-lighter-book-imbalance --package nautilus-tutorials --features examples`

use std::path::PathBuf;

use anyhow::Context;
use aws_lc_rs::digest;
use nautilus_backtest::{
    config::{BacktestDataConfig, BacktestEngineConfig, BacktestRunConfig, BacktestVenueConfig},
    node::BacktestNode,
};
use nautilus_coinapi::loaders::CoinApiDataLoader;
use nautilus_core::{DurationNanos, UnixNanos, hex};
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::models::{
    fee::{FeeModelAny, MakerTakerFeeModel},
    latency::{LatencyModelAny, StaticLatencyModel},
};
use nautilus_model::{
    accounts::margin_model::{MarginModelAny, StandardMarginModel},
    data::NautilusDataType,
    enums::{AccountType, BookAction, BookType, OmsType, RecordFlag},
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny},
    orderbook::{OrderBook, analysis::book_check_integrity},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_persistence::{
    backend::parquet::catalog::ParquetDataCatalog, config::DataCatalogConfig,
};
use nautilus_trading::examples::strategies::{OrderBookImbalance, OrderBookImbalanceConfig};
use rust_decimal_macros::dec;

const BUCKET: &str = "coinapi";
const OBJECT_KEY: &str =
    "T-LIMITBOOK_FULL/D-2026112315/E-LIGHTER/IDDI-48047693+SC-LIGHTER_PERP_ANT_USD+S-ANT.csv.gz";
const FILE_SHA256: &str = "5a05a328f1622c88091238ad190d7e63c370d1d3e845f5b5d0f5b49401ccf8e1";
const RUN_ID: &str = "coinapi-lighter";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let instrument_id = InstrumentId::from("ANT-PERP.LIGHTER");
    let book_type = BookType::L2_MBP;

    let data_dir = match std::env::var_os("NAUTILUS_DATA_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::home_dir()
            .context("home directory not found, set NAUTILUS_DATA_DIR")?
            .join("Downloads")
            .join("Data"),
    }
    .join("CoinAPI");

    let filepath = data_dir.join(OBJECT_KEY);
    let catalog_path = data_dir.join("lighter-ant-20261123-15");
    if !catalog_path.exists() {
        let loader = CoinApiDataLoader::new(None); // Reads COINAPI_API_KEY to download
        if !filepath.is_file() {
            loader
                .download_flat_file(BUCKET, OBJECT_KEY, &filepath)
                .await?;
        }

        let file = std::fs::read(&filepath)?;
        let file_digest = digest::digest(&digest::SHA256, &file);
        let checksum = hex::encode(file_digest);
        anyhow::ensure!(
            checksum == FILE_SHA256,
            "unexpected SHA-256 for {}, verify the selected export",
            filepath.display()
        );

        let usdc = Currency::USDC();
        let raw_symbol = Symbol::from("ANT");
        let base_currency = Currency::get_or_create_crypto("ANT");
        let price_increment = Price::from("0.01");
        let size_increment = Quantity::from("0.0001");
        let min_quantity = Quantity::from("0.1000");
        let min_notional = Money::from_decimal(dec!(10), usdc)?;
        let instrument = CryptoPerpetual::builder()
            .instrument_id(instrument_id)
            .raw_symbol(raw_symbol)
            .base_currency(base_currency)
            .quote_currency(usdc)
            .settlement_currency(usdc)
            .is_inverse(false)
            .price_precision(2)
            .size_precision(4)
            .price_increment(price_increment)
            .size_increment(size_increment)
            .min_quantity(min_quantity)
            .min_notional(min_notional)
            .margin_init(dec!(0.10))
            .margin_maint(dec!(0.06))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()?;

        let instrument = InstrumentAny::CryptoPerpetual(instrument);
        loader.add_instrument(instrument.clone(), None, None)?;
        let deltas = loader.load_book_deltas(&filepath, instrument_id, None, None)?;

        std::fs::create_dir_all(&catalog_path)?;
        let catalog = ParquetDataCatalog::builder().uri(&catalog_path).build()?;
        let instruments = vec![instrument];
        catalog.write_instruments(instruments)?;
        catalog.write_to_parquet(&deltas, None, None, None)?;
    }

    let mut catalog = ParquetDataCatalog::builder().uri(&catalog_path).build()?;
    let instrument_ids = vec![instrument_id.to_string()];
    let instrument = catalog
        .instruments(Some(&instrument_ids), None, None)?
        .into_iter()
        .next()
        .context("instrument missing from catalog")?;
    let deltas = catalog
        .order_book_deltas()
        .identifiers(instrument_ids)
        .execute()?;
    println!("Catalog: {}", catalog_path.display());
    println!(
        "Instrument: {}, price precision {}, size precision {}",
        instrument.id(),
        instrument.price_precision(),
        instrument.size_precision()
    );

    let first = deltas.first().context("catalog has no book deltas")?;
    anyhow::ensure!(
        first.action == BookAction::Clear && RecordFlag::F_SNAPSHOT.matches(first.flags),
        "book deltas do not open with a snapshot"
    );

    let mut book = OrderBook::new(instrument_id, book_type);
    let mut batches = 0;
    let mut locked_or_crossed = 0;

    for delta in &deltas {
        book.apply_delta(delta)?;
        if !RecordFlag::F_LAST.matches(delta.flags) {
            continue;
        }

        batches += 1;

        if RecordFlag::F_SNAPSHOT.matches(delta.flags) {
            book_check_integrity(&book)?;
        }

        if let (Some(bid), Some(ask)) = (book.best_bid_price(), book.best_ask_price())
            && bid >= ask
        {
            locked_or_crossed += 1;
        }
    }

    book_check_integrity(&book)?;
    let top_levels = book.pprint(3, None);
    println!("Loaded {} deltas in {batches} batches", deltas.len());
    println!("Locked or crossed books: {locked_or_crossed}");
    println!("{top_levels}");

    let data_engine = DataEngineConfig::builder()
        .emit_quotes_from_book(true)
        .build();
    let engine = BacktestEngineConfig::builder()
        .data_engine(data_engine)
        .build();

    let fees = MakerTakerFeeModel::zero();
    let order_delay = DurationNanos::from_millis(300);
    let latency =
        StaticLatencyModel::new(DurationNanos::ZERO, order_delay, order_delay, order_delay);
    let venue = BacktestVenueConfig::builder()
        .name(instrument_id.venue.as_str())
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .starting_balances(vec!["100000 USDC".to_string()])
        .book_type(book_type)
        .margin_model(MarginModelAny::Standard(StandardMarginModel))
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

    let max_trade_size = Quantity::from("0.2500");
    let strategy_config = OrderBookImbalanceConfig::builder()
        .instrument_id(instrument_id)
        .book_type(book_type)
        .max_trade_size(max_trade_size)
        .trigger_min_size(dec!(1.0))
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
