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
pub enum OrderEvent {
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

impl OrderEvent {
    #[must_use]
    pub fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::Initialized(e) => e.client_order_id,
            Self::Denied(e) => e.client_order_id,
            Self::Emulated(e) => e.client_order_id,
            Self::Released(e) => e.client_order_id,
            Self::Submitted(e) => e.client_order_id,
            Self::Accepted(e) => e.client_order_id,
            Self::Rejected(e) => e.client_order_id,
            Self::Canceled(e) => e.client_order_id,
            Self::Expired(e) => e.client_order_id,
            Self::Triggered(e) => e.client_order_id,
            Self::PendingUpdate(e) => e.client_order_id,
            Self::PendingCancel(e) => e.client_order_id,
            Self::ModifyRejected(e) => e.client_order_id,
            Self::CancelRejected(e) => e.client_order_id,
            Self::Updated(e) => e.client_order_id,
            Self::PartiallyFilled(e) => e.client_order_id,
            Self::Filled(e) => e.client_order_id,
        }
    }

    #[must_use]
    pub fn strategy_id(&self) -> StrategyId {
        match self {
            Self::Initialized(e) => e.strategy_id,
            Self::Denied(e) => e.strategy_id,
            Self::Emulated(e) => e.strategy_id,
            Self::Released(e) => e.strategy_id,
            Self::Submitted(e) => e.strategy_id,
            Self::Accepted(e) => e.strategy_id,
            Self::Rejected(e) => e.strategy_id,
            Self::Canceled(e) => e.strategy_id,
            Self::Expired(e) => e.strategy_id,
            Self::Triggered(e) => e.strategy_id,
            Self::PendingUpdate(e) => e.strategy_id,
            Self::PendingCancel(e) => e.strategy_id,
            Self::ModifyRejected(e) => e.strategy_id,
            Self::CancelRejected(e) => e.strategy_id,
            Self::Updated(e) => e.strategy_id,
            Self::PartiallyFilled(e) => e.strategy_id,
            Self::Filled(e) => e.strategy_id,
        }
    }

    #[must_use]
    pub fn ts_event(&self) -> UnixNanos {
        match self {
            Self::Initialized(e) => e.ts_event,
            Self::Denied(e) => e.ts_event,
            Self::Emulated(e) => e.ts_event,
            Self::Released(e) => e.ts_event,
            Self::Submitted(e) => e.ts_event,
            Self::Accepted(e) => e.ts_event,
            Self::Rejected(e) => e.ts_event,
            Self::Canceled(e) => e.ts_event,
            Self::Expired(e) => e.ts_event,
            Self::Triggered(e) => e.ts_event,
            Self::PendingUpdate(e) => e.ts_event,
            Self::PendingCancel(e) => e.ts_event,
            Self::ModifyRejected(e) => e.ts_event,
            Self::CancelRejected(e) => e.ts_event,
            Self::Updated(e) => e.ts_event,
            Self::PartiallyFilled(e) => e.ts_event,
            Self::Filled(e) => e.ts_event,
        }
    }
}
