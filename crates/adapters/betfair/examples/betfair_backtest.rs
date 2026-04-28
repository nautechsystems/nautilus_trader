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

//! Betfair backtest: loads historical exchange data and runs a book imbalance actor.
//!
//! This example shows how to:
//! 1. Load raw Betfair `.gz` streaming data into Nautilus domain objects
//! 2. Convert those objects into the `Data` enum the backtest engine accepts
//! 3. Configure a `BacktestEngine` with a BETFAIR venue
//! 4. Use the `BookImbalanceActor` to track order book imbalance
//!
//! The actor computes a running bid/ask quoted volume imbalance per runner.
//! In Betfair terms: bids = "back" orders, asks = "lay" orders.
//!
//! Run with: `cargo run -p nautilus-betfair --features examples --example betfair-backtest`
//!
//! Or pass a custom file path:
//!   `cargo run -p nautilus-betfair --features examples --example betfair-backtest -- path/to/file.gz`

use std::path::PathBuf;

use ahash::{AHashMap, AHashSet};
use nautilus_backtest::{
    config::{BacktestEngineConfig, SimulatedVenueConfig},
    engine::BacktestEngine,
};
use nautilus_betfair::loader::{BetfairDataItem, BetfairDataLoader};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    enums::{AccountType, BookType, OmsType},
    identifiers::{InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::{Currency, Money},
};
use nautilus_trading::examples::actors::BookImbalanceActor;

/// Loads a Betfair `.gz` streaming file and separates instruments from data.
///
/// `BetfairDataLoader` parses raw files into `BetfairDataItem` variants.
/// The backtest engine only accepts the `Data` enum, so we convert book
/// deltas, trades, and settlement events, and skip Betfair-specific types
/// (tickers, BSP, race GPS data) that have no `Data` variant.
///
/// `OrderBookDeltas_API` is a thin wrapper around `OrderBookDeltas` needed
/// by the `Data` enum (legacy FFI shim, will be removed).
fn load_betfair_data(
    filepath: &std::path::Path,
) -> anyhow::Result<(AHashMap<InstrumentId, InstrumentAny>, Vec<Data>)> {
    let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
    let items = loader.load(filepath)?;
    println!("Loaded {} raw items", items.len());

    let mut instruments: AHashMap<InstrumentId, InstrumentAny> = AHashMap::new();
    let mut data: Vec<Data> = Vec::new();
    let mut seen_ids: AHashSet<InstrumentId> = AHashSet::new();

    for item in items {
        match item {
            // Instruments are re-emitted on every market definition update,
            // so we deduplicate and keep the latest version.
            BetfairDataItem::Instrument(inst) => {
                let id = inst.id();
                if seen_ids.insert(id)
                    && let InstrumentAny::Betting(ref b) = *inst
                {
                    println!(
                        "  Runner: {}  (selection: {}, name: {})",
                        id, b.selection_id, b.selection_name
                    );
                }
                instruments.insert(id, *inst);
            }
            // Order book deltas and trades map directly to Data variants
            BetfairDataItem::Deltas(d) => {
                data.push(Data::Deltas(OrderBookDeltas_API::new(d)));
            }
            BetfairDataItem::Trade(t) => {
                data.push(Data::Trade(t));
            }
            BetfairDataItem::InstrumentClose(c) => {
                data.push(Data::InstrumentClose(c));
            }
            // Betfair-specific types (tickers, BSP, race GPS data)
            // are skipped here. A real system might handle these as custom data.
            _ => {}
        }
    }

    Ok((instruments, data))
}

fn resolve_filepath() -> PathBuf {
    std::env::args().nth(1).map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .unwrap()
                .join("tests/test_data/local/betfair/1.253378068.gz")
        },
        PathBuf::from,
    )
}

fn main() -> anyhow::Result<()> {
    let filepath = resolve_filepath();
    if !filepath.exists() {
        anyhow::bail!(
            "File not found: {}\n\nCopy Betfair .gz files to tests/test_data/local/betfair/",
            filepath.display()
        );
    }

    println!("Loading: {}", filepath.display());
    let (instruments, data) = load_betfair_data(&filepath)?;
    let instrument_ids: Vec<InstrumentId> = instruments.keys().copied().collect();
    println!(
        "\n{} instruments, {} data points",
        instruments.len(),
        data.len()
    );

    // Configure the backtest engine with a simulated BETFAIR venue.
    // Betfair is a cash-settled betting exchange with L2 order books.
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default())?;

    engine.add_venue(
        SimulatedVenueConfig::builder()
            .venue(Venue::from("BETFAIR"))
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Cash)
            .book_type(BookType::L2_MBP)
            .starting_balances(vec![Money::from("1_000_000 GBP")])
            .build(),
    )?;

    for instrument in instruments.values() {
        engine.add_instrument(instrument)?;
    }

    let log_interval: u64 = std::env::var("IMBALANCE_LOG_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);
    let actor = BookImbalanceActor::new(instrument_ids, log_interval, None);
    engine.add_actor(actor)?;
    engine.add_data(data, None, true, true)?;

    println!("\nRunning backtest...");
    engine.run(None, None, None, false)?;
    println!("\nBacktest complete: {} iterations", engine.iteration());

    Ok(())
}
