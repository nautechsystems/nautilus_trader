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

//! Implements typed data responses that Python adapters construct to answer core requests.
//!
//! Provides PyO3 constructors and read-only payload access for historical data, instruments,
//! and option-chain reference prices. Converts supported response objects into Rust data
//! responses with their client identifiers for delivery through the live runner.

use std::sync::Arc;

use nautilus_common::messages::data::{
    BarsResponse, BookDeltasResponse, BookDepthResponse, BookResponse, CustomDataResponse,
    DataResponse, FundingRatesResponse, InstrumentResponse, InstrumentsResponse,
    OptionChainReferencePriceResponse, QuotesResponse, TradesResponse,
};
use nautilus_core::{
    UUID4,
    python::{
        params::{params_to_pydict, pydict_to_params},
        to_pytype_err,
    },
};
use nautilus_model::{
    data::{
        Bar, BarType, CustomData, DataType, FundingRateUpdate, OrderBookDelta, OrderBookDepth10,
        QuoteTick, TradeTick,
    },
    identifiers::{ClientId, InstrumentId, OptionSeriesId, Venue},
    orderbook::OrderBook,
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    types::Price,
};
use pyo3::{prelude::*, types::PyDict};

macro_rules! response {
    ($wrapper:ident, $response:ident, $name:tt, $key:ident: $key_type:ty, $data:ty) => {
        #[doc = concat!("An owned ", $name, " response.")]
        #[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
        #[pyclass(name = $name, module = "nautilus_trader.live", frozen, from_py_object)]
        #[derive(Debug, Clone)]
        pub struct $wrapper {
            pub(crate) response: $response,
        }

        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pymethods]
        impl $wrapper {
            #[new]
            #[pyo3(signature = (client_id, $key, data, correlation_id, ts_init, start=None, end=None, params=None))]
            #[expect(clippy::too_many_arguments, reason = "response fields cross the Python boundary together")]
            fn py_new(py: Python<'_>, client_id: ClientId, $key: $key_type, data: $data,
                correlation_id: UUID4, ts_init: u64, start: Option<u64>, end: Option<u64>,
                params: Option<Py<PyDict>>) -> PyResult<Self> {
                Ok(Self { response: $response {
                    client_id, $key, data, correlation_id, ts_init: ts_init.into(),
                    start: start.map(Into::into), end: end.map(Into::into),
                    params: params.as_ref().map(|params| pydict_to_params(py, params)).transpose()?.flatten(),
                }})
            }

            #[getter]
            fn client_id(&self) -> ClientId { self.response.client_id }

            #[getter]
            fn $key(&self) -> $key_type { self.response.$key }

            #[getter]
            fn data(&self) -> $data { self.response.data.clone() }

            #[getter]
            fn correlation_id(&self) -> UUID4 { self.response.correlation_id }

            #[getter]
            fn ts_init(&self) -> u64 { self.response.ts_init.as_u64() }

            #[getter]
            fn start(&self) -> Option<u64> { self.response.start.map(|value| value.as_u64()) }

            #[getter]
            fn end(&self) -> Option<u64> { self.response.end.map(|value| value.as_u64()) }

            #[getter]
            fn params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
                self.response.params.as_ref().map(|params| params_to_pydict(py, params)).transpose()
            }
        }
    };
}

response!(PyQuotesResponse, QuotesResponse, "QuotesResponse", instrument_id: InstrumentId, Vec<QuoteTick>);
response!(PyTradesResponse, TradesResponse, "TradesResponse", instrument_id: InstrumentId, Vec<TradeTick>);
response!(PyFundingRatesResponse, FundingRatesResponse, "FundingRatesResponse", instrument_id: InstrumentId, Vec<FundingRateUpdate>);
response!(PyBarsResponse, BarsResponse, "BarsResponse", bar_type: BarType, Vec<Bar>);
response!(PyBookDeltasResponse, BookDeltasResponse, "BookDeltasResponse", instrument_id: InstrumentId, Vec<OrderBookDelta>);
response!(PyBookDepthResponse, BookDepthResponse, "BookDepthResponse", instrument_id: InstrumentId, Vec<OrderBookDepth10>);
response!(PyBookResponse, BookResponse, "BookResponse", instrument_id: InstrumentId, OrderBook);

/// An owned `InstrumentResponse` response.
#[pyclass(
    name = "InstrumentResponse",
    module = "nautilus_trader.live",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
