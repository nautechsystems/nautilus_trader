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

//! Events for the trading domain model.

pub mod account;
pub mod order;
pub mod position;

use nautilus_core::UnixNanos;

use crate::data::HasTsInit;
// Re-exports
pub use crate::events::{
    account::state::AccountState,
    order::{
        OrderEvent, OrderEventType, accepted::OrderAccepted, accepted_batch::OrderAcceptedBatch,
        any::OrderEventAny, cancel_rejected::OrderCancelRejected, canceled::OrderCanceled,
        canceled_batch::OrderCanceledBatch, denied::OrderDenied, emulated::OrderEmulated,
        expired::OrderExpired, filled::OrderFilled, initialized::OrderInitialized,
        modify_rejected::OrderModifyRejected, pending_cancel::OrderPendingCancel,
        pending_update::OrderPendingUpdate, rejected::OrderRejected, released::OrderReleased,
        snapshot::OrderSnapshot, submitted::OrderSubmitted, submitted_batch::OrderSubmittedBatch,
        triggered::OrderTriggered, updated::OrderUpdated,
    },
    position::{
        PositionEvent, adjusted::PositionAdjusted, changed::PositionChanged,
        closed::PositionClosed, opened::PositionOpened, snapshot::PositionSnapshot,
    },
};

impl HasTsInit for AccountState {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderInitialized {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderDenied {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderEmulated {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderSubmitted {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderAccepted {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderRejected {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderPendingCancel {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderCanceled {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderCancelRejected {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderExpired {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderTriggered {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderPendingUpdate {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderReleased {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderModifyRejected {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderUpdated {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderFilled {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderSnapshot {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionOpened {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionChanged {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionClosed {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionAdjusted {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionSnapshot {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

crate::impl_catalog_path_prefix!(AccountState, "account_state");
crate::impl_catalog_path_prefix!(OrderInitialized, "order_initialized");
crate::impl_catalog_path_prefix!(OrderDenied, "order_denied");
crate::impl_catalog_path_prefix!(OrderEmulated, "order_emulated");
crate::impl_catalog_path_prefix!(OrderSubmitted, "order_submitted");
crate::impl_catalog_path_prefix!(OrderAccepted, "order_accepted");
crate::impl_catalog_path_prefix!(OrderRejected, "order_rejected");
crate::impl_catalog_path_prefix!(OrderPendingCancel, "order_pending_cancel");
crate::impl_catalog_path_prefix!(OrderCanceled, "order_canceled");
crate::impl_catalog_path_prefix!(OrderCancelRejected, "order_cancel_rejected");
crate::impl_catalog_path_prefix!(OrderExpired, "order_expired");
crate::impl_catalog_path_prefix!(OrderTriggered, "order_triggered");
crate::impl_catalog_path_prefix!(OrderPendingUpdate, "order_pending_update");
crate::impl_catalog_path_prefix!(OrderReleased, "order_released");
crate::impl_catalog_path_prefix!(OrderModifyRejected, "order_modify_rejected");
crate::impl_catalog_path_prefix!(OrderUpdated, "order_updated");
crate::impl_catalog_path_prefix!(OrderFilled, "order_filled");
crate::impl_catalog_path_prefix!(PositionOpened, "position_opened");
crate::impl_catalog_path_prefix!(PositionChanged, "position_changed");
crate::impl_catalog_path_prefix!(PositionClosed, "position_closed");
crate::impl_catalog_path_prefix!(PositionAdjusted, "position_adjusted");
crate::impl_catalog_path_prefix!(OrderSnapshot, "order_snapshot");
crate::impl_catalog_path_prefix!(PositionSnapshot, "position_snapshot");
