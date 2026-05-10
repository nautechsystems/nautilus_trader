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

//! Loads a Betfair historical data file and prints summary statistics.
//!
//! Run with: `cargo run -p nautilus-betfair --example betfair-load-file -- <path>`
//!
//! The path should point to a `.gz` file containing Betfair Exchange Streaming
//! API data (newline-delimited JSON MCM messages).
//!
//! Default path: `tests/test_data/local/betfair/1.253378068.gz`

use std::{collections::BTreeMap, path::PathBuf};

use nautilus_betfair::loader::{BetfairDataItem, BetfairDataLoader};
use nautilus_model::{
    instruments::{Instrument, InstrumentAny},
    types::Currency,
};

fn main() -> anyhow::Result<()> {
    let filepath = std::env::args().nth(1).map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .unwrap()
                .join("tests/test_data/local/betfair/1.253378068.gz")
        },
        PathBuf::from,
    );

    if !filepath.exists() {
        anyhow::bail!(
            "File not found: {}\n\nCopy Betfair .gz files to tests/test_data/local/betfair/",
            filepath.display()
        );
    }

    println!("Loading: {}", filepath.display());

    let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
    let items = loader.load(&filepath)?;

    let mut instruments = 0u64;
    let mut statuses = 0u64;
    let mut deltas = 0u64;
    let mut trades = 0u64;
    let mut tickers = 0u64;
    let mut starting_prices = 0u64;
    let mut bsp_deltas = 0u64;
    let mut closes = 0u64;
    let mut sequences = 0u64;
    let mut race_runners = 0u64;
    let mut race_progress = 0u64;

    for item in &items {
        match item {
            BetfairDataItem::Instrument(_) => instruments += 1,
            BetfairDataItem::Status(_) => statuses += 1,
            BetfairDataItem::Deltas(_) => deltas += 1,
            BetfairDataItem::Trade(_) => trades += 1,
            BetfairDataItem::Ticker(_) => tickers += 1,
            BetfairDataItem::StartingPrice(_) => starting_prices += 1,
            BetfairDataItem::BspBookDelta(_) => bsp_deltas += 1,
            BetfairDataItem::InstrumentClose(_) => closes += 1,
            BetfairDataItem::SequenceCompleted(_) => sequences += 1,
            BetfairDataItem::RaceRunnerData(_) => race_runners += 1,
            BetfairDataItem::RaceProgress(_) => race_progress += 1,
        }
    }

    println!("\n--- Summary ---");
    println!("Total items:       {}", items.len());
    println!("Instruments:       {instruments}");
    println!("Status updates:    {statuses}");
    println!("Book deltas:       {deltas}");
    println!("Trade ticks:       {trades}");
    println!("Tickers:           {tickers}");
    println!("Starting prices:   {starting_prices}");
    println!("BSP book deltas:   {bsp_deltas}");
    println!("Instrument closes: {closes}");
    println!("Sequence markers:  {sequences}");

    if race_runners > 0 {
        println!("Race runner data:  {race_runners}");
    }

    if race_progress > 0 {
        println!("Race progress:     {race_progress}");
    }

    println!("\n--- Instruments ---");
    let mut seen: BTreeMap<String, &InstrumentAny> = BTreeMap::new();

    for item in &items {
        if let BetfairDataItem::Instrument(inst) = item {
            seen.entry(inst.id().to_string()).or_insert(inst);
        }
    }

    for (id, inst) in &seen {
        if let InstrumentAny::Betting(b) = inst {
            println!(
                "  {id}  (selection: {}, name: {})",
                b.selection_id, b.selection_name
            );
        }
    }

    println!("\n--- Settlement ---");

    for item in &items {
        if let BetfairDataItem::InstrumentClose(close) = item {
            let label = if close.close_price.as_f64() > 0.5 {
                "WINNER"
            } else {
                "LOSER"
            };
            println!(
                "  {} -> {} (price: {})",
                close.instrument_id, label, close.close_price
            );
        }
    }

    Ok(())
}
