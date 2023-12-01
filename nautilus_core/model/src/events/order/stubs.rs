// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::str::FromStr;

use nautilus_core::uuid::UUID4;
use rstest::fixture;
use ustr::Ustr;

use crate::{
    enums::{ContingencyType, LiquiditySide, OrderSide, OrderType, TimeInForce, TriggerType},
    events::order::{
        denied::OrderDenied, filled::OrderFilled, initialized::OrderInitialized,
        rejected::OrderRejected, triggered::OrderTriggered,
    },
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
        order_list_id::OrderListId, strategy_id::StrategyId, stubs::*, trade_id::TradeId,
        trader_id::TraderId, venue_order_id::VenueOrderId,
    },
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

#[fixture]
pub fn order_filled(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
) -> OrderFilled {
    let event_id = UUID4::new();
    OrderFilled::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        VenueOrderId::new("123456").unwrap(),
        AccountId::new("SIM-001").unwrap(),
        TradeId::new("1").unwrap(),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from_str("0.561").unwrap(),
        Price::from_str("22000").unwrap(),
        Currency::from_str("USDT").unwrap(),
        LiquiditySide::Taker,
        event_id,
        0,
        0,
        false,
        None,
        Some(Money::from_str("12.2 USDT").unwrap()),
    )
    .unwrap()
}

#[fixture]
pub fn order_denied_max_submitted_rate(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
) -> OrderDenied {
    let event_id = UUID4::new();
    OrderDenied::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        Ustr::from("Exceeded MAX_ORDER_SUBMIT_RATE"),
        event_id,
        0,
        0,
    )
    .unwrap()
}

#[fixture]
pub fn order_rejected_insufficient_margin(
    trader_id: TraderId,
    account_id: AccountId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
) -> OrderRejected {
    let event_id = UUID4::new();
    OrderRejected::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        account_id,
        Ustr::from("INSUFFICIENT_MARGIN"),
        event_id,
        0,
        0,
        false,
    )
    .unwrap()
}

#[fixture]
pub fn order_initialized_buy_limit(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
) -> OrderInitialized {
    let event_id = UUID4::new();
    let order_list_id = OrderListId::new("1").unwrap();
    let linked_order_ids = vec![ClientOrderId::new("O-2020872378424").unwrap()];
    OrderInitialized::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from_str("0.561").unwrap(),
        TimeInForce::Day,
        true,
        true,
        false,
        false,
        event_id,
        0,
        0,
        Some(Price::from_str("22000").unwrap()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(TriggerType::BidAsk),
        Some(instrument_id_btc_usdt),
        Some(ContingencyType::Oto),
        Some(order_list_id),
        Some(linked_order_ids),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap()
}

#[fixture]
pub fn order_triggered(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
) -> OrderTriggered {
    let event_id = UUID4::new();
    OrderTriggered::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        event_id,
        0,
        0,
        false,
        Some(venue_order_id),
        Some(account_id),
    )
    .unwrap()
}
