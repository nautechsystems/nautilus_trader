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

//! Example: EMA crossover strategy backtest using [`BacktestNode`] with a catalog.
//!
//! Demonstrates the same dual-EMA crossover strategy as `engine-ema-cross`, but
//! driven by a [`ParquetDataCatalog`] through [`BacktestNode`]. Synthetic quote
//! data is written to a temporary catalog, then loaded and streamed by the node.
//!
//! Run with: `cargo run -p nautilus-backtest --features examples,streaming --example node-ema-cross`

use nautilus_backtest::{
    config::{BacktestDataConfig, BacktestRunConfig, BacktestVenueConfig, NautilusDataType},
    node::BacktestNode,
};
use nautilus_model::{
    data::QuoteTick,
    enums::{AccountType, BookType, OmsType},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
    types::{Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_trading::examples::strategies::EmaCross;
use tempfile::TempDir;
use ustr::Ustr;

fn generate_quotes(instrument_id: InstrumentId) -> Vec<QuoteTick> {
    let spread = 0.00020;
    let base_ts: u64 = 1_735_689_600_000_000_000; // 2025-01-01T00:00:00Z
    let interval: u64 = 1_000_000_000;
    let mut quotes = Vec::new();
    let mut tick: u64 = 0;

    let mut add = |mid: f64| {
        let bid = format!("{mid:.5}");
        let ask = format!("{:.5}", mid + spread);
        quotes.push(QuoteTick::new(
            instrument_id,
            Price::from(bid.as_str()),
            Price::from(ask.as_str()),
            Quantity::from("100000"),
            Quantity::from("100000"),
            (base_ts + tick * interval).into(),
            (base_ts + tick * interval).into(),
        ));
        tick += 1;
    };

    // Flat initialization — both EMAs converge around 0.65000
    for _ in 0..25 {
        add(0.65000);
    }

    // Repeated up/down cycles to generate multiple crossovers
    let cycles = 6;
    for cycle in 0..cycles {
        let base = 0.65000 + (cycle as f64 * 0.00100);

        // Ramp up — fast EMA crosses above slow -> BUY signal
        for i in 0..40 {
            add(base + (i as f64 * 0.00050));
        }

        // Ramp down — fast EMA crosses below slow -> SELL signal
        for i in 0..80 {
            let peak = base + 39.0 * 0.00050;
            add(peak - (i as f64 * 0.00050));
        }
    }

    quotes
}

fn main() -> anyhow::Result<()> {
    // Write synthetic data to a temporary parquet catalog
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let instrument_id = instrument.id();
    let quotes = generate_quotes(instrument_id);
    let num_quotes = quotes.len();

    let temp_dir = TempDir::new()?;
    let catalog_path = temp_dir.path().to_str().unwrap().to_string();
    let catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
    catalog.write_instruments(vec![instrument])?;
    catalog.write_to_parquet(quotes, None, None, None)?;

    println!("Wrote {num_quotes} quotes to catalog: {catalog_path}");

    // Configure the backtest run
    let venue_config = BacktestVenueConfig::builder()
        .name(Ustr::from("SIM"))
        .oms_type(OmsType::Hedging)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec!["1_000_000 USD".to_string()])
        .build();

    let data_config = BacktestDataConfig::builder()
        .data_type(NautilusDataType::QuoteTick)
        .catalog_path(catalog_path)
        .instrument_id(instrument_id)
        .build();

    let run_config = BacktestRunConfig::builder()
        .id("ema-cross-run".to_string())
        .venues(vec![venue_config])
        .data(vec![data_config])
        .chunk_size(100) // Stream in chunks of 100
        .build();

    // Build and run the backtest
    let mut node = BacktestNode::new(vec![run_config])?;
    node.build()?;

    let engine = node.get_engine_mut("ema-cross-run").unwrap();
    engine.add_strategy(EmaCross::new(
        instrument_id,
        Quantity::from("100000"),
        10,
        20,
    ))?;

    node.run()?;

    Ok(())
}
