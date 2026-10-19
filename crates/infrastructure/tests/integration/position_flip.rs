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

use nautilus_common::cache::Cache;
use nautilus_core::UUID4;
use nautilus_model::{
    enums::{OmsType, OrderSide, OrderType},
    events::{
        OrderEventAny, OrderFilled,
        order::spec::{OrderAcceptedSpec, OrderFillVoidedSpec, OrderSubmittedSpec},
    },
    identifiers::{AccountId, ClientOrderId, PositionId, TradeId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    types::{Money, Price, Quantity},
};

pub(super) fn populate_flip_cache(
    cache: &mut Cache,
    trader_id: TraderId,
    reopen: &str,
    void_fill: bool,
) -> (ClientOrderId, PositionId, PositionId) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    cache
        .add_currency(instrument.base_currency().unwrap())
        .unwrap();
    cache.add_currency(instrument.quote_currency()).unwrap();
    cache.add_instrument(instrument.clone()).unwrap();
    let source_id = PositionId::from("P-PERSISTED-FLIP-SOURCE");
    let destination_id = PositionId::from("P-PERSISTED-FLIP-DESTINATION");
    let opening_order = submitted_order(
        cache,
        trader_id,
        OrderSide::Buy,
        "O-PERSISTED-OPEN",
        "2.000",
    );
    let opening_fill = fill_for_order(
        cache,
        &opening_order,
        &instrument,
        source_id,
        "T-PERSISTED-OPEN",
        "2.000",
        "0.20 USDT",
    );
    cache
        .update_order(&OrderEventAny::Filled(opening_fill.clone()))
        .unwrap();
    let mut source = Position::new(&instrument, opening_fill);
    cache.add_position(&source, OmsType::Hedging).unwrap();
    let order = submitted_order(
        cache,
        trader_id,
        OrderSide::Sell,
        "O-PERSISTED-FLIP",
        "5.000",
    );
    let fill = fill_for_order(
        cache,
        &order,
        &instrument,
        source_id,
        "T-PERSISTED-FLIP",
        "3.000",
        "0.75 USDT",
    );
    cache
        .update_order(&OrderEventAny::Filled(fill.clone()))
        .unwrap();
    let (closing, opening) = fill
        .split_for_position_flip(source.quantity, Some(destination_id), UUID4::new())
        .unwrap();
    source.apply(&closing);
    cache.update_position(&source).unwrap();
    let mut destination = Position::new(&instrument, opening.clone());
    cache.add_position(&destination, OmsType::Hedging).unwrap();

    if reopen == "source" {
        let order = submitted_order(
            cache,
            trader_id,
            OrderSide::Buy,
            "O-PERSISTED-REOPEN-SOURCE",
            "1.000",
        );
        let fill = fill_for_order(
            cache,
            &order,
            &instrument,
            source_id,
            "T-PERSISTED-REOPEN-SOURCE",
            "1.000",
            "0.30 USDT",
        );
        cache
            .update_order(&OrderEventAny::Filled(fill.clone()))
            .unwrap();
        source = Position::new(&instrument, fill);
        cache
            .replace_position(&source, OmsType::Hedging, true, true)
            .unwrap();
        source = cache.position(&source_id).unwrap().cloned();
    }

    if reopen == "destination" {
        let closing_order = submitted_order(
            cache,
            trader_id,
            OrderSide::Buy,
            "O-PERSISTED-CLOSE",
            "1.000",
        );
        let closing_fill = fill_for_order(
            cache,
            &closing_order,
            &instrument,
            destination_id,
            "T-PERSISTED-CLOSE",
            "1.000",
            "0.15 USDT",
        );
        cache
            .update_order(&OrderEventAny::Filled(closing_fill.clone()))
            .unwrap();
        destination.apply(&closing_fill);
        cache.update_position(&destination).unwrap();
        let remainder = fill_for_order(
            cache,
            &order,
            &instrument,
            destination_id,
            "T-PERSISTED-REMAINDER",
            "2.000",
            "0.50 USDT",
        );
        cache
            .update_order(&OrderEventAny::Filled(remainder.clone()))
            .unwrap();
        let reopened = Position::new(&instrument, remainder);
        cache
            .replace_position(&reopened, OmsType::Hedging, true, true)
            .unwrap();
        destination = cache.position(&destination_id).unwrap().cloned();
    }

    if void_fill {
        let voided = OrderFillVoidedSpec::builder()
            .trader_id(fill.trader_id)
            .strategy_id(fill.strategy_id)
            .instrument_id(fill.instrument_id)
            .client_order_id(fill.client_order_id)
            .venue_order_id(fill.venue_order_id)
            .account_id(fill.account_id)
            .trade_id(fill.trade_id)
            .voided_qty(fill.last_qty)
            .order_side(fill.order_side)
            .order_type(fill.order_type)
            .last_px(fill.last_px)
            .currency(fill.currency)
            .liquidity_side(fill.liquidity_side)
            .position_id(source_id)
            .commission_voided(fill.commission.unwrap())
            .is_reopened(true)
            .event_id(UUID4::new())
            .build();
        source
            .apply_fill_void(voided.clone(), closing.last_qty, closing.commission)
            .unwrap();
        destination
            .apply_fill_void(voided.clone(), opening.last_qty, opening.commission)
            .unwrap();
        cache.update_position(&source).unwrap();
        cache.update_position(&destination).unwrap();
        cache
            .update_order(&OrderEventAny::FillVoided(voided))
            .unwrap();
    }

    (order.client_order_id(), source_id, destination_id)
}

fn submitted_order(
    cache: &mut Cache,
    trader_id: TraderId,
    side: OrderSide,
    client_order_id: &str,
    quantity: &str,
) -> OrderAny {
    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(trader_id)
        .instrument_id(crypto_perpetual_ethusdt().id)
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(side)
        .quantity(Quantity::from(quantity))
        .build();
    cache.add_order(order.clone(), None, None, true).unwrap();
    let submitted = OrderSubmittedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(order.strategy_id())
        .instrument_id(order.instrument_id())
        .client_order_id(order.client_order_id())
        .account_id(AccountId::from("BINANCE-001"))
        .event_id(UUID4::new())
        .build();
    cache
        .update_order(&OrderEventAny::Submitted(submitted))
        .unwrap();
    let accepted = OrderAcceptedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(order.strategy_id())
        .instrument_id(order.instrument_id())
        .client_order_id(order.client_order_id())
        .account_id(AccountId::from("BINANCE-001"))
        .venue_order_id(VenueOrderId::from(format!("V-{client_order_id}")))
        .event_id(UUID4::new())
        .build();
    cache
        .update_order(&OrderEventAny::Accepted(accepted))
        .unwrap();
    order
}

fn fill_for_order(
    cache: &Cache,
    order: &OrderAny,
    instrument: &InstrumentAny,
    position_id: PositionId,
    trade_id: &str,
    quantity: &str,
    commission: &str,
) -> OrderFilled {
    let order = cache.order(&order.client_order_id()).unwrap();
    let mut fill: OrderFilled = TestOrderEventStubs::filled(
        &order,
        instrument,
        Some(TradeId::from(trade_id)),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from(quantity)),
        None,
        Some(Money::from(commission)),
        None,
        Some(AccountId::from("BINANCE-001")),
    )
    .into();
    fill.event_id = UUID4::new();
    fill
}
