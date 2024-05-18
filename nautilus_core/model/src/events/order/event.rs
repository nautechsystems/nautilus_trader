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

use nautilus_core::nanos::UnixNanos;
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
pub enum OrderEventAny {
    Initialized(OrderInitialized),
    Denied(OrderDenied),
    Emulated(OrderEmulated),
    Released(OrderReleased),
    Submitted(OrderSubmitted),
    Accepted(OrderAccepted),
    Rejected(OrderRejected),
    Canceled(OrderCanceled),
    Expired(OrderExpired),
    Triggered(OrderTriggered),
    PendingUpdate(OrderPendingUpdate),
    PendingCancel(OrderPendingCancel),
    ModifyRejected(OrderModifyRejected),
    CancelRejected(OrderCancelRejected),
    Updated(OrderUpdated),
    PartiallyFilled(OrderFilled),
    Filled(OrderFilled),
}

impl OrderEventAny {
    #[must_use]
    pub fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::Initialized(event) => event.client_order_id,
            Self::Denied(event) => event.client_order_id,
            Self::Emulated(event) => event.client_order_id,
            Self::Released(event) => event.client_order_id,
            Self::Submitted(event) => event.client_order_id,
            Self::Accepted(event) => event.client_order_id,
            Self::Rejected(event) => event.client_order_id,
            Self::Canceled(event) => event.client_order_id,
            Self::Expired(event) => event.client_order_id,
            Self::Triggered(event) => event.client_order_id,
            Self::PendingUpdate(event) => event.client_order_id,
            Self::PendingCancel(event) => event.client_order_id,
            Self::ModifyRejected(event) => event.client_order_id,
            Self::CancelRejected(event) => event.client_order_id,
            Self::Updated(event) => event.client_order_id,
            Self::PartiallyFilled(event) => event.client_order_id,
            Self::Filled(event) => event.client_order_id,
        }
    }

    #[must_use]
    pub fn strategy_id(&self) -> StrategyId {
        match self {
            Self::Initialized(event) => event.strategy_id,
            Self::Denied(event) => event.strategy_id,
            Self::Emulated(event) => event.strategy_id,
            Self::Released(event) => event.strategy_id,
            Self::Submitted(event) => event.strategy_id,
            Self::Accepted(event) => event.strategy_id,
            Self::Rejected(event) => event.strategy_id,
            Self::Canceled(event) => event.strategy_id,
            Self::Expired(event) => event.strategy_id,
            Self::Triggered(event) => event.strategy_id,
            Self::PendingUpdate(event) => event.strategy_id,
            Self::PendingCancel(event) => event.strategy_id,
            Self::ModifyRejected(event) => event.strategy_id,
            Self::CancelRejected(event) => event.strategy_id,
            Self::Updated(event) => event.strategy_id,
            Self::PartiallyFilled(event) => event.strategy_id,
            Self::Filled(event) => event.strategy_id,
        }
    }

    #[must_use]
    pub fn ts_event(&self) -> UnixNanos {
        match self {
            Self::Initialized(event) => event.ts_event,
            Self::Denied(event) => event.ts_event,
            Self::Emulated(event) => event.ts_event,
            Self::Released(event) => event.ts_event,
            Self::Submitted(event) => event.ts_event,
            Self::Accepted(event) => event.ts_event,
            Self::Rejected(event) => event.ts_event,
            Self::Canceled(event) => event.ts_event,
            Self::Expired(event) => event.ts_event,
            Self::Triggered(event) => event.ts_event,
            Self::PendingUpdate(event) => event.ts_event,
            Self::PendingCancel(event) => event.ts_event,
            Self::ModifyRejected(event) => event.ts_event,
            Self::CancelRejected(event) => event.ts_event,
            Self::Updated(event) => event.ts_event,
            Self::PartiallyFilled(event) => event.ts_event,
            Self::Filled(event) => event.ts_event,
        }
    }
}
