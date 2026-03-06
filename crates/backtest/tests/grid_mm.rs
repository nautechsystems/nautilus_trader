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

use ahash::AHashMap;
use nautilus_backtest::{config::BacktestEngineConfig, engine::BacktestEngine};
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModelAny};
use nautilus_model::{
    data::{Data, QuoteTick},
    enums::{AccountType, BookType, OmsType},
    identifiers::{InstrumentId, Venue},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Money, Price, Quantity},
};
use nautilus_trading::examples::strategies::{GridMarketMaker, GridMarketMakerConfig};
use rstest::*;

fn create_engine() -> BacktestEngine {
    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();
    engine
        .add_venue(
            Venue::from("BINANCE"),
            OmsType::Netting,
            AccountType::Margin,
            BookType::L1_MBP,
            vec![Money::from("1_000_000 USDT")],
            None,
            None,
            AHashMap::new(),
            None,
            vec![],
            FillModelAny::default(),
            FeeModelAny::default(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
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
fn test_generates_orders(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(instrument).unwrap();

    let config = GridMarketMakerConfig::new(instrument_id, Quantity::from("10.0"))
        .with_trade_size(Quantity::from("0.100"))
        .with_num_levels(3)
        .with_grid_step_bps(10)
        .with_skew_factor(0.01)
        .with_requote_threshold_bps(5);
    engine.add_strategy(GridMarketMaker::new(config)).unwrap();

    // Phase 1: Stable at 1000 (5 ticks, within threshold) — initial quote placed, rest skip
    // Phase 2: Ramp up to 1010 (20 ticks, 0.5/tick) — triggers requotes as mid moves
    // Phase 3: Ramp down to 1000 (20 ticks) — triggers requotes in opposite direction
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

    // Phase 1: Stable
    for _ in 0..5 {
        add_quote(&mut quotes, 1000.0, &mut tick);
    }

    // Phase 2: Ramp up
    for i in 0..20 {
        add_quote(&mut quotes, 1000.0 + (i as f64 * 0.5), &mut tick);
    }

    // Phase 3: Ramp down
    for i in 0..20 {
        add_quote(&mut quotes, 1009.5 - (i as f64 * 0.5), &mut tick);
    }

    let total_quotes = quotes.len();
    engine.add_data(quotes, None, true, true);

    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, total_quotes);
    // With 3 levels, each requote submits up to 6 orders (3 buy + 3 sell)
    assert!(
        bt_result.total_orders >= 6,
        "Expected limit orders from grid market maker, was {}",
        bt_result.total_orders
    );
}

#[rstest]
fn test_skips_requote_within_threshold(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(instrument).unwrap();

    let config = GridMarketMakerConfig::new(instrument_id, Quantity::from("10.0"))
        .with_trade_size(Quantity::from("0.100"))
        .with_num_levels(3)
        .with_grid_step_bps(10)
        .with_skew_factor(0.01)
        .with_requote_threshold_bps(50);
    engine.add_strategy(GridMarketMaker::new(config)).unwrap();

    // All quotes within the 5.0 threshold — only the first triggers orders
    let quotes: Vec<Data> = (0..10u64)
        .map(|i| {
            let mid = 1000.0 + (i as f64 * 0.1);
            quote(
                instrument_id,
                &format!("{:.2}", mid - 0.05),
                &format!("{:.2}", mid + 0.05),
                1_000_000_000 + i * 1_000_000_000,
            )
        })
        .collect();
    engine.add_data(quotes, None, true, true);

    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, 10);
    // Only 1 requote (the first tick), 3 levels × 2 sides = 6 orders
    assert_eq!(
        bt_result.total_orders, 6,
        "Expected exactly 6 orders from single initial quote, was {}",
        bt_result.total_orders
    );
}

#[rstest]
fn test_enforces_max_position_across_levels(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(instrument).unwrap();

    let config = GridMarketMakerConfig::new(instrument_id, Quantity::from("0.150"))
        .with_trade_size(Quantity::from("0.100"))
        .with_num_levels(3)
        .with_grid_step_bps(10)
        .with_requote_threshold_bps(5);
    engine.add_strategy(GridMarketMaker::new(config)).unwrap();

    // Single quote to trigger one requote cycle
    let quotes = vec![quote(instrument_id, "999.95", "1000.05", 1_000_000_000)];
    engine.add_data(quotes, None, true, true);

    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();
    // max_position=0.15, trade_size=0.1: only 1 buy + 1 sell fit
    assert_eq!(
        bt_result.total_orders, 2,
        "Expected 2 orders (1 buy + 1 sell) due to max_position limit, was {}",
        bt_result.total_orders
    );
}
