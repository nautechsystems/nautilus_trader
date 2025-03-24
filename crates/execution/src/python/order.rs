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

use nautilus_core::{
    UUID4,
    python::{IntoPyObjectNautilusExt, serialization::from_dict_pyo3},
};
use nautilus_model::{
    enums::{
        ContingencyType, OrderSide, OrderStatus, OrderType, TimeInForce, TrailingOffsetType,
        TriggerType,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, OrderListId, PositionId, VenueOrderId},
    types::{Price, Quantity},
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use rust_decimal::Decimal;

use crate::reports::order::OrderStatusReport;

#[pymethods]
impl OrderStatusReport {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (
        account_id,
        instrument_id,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        client_order_id=None,
        report_id=None,
        order_list_id=None,
        venue_position_id=None,
        contingency_type=None,
        expire_time=None,
        price=None,
        trigger_price=None,
        trigger_type=None,
        limit_offset=None,
        trailing_offset=None,
        trailing_offset_type=None,
        avg_px=None,
        display_qty=None,
        post_only=false,
        reduce_only=false,
        cancel_reason=None,
        ts_triggered=None,
    ))]
    fn py_new(
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        time_in_force: TimeInForce,
        order_status: OrderStatus,
        quantity: Quantity,
        filled_qty: Quantity,
        ts_accepted: u64,
        ts_last: u64,
        ts_init: u64,
        client_order_id: Option<ClientOrderId>,
        report_id: Option<UUID4>,
        order_list_id: Option<OrderListId>,
        venue_position_id: Option<PositionId>,
        contingency_type: Option<ContingencyType>,
        expire_time: Option<u64>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        limit_offset: Option<Decimal>,
        trailing_offset: Option<Decimal>,
        trailing_offset_type: Option<TrailingOffsetType>,
        avg_px: Option<f64>,
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
        cancel_reason: Option<String>,
        ts_triggered: Option<u64>,
    ) -> PyResult<Self> {
        let mut report = Self::new(
            account_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            order_side,
            order_type,
            time_in_force,
            order_status,
            quantity,
            filled_qty,
            ts_accepted.into(),
            ts_last.into(),
            ts_init.into(),
            report_id,
        );

        if let Some(order_list_id) = order_list_id {
            report = report.with_order_list_id(order_list_id);
        }
        if let Some(venue_position_id) = venue_position_id {
            report = report.with_venue_position_id(venue_position_id);
        }
        if let Some(contingency_type) = contingency_type {
            report = report.with_contingency_type(contingency_type);
        }
        if let Some(expire_time) = expire_time {
            report = report.with_expire_time(expire_time.into());
        }
        if let Some(price) = price {
            report = report.with_price(price);
        }
        if let Some(trigger_price) = trigger_price {
            report = report.with_trigger_price(trigger_price);
        }
        if let Some(trigger_type) = trigger_type {
            report = report.with_trigger_type(trigger_type);
        }
        if let Some(limit_offset) = limit_offset {
            report = report.with_limit_offset(limit_offset);
        }
        if let Some(trailing_offset) = trailing_offset {
            report = report.with_trailing_offset(trailing_offset);
        }
        if let Some(trailing_offset_type) = trailing_offset_type {
            report = report.with_trailing_offset_type(trailing_offset_type);
        }
        if let Some(avg_px) = avg_px {
            report = report.with_avg_px(avg_px);
        }
        if let Some(display_qty) = display_qty {
            report = report.with_display_qty(display_qty);
        }
        if post_only {
            report = report.with_post_only(post_only);
        }
        if reduce_only {
            report = report.with_reduce_only(reduce_only);
        }
        if let Some(cancel_reason) = cancel_reason {
            report = report.with_cancel_reason(cancel_reason);
        }
        if let Some(ts_triggered) = ts_triggered {
            report = report.with_ts_triggered(ts_triggered.into());
        }

        Ok(report)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "account_id")]
    const fn py_account_id(&self) -> AccountId {
        self.account_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    const fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "venue_order_id")]
    const fn py_venue_order_id(&self) -> VenueOrderId {
        self.venue_order_id
    }

    #[getter]
    #[pyo3(name = "order_side")]
    const fn py_order_side(&self) -> OrderSide {
        self.order_side
    }

    #[getter]
    #[pyo3(name = "order_type")]
    const fn py_order_type(&self) -> OrderType {
        self.order_type
    }

    #[getter]
    #[pyo3(name = "time_in_force")]
    const fn py_time_in_force(&self) -> TimeInForce {
        self.time_in_force
    }

    #[getter]
    #[pyo3(name = "order_status")]
    const fn py_order_status(&self) -> OrderStatus {
        self.order_status
    }

    #[getter]
    #[pyo3(name = "quantity")]
    const fn py_quantity(&self) -> Quantity {
        self.quantity
    }

    #[getter]
    #[pyo3(name = "filled_qty")]
    const fn py_filled_qty(&self) -> Quantity {
        self.filled_qty
    }

    #[getter]
    #[pyo3(name = "report_id")]
    const fn py_report_id(&self) -> UUID4 {
        self.report_id
    }

    #[getter]
    #[pyo3(name = "ts_accepted")]
    const fn py_ts_accepted(&self) -> u64 {
        self.ts_accepted.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_last")]
    const fn py_ts_last(&self) -> u64 {
        self.ts_last.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    const fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "client_order_id")]
    const fn py_client_order_id(&self) -> Option<ClientOrderId> {
        self.client_order_id
    }

    #[getter]
    #[pyo3(name = "order_list_id")]
    const fn py_order_list_id(&self) -> Option<OrderListId> {
        self.order_list_id
    }

    #[getter]
    #[pyo3(name = "venue_position_id")]
    const fn py_venue_position_id(&self) -> Option<PositionId> {
        self.venue_position_id
    }

    #[getter]
    #[pyo3(name = "contingency_type")]
    const fn py_contingency_type(&self) -> ContingencyType {
        self.contingency_type
    }

    #[getter]
    #[pyo3(name = "expire_time")]
    fn py_expire_time(&self) -> Option<u64> {
        self.expire_time.map(|t| t.as_u64())
    }

    #[getter]
    #[pyo3(name = "price")]
    const fn py_price(&self) -> Option<Price> {
        self.price
    }

    #[getter]
    #[pyo3(name = "trigger_price")]
    const fn py_trigger_price(&self) -> Option<Price> {
        self.trigger_price
    }

    #[getter]
    #[pyo3(name = "trigger_type")]
    const fn py_trigger_type(&self) -> Option<TriggerType> {
        self.trigger_type
    }

    #[getter]
    #[pyo3(name = "limit_offset")]
    const fn py_limit_offset(&self) -> Option<Decimal> {
        self.limit_offset
    }

    #[getter]
    #[pyo3(name = "trailing_offset")]
    const fn py_trailing_offset(&self) -> Option<Decimal> {
        self.trailing_offset
    }

    #[getter]
    #[pyo3(name = "trailing_offset_type")]
    const fn py_trailing_offset_type(&self) -> TrailingOffsetType {
        self.trailing_offset_type
    }

    #[getter]
    #[pyo3(name = "avg_px")]
    const fn py_avg_px(&self) -> Option<f64> {
        self.avg_px
    }

    #[getter]
    #[pyo3(name = "display_qty")]
    const fn py_display_qty(&self) -> Option<Quantity> {
        self.display_qty
    }

    #[getter]
    #[pyo3(name = "post_only")]
    const fn py_post_only(&self) -> bool {
        self.post_only
    }

    #[getter]
    #[pyo3(name = "reduce_only")]
    const fn py_reduce_only(&self) -> bool {
        self.reduce_only
    }

    #[getter]
    #[pyo3(name = "cancel_reason")]
    fn py_cancel_reason(&self) -> Option<String> {
        self.cancel_reason.clone()
    }

    #[getter]
    #[pyo3(name = "ts_triggered")]
    fn py_ts_triggered(&self) -> Option<u64> {
        self.ts_triggered.map(|t| t.as_u64())
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(OrderStatusReport))?;
        dict.set_item("account_id", self.account_id.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        dict.set_item("venue_order_id", self.venue_order_id.to_string())?;
        dict.set_item("order_side", self.order_side.to_string())?;
        dict.set_item("order_type", self.order_type.to_string())?;
        dict.set_item("time_in_force", self.time_in_force.to_string())?;
        dict.set_item("order_status", self.order_status.to_string())?;
        dict.set_item("quantity", self.quantity.to_string())?;
        dict.set_item("filled_qty", self.filled_qty.to_string())?;
        dict.set_item("report_id", self.report_id.to_string())?;
        dict.set_item("ts_accepted", self.ts_accepted.as_u64())?;
        dict.set_item("ts_last", self.ts_last.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        dict.set_item("contingency_type", self.contingency_type.to_string())?;
        dict.set_item(
            "trailing_offset_type",
            self.trailing_offset_type.to_string(),
        )?;
        dict.set_item("post_only", self.post_only)?;
        dict.set_item("reduce_only", self.reduce_only)?;

        match &self.client_order_id {
            Some(id) => dict.set_item("client_order_id", id.to_string())?,
            None => dict.set_item("client_order_id", py.None())?,
        }
        match &self.order_list_id {
            Some(id) => dict.set_item("order_list_id", id.to_string())?,
            None => dict.set_item("order_list_id", py.None())?,
        }
        match &self.expire_time {
            Some(t) => dict.set_item("expire_time", t.as_u64())?,
            None => dict.set_item("expire_time", py.None())?,
        }
        match &self.price {
            Some(p) => dict.set_item("price", p.to_string())?,
            None => dict.set_item("price", py.None())?,
        }
        match &self.trigger_price {
            Some(p) => dict.set_item("trigger_price", p.to_string())?,
            None => dict.set_item("trigger_price", py.None())?,
        }
        match &self.trigger_type {
            Some(t) => dict.set_item("trigger_type", t.to_string())?,
            None => dict.set_item("trigger_type", py.None())?,
        }
        match &self.limit_offset {
            Some(o) => dict.set_item("limit_offset", o.to_string())?,
            None => dict.set_item("limit_offset", py.None())?,
        }
        match &self.trailing_offset {
            Some(o) => dict.set_item("trailing_offset", o.to_string())?,
            None => dict.set_item("trailing_offset", py.None())?,
        }
        match &self.avg_px {
            Some(p) => dict.set_item("avg_px", p)?,
            None => dict.set_item("avg_px", py.None())?,
        }
        match &self.display_qty {
            Some(q) => dict.set_item("display_qty", q.to_string())?,
            None => dict.set_item("display_qty", py.None())?,
        }
        match &self.cancel_reason {
            Some(r) => dict.set_item("cancel_reason", r)?,
            None => dict.set_item("cancel_reason", py.None())?,
        }
        match &self.ts_triggered {
            Some(t) => dict.set_item("ts_triggered", t.as_u64())?,
            None => dict.set_item("ts_triggered", py.None())?,
        }

        Ok(dict.into())
    }
}
