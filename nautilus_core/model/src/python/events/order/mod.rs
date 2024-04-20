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

use nautilus_core::python::to_pyvalue_err;
use pyo3::{IntoPy, PyObject, PyResult, Python};

use crate::events::order::{
    accepted::OrderAccepted, cancel_rejected::OrderCancelRejected, canceled::OrderCanceled,
    denied::OrderDenied, emulated::OrderEmulated, event::OrderEvent, expired::OrderExpired,
    filled::OrderFilled, initialized::OrderInitialized, modify_rejected::OrderModifyRejected,
    pending_cancel::OrderPendingCancel, pending_update::OrderPendingUpdate,
    rejected::OrderRejected, released::OrderReleased, submitted::OrderSubmitted,
    triggered::OrderTriggered, updated::OrderUpdated,
};

pub fn convert_order_event_to_pyobject(py: Python, order_event: OrderEvent) -> PyResult<PyObject> {
    match order_event {
        OrderEvent::OrderInitialized(event) => Ok(event.into_py(py)),
        OrderEvent::OrderDenied(event) => Ok(event.into_py(py)),
        OrderEvent::OrderEmulated(event) => Ok(event.into_py(py)),
        OrderEvent::OrderReleased(event) => Ok(event.into_py(py)),
        OrderEvent::OrderSubmitted(event) => Ok(event.into_py(py)),
        OrderEvent::OrderAccepted(event) => Ok(event.into_py(py)),
        OrderEvent::OrderRejected(event) => Ok(event.into_py(py)),
        OrderEvent::OrderCanceled(event) => Ok(event.into_py(py)),
        OrderEvent::OrderExpired(event) => Ok(event.into_py(py)),
        OrderEvent::OrderTriggered(event) => Ok(event.into_py(py)),
        OrderEvent::OrderPendingUpdate(event) => Ok(event.into_py(py)),
        OrderEvent::OrderPendingCancel(event) => Ok(event.into_py(py)),
        OrderEvent::OrderModifyRejected(event) => Ok(event.into_py(py)),
        OrderEvent::OrderCancelRejected(event) => Ok(event.into_py(py)),
        OrderEvent::OrderUpdated(event) => Ok(event.into_py(py)),
        OrderEvent::OrderPartiallyFilled(event) => Ok(event.into_py(py)),
        OrderEvent::OrderFilled(event) => Ok(event.into_py(py)),
    }
}

pub fn convert_pyobject_to_order_event(py: Python, order_event: PyObject) -> PyResult<OrderEvent> {
    let order_event_type = order_event
        .getattr(py, "order_event_type")?
        .extract::<String>(py)?;
    if order_event_type == "OrderAccepted" {
        let order_accepted = order_event.extract::<OrderAccepted>(py)?;
        Ok(OrderEvent::OrderAccepted(order_accepted))
    } else if order_event_type == "OrderCanceled" {
        let order_canceled = order_event.extract::<OrderCanceled>(py)?;
        Ok(OrderEvent::OrderCanceled(order_canceled))
    } else if order_event_type == "OrderCancelRejected" {
        let order_cancel_rejected = order_event.extract::<OrderCancelRejected>(py)?;
        Ok(OrderEvent::OrderCancelRejected(order_cancel_rejected))
    } else if order_event_type == "OrderDenied" {
        let order_denied = order_event.extract::<OrderDenied>(py)?;
        Ok(OrderEvent::OrderDenied(order_denied))
    } else if order_event_type == "OrderEmulated" {
        let order_emulated = order_event.extract::<OrderEmulated>(py)?;
        Ok(OrderEvent::OrderEmulated(order_emulated))
    } else if order_event_type == "OrderExpired" {
        let order_expired = order_event.extract::<OrderExpired>(py)?;
        Ok(OrderEvent::OrderExpired(order_expired))
    } else if order_event_type == "OrderFilled" {
        let order_filled = order_event.extract::<OrderFilled>(py)?;
        Ok(OrderEvent::OrderFilled(order_filled))
    } else if order_event_type == "OrderInitialized" {
        let order_initialized = order_event.extract::<OrderInitialized>(py)?;
        Ok(OrderEvent::OrderInitialized(order_initialized))
    } else if order_event_type == "OrderModifyRejected" {
        let order_modify_rejected = order_event.extract::<OrderModifyRejected>(py)?;
        Ok(OrderEvent::OrderModifyRejected(order_modify_rejected))
    } else if order_event_type == "OrderPendingCancel" {
        let order_pending_cancel = order_event.extract::<OrderPendingCancel>(py)?;
        Ok(OrderEvent::OrderPendingCancel(order_pending_cancel))
    } else if order_event_type == "OrderPendingUpdate" {
        let order_pending_update = order_event.extract::<OrderPendingUpdate>(py)?;
        Ok(OrderEvent::OrderPendingUpdate(order_pending_update))
    } else if order_event_type == "OrderRejected" {
        let order_rejected = order_event.extract::<OrderRejected>(py)?;
        Ok(OrderEvent::OrderRejected(order_rejected))
    } else if order_event_type == "OrderReleased" {
        let order_released = order_event.extract::<OrderReleased>(py)?;
        Ok(OrderEvent::OrderReleased(order_released))
    } else if order_event_type == "OrderSubmitted" {
        let order_submitted = order_event.extract::<OrderSubmitted>(py)?;
        Ok(OrderEvent::OrderSubmitted(order_submitted))
    } else if order_event_type == "OrderTriggered" {
        let order_triggered = order_event.extract::<OrderTriggered>(py)?;
        Ok(OrderEvent::OrderTriggered(order_triggered))
    } else if order_event_type == "OrderUpdated" {
        let order_updated = order_event.extract::<OrderUpdated>(py)?;
        Ok(OrderEvent::OrderUpdated(order_updated))
    } else {
        Err(to_pyvalue_err(
            "Error in conversion from pyobject to order event",
        ))
    }
}

pub mod accepted;
pub mod cancel_rejected;
pub mod canceled;
pub mod denied;
pub mod emulated;
pub mod expired;
pub mod filled;
pub mod initialized;
pub mod modify_rejected;
pub mod pending_cancel;
pub mod pending_update;
pub mod rejected;
pub mod released;
pub mod submitted;
pub mod triggered;
pub mod updated;
