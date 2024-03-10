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

use nautilus_core::time::UnixNanos;
use serde::{Deserialize, Serialize};
use strum::Display;

use crate::{
    events::order::{
        accepted::OrderAccepted, cancel_rejected::OrderCancelRejected, canceled::OrderCanceled,
        denied::OrderDenied, emulated::OrderEmulated, expired::OrderExpired, filled::OrderFilled,
        initialized::OrderInitialized, modify_rejected::OrderModifyRejected,
        pending_cancel::OrderPendingCancel, pending_update::OrderPendingUpdate,
        rejected::OrderRejected, released::OrderReleased, submitted::OrderSubmitted,
        triggered::OrderTriggered, updated::OrderUpdated,
    },
    identifiers::{client_order_id::ClientOrderId, strategy_id::StrategyId},
};

#[derive(Clone, PartialEq, Eq, Display, Debug, Serialize, Deserialize)]
pub enum OrderEvent {
    OrderInitialized(OrderInitialized),
    OrderDenied(OrderDenied),
    OrderEmulated(OrderEmulated),
    OrderReleased(OrderReleased),
    OrderSubmitted(OrderSubmitted),
    OrderAccepted(OrderAccepted),
    OrderRejected(OrderRejected),
    OrderCanceled(OrderCanceled),
    OrderExpired(OrderExpired),
    OrderTriggered(OrderTriggered),
    OrderPendingUpdate(OrderPendingUpdate),
    OrderPendingCancel(OrderPendingCancel),
    OrderModifyRejected(OrderModifyRejected),
    OrderCancelRejected(OrderCancelRejected),
    OrderUpdated(OrderUpdated),
    OrderPartiallyFilled(OrderFilled),
    OrderFilled(OrderFilled),
}

impl OrderEvent {
    #[must_use]
    pub fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::OrderInitialized(e) => e.client_order_id,
            Self::OrderDenied(e) => e.client_order_id,
            Self::OrderEmulated(e) => e.client_order_id,
            Self::OrderReleased(e) => e.client_order_id,
            Self::OrderSubmitted(e) => e.client_order_id,
            Self::OrderAccepted(e) => e.client_order_id,
            Self::OrderRejected(e) => e.client_order_id,
            Self::OrderCanceled(e) => e.client_order_id,
            Self::OrderExpired(e) => e.client_order_id,
            Self::OrderTriggered(e) => e.client_order_id,
            Self::OrderPendingUpdate(e) => e.client_order_id,
            Self::OrderPendingCancel(e) => e.client_order_id,
            Self::OrderModifyRejected(e) => e.client_order_id,
            Self::OrderCancelRejected(e) => e.client_order_id,
            Self::OrderUpdated(e) => e.client_order_id,
            Self::OrderPartiallyFilled(e) => e.client_order_id,
            Self::OrderFilled(e) => e.client_order_id,
        }
    }

    #[must_use]
    pub fn strategy_id(&self) -> StrategyId {
        match self {
            Self::OrderInitialized(e) => e.strategy_id,
            Self::OrderDenied(e) => e.strategy_id,
            Self::OrderEmulated(e) => e.strategy_id,
            Self::OrderReleased(e) => e.strategy_id,
            Self::OrderSubmitted(e) => e.strategy_id,
            Self::OrderAccepted(e) => e.strategy_id,
            Self::OrderRejected(e) => e.strategy_id,
            Self::OrderCanceled(e) => e.strategy_id,
            Self::OrderExpired(e) => e.strategy_id,
            Self::OrderTriggered(e) => e.strategy_id,
            Self::OrderPendingUpdate(e) => e.strategy_id,
            Self::OrderPendingCancel(e) => e.strategy_id,
            Self::OrderModifyRejected(e) => e.strategy_id,
            Self::OrderCancelRejected(e) => e.strategy_id,
            Self::OrderUpdated(e) => e.strategy_id,
            Self::OrderPartiallyFilled(e) => e.strategy_id,
            Self::OrderFilled(e) => e.strategy_id,
        }
    }

    #[must_use]
    pub fn ts_event(&self) -> UnixNanos {
        match self {
            Self::OrderInitialized(e) => e.ts_event,
            Self::OrderDenied(e) => e.ts_event,
            Self::OrderEmulated(e) => e.ts_event,
            Self::OrderReleased(e) => e.ts_event,
            Self::OrderSubmitted(e) => e.ts_event,
            Self::OrderAccepted(e) => e.ts_event,
            Self::OrderRejected(e) => e.ts_event,
            Self::OrderCanceled(e) => e.ts_event,
            Self::OrderExpired(e) => e.ts_event,
            Self::OrderTriggered(e) => e.ts_event,
            Self::OrderPendingUpdate(e) => e.ts_event,
            Self::OrderPendingCancel(e) => e.ts_event,
            Self::OrderModifyRejected(e) => e.ts_event,
            Self::OrderCancelRejected(e) => e.ts_event,
            Self::OrderUpdated(e) => e.ts_event,
            Self::OrderPartiallyFilled(e) => e.ts_event,
            Self::OrderFilled(e) => e.ts_event,
        }
    }
}
