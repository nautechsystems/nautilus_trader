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

//! Example: EMA crossover strategy backtest using [`BacktestEngine`] directly.
//!
//! Demonstrates a dual-EMA crossover strategy running on synthetic quote data
//! for the AUD/USD FX pair on a simulated venue.
//!
//! Run with: `cargo run -p nautilus-backtest --features examples --example engine-ema-cross`

use ahash::AHashMap;
use nautilus_backtest::{config::BacktestEngineConfig, engine::BacktestEngine};
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModelAny};
use nautilus_model::{
    data::{Data, QuoteTick},
    enums::{AccountType, BookType, OmsType},
    identifiers::{InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
    types::{Money, Price, Quantity},
};
use nautilus_trading::examples::strategies::EmaCross;

fn quote(instrument_id: InstrumentId, bid: &str, ask: &str, ts: u64) -> Data {
    Data::Quote(QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from("100000"),
        Quantity::from("100000"),
        ts.into(),
        ts.into(),
    ))
}

fn generate_quotes(instrument_id: InstrumentId) -> Vec<Data> {
    let spread = 0.00020;
    let base_ts: u64 = 1_735_689_600_000_000_000; // 2025-01-01T00:00:00Z
    let interval: u64 = 1_000_000_000;
    let mut quotes = Vec::new();
    let mut tick: u64 = 0;

    let mut add = |mid: f64| {
        let bid = format!("{mid:.5}");
        let ask = format!("{:.5}", mid + spread);
        quotes.push(quote(instrument_id, &bid, &ask, base_ts + tick * interval));
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

        // Ramp up — fast EMA crosses above slow → BUY signal
        for i in 0..40 {
            add(base + (i as f64 * 0.00050));
        }

        // Ramp down — fast EMA crosses below slow → SELL signal
        for i in 0..80 {
            let peak = base + 39.0 * 0.00050;
            add(peak - (i as f64 * 0.00050));
        }
    }

    quotes
}

fn main() -> anyhow::Result<()> {
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default())?;

    engine.add_venue(
        Venue::from("SIM"),
        OmsType::Hedging,
        AccountType::Margin,
        BookType::L1_MBP,
        vec![Money::from("1_000_000 USD")],
        None,            // base_currency
        None,            // default_leverage (defaults to 10x for Margin)
        AHashMap::new(), // per-instrument leverages
        None,            // margin_model
        vec![],          // simulation modules
        FillModelAny::default(),
        FeeModelAny::default(),
        None, // latency_model
        None, // routing
        None, // reject_stop_orders
        None, // support_gtd_orders
        None, // support_contingent_orders
        None, // use_position_ids
        None, // use_random_ids
        None, // use_reduce_only
        None, // use_message_queue
        None, // use_market_order_acks
        None, // bar_execution
        None, // bar_adaptive_high_low_ordering
        None, // trade_execution
        None, // liquidity_consumption
        None, // allow_cash_borrowing
        None, // frozen_account
        None, // queue_position
        None, // oto_full_trigger
        None, // price_protection_points
    )?;

    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let instrument_id = instrument.id();
    engine.add_instrument(instrument)?;

    engine.add_strategy(EmaCross::new(
        instrument_id,
        Quantity::from("100000"),
        10,
        20,
    ))?;

    let quotes = generate_quotes(instrument_id);
    engine.add_data(quotes, None, true, true);
    engine.run(None, None, None, false)?;

    Ok(())
}
