// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use pyo3::{PyObject, PyResult, Python};

use crate::events::{
    OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated, OrderEventAny,
    OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
    OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered, OrderUpdated,
};

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
pub mod snapshot;
pub mod submitted;
pub mod triggered;
pub mod updated;

pub fn order_event_to_pyobject(py: Python, order_event: OrderEventAny) -> PyResult<PyObject> {
    match order_event {
        OrderEventAny::Initialized(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Denied(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Emulated(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Released(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Submitted(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Accepted(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Rejected(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Canceled(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Expired(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Triggered(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::PendingUpdate(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::PendingCancel(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::ModifyRejected(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::CancelRejected(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Updated(event) => Ok(event.into_py_any_unwrap(py)),
        OrderEventAny::Filled(event) => Ok(event.into_py_any_unwrap(py)),
    }
}

pub fn pyobject_to_order_event(py: Python, order_event: PyObject) -> PyResult<OrderEventAny> {
    let class = order_event.getattr(py, "__class__")?;
    match class.getattr(py, "__name__")?.extract::<&str>(py)? {
        stringify!(OrderAccepted) => Ok(OrderEventAny::Accepted(
            order_event.extract::<OrderAccepted>(py)?,
        )),
        stringify!(OrderCancelRejected) => Ok(OrderEventAny::CancelRejected(
            order_event.extract::<OrderCancelRejected>(py)?,
        )),
        stringify!(OrderCanceled) => Ok(OrderEventAny::Canceled(
            order_event.extract::<OrderCanceled>(py)?,
        )),
        stringify!(OrderDenied) => Ok(OrderEventAny::Denied(
            order_event.extract::<OrderDenied>(py)?,
        )),
        stringify!(OrderEmulated) => Ok(OrderEventAny::Emulated(
            order_event.extract::<OrderEmulated>(py)?,
        )),
        stringify!(OrderExpired) => Ok(OrderEventAny::Expired(
            order_event.extract::<OrderExpired>(py)?,
        )),
        stringify!(OrderFilled) => Ok(OrderEventAny::Filled(
            order_event.extract::<OrderFilled>(py)?,
        )),
        stringify!(OrderInitialized) => Ok(OrderEventAny::Initialized(
            order_event.extract::<OrderInitialized>(py)?,
        )),
        stringify!(OrderModifyRejected) => Ok(OrderEventAny::ModifyRejected(
            order_event.extract::<OrderModifyRejected>(py)?,
        )),
        stringify!(OrderPendingCancel) => Ok(OrderEventAny::PendingCancel(
            order_event.extract::<OrderPendingCancel>(py)?,
        )),
        stringify!(OrderPendingUpdate) => Ok(OrderEventAny::PendingUpdate(
            order_event.extract::<OrderPendingUpdate>(py)?,
        )),
        stringify!(OrderRejected) => Ok(OrderEventAny::Rejected(
            order_event.extract::<OrderRejected>(py)?,
        )),
        stringify!(OrderReleased) => Ok(OrderEventAny::Released(
            order_event.extract::<OrderReleased>(py)?,
        )),
        stringify!(OrderSubmitted) => Ok(OrderEventAny::Submitted(
            order_event.extract::<OrderSubmitted>(py)?,
        )),
        stringify!(OrderTriggered) => Ok(OrderEventAny::Triggered(
            order_event.extract::<OrderTriggered>(py)?,
        )),
        stringify!(OrderUpdated) => Ok(OrderEventAny::Updated(
            order_event.extract::<OrderUpdated>(py)?,
        )),
        _ => Err(to_pyvalue_err(
            "Error in conversion from `PyObject` to `OrderEventAny`",
        )),
    }
}
