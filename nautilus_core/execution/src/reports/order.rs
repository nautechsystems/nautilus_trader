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

use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    enums::{
        ContingencyType, OrderSide, OrderStatus, OrderType, TimeInForce, TrailingOffsetType,
        TriggerType,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, OrderListId, VenueOrderId},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.execution")
)]
pub struct OrderStatusReport {
    pub account_id: AccountId,
    pub instrument_id: InstrumentId,
    pub venue_order_id: VenueOrderId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub order_status: OrderStatus,
    pub quantity: Quantity,
    pub filled_qty: Quantity,
    pub report_id: UUID4,
    pub ts_accepted: UnixNanos,
    pub ts_last: UnixNanos,
    pub ts_init: UnixNanos,
    pub client_order_id: Option<ClientOrderId>,
    pub order_list_id: Option<OrderListId>,
    pub contingency_type: ContingencyType,
    pub expire_time: Option<UnixNanos>,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub trigger_type: Option<TriggerType>,
    pub limit_offset: Option<Decimal>,
    pub trailing_offset: Option<Decimal>,
    pub trailing_offset_type: TrailingOffsetType,
    pub avg_px: Option<Decimal>,
    pub display_qty: Option<Quantity>,
    pub post_only: bool,
    pub reduce_only: bool,
    pub cancel_reason: Option<String>,
    pub ts_triggered: Option<UnixNanos>,
}

impl OrderStatusReport {
    /// Creates a new [`OrderStatusReport`] instance with required fields.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        time_in_force: TimeInForce,
        order_status: OrderStatus,
        quantity: Quantity,
        filled_qty: Quantity,
        report_id: UUID4,
        ts_accepted: UnixNanos,
        ts_last: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            account_id,
            instrument_id,
            venue_order_id,
            order_side,
            order_type,
            time_in_force,
            order_status,
            quantity,
            filled_qty,
            report_id,
            ts_accepted,
            ts_last,
            ts_init,
            client_order_id: None,
            order_list_id: None,
            contingency_type: ContingencyType::default(),
            expire_time: None,
            price: None,
            trigger_price: None,
            trigger_type: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: TrailingOffsetType::default(),
            avg_px: None,
            display_qty: None,
            post_only: false,
            reduce_only: false,
            cancel_reason: None,
            ts_triggered: None,
        }
    }

    /// Sets the client order ID.
    #[must_use]
    pub const fn with_client_order_id(mut self, client_order_id: ClientOrderId) -> Self {
        self.client_order_id = Some(client_order_id);
        self
    }

    /// Sets the order list ID.
    #[must_use]
    pub const fn with_order_list_id(mut self, order_list_id: OrderListId) -> Self {
        self.order_list_id = Some(order_list_id);
        self
    }

    /// Sets the price.
    #[must_use]
    pub const fn with_price(mut self, price: Price) -> Self {
        self.price = Some(price);
        self
    }

    /// Sets the average price.
    #[must_use]
    pub const fn with_avg_px(mut self, avg_px: Decimal) -> Self {
        self.avg_px = Some(avg_px);
        self
    }

    /// Sets the trigger price.
    #[must_use]
    pub const fn with_trigger_price(mut self, trigger_price: Price) -> Self {
        self.trigger_price = Some(trigger_price);
        self
    }

    /// Sets the display quantity.
    #[must_use]
    pub const fn with_display_qty(mut self, display_qty: Quantity) -> Self {
        self.display_qty = Some(display_qty);
        self
    }

    /// Sets the expire time.
    #[must_use]
    pub const fn with_expire_time(mut self, expire_time: UnixNanos) -> Self {
        self.expire_time = Some(expire_time);
        self
    }

    /// Sets `post_only` flag.
    #[must_use]
    pub const fn with_post_only(mut self, post_only: bool) -> Self {
        self.post_only = post_only;
        self
    }

    /// Sets `reduce_only` flag.
    #[must_use]
    pub const fn with_reduce_only(mut self, reduce_only: bool) -> Self {
        self.reduce_only = reduce_only;
        self
    }

    /// Sets cancel reason.
    #[must_use]
    pub fn with_cancel_reason(mut self, cancel_reason: &str) -> Self {
        self.cancel_reason = Some(cancel_reason.to_string());
        self
    }

    /// Sets the triggered timestamp.
    #[must_use]
    pub const fn with_ts_triggered(mut self, ts_triggered: UnixNanos) -> Self {
        self.ts_triggered = Some(ts_triggered);
        self
    }

    /// Sets the contingency type.
    #[must_use]
    pub const fn with_contingency_type(mut self, contingency_type: ContingencyType) -> Self {
        self.contingency_type = contingency_type;
        self
    }
}
