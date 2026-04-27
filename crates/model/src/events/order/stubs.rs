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

use std::str::FromStr;

use nautilus_core::{UUID4, UnixNanos};
use rstest::fixture;
use ustr::Ustr;

use crate::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TriggerType},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, order::spec::OrderFilledSpec,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TradeId, TraderId,
        VenueOrderId, stubs as id_stubs,
    },
    stubs::TestDefault,
    types::{Currency, Money, Price, Quantity},
};

// Fixture wrappers for rstest - these are not glob-exported to avoid ambiguity
#[fixture]
pub fn trader_id() -> TraderId {
    id_stubs::trader_id()
}

#[fixture]
pub fn strategy_id_ema_cross() -> StrategyId {
    id_stubs::strategy_id_ema_cross()
}

#[fixture]
pub fn instrument_id_btc_usdt() -> InstrumentId {
    id_stubs::instrument_id_btc_usdt()
}

#[fixture]
pub fn client_order_id() -> ClientOrderId {
    id_stubs::client_order_id()
}

#[fixture]
pub fn venue_order_id() -> VenueOrderId {
    id_stubs::venue_order_id()
}

#[fixture]
pub fn account_id() -> AccountId {
    id_stubs::account_id()
}

#[fixture]
pub fn uuid4() -> UUID4 {
    id_stubs::uuid4()
}

#[fixture]
pub fn order_filled(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    uuid4: UUID4,
) -> OrderFilled {
    OrderFilledSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id_ema_cross)
        .instrument_id(instrument_id_btc_usdt)
        .client_order_id(client_order_id)
        .venue_order_id(VenueOrderId::new("123456"))
        .account_id(AccountId::new("SIM-001"))
        .trade_id(TradeId::new("1"))
        .order_type(OrderType::Limit)
        .last_qty(Quantity::from_str("0.561").unwrap())
        .last_px(Price::from_str("22000").unwrap())
        .currency(Currency::from_str("USDT").unwrap())
        .event_id(uuid4)
        .commission(Money::from("12.2 USDT"))
        .build()
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
        UnixNanos::default(),
        UnixNanos::default(),
    )
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
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        false,
    )
}

#[fixture]
pub fn order_initialized_buy_limit(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
    uuid4: UUID4,
) -> OrderInitialized {
    let order_list_id = OrderListId::new("1");
    let linked_order_ids = vec![ClientOrderId::new("O-2020872378424")];
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
        UnixNanos::default(),
        UnixNanos::default(),
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
        UnixNanos::default(),
        UnixNanos::default(),
    )
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
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(venue_order_id),
        Some(account_id),
    )
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
        UnixNanos::default(),
        UnixNanos::default(),
    )
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
        UnixNanos::default(),
        UnixNanos::default(),
    )
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
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(venue_order_id),
        Some(account_id),
        Some(Price::from("22000")),
        None,
        None,
        false, // is_quote_quantity
    )
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
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(venue_order_id),
    )
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
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(venue_order_id),
    )
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
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(venue_order_id),
        Some(account_id),
    )
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
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    )
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
        Ustr::from("ORDER_DOES_NOT_EXIST"),
        uuid4,
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(venue_order_id),
        Some(account_id),
    )
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
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(venue_order_id),
        Some(account_id),
    )
}

// TestDefault implementations for order events
// These provide test-only default values for use in tests and stubs.

impl TestDefault for OrderAccepted {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            venue_order_id: VenueOrderId::test_default(),
            account_id: AccountId::test_default(),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
        }
    }
}

impl Default for OrderAccepted {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderCanceled {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
            venue_order_id: None,
            account_id: None,
        }
    }
}

impl Default for OrderCanceled {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderCancelRejected {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            reason: Ustr::from("TEST"),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
            venue_order_id: None,
            account_id: None,
        }
    }
}

impl Default for OrderCancelRejected {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderDenied {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            reason: Ustr::from("TEST"),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Default for OrderDenied {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderEmulated {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Default for OrderEmulated {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderExpired {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
            venue_order_id: None,
            account_id: None,
        }
    }
}

impl Default for OrderExpired {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderInitialized {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::new(100_000.0, 0),
            price: Option::default(),
            trigger_price: Option::default(),
            trigger_type: Option::default(),
            time_in_force: TimeInForce::Day,
            expire_time: Option::default(),
            post_only: bool::default(),
            reduce_only: bool::default(),
            display_qty: Option::default(),
            quote_quantity: bool::default(),
            limit_offset: Option::default(),
            trailing_offset: Option::default(),
            trailing_offset_type: Option::default(),
            emulation_trigger: Option::default(),
            trigger_instrument_id: Option::default(),
            contingency_type: Option::default(),
            order_list_id: Option::default(),
            linked_order_ids: Option::default(),
            parent_order_id: Option::default(),
            exec_algorithm_id: Option::default(),
            exec_algorithm_params: Option::default(),
            exec_spawn_id: Option::default(),
            tags: Option::default(),
            reconciliation: false,
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Default for OrderInitialized {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderModifyRejected {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            reason: Ustr::from("TEST"),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
            venue_order_id: None,
            account_id: None,
        }
    }
}

impl Default for OrderModifyRejected {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderPendingCancel {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
            venue_order_id: None,
            account_id: AccountId::test_default(),
        }
    }
}

impl Default for OrderPendingCancel {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderPendingUpdate {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
            venue_order_id: None,
            account_id: AccountId::test_default(),
        }
    }
}

impl Default for OrderPendingUpdate {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderRejected {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            account_id: AccountId::test_default(),
            reason: Ustr::from("TEST"),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
            due_post_only: 0,
        }
    }
}

impl Default for OrderRejected {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderReleased {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            released_price: Price::from("1.00000"),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Default for OrderReleased {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderSubmitted {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            account_id: AccountId::test_default(),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Default for OrderSubmitted {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderTriggered {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
            venue_order_id: None,
            account_id: None,
        }
    }
}

impl Default for OrderTriggered {
    fn default() -> Self {
        Self::test_default()
    }
}

impl TestDefault for OrderUpdated {
    fn test_default() -> Self {
        Self {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id: InstrumentId::test_default(),
            client_order_id: ClientOrderId::test_default(),
            venue_order_id: None,
            account_id: None,
            quantity: Quantity::new(100_000.0, 0),
            price: None,
            trigger_price: None,
            protection_price: None,
            is_quote_quantity: false,
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
        }
    }
}

impl Default for OrderUpdated {
    fn default() -> Self {
        Self::test_default()
    }
}
