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

//! Factory for generating order and account events.

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{AccountType, LiquiditySide},
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEventAny, OrderExpired, OrderFilled, OrderModifyRejected, OrderRejected,
        OrderSubmitted, OrderTriggered, OrderUpdated,
    },
    identifiers::{AccountId, PositionId, TradeId, TraderId, VenueOrderId},
    orders::{Order, OrderAny},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};

/// Factory for generating order and account events.
///
/// This struct holds the identity information needed to construct events and provides
/// methods to generate all order event types. It is `Clone` and `Send`, allowing it
/// to be used in async contexts.
#[derive(Debug, Clone)]
pub struct OrderEventFactory {
    trader_id: TraderId,
    account_id: AccountId,
    account_type: AccountType,
    base_currency: Option<Currency>,
}

impl OrderEventFactory {
    /// Creates a new [`OrderEventFactory`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        account_id: AccountId,
        account_type: AccountType,
        base_currency: Option<Currency>,
    ) -> Self {
        Self {
            trader_id,
            account_id,
            account_type,
            base_currency,
        }
    }

    /// Returns the trader ID.
    #[must_use]
    pub fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    /// Returns the account ID.
    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Generates an account state event.
    #[must_use]
    pub fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> AccountState {
        AccountState::new(
            self.account_id,
            self.account_type,
            balances,
            margins,
            reported,
            UUID4::new(),
            ts_event,
            ts_init,
            self.base_currency,
        )
    }

    /// Generates an order denied event.
    ///
    /// The event timestamp `ts_event` is the same as the initialized timestamp `ts_init`.
    #[must_use]
    pub fn generate_order_denied(
        &self,
        order: &OrderAny,
        reason: &str,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderDenied::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts_init,
            ts_init,
        );
        OrderEventAny::Denied(event)
    }

    /// Generates an order submitted event.
    ///
    /// The event timestamp `ts_event` is the same as the initialized timestamp `ts_init`.
    #[must_use]
    pub fn generate_order_submitted(&self, order: &OrderAny, ts_init: UnixNanos) -> OrderEventAny {
        let event = OrderSubmitted::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            self.account_id,
            UUID4::new(),
            ts_init,
            ts_init,
        );
        OrderEventAny::Submitted(event)
    }

    /// Generates an order rejected event.
    #[must_use]
    pub fn generate_order_rejected(
        &self,
        order: &OrderAny,
        reason: &str,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        due_post_only: bool,
    ) -> OrderEventAny {
        let event = OrderRejected::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            self.account_id,
            reason.into(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            due_post_only,
        );
        OrderEventAny::Rejected(event)
    }

    /// Generates an order accepted event.
    #[must_use]
    pub fn generate_order_accepted(
        &self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderAccepted::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            self.account_id,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
        );
        OrderEventAny::Accepted(event)
    }

    /// Generates an order modify rejected event.
    #[must_use]
    pub fn generate_order_modify_rejected(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderModifyRejected::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::ModifyRejected(event)
    }

    /// Generates an order cancel rejected event.
    #[must_use]
    pub fn generate_order_cancel_rejected(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderCancelRejected::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::CancelRejected(event)
    }

    /// Generates an order updated event.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn generate_order_updated(
        &self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
        protection_price: Option<Price>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderUpdated::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            quantity,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            Some(venue_order_id),
            Some(self.account_id),
            price,
            trigger_price,
            protection_price,
        );
        OrderEventAny::Updated(event)
    }

    /// Generates an order canceled event.
    #[must_use]
    pub fn generate_order_canceled(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderCanceled::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::Canceled(event)
    }

    /// Generates an order triggered event.
    #[must_use]
    pub fn generate_order_triggered(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderTriggered::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::Triggered(event)
    }

    /// Generates an order expired event.
    #[must_use]
    pub fn generate_order_expired(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderExpired::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::Expired(event)
    }

    /// Generates an order filled event.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn generate_order_filled(
        &self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        venue_position_id: Option<PositionId>,
        trade_id: TradeId,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Option<Money>,
        liquidity_side: LiquiditySide,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderFilled::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            self.account_id,
            trade_id,
            order.order_side(),
            order.order_type(),
            last_qty,
            last_px,
            quote_currency,
            liquidity_side,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_position_id,
            commission,
        );
        OrderEventAny::Filled(event)
    }
}
