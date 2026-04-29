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

#![cfg(feature = "examples")]

use nautilus_backtest::{
    config::{BacktestEngineConfig, SimulatedVenueConfig},
    engine::BacktestEngine,
};
use nautilus_model::{
    data::{Data, QuoteTick},
    enums::{AccountType, BookType, OmsType},
    identifiers::{InstrumentId, StrategyId, Venue},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Money, Price, Quantity},
};
use nautilus_trading::examples::strategies::{EmaCross, EmaCrossConfig};
use rstest::*;

fn create_engine() -> BacktestEngine {
    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();
    engine
        .add_venue(
            SimulatedVenueConfig::builder()
                .venue(Venue::from("BINANCE"))
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L1_MBP)
                .starting_balances(vec![Money::from("1_000_000 USDT")])
                .build(),
        )
        .unwrap();
    engine
}

fn quote(instrument_id: InstrumentId, bid: &str, ask: &str, ts: u64) -> Data {
    Data::Quote(QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        ts.into(),
        ts.into(),
    ))
}

#[rstest]
fn test_from_config_generates_orders(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let config = EmaCrossConfig::new(instrument_id, Quantity::from("0.100"), 10, 20);
    engine.add_strategy(EmaCross::from_config(config)).unwrap();

    // Phase 1: Flat at 1000 (25 ticks) — both EMAs initialize and converge
    // Phase 2: Ramp up to 1200 (40 ticks) — fast EMA crosses above slow -> BUY
    // Phase 3: Ramp down to 800 (80 ticks) — fast EMA crosses below slow -> SELL
    // Phase 4: Ramp up to 1000 (40 ticks) — fast crosses above again -> BUY
    let spread = 0.10;
    let mut quotes = Vec::new();
    let base_ts: u64 = 1_000_000_000;
    let interval: u64 = 1_000_000_000;
    let mut tick: u64 = 0;

    let add_quote = |quotes: &mut Vec<Data>, mid: f64, tick: &mut u64| {
        let bid = format!("{:.2}", mid - spread / 2.0);
        let ask = format!("{:.2}", mid + spread / 2.0);
        quotes.push(quote(instrument_id, &bid, &ask, base_ts + *tick * interval));
        *tick += 1;
    };

    for _ in 0..25 {
        add_quote(&mut quotes, 1000.0, &mut tick);
    }

    for i in 0..40 {
        add_quote(&mut quotes, 1000.0 + (i as f64 * 5.0), &mut tick);
    }

    for i in 0..80 {
        add_quote(&mut quotes, 1195.0 - (i as f64 * 5.0), &mut tick);
    }

    for i in 0..40 {
        add_quote(&mut quotes, 800.0 + (i as f64 * 5.0), &mut tick);
    }

    let total_quotes = quotes.len();
    engine.add_data(quotes, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, total_quotes);
    assert!(
        bt_result.total_orders >= 2,
        "Expected at least 2 orders (buy + sell crossovers), was {}",
        bt_result.total_orders,
    );
    assert!(
        bt_result.total_positions > 0,
        "Expected positions from filled orders",
    );
}

#[rstest]
fn test_from_config_with_custom_strategy_id(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let config = EmaCrossConfig::new(instrument_id, Quantity::from("0.100"), 5, 15)
        .with_strategy_id(StrategyId::from("MY_EMA-002"))
        .with_order_id_tag("002".to_string());

    engine.add_strategy(EmaCross::from_config(config)).unwrap();

    // Flat data to verify startup and shutdown without panics
    let quotes: Vec<Data> = (0..20u64)
        .map(|i| {
            quote(
                instrument_id,
                "999.95",
                "1000.05",
                1_000_000_000 + i * 1_000_000_000,
            )
        })
        .collect();
    engine.add_data(quotes, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();
    assert_eq!(engine.get_result().iterations, 20);
}
