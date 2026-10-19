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

//! Exposes historical data and instrument requests to Python adapters as owned PyO3 objects.
//!
//! Implements read-only access to request identifiers, routing metadata, and query parameters
//! used by Python data clients. Time-bounded requests expose their start and end values as both
//! Python datetimes and nanosecond timestamps.

use std::num::NonZero;

use nautilus_common::messages::data::{
    RequestBars, RequestBookDeltas, RequestBookDepth, RequestBookSnapshot, RequestCustomData,
    RequestFundingRates, RequestInstrument, RequestInstruments, RequestOptionChainReferencePrice,
    RequestQuotes, RequestTrades,
};
use nautilus_core::{UUID4, python::params::params_to_pydict};
use nautilus_model::{
    data::{BarType, DataType},
    identifiers::{ClientId, InstrumentId, OptionSeriesId, Venue},
};
use pyo3::{IntoPyObjectExt, prelude::*, types::PyDict};

macro_rules! request {
    ($wrapper:ident, $request:ident, $name:tt, $client_id:ty, {$($field:ident: $ty:ty = $convert:expr),* $(,)?}) => {
        #[doc = concat!("An owned ", $name, " request.")]
        #[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
        #[pyclass(name = $name, module = "nautilus_trader.live", frozen)]
        #[derive(Debug)]
        pub struct $wrapper {
            pub(crate) request: $request,
        }

        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pymethods]
        impl $wrapper {
            #[getter]
            fn client_id(&self) -> $client_id { self.request.client_id }

            #[getter]
            fn request_id(&self) -> UUID4 { self.request.request_id }

            #[getter]
            fn ts_init(&self) -> u64 { self.request.ts_init.as_u64() }

            #[getter]
            fn params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
                self.request.params.as_ref().map(|params| params_to_pydict(py, params)).transpose()
            }
            $(
                #[getter]
                fn $field(&self) -> $ty { ($convert)(&self.request) }
            )*
        }
    };
}

macro_rules! request_window {
    ($wrapper:ident) => {
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pymethods]
        impl $wrapper {
            #[getter]
            #[gen_stub(override_return_type(type_repr = "datetime.datetime | None", imports = ("datetime",)))]
            fn start(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                self.request.start.into_py_any(py)
            }

            #[getter]
            #[gen_stub(override_return_type(type_repr = "datetime.datetime | None", imports = ("datetime",)))]
            fn end(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                self.request.end.into_py_any(py)
            }

            #[getter]
            fn start_ns(&self) -> Option<i128> {
                self.request.start.map(|timestamp| timestamp.as_nanosecond())
            }

            #[getter]
            fn end_ns(&self) -> Option<i128> {
                self.request.end.map(|timestamp| timestamp.as_nanosecond())
            }
        }
    };
}

request!(PyRequestCustomData, RequestCustomData, "RequestCustomData", ClientId, {
    data_type: DataType = |request: &RequestCustomData| request.data_type.clone(),
    limit: Option<usize> = |request: &RequestCustomData| request.limit.map(NonZero::get)
});
request_window!(PyRequestCustomData);

request!(PyRequestInstruments, RequestInstruments, "RequestInstruments", Option<ClientId>, {
    venue: Option<Venue> = |request: &RequestInstruments| request.venue
});
request_window!(PyRequestInstruments);

request!(PyRequestInstrument, RequestInstrument, "RequestInstrument", Option<ClientId>, {
    instrument_id: InstrumentId = |request: &RequestInstrument| request.instrument_id
});
request_window!(PyRequestInstrument);

request!(PyRequestBookSnapshot, RequestBookSnapshot, "RequestBookSnapshot", Option<ClientId>, {
    instrument_id: InstrumentId = |request: &RequestBookSnapshot| request.instrument_id,
    depth: Option<usize> = |request: &RequestBookSnapshot| request.depth.map(NonZero::get)
});

request!(PyRequestQuotes, RequestQuotes, "RequestQuotes", Option<ClientId>, {
    instrument_id: InstrumentId = |request: &RequestQuotes| request.instrument_id,
    limit: Option<usize> = |request: &RequestQuotes| request.limit.map(NonZero::get)
});
request_window!(PyRequestQuotes);

request!(PyRequestTrades, RequestTrades, "RequestTrades", Option<ClientId>, {
    instrument_id: InstrumentId = |request: &RequestTrades| request.instrument_id,
    limit: Option<usize> = |request: &RequestTrades| request.limit.map(NonZero::get)
});
request_window!(PyRequestTrades);

request!(PyRequestFundingRates, RequestFundingRates, "RequestFundingRates", Option<ClientId>, {
    instrument_id: InstrumentId = |request: &RequestFundingRates| request.instrument_id,
    limit: Option<usize> = |request: &RequestFundingRates| request.limit.map(NonZero::get)
});
request_window!(PyRequestFundingRates);

request!(PyRequestOptionChainReferencePrice, RequestOptionChainReferencePrice, "RequestOptionChainReferencePrice", Option<ClientId>, {
    instrument_id: InstrumentId = |request: &RequestOptionChainReferencePrice| request.instrument_id,
    series_id: OptionSeriesId = |request: &RequestOptionChainReferencePrice| request.series_id
});

request!(PyRequestBars, RequestBars, "RequestBars", Option<ClientId>, {
    bar_type: BarType = |request: &RequestBars| request.bar_type,
    limit: Option<usize> = |request: &RequestBars| request.limit.map(NonZero::get)
});
request_window!(PyRequestBars);

request!(PyRequestBookDepth, RequestBookDepth, "RequestBookDepth", Option<ClientId>, {
    instrument_id: InstrumentId = |request: &RequestBookDepth| request.instrument_id,
    limit: Option<usize> = |request: &RequestBookDepth| request.limit.map(NonZero::get),
    depth: Option<usize> = |request: &RequestBookDepth| request.depth.map(NonZero::get)
});
request_window!(PyRequestBookDepth);

request!(PyRequestBookDeltas, RequestBookDeltas, "RequestBookDeltas", Option<ClientId>, {
    instrument_id: InstrumentId = |request: &RequestBookDeltas| request.instrument_id,
    limit: Option<usize> = |request: &RequestBookDeltas| request.limit.map(NonZero::get)
});
request_window!(PyRequestBookDeltas);

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyRequestCustomData>()?;
    m.add_class::<PyRequestInstruments>()?;
    m.add_class::<PyRequestInstrument>()?;
    m.add_class::<PyRequestBookSnapshot>()?;
    m.add_class::<PyRequestQuotes>()?;
    m.add_class::<PyRequestTrades>()?;
    m.add_class::<PyRequestFundingRates>()?;
    m.add_class::<PyRequestOptionChainReferencePrice>()?;
    m.add_class::<PyRequestBars>()?;
    m.add_class::<PyRequestBookDepth>()?;
    m.add_class::<PyRequestBookDeltas>()?;
    Ok(())
}