#[derive(Debug, Clone)]
pub struct PyInstrumentResponse {
    response: InstrumentResponse,
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyInstrumentResponse {
    #[new]
    #[pyo3(signature = (client_id, instrument_id, data, correlation_id, ts_init, start=None, end=None, params=None))]
    #[expect(
        clippy::too_many_arguments,
        clippy::needless_pass_by_value,
        reason = "response fields cross the Python boundary together"
    )]
    fn py_new(
        py: Python<'_>,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: Py<PyAny>,
        correlation_id: UUID4,
        ts_init: u64,
        start: Option<u64>,
        end: Option<u64>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<Self> {
        Ok(Self {
            response: InstrumentResponse {
                client_id,
                instrument_id,
                data: pyobject_to_instrument_any(py, data)?,
                correlation_id,
                ts_init: ts_init.into(),
                start: start.map(Into::into),
                end: end.map(Into::into),
                params: params
                    .as_ref()
                    .map(|params| pydict_to_params(py, params))
                    .transpose()?
                    .flatten(),
            },
        })
    }

    #[getter]
    fn client_id(&self) -> ClientId {
        self.response.client_id
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.response.instrument_id
    }

    #[getter]
    fn data(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        instrument_any_to_pyobject(py, self.response.data.clone())
    }

    #[getter]
    fn correlation_id(&self) -> UUID4 {
        self.response.correlation_id
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.response.ts_init.as_u64()
    }

    #[getter]
    fn start(&self) -> Option<u64> {
        self.response.start.map(|value| value.as_u64())
    }

    #[getter]
    fn end(&self) -> Option<u64> {
        self.response.end.map(|value| value.as_u64())
    }

    #[getter]
    fn params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.response
            .params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }
}

/// An owned `InstrumentsResponse` response.
#[pyclass(
    name = "InstrumentsResponse",
    module = "nautilus_trader.live",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
#[derive(Debug, Clone)]
pub struct PyInstrumentsResponse {
    response: InstrumentsResponse,
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyInstrumentsResponse {
    #[new]
    #[pyo3(signature = (client_id, venue, data, correlation_id, ts_init, start=None, end=None, params=None))]
    #[expect(
        clippy::too_many_arguments,
        clippy::needless_pass_by_value,
        reason = "response fields cross the Python boundary together"
    )]
    fn py_new(
        py: Python<'_>,
        client_id: ClientId,
        venue: Venue,
        data: Vec<Py<PyAny>>,
        correlation_id: UUID4,
        ts_init: u64,
        start: Option<u64>,
        end: Option<u64>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<Self> {
        Ok(Self {
            response: InstrumentsResponse {
                client_id,
                venue,
                data: data
                    .into_iter()
                    .map(|data| pyobject_to_instrument_any(py, data))
                    .collect::<PyResult<Vec<_>>>()?,
                correlation_id,
                ts_init: ts_init.into(),
                start: start.map(Into::into),
                end: end.map(Into::into),
                params: params
                    .as_ref()
                    .map(|params| pydict_to_params(py, params))
                    .transpose()?
                    .flatten(),
            },
        })
    }

    #[getter]
    fn client_id(&self) -> ClientId {
        self.response.client_id
    }

    #[getter]
    fn venue(&self) -> Venue {
        self.response.venue
    }

    #[getter]
    fn data(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.response
            .data
            .iter()
            .cloned()
            .map(|data| instrument_any_to_pyobject(py, data))
            .collect()
    }

    #[getter]
    fn correlation_id(&self) -> UUID4 {
        self.response.correlation_id
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.response.ts_init.as_u64()
    }

    #[getter]
    fn start(&self) -> Option<u64> {
        self.response.start.map(|value| value.as_u64())
    }

    #[getter]
    fn end(&self) -> Option<u64> {
        self.response.end.map(|value| value.as_u64())
    }

    #[getter]
    fn params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.response
            .params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }
}

/// An owned `CustomDataResponse` response.
#[pyclass(
    name = "CustomDataResponse",
    module = "nautilus_trader.live",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
#[derive(Debug, Clone)]
pub struct PyCustomDataResponse {
    response: CustomDataResponse,
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyCustomDataResponse {
    #[new]
    #[pyo3(signature = (client_id, data_type, venue, data, correlation_id, ts_init, start=None, end=None, params=None))]
    #[expect(
        clippy::too_many_arguments,
        clippy::needless_pass_by_value,
        reason = "response fields cross the Python boundary together"
    )]
    fn py_new(
        py: Python<'_>,
        client_id: ClientId,
        data_type: DataType,
        venue: Option<Venue>,
        data: Vec<CustomData>,
        correlation_id: UUID4,
        ts_init: u64,
        start: Option<u64>,
        end: Option<u64>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<Self> {
        Ok(Self {
            response: CustomDataResponse {
                client_id,
                data_type,
                venue,
                data: Arc::new(data),
                correlation_id,
                ts_init: ts_init.into(),
                start: start.map(Into::into),
                end: end.map(Into::into),
                params: params
                    .as_ref()
                    .map(|params| pydict_to_params(py, params))
                    .transpose()?
                    .flatten(),
            },
        })
    }

    #[getter]
    fn client_id(&self) -> ClientId {
        self.response.client_id
    }

    #[getter]
    fn data_type(&self) -> DataType {
        self.response.data_type.clone()
    }

    #[getter]
    fn venue(&self) -> Option<Venue> {
        self.response.venue
    }

    #[getter]
    fn data(&self, _py: Python<'_>) -> PyResult<Vec<CustomData>> {
        self.response
            .data
            .downcast_ref::<Vec<CustomData>>()
            .cloned()
            .ok_or_else(|| to_pytype_err("Expected custom data response payload"))
    }

    #[getter]
    fn correlation_id(&self) -> UUID4 {
        self.response.correlation_id
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.response.ts_init.as_u64()
    }

    #[getter]
    fn start(&self) -> Option<u64> {
        self.response.start.map(|value| value.as_u64())
    }

    #[getter]
    fn end(&self) -> Option<u64> {
        self.response.end.map(|value| value.as_u64())
    }

    #[getter]
    fn params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.response
            .params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }
}

