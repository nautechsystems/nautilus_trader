// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
        accepted::OrderAccepted, cancel_rejected::OrderCancelRejected, denied::OrderDenied,
        emulated::OrderEmulated, expired::OrderExpired, filled::OrderFilled,
        initialized::OrderInitialized, modify_rejected::OrderModifyRejected,
        pending_cancel::OrderPendingCancel, pending_update::OrderPendingUpdate,
        rejected::OrderRejected, released::OrderReleased, submitted::OrderSubmitted,
        triggered::OrderTriggered, updated::OrderUpdated,
    },
    identifiers::{
        account_id::AccountId,
        client_order_id::ClientOrderId,
        instrument_id::InstrumentId,
        order_list_id::OrderListId,
        strategy_id::StrategyId,
        stubs::{
            account_id, client_order_id, instrument_id_btc_usdt, strategy_id_ema_cross, trader_id,
            uuid4, venue_order_id,
        },
        trade_id::TradeId,
        trader_id::TraderId,
        venue_order_id::VenueOrderId,
    },
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

#[fixture]
pub fn order_filled(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    uuid4: UUID4,
) -> OrderFilled {
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
        uuid4,
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
    uuid4: UUID4,
) -> OrderDenied {
    OrderDenied::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        Ustr::from("Exceeded MAX_ORDER_SUBMIT_RATE"),
        uuid4,
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
    uuid4: UUID4,
) -> OrderRejected {
    OrderRejected::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        account_id,
        Ustr::from("INSUFFICIENT_MARGIN"),
        uuid4,
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
    uuid4: UUID4,
) -> OrderInitialized {
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
        uuid4,
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
pub fn order_submitted(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    uuid4: UUID4,
) -> OrderSubmitted {
    OrderSubmitted::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        account_id,
        uuid4,
        0,
        0,
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
    uuid4: UUID4,
) -> OrderTriggered {
    OrderTriggered::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        uuid4,
        0,
        0,
        false,
        Some(venue_order_id),
        Some(account_id),
    )
    .unwrap()
}

#[fixture]
pub fn order_emulated(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    uuid4: UUID4,
) -> OrderEmulated {
    OrderEmulated::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        uuid4,
        0,
        0,
    )
    .unwrap()
}

#[fixture]
pub fn order_released(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    uuid4: UUID4,
) -> OrderReleased {
    OrderReleased::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        Price::from_str("22000").unwrap(),
        uuid4,
        0,
        0,
    )
    .unwrap()
}

#[fixture]
pub fn order_updated(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    uuid4: UUID4,
) -> OrderUpdated {
    OrderUpdated::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        Quantity::from(100),
        uuid4,
        0,
        0,
        false,
        Some(venue_order_id),
        Some(account_id),
        Some(Price::from("22000")),
        None,
    )
    .unwrap()
}

#[fixture]
pub fn order_pending_update(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    uuid4: UUID4,
) -> OrderPendingUpdate {
    OrderPendingUpdate::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        account_id,
        uuid4,
        0,
        0,
        false,
        Some(venue_order_id),
    )
    .unwrap()
}

#[fixture]
pub fn order_pending_cancel(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    uuid4: UUID4,
) -> OrderPendingCancel {
    OrderPendingCancel::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        account_id,
        uuid4,
        0,
        0,
        false,
        Some(venue_order_id),
    )
    .unwrap()
}

#[fixture]
pub fn order_modify_rejected(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    uuid4: UUID4,
) -> OrderModifyRejected {
    OrderModifyRejected::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        Ustr::from("ORDER_DOES_NOT_EXIST"),
        uuid4,
        0,
        0,
        false,
        Some(venue_order_id),
        Some(account_id),
    )
    .unwrap()
}

#[fixture]
pub fn order_accepted(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    uuid4: UUID4,
) -> OrderAccepted {
    OrderAccepted::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        venue_order_id,
        account_id,
        uuid4,
        0,
        0,
        false,
    )
    .unwrap()
}

#[fixture]
pub fn order_cancel_rejected(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    uuid4: UUID4,
) -> OrderCancelRejected {
    OrderCancelRejected::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        Ustr::from("ORDER_DOES_NOT_EXISTS"),
        uuid4,
        0,
        0,
        false,
        Some(venue_order_id),
        Some(account_id),
    )
    .unwrap()
}

#[fixture]
pub fn order_expired(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    uuid4: UUID4,
) -> OrderExpired {
    OrderExpired::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        uuid4,
        0,
        0,
        false,
        Some(venue_order_id),
        Some(account_id),
    )
    .unwrap()
}
