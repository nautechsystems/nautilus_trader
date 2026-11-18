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

use nautilus_common::cache::{
    Cache, CacheConfig,
    database::{CacheDatabaseAdapter, CacheDatabaseFactory},
};
use nautilus_core::UUID4;
use nautilus_model::{
    enums::{OmsType, OrderSide, PositionSide},
    events::{
        OrderFilled,
        order::spec::{OrderFillVoidedSpec, OrderFilledSpec},
    },
    identifiers::{ClientOrderId, PositionId, StrategyId, TradeId, TraderId},
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    position::Position,
    types::{Price, Quantity},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplayMode {
    Fresh,
    Carry,
    Corrected,
}

pub(super) async fn check_netting_position_reopen(
    adapter: Box<dyn CacheDatabaseAdapter>,
    database: &impl CacheDatabaseFactory,
    config: CacheConfig,
    trader_id: TraderId,
    mode: ReplayMode,
) -> Box<dyn CacheDatabaseAdapter> {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let strategy_id = StrategyId::from("NETTING-001");
    let position_id = PositionId::from(format!("{}-{strategy_id}", instrument.id()));
    let mut cache = Cache::default();
    cache.set_database(adapter);
    cache
        .add_currency(instrument.base_currency().unwrap())
        .unwrap();
    cache.add_currency(instrument.quote_currency()).unwrap();
    cache.add_instrument(instrument.clone()).unwrap();

    let opening = OrderFilledSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("O-PRIOR-OPEN"))
        .trade_id(TradeId::from("T-PRIOR-OPEN"))
        .last_qty(Quantity::from("2.000"))
        .last_px(Price::from("100.00"))
        .currency(instrument.quote_currency())
        .event_id(UUID4::new())
        .position_id(position_id)
        .build();
    let mut prior = Position::new(&instrument, opening.clone());
    cache
        .add_position_without_order(&prior, OmsType::Netting)
        .unwrap();
    apply_partial_void(&mut prior, &opening);
    cache.update_position(&prior).unwrap();

    let closing = OrderFilled {
        client_order_id: ClientOrderId::from("O-PRIOR-CLOSE"),
        trade_id: TradeId::from("T-PRIOR-CLOSE"),
        order_side: OrderSide::Sell,
        last_qty: Quantity::from("1.000"),
        event_id: UUID4::new(),
        ..opening
    };

    prior.try_apply(&closing).unwrap();
    cache.update_position(&prior).unwrap();
    assert!(prior.is_closed());

    let reopening = OrderFilled {
        client_order_id: ClientOrderId::from("O-CURRENT-OPEN"),
        trade_id: TradeId::from("T-CURRENT-OPEN"),
        last_qty: Quantity::from("4.000"),
        event_id: UUID4::new(),
        ..closing
    };

    let mut current = Position::new(&instrument, reopening.clone());
    if mode == ReplayMode::Corrected {
        apply_partial_void(&mut current, &reopening);
    }

    cache
        .replace_position(&current, OmsType::Netting, false, mode == ReplayMode::Carry)
        .unwrap();
    cache.dispose();

    let restarted = database
        .create(trader_id, UUID4::new(), config)
        .await
        .unwrap();
    let positions = restarted.load_positions().await.unwrap();
    let loaded = positions.get(&position_id).unwrap();

    let (quantity, expected_voids) = match mode {
        ReplayMode::Fresh => ("4.000", &current.fill_voids),
        ReplayMode::Carry => ("4.000", &prior.fill_voids),
        ReplayMode::Corrected => ("3.000", &current.fill_voids),
    };

    assert_eq!(loaded.quantity, Quantity::from(quantity));
    assert_eq!(loaded.side, PositionSide::Short);
    assert_eq!(
        loaded.opening_order_id,
        ClientOrderId::from("O-CURRENT-OPEN")
    );
    assert_eq!(
        serde_json::to_value(&loaded.fill_voids).unwrap(),
        serde_json::to_value(expected_voids).unwrap(),
    );

    let prior_events = if mode == ReplayMode::Carry {
        prior.replay_events.as_slice()
    } else {
        &[]
    };

    let expected_replay: Vec<_> = prior_events.iter().chain(&current.replay_events).collect();
    assert_eq!(
        serde_json::to_value(&loaded.replay_events).unwrap(),
        serde_json::to_value(expected_replay).unwrap(),
    );

    restarted
}

fn apply_partial_void(position: &mut Position, fill: &OrderFilled) {
    let voided_qty = Quantity::from("1.000");
    let event = OrderFillVoidedSpec::builder()
        .trader_id(fill.trader_id)
        .strategy_id(fill.strategy_id)
        .instrument_id(fill.instrument_id)
        .client_order_id(fill.client_order_id)
        .venue_order_id(fill.venue_order_id)
        .account_id(fill.account_id)
        .trade_id(fill.trade_id)
        .voided_qty(voided_qty)
        .order_side(fill.order_side)
        .order_type(fill.order_type)
        .last_px(fill.last_px)
        .currency(fill.currency)
        .liquidity_side(fill.liquidity_side)
        .position_id(position.id)
        .event_id(UUID4::new())
        .build();
    position.apply_fill_void(event, voided_qty, None).unwrap();
}