/// An owned `OptionChainReferencePriceResponse` response.
#[pyclass(
    name = "OptionChainReferencePriceResponse",
    module = "nautilus_trader.live",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
#[derive(Debug, Clone)]
pub struct PyOptionChainReferencePriceResponse {
    response: OptionChainReferencePriceResponse,
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyOptionChainReferencePriceResponse {
    #[new]
    #[pyo3(signature = (client_id, series_id, price, correlation_id, ts_init, params=None))]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "response fields cross the Python boundary together"
    )]
    fn py_new(
        py: Python<'_>,
        client_id: ClientId,
        series_id: OptionSeriesId,
        price: Option<Price>,
        correlation_id: UUID4,
        ts_init: u64,
        params: Option<Py<PyDict>>,
    ) -> PyResult<Self> {
        Ok(Self {
            response: OptionChainReferencePriceResponse {
                client_id,
                series_id,
                price,
                correlation_id,
                ts_init: ts_init.into(),

                params: params
                    .as_ref()
                    .map(|params| pydict_to_params(py, params))
                    .transpose()?
                    .flatten(),
            },
        })
    }

    #[getter]
    fn client_id(&self) -> ClientId {
        self.response.client_id
    }

    #[getter]
    fn series_id(&self) -> OptionSeriesId {
        self.response.series_id
    }

    #[getter]
    fn price(&self) -> Option<Price> {
        self.response.price
    }

    #[getter]
    fn correlation_id(&self) -> UUID4 {
        self.response.correlation_id
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.response.ts_init.as_u64()
    }

    #[getter]
    fn params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.response
            .params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }
}

pub(crate) fn extract_response(response: &Bound<'_, PyAny>) -> PyResult<(ClientId, DataResponse)> {
    if response.is_instance_of::<PyQuotesResponse>() {
        let response = response.extract::<PyQuotesResponse>()?.response;
        return Ok((response.client_id, DataResponse::Quotes(response)));
    }

    if response.is_instance_of::<PyTradesResponse>() {
        let response = response.extract::<PyTradesResponse>()?.response;
        return Ok((response.client_id, DataResponse::Trades(response)));
    }

    if response.is_instance_of::<PyFundingRatesResponse>() {
        let response = response.extract::<PyFundingRatesResponse>()?.response;
        return Ok((response.client_id, DataResponse::FundingRates(response)));
    }

    if response.is_instance_of::<PyBarsResponse>() {
        let response = response.extract::<PyBarsResponse>()?.response;
        return Ok((response.client_id, DataResponse::Bars(response)));
    }

    if response.is_instance_of::<PyBookDeltasResponse>() {
        let response = response.extract::<PyBookDeltasResponse>()?.response;
        return Ok((response.client_id, DataResponse::BookDeltas(response)));
    }

    if response.is_instance_of::<PyBookDepthResponse>() {
        let response = response.extract::<PyBookDepthResponse>()?.response;
        return Ok((response.client_id, DataResponse::BookDepth(response)));
    }

    if response.is_instance_of::<PyBookResponse>() {
        let response = response.extract::<PyBookResponse>()?.response;
        return Ok((response.client_id, DataResponse::Book(response)));
    }

    if response.is_instance_of::<PyInstrumentResponse>() {
        let response = response.extract::<PyInstrumentResponse>()?.response;
        return Ok((
            response.client_id,
            DataResponse::Instrument(Box::new(response)),
        ));
    }

    if response.is_instance_of::<PyInstrumentsResponse>() {
        let response = response.extract::<PyInstrumentsResponse>()?.response;
        return Ok((response.client_id, DataResponse::Instruments(response)));
    }

    if response.is_instance_of::<PyCustomDataResponse>() {
        let response = response.extract::<PyCustomDataResponse>()?.response;
        return Ok((response.client_id, DataResponse::Data(response)));
    }

    if response.is_instance_of::<PyOptionChainReferencePriceResponse>() {
        let response = response
            .extract::<PyOptionChainReferencePriceResponse>()?
            .response;
        return Ok((
            response.client_id,
            DataResponse::OptionChainReferencePrice(response),
        ));
    }

    Err(to_pytype_err(
        "Expected a Nautilus data response from the installed wheel",
    ))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyInstrumentResponse>()?;
    m.add_class::<PyInstrumentsResponse>()?;
    m.add_class::<PyCustomDataResponse>()?;
    m.add_class::<PyOptionChainReferencePriceResponse>()?;

    m.add_class::<PyQuotesResponse>()?;
    m.add_class::<PyTradesResponse>()?;
    m.add_class::<PyFundingRatesResponse>()?;
    m.add_class::<PyBarsResponse>()?;
    m.add_class::<PyBookDeltasResponse>()?;
    m.add_class::<PyBookDepthResponse>()?;
    m.add_class::<PyBookResponse>()?;
    Ok(())
}
