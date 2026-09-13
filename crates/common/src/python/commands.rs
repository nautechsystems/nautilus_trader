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

//! Python properties for native subscription and execution commands.
//!
//! Exposes owned Rust messages through read-only getters, converting timestamps, parameters,
//! and order payloads into their Python representations. These classes are registered in the
//! Python live module for delivery to custom adapter handlers.

use std::num::NonZero;

use nautilus_core::{
    UUID4,
    python::{params::params_to_pydict, to_pyvalue_err},
};
use nautilus_model::{
    data::{BarType, DataType},
    enums::{BookType, OrderSide},
    events::OrderInitialized,
    identifiers::{
        AccountId, ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId, StrategyId,
        TraderId, Venue, VenueOrderId,
    },
    orders::{OrderAny, OrderList},
    python::orders::order_any_to_pyobject,
    types::{Price, Quantity},
};
use pyo3::{prelude::*, types::PyDict};

use crate::{
    enums::LogLevel,
    messages::{
        data::{
            SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10, SubscribeCustomData,
            SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
            SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments,
            SubscribeMarkPrices, SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades,
            UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeCustomData,
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrument,
            UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus, UnsubscribeInstruments,
            UnsubscribeMarkPrices, UnsubscribeOptionGreeks, UnsubscribeQuotes, UnsubscribeTrades,
        },
        execution::{
            BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder,
            GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
            GeneratePositionStatusReports, ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
            SubmitOrderList,
        },
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeCustomData {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "data_type")]
    fn py_data_type(&self) -> DataType {
        self.data_type.clone()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeInstruments {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeInstrument {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeBookDeltas {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "book_type")]
    fn py_book_type(&self) -> BookType {
        self.book_type
    }

    #[getter]
    #[pyo3(name = "depth")]
    fn py_depth(&self) -> Option<usize> {
        self.depth.map(NonZero::get)
    }

    #[getter]
    #[pyo3(name = "managed")]
    fn py_managed(&self) -> bool {
        self.managed
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeBookDepth10 {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "book_type")]
    fn py_book_type(&self) -> BookType {
        self.book_type
    }

    #[getter]
    #[pyo3(name = "depth")]
    fn py_depth(&self) -> Option<usize> {
        self.depth.map(NonZero::get)
    }

    #[getter]
    #[pyo3(name = "managed")]
    fn py_managed(&self) -> bool {
        self.managed
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeQuotes {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeTrades {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeMarkPrices {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeIndexPrices {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeFundingRates {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeBars {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "bar_type")]
    fn py_bar_type(&self) -> BarType {
        self.bar_type
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeInstrumentStatus {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeInstrumentClose {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubscribeOptionGreeks {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeCustomData {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "data_type")]
    fn py_data_type(&self) -> DataType {
        self.data_type.clone()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeInstruments {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeInstrument {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeBookDeltas {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeBookDepth10 {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeQuotes {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeTrades {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeMarkPrices {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeIndexPrices {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeFundingRates {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeBars {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "bar_type")]
    fn py_bar_type(&self) -> BarType {
        self.bar_type
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeInstrumentStatus {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeInstrumentClose {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UnsubscribeOptionGreeks {
    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubmitOrder {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    #[getter]
    #[pyo3(name = "order_init")]
    fn py_order_init(&self) -> OrderInitialized {
        self.order_init.clone()
    }

    #[getter]
    #[pyo3(name = "exec_algorithm_id")]
    fn py_exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id
    }

    #[getter]
    #[pyo3(name = "position_id")]
    fn py_position_id(&self) -> Option<PositionId> {
        self.position_id
    }

    #[getter]
    #[pyo3(name = "order")]
    fn py_order(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let order = OrderAny::try_from(self.order_init.clone()).map_err(to_pyvalue_err)?;
        order_any_to_pyobject(py, order)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubmitOrderList {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "order_list")]
    fn py_order_list(&self) -> OrderList {
        self.order_list.clone()
    }

    #[getter]
    #[pyo3(name = "order_inits")]
    fn py_order_inits(&self) -> Vec<OrderInitialized> {
        self.order_inits.clone()
    }

    #[getter]
    #[pyo3(name = "exec_algorithm_id")]
    fn py_exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id
    }

    #[getter]
    #[pyo3(name = "position_id")]
    fn py_position_id(&self) -> Option<PositionId> {
        self.position_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ModifyOrder {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    #[getter]
    #[pyo3(name = "venue_order_id")]
    fn py_venue_order_id(&self) -> Option<VenueOrderId> {
        self.venue_order_id
    }

    #[getter]
    #[pyo3(name = "quantity")]
    fn py_quantity(&self) -> Option<Quantity> {
        self.quantity
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Option<Price> {
        self.price
    }

    #[getter]
    #[pyo3(name = "trigger_price")]
    fn py_trigger_price(&self) -> Option<Price> {
        self.trigger_price
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BatchModifyOrders {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "modifies")]
    fn py_modifies(&self) -> Vec<ModifyOrder> {
        self.modifies.clone()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CancelOrder {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    #[getter]
    #[pyo3(name = "venue_order_id")]
    fn py_venue_order_id(&self) -> Option<VenueOrderId> {
        self.venue_order_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CancelAllOrders {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "order_side")]
    fn py_order_side(&self) -> Option<OrderSide> {
        self.order_side
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BatchCancelOrders {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "cancels")]
    fn py_cancels(&self) -> Vec<CancelOrder> {
        self.cancels.clone()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl QueryAccount {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "account_id")]
    fn py_account_id(&self) -> AccountId {
        self.account_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl QueryOrder {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    #[getter]
    #[pyo3(name = "venue_order_id")]
    fn py_venue_order_id(&self) -> Option<VenueOrderId> {
        self.venue_order_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl GenerateOrderStatusReport {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> Option<InstrumentId> {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self) -> Option<ClientOrderId> {
        self.client_order_id
    }

    #[getter]
    #[pyo3(name = "venue_order_id")]
    fn py_venue_order_id(&self) -> Option<VenueOrderId> {
        self.venue_order_id
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl GenerateOrderStatusReports {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "open_only")]
    fn py_open_only(&self) -> bool {
        self.open_only
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> Option<InstrumentId> {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "start")]
    fn py_start(&self) -> Option<u64> {
        self.start.map(|timestamp| timestamp.as_u64())
    }

    #[getter]
    #[pyo3(name = "end")]
    fn py_end(&self) -> Option<u64> {
        self.end.map(|timestamp| timestamp.as_u64())
    }

    #[getter]
    #[pyo3(name = "log_receipt_level")]
    fn py_log_receipt_level(&self) -> LogLevel {
        self.log_receipt_level
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl GenerateFillReports {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> Option<InstrumentId> {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "venue_order_id")]
    fn py_venue_order_id(&self) -> Option<VenueOrderId> {
        self.venue_order_id
    }

    #[getter]
    #[pyo3(name = "start")]
    fn py_start(&self) -> Option<u64> {
        self.start.map(|timestamp| timestamp.as_u64())
    }

    #[getter]
    #[pyo3(name = "end")]
    fn py_end(&self) -> Option<u64> {
        self.end.map(|timestamp| timestamp.as_u64())
    }

    #[getter]
    #[pyo3(name = "log_receipt_level")]
    fn py_log_receipt_level(&self) -> LogLevel {
        self.log_receipt_level
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl GeneratePositionStatusReports {
    #[getter]
    #[pyo3(name = "command_id")]
    fn py_command_id(&self) -> UUID4 {
        self.command_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "correlation_id")]
    fn py_correlation_id(&self) -> Option<UUID4> {
        self.correlation_id
    }

    #[getter]
    #[pyo3(name = "causation_id")]
    fn py_causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> Option<InstrumentId> {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "start")]
    fn py_start(&self) -> Option<u64> {
        self.start.map(|timestamp| timestamp.as_u64())
    }

    #[getter]
    #[pyo3(name = "end")]
    fn py_end(&self) -> Option<u64> {
        self.end.map(|timestamp| timestamp.as_u64())
    }

    #[getter]
    #[pyo3(name = "log_receipt_level")]
    fn py_log_receipt_level(&self) -> LogLevel {
        self.log_receipt_level
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::Params;
    use pyo3::{exceptions::PyAttributeError, types::PyList};
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(Some(7), Some(19))]
    #[case(None, None)]
    fn test_subscription_getters_preserve_values_and_isolate_params(
        #[case] depth: Option<usize>,
        #[case] parameter: Option<i64>,
    ) {
        Python::initialize();

        let params = parameter.map(|value| {
            let mut params = Params::default();
            params.insert("limit".to_owned(), serde_json::Value::from(value));
            params
        });

        let command = SubscribeBookDepth10::new(
            InstrumentId::from("AUD/USD.SIM"),
            BookType::L2_MBP,
            Some(ClientId::from("CLIENT")),
            Some(Venue::from("SIM")),
            UUID4::new(),
            9_007_199_254_740_993_u64.into(),
            depth.and_then(NonZero::new),
            true,
            Some(UUID4::new()),
            params,
        );
        Python::attach(|py| {
            let bound = Py::new(py, command.clone()).unwrap().into_bound(py);
            let values = bound.getattr("params").unwrap();

            if let Some(parameter) = parameter {
                assert_eq!(
                    values.get_item("limit").unwrap().extract::<i64>().unwrap(),
                    parameter
                );
                values.set_item("limit", 999).unwrap();
                assert_eq!(
                    bound
                        .getattr("params")
                        .unwrap()
                        .get_item("limit")
                        .unwrap()
                        .extract::<i64>()
                        .unwrap(),
                    parameter
                );
            } else {
                assert!(values.is_none());
            }

            assert_eq!(bound.get_type().name().unwrap(), "SubscribeBookDepth10");
            assert_eq!(
                bound
                    .get_type()
                    .getattr("__module__")
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "nautilus_trader.live"
            );
            assert_eq!(
                bound
                    .getattr("instrument_id")
                    .unwrap()
                    .extract::<InstrumentId>()
                    .unwrap(),
                command.instrument_id
            );
            assert_eq!(
                bound
                    .getattr("client_id")
                    .unwrap()
                    .extract::<ClientId>()
                    .unwrap(),
                command.client_id.unwrap()
            );
            assert_eq!(
                bound.getattr("venue").unwrap().extract::<Venue>().unwrap(),
                command.venue.unwrap()
            );
            assert_eq!(
                bound
                    .getattr("book_type")
                    .unwrap()
                    .extract::<BookType>()
                    .unwrap(),
                command.book_type
            );
            assert_eq!(
                bound
                    .getattr("command_id")
                    .unwrap()
                    .extract::<UUID4>()
                    .unwrap(),
                command.command_id
            );
            assert_eq!(
                bound
                    .getattr("correlation_id")
                    .unwrap()
                    .extract::<UUID4>()
                    .unwrap(),
                command.correlation_id.unwrap()
            );
            assert_eq!(
                bound.getattr("ts_init").unwrap().extract::<u64>().unwrap(),
                9_007_199_254_740_993
            );
            assert_eq!(
                bound
                    .getattr("depth")
                    .unwrap()
                    .extract::<Option<usize>>()
                    .unwrap(),
                depth
            );
            assert!(bound.getattr("managed").unwrap().extract::<bool>().unwrap());
            assert!(
                bound
                    .setattr("ts_init", 1)
                    .unwrap_err()
                    .is_instance_of::<PyAttributeError>(py)
            );
        });
    }

    #[rstest]
    fn test_batch_modify_getter_returns_owned_native_commands() {
        Python::initialize();

        let first = ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("CLIENT")),
            StrategyId::from("STRATEGY-001"),
            InstrumentId::from("AUD/USD.SIM"),
            ClientOrderId::from("ORDER-001"),
            Some(VenueOrderId::from("VENUE-001")),
            Some(Quantity::from("3.25")),
            Some(Price::from("0.65123")),
            Some(Price::from("0.65234")),
            UUID4::new(),
            271.into(),
            None,
            Some(UUID4::new()),
        );
        let mut second = first.clone();
        second.client_order_id = ClientOrderId::from("ORDER-002");
        second.command_id = UUID4::new();
        second.ts_init = 283.into();
        second.causation_id = Some(UUID4::new());
        let expected = vec![first, second];

        let batch = BatchModifyOrders::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("CLIENT")),
            StrategyId::from("STRATEGY-001"),
            InstrumentId::from("AUD/USD.SIM"),
            expected.clone(),
            UUID4::new(),
            293.into(),
            None,
            Some(UUID4::new()),
        );
        Python::attach(|py| {
            let bound = Py::new(py, batch).unwrap().into_bound(py);
            let children = bound.getattr("modifies").unwrap();
            let children = children.cast::<PyList>().unwrap();
            assert_eq!(children.extract::<Vec<ModifyOrder>>().unwrap(), expected);
            let child = children.get_item(1).unwrap();
            assert_eq!(child.get_type().name().unwrap(), "ModifyOrder");
            assert_eq!(
                child.getattr("ts_init").unwrap().extract::<u64>().unwrap(),
                283
            );
            assert_eq!(
                child.getattr("price").unwrap().extract::<Price>().unwrap(),
                Price::from("0.65123")
            );
            assert_eq!(
                child
                    .getattr("trigger_price")
                    .unwrap()
                    .extract::<Price>()
                    .unwrap(),
                Price::from("0.65234")
            );
            assert_eq!(
                child
                    .getattr("quantity")
                    .unwrap()
                    .extract::<Quantity>()
                    .unwrap(),
                Quantity::from("3.25")
            );
            assert_eq!(
                child
                    .getattr("causation_id")
                    .unwrap()
                    .extract::<UUID4>()
                    .unwrap(),
                expected[1].causation_id.unwrap()
            );
            assert!(
                child
                    .setattr("price", Price::from("1.00"))
                    .unwrap_err()
                    .is_instance_of::<PyAttributeError>(py)
            );
            children.del_item(0).unwrap();
            assert_eq!(
                bound
                    .getattr("modifies")
                    .unwrap()
                    .extract::<Vec<ModifyOrder>>()
                    .unwrap(),
                expected
            );
        });
    }
}
