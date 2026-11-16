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
use nautilus_core::{DurationNanos, UnixNanos};
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::models::{
    fee::{FeeModelAny, MakerTakerFeeModel},
    latency::{LatencyModelHandle, StaticLatencyModel},
};
use nautilus_model::{
    data::{BookOrder, Data, OrderBookDelta, OrderBookDeltas},
    enums::{
        AccountType, BookAction, BookType, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce,
    },
    identifiers::{InstrumentId, Venue},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::Order,
    types::{Money, Price, Quantity},
};
use nautilus_trading::examples::strategies::{OrderBookImbalance, OrderBookImbalanceConfig};
use rstest::*;

fn create_engine(latency_model: Option<LatencyModelHandle>) -> BacktestEngine {
    let config = BacktestEngineConfig {
        data_engine: Some(DataEngineConfig {
            emit_quotes_from_book: true,
            ..Default::default()
        }),
        ..Default::default()
    };

    let mut engine = BacktestEngine::new(config).unwrap();
    engine
        .add_venue(
            SimulatedVenueConfig::builder()
                .venue(Venue::from("BINANCE"))
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L2_MBP)
                .starting_balances(vec![Money::from("1_000_000 USDT")])
                .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into())
                .maybe_latency_model(latency_model)
                .build()
                .unwrap(),
        )
        .unwrap();
    engine
}

#[rstest]
#[case::trading(false, 6, 1)]
#[case::dry_run(true, 0, 0)]
fn test_from_config_consumes_l2_book_deltas(
    crypto_perpetual_ethusdt: CryptoPerpetual,
    #[case] dry_run: bool,
    #[case] orders: usize,
    #[case] positions: usize,
    #[values(TimeInForce::Fok, TimeInForce::Ioc)] time_in_force: TimeInForce,
) {
    let mut engine = create_engine(None);
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let config = OrderBookImbalanceConfig::builder()
        .instrument_id(instrument_id)
        .max_trade_size(Quantity::from("0.100"))
        .time_in_force(time_in_force)
        .log_interval(1)
        .dry_run(dry_run)
        .build();
    engine
        .add_strategy(OrderBookImbalance::from_config(config).unwrap())
        .unwrap();

    let updates = (0..5)
        .map(|i| (i * 1_000_000_000, "1.000"))
        .collect::<Vec<_>>();
    engine
        .add_data(book_deltas(instrument_id, &updates), None, true, true)
        .unwrap();

    engine.run(None, None, None, false).unwrap();
    let result = engine.get_result();
    assert_eq!(result.iterations, 5);
    assert_eq!(result.total_orders, orders);
    assert_eq!(result.total_positions, positions);
    assert_eq!(result.summary["orders.open"], "0");
    assert_eq!(result.summary["positions.open"], "0");
    assert_eq!(result.summary["orders.closed"], orders.to_string());
    assert_eq!(result.summary["positions.closed"], positions.to_string());
}

#[rstest]
#[case::fok(TimeInForce::Fok, "0.000", 1, 0)]
#[case::ioc(TimeInForce::Ioc, "0.040", 2, 1)]
fn test_partial_liquidity_at_order_arrival(
    crypto_perpetual_ethusdt: CryptoPerpetual,
    #[case] time_in_force: TimeInForce,
    #[case] filled_qty: &str,
    #[case] orders: usize,
    #[case] positions: usize,
) {
    let latency = StaticLatencyModel::new(
        DurationNanos::default(),
        DurationNanos::from_secs(1),
        DurationNanos::default(),
        DurationNanos::default(),
    );

    let mut engine = create_engine(Some(LatencyModelHandle::new(latency)));
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();
    let config = OrderBookImbalanceConfig::builder()
        .instrument_id(instrument_id)
        .max_trade_size(Quantity::from("0.100"))
        .min_seconds_between_triggers(3600.0)
        .time_in_force(time_in_force)
        .build();
    engine
        .add_strategy(OrderBookImbalance::from_config(config).unwrap())
        .unwrap();

    let updates = [
        (0, "1.000"),
        (500_000_000, "0.040"),
        (2_000_000_000, "0.040"),
    ];

    engine
        .add_data(book_deltas(instrument_id, &updates), None, true, true)
        .unwrap();

    engine.run(None, None, None, true).unwrap();

    {
        let cache = engine.kernel().cache.borrow();
        let entries = cache.orders(None, None, None, None, None);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.order_type(), OrderType::Limit);
        assert_eq!(entry.time_in_force(), time_in_force);
        assert_eq!(entry.order_side(), OrderSide::Buy);
        assert_eq!(entry.quantity(), Quantity::from("0.100"));
        assert_eq!(entry.price(), Some(Price::from("2000.10")));
        assert_eq!(entry.filled_qty(), Quantity::from(filled_qty));
        assert_eq!(entry.status(), OrderStatus::Canceled);
        assert_eq!(
            cache.positions_open_count(None, None, None, None, None),
            positions
        );
    }

    engine.end().unwrap();
    let result = engine.get_result();
    assert_eq!(result.total_orders, orders);
    assert_eq!(result.total_positions, positions);
    assert_eq!(result.summary["orders.open"], "0");
    assert_eq!(result.summary["positions.open"], "0");
    assert_eq!(result.summary["orders.closed"], orders.to_string());
    assert_eq!(result.summary["positions.closed"], positions.to_string());
}

fn book_deltas(instrument_id: InstrumentId, updates: &[(u64, &str)]) -> Vec<Data> {
    updates
        .iter()
        .enumerate()
        .map(|(i, (offset, ask_size))| {
            let ts = 1_600_000_000_000_000_000 + offset;

            let action = if i == 0 {
                BookAction::Add
            } else {
                BookAction::Update
            };

            Data::BookDeltas(Box::new(OrderBookDeltas::new(
                instrument_id,
                vec![
                    OrderBookDelta::new(
                        instrument_id,
                        action,
                        BookOrder::new(
                            OrderSide::Buy,
                            Price::from("1999.90"),
                            Quantity::from("150.000"),
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
                            Price::from("2000.10"),
                            Quantity::from(*ask_size),
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
