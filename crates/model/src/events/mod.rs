//! Events for the trading domain model.

pub mod account;
pub mod order;
pub mod position;

// Re-exports
pub use crate::events::{
    account::state::AccountState,
    order::{
        OrderEvent, OrderEventType, accepted::OrderAccepted, any::OrderEventAny,
        cancel_rejected::OrderCancelRejected, canceled::OrderCanceled, denied::OrderDenied,
        emulated::OrderEmulated, expired::OrderExpired, filled::OrderFilled,
        initialized::OrderInitialized, modify_rejected::OrderModifyRejected,
        pending_cancel::OrderPendingCancel, pending_update::OrderPendingUpdate,
        rejected::OrderRejected, released::OrderReleased, snapshot::OrderSnapshot,
        submitted::OrderSubmitted, triggered::OrderTriggered, updated::OrderUpdated,
    },
    position::{
        PositionEvent, adjusted::PositionAdjusted, changed::PositionChanged,
        closed::PositionClosed, opened::PositionOpened, snapshot::PositionSnapshot,
    },
};
