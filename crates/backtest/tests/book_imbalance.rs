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
use nautilus_common::actor::registry::try_get_actor_unchecked;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, Data, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API},
    enums::{AccountType, BookAction, BookType, OmsType, OrderSide},
    identifiers::{ActorId, InstrumentId, Venue},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Money, Price, Quantity},
};
use nautilus_trading::examples::actors::{BookImbalanceActor, BookImbalanceActorConfig};
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
                .book_type(BookType::L2_MBP)
                .starting_balances(vec![Money::from("1_000_000 USDT")])
                .build()
                .unwrap(),
        )
        .unwrap();
    engine
}

#[rstest]
fn test_from_config_consumes_l2_book_deltas(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let actor_id = ActorId::from("BOOK_IMBALANCE-001");
    let config = BookImbalanceActorConfig::builder()
        .instrument_ids(vec![instrument_id])
        .log_interval(1)
        .actor_id(actor_id)
        .build();
    engine
        .add_actor(BookImbalanceActor::from_config(config))
        .unwrap();

    engine
        .add_data(book_deltas(instrument_id), None, true, true)
        .unwrap();

    engine.run(None, None, None, false).unwrap();
    let actor = try_get_actor_unchecked::<BookImbalanceActor>(&actor_id.inner()).unwrap();
    let state = &actor.states()[&instrument_id];

    assert_eq!(state.update_count, 5);
    assert!((state.bid_volume_total - 50.0).abs() < 1e-10);
    assert!((state.ask_volume_total - 50.0).abs() < 1e-10);
    assert!(state.imbalance().abs() < 1e-10);

    let result = engine.get_result();
    assert_eq!(result.iterations, 5);
    assert_eq!(result.total_orders, 0);
    assert_eq!(result.total_positions, 0);
    assert_eq!(result.summary["orders.open"], "0");
    assert_eq!(result.summary["positions.open"], "0");
}

fn book_deltas(instrument_id: InstrumentId) -> Vec<Data> {
    [
        ("1999.90", "2000.10"),
        ("2000.00", "2000.20"),
        ("2000.10", "2000.30"),
        ("2000.20", "2000.40"),
        ("2000.30", "2000.50"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (bid, ask))| {
        let ts = 1_600_000_000_000_000_000 + (i as u64 * 1_000_000_000);
        let action = if i == 0 {
            BookAction::Add
        } else {
            BookAction::Update
        };
        Data::Deltas(OrderBookDeltas_API::new(OrderBookDeltas::new(
            instrument_id,
            vec![
                OrderBookDelta::new(
                    instrument_id,
                    action,
                    BookOrder::new(
                        OrderSide::Buy,
                        Price::from(bid),
                        Quantity::from("10.000"),
                        1,
                    ),
                    0,
                    (i as u64 * 2) + 1,
                    UnixNanos::from(ts),
                    UnixNanos::from(ts),
                ),
                OrderBookDelta::new(
                    instrument_id,
                    action,
                    BookOrder::new(
                        OrderSide::Sell,
                        Price::from(ask),
                        Quantity::from("10.000"),
                        2,
                    ),
                    0,
                    (i as u64 * 2) + 2,
                    UnixNanos::from(ts),
                    UnixNanos::from(ts),
                ),
            ],
        )))
    })
    .collect()
}
