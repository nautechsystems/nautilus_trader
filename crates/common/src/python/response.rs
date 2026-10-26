// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Historical response conversion and Python callback delivery.

use std::fmt::Debug;

use nautilus_core::{
    UUID4,
    python::{params::params_to_pydict, to_pyruntime_err, to_pytype_err, to_pyvalue_err},
};
use nautilus_model::{
    data::CustomData,
    identifiers::ClientId,
    python::{data::PyNautilusDataType, instruments::instrument_any_to_pyobject},
};
use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    sync::PyOnceLock,
    types::{PyDict, PyList},
};
use ustr::Ustr;

use crate::{
    actor::{DataActor, DataActorNative, ResponseScope, registry::try_get_actor_unchecked},
    messages::data::{DataResponse, DataResponseEnvelope},
    msgbus::TypedHandler,
    python::logging::format_exception,
};

/// Historical response metadata with a lazily converted Python payload.
#[pyclass(name = "DataResponse", module = "nautilus_trader.common", frozen)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")]
pub struct PyDataResponse {
    envelope: DataResponseEnvelope,
    data: PyOnceLock<Py<PyAny>>,
}

impl Debug for PyDataResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyDataResponse))
            .field("envelope", &self.envelope)
            .finish_non_exhaustive()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyDataResponse {
    #[getter]
    fn correlation_id(&self) -> String {
        self.envelope.correlation_id().to_string()
    }

    #[getter]
    fn completed(&self) -> bool {
        self.envelope.completed()
    }

    #[getter]
    fn sub_correlation_id(&self) -> Option<String> {
        self.envelope.sub_correlation_id().map(|id| id.to_string())
    }

    #[getter]
    fn failure_reason(&self) -> Option<&str> {
        self.envelope.failure_reason()
    }

    #[getter]
    fn kind(&self) -> Option<PyNautilusDataType> {
        self.envelope
            .response()
            .map(|r| PyNautilusDataType::new(r.data_kind()))
    }

    #[getter]
    fn client_id(&self) -> Option<ClientId> {
        self.envelope.response().map(|r| *r.client_id())
    }

    #[getter]
    fn params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.envelope
            .response()
            .and_then(DataResponse::params)
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    fn start(&self) -> Option<u64> {
        self.envelope
            .response()
            .and_then(DataResponse::start)
            .map(|v| v.as_u64())
    }

    #[getter]
    fn end(&self) -> Option<u64> {
        self.envelope
            .response()
            .and_then(DataResponse::end)
            .map(|v| v.as_u64())
    }

    #[getter]
    fn ts_init(&self) -> Option<u64> {
        self.envelope.response().map(|r| r.ts_init().as_u64())
    }

    #[getter]
    fn data(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.data
            .get_or_try_init(py, || response_data(py, self.envelope.response()))
            .map(|value| value.clone_ref(py))
    }
}

/// Builds a response handler that releases actor access before invoking Python hooks.
#[doc(hidden)]
pub fn response_handler<T>(
    actor_id: Ustr,
    instance: fn(&T) -> PyResult<Option<Py<PyAny>>>,
    callback: Option<TypedHandler<DataResponseEnvelope>>,
) -> TypedHandler<DataResponseEnvelope>
where
    T: DataActor + DataActorNative + Debug + 'static,
{
    TypedHandler::from(move |envelope: &DataResponseEnvelope| {
        let Some((mut scope, instance)) = prepare_response::<T>(actor_id, instance, envelope)
        else {
            return;
        };

        Python::attach(|py| {
            let wrapper = PyDataResponse {
                envelope: envelope.clone(),
                data: PyOnceLock::new(),
            };

            if let Some(instance) = &instance
                && let Err(e) = dispatch_payload(py, instance, &wrapper)
            {
                log::error!(
                    "Python historical data hook failed: {}",
                    format_exception(&e)
                );
            }

            let current = try_get_actor_unchecked::<T>(&actor_id)
                .is_some_and(|actor| actor.core().complete_request(envelope, &scope));

            if current {
                dispatch_completion(py, instance.as_ref(), wrapper);
                dispatch_callback::<T>(actor_id, &scope, envelope, callback.as_ref());
            }

            scope.leave();
            drain_groups::<T>(py, actor_id, instance.as_ref(), &scope);
        });
    })
}

/// Builds a Python historical response handler with an optional terminal callback.
///
/// # Errors
///
/// Returns an error if `callback` is not callable.
#[doc(hidden)]
pub fn request_handler<T>(
    py: Python<'_>,
    actor_id: Ustr,
    instance: fn(&T) -> PyResult<Option<Py<PyAny>>>,
    callback: Option<Py<PyAny>>,
) -> PyResult<TypedHandler<DataResponseEnvelope>>
where
    T: DataActor + DataActorNative + Debug + 'static,
{
    let callback = completion_handler(py, callback)?;
    Ok(response_handler(actor_id, instance, callback))
}

fn drain_groups<T>(
    py: Python<'_>,
    actor_id: Ustr,
    instance: Option<&Py<PyAny>>,
    origin: &ResponseScope,
) where
    T: DataActor + DataActorNative + Debug + 'static,
{
    loop {
        let prepared = try_get_actor_unchecked::<T>(&actor_id).and_then(|actor| {
            if !actor.core().is_response_current(origin) {
                return None;
            }

            let (envelope, callback) = actor.core().next_group_response()?;
            let scope = actor.core().begin_response(envelope.correlation_id())?;
            actor
                .core()
                .complete_request(&envelope, &scope)
                .then_some((envelope, scope, callback))
        });

        let Some((envelope, scope, callback)) = prepared else {
            return;
        };

        let wrapper = PyDataResponse {
            envelope: envelope.clone(),
            data: PyOnceLock::new(),
        };

        dispatch_completion(py, instance, wrapper);
        dispatch_callback::<T>(actor_id, &scope, &envelope, callback.as_ref());
    }
}

/// Extracts a request ID accepted by Python joins and pending inspection.
///
/// # Errors
///
/// Returns an error if the value is neither a UUID nor a valid UUID string.
#[doc(hidden)]
pub fn extract_request_id(value: &Bound<'_, PyAny>) -> PyResult<UUID4> {
    if let Ok(id) = value.extract::<UUID4>() {
        return Ok(id);
    }

    value.extract::<String>()?.parse().map_err(to_pyvalue_err)
}

/// Registers a Python completion join and drains ready notifications outside actor access.
///
/// # Errors
///
/// Returns an error if the actor is unavailable, its Python instance cannot be resolved,
/// the callback is not callable, or the group IDs fail native validation.
#[doc(hidden)]
pub fn request_join<T>(
    py: Python<'_>,
    actor_id: Ustr,
    instance: fn(&T) -> PyResult<Option<Py<PyAny>>>,
    request_ids: Vec<UUID4>,
    request_id: Option<UUID4>,
    callback: Option<Py<PyAny>>,
) -> PyResult<String>
where
    T: DataActor + DataActorNative + Debug + 'static,
{
    let callback = completion_handler(py, callback)?;

    let (id, mut scope, instance) = {
        let actor = try_get_actor_unchecked::<T>(&actor_id)
            .ok_or_else(|| to_pyruntime_err("Actor is not registered"))?;
        let instance = instance(&actor)?;
        let id = actor
            .core()
            .register_request_group(request_ids, request_id, callback)
            .map_err(to_pyvalue_err)?;
        let scope = actor
            .core()
            .begin_response(&id)
            .expect("newly registered group is pending");
        (id, scope, instance)
    };

    scope.leave();
    drain_groups::<T>(py, actor_id, instance.as_ref(), &scope);
    Ok(id.to_string())
}

fn completion_handler(
    py: Python<'_>,
    callback: Option<Py<PyAny>>,
) -> PyResult<Option<TypedHandler<DataResponseEnvelope>>> {
    let Some(callback) = callback else {
        return Ok(None);
    };

    if !callback.bind(py).is_callable() {
        return Err(to_pytype_err("callback must be callable"));
    }

    Ok(Some(TypedHandler::from(
        move |envelope: &DataResponseEnvelope| {
            Python::attach(|py| {
                if let Err(e) = callback.call1(py, (envelope.correlation_id().to_string(),)) {
                    log::error!(
                        "Python historical request callback failed: {}",
                        format_exception(&e)
                    );
                }
            });
        },
    )))
}

fn dispatch_callback<T>(
    actor_id: Ustr,
    scope: &ResponseScope,
    envelope: &DataResponseEnvelope,
    callback: Option<&TypedHandler<DataResponseEnvelope>>,
) where
    T: DataActor + DataActorNative + Debug + 'static,
{
    let Some(callback) = callback else {
        return;
    };

    if envelope.completed()
        && try_get_actor_unchecked::<T>(&actor_id)
            .is_some_and(|actor| actor.core().is_response_current(scope))
    {
        callback.0.handle(envelope);
    }
}

fn dispatch_completion(py: Python<'_>, instance: Option<&Py<PyAny>>, wrapper: PyDataResponse) {
    let Some(instance) = instance else {
        return;
    };

    let result = Py::new(py, wrapper)
        .and_then(|response| instance.call_method1(py, "on_data_response", (response,)));

    if let Err(e) = result {
        log::error!("Python on_data_response failed: {}", format_exception(&e));
    }
}

fn prepare_response<T>(
    actor_id: Ustr,
    instance: fn(&T) -> PyResult<Option<Py<PyAny>>>,
    envelope: &DataResponseEnvelope,
) -> Option<(ResponseScope, Option<Py<PyAny>>)>
where
    T: DataActor + DataActorNative + Debug + 'static,
{
    let Some(actor) = try_get_actor_unchecked::<T>(&actor_id) else {
        log::error!("Actor {actor_id} not found for response handling");
        return None;
    };

    let scope = actor.core().begin_response(envelope.correlation_id())?;
    let result = match envelope.response() {
        Some(DataResponse::Quotes(r)) => actor.core().handle_indicators_for_quotes(&r.data),
        Some(DataResponse::Trades(r)) => actor.core().handle_indicators_for_trades(&r.data),
        Some(DataResponse::Bars(r)) => actor.core().handle_indicators_for_bars(&r.data),
        _ => Ok(()),
    };

    if let Err(e) = result {
        log::error!("Historical indicator update failed: {e}");
    }

    match instance(&actor) {
        Ok(instance) => Some((scope, instance)),
        Err(e) => {
            log::error!(
                "Python response instance lookup failed: {}",
                format_exception(&e)
            );
            Some((scope, None))
        }
    }
}

fn dispatch_payload(
    py: Python<'_>,
    instance: &Py<PyAny>,
    wrapper: &PyDataResponse,
) -> PyResult<()> {
    let Some(response) = wrapper.envelope.response() else {
        return Ok(());
    };

    match response {
        DataResponse::Instruments(r) => {
            for instrument in &r.data {
                let value = instrument_any_to_pyobject(py, instrument.clone())?;
                instance.call_method1(py, "on_instrument", (value,))?;
            }

            return Ok(());
        }
        DataResponse::OptionChainReferencePrice(_) => return Ok(()),
        _ => {}
    }

    let hook = match response {
        DataResponse::Data(_) => "on_historical_data",
        DataResponse::Instrument(_) => "on_instrument",
        DataResponse::Book(_) => "on_book",
        DataResponse::BookDeltas(_) => "on_historical_book_deltas",
        DataResponse::BookDepth(_) => "on_historical_book_depth",
        DataResponse::Quotes(_) => "on_historical_quotes",
        DataResponse::Trades(_) => "on_historical_trades",
        DataResponse::FundingRates(_) => "on_historical_funding_rates",
        DataResponse::Bars(_) => "on_historical_bars",
        DataResponse::Instruments(_) | DataResponse::OptionChainReferencePrice(_) => unreachable!(),
    };

    let data = wrapper.data(py)?;
    instance.call_method1(py, hook, (data,))?;
    Ok(())
}

fn response_data(py: Python<'_>, response: Option<&DataResponse>) -> PyResult<Py<PyAny>> {
    let Some(response) = response else {
        return Ok(py.None());
    };

    match response {
        DataResponse::Data(r) => {
            if let Some(data) = r.data.downcast_ref::<CustomData>() {
                data.clone().into_py_any(py)
            } else if let Some(data) = r.data.downcast_ref::<Vec<CustomData>>() {
                data.clone().into_py_any(py)
            } else {
                Err(to_pytype_err("Unsupported historical custom data payload"))
            }
        }
        DataResponse::Instrument(r) => instrument_any_to_pyobject(py, r.data.clone()),
        DataResponse::Instruments(r) => {
            let values = r
                .data
                .iter()
                .map(|value| instrument_any_to_pyobject(py, value.clone()))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyList::new(py, values)?.into_any().unbind())
        }
        DataResponse::Book(r) => r.data.clone().into_py_any(py),
        DataResponse::BookDeltas(r) => r.data.to_vec().into_py_any(py),
        DataResponse::BookDepth(r) => r.data.to_vec().into_py_any(py),
        DataResponse::Quotes(r) => r.data.to_vec().into_py_any(py),
        DataResponse::Trades(r) => r.data.to_vec().into_py_any(py),
        DataResponse::FundingRates(r) => r.data.to_vec().into_py_any(py),
        DataResponse::Bars(r) => r.data.to_vec().into_py_any(py),
        DataResponse::OptionChainReferencePrice(r) => r.price.into_py_any(py),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::{BatchView, DataType, NautilusDataType, QuoteTick},
        identifiers::ClientId,
    };
    use rstest::rstest;

    use super::*;
    use crate::messages::data::{CustomDataResponse, QuotesResponse};

    #[rstest]
    fn test_response_metadata_is_lazy_and_terminal_can_omit_payload() {
        Python::initialize();
        Python::attach(|py| {
            let root = UUID4::new();

            let payload = CustomDataResponse::new(
                root,
                ClientId::new("HISTORY"),
                None,
                DataType::new("Opaque", None, None),
                42_u8,
                Some(UnixNanos::from(11)),
                Some(UnixNanos::from(23)),
                UnixNanos::from(37),
                None,
            );

            let wrapper = PyDataResponse {
                envelope: DataResponseEnvelope::new(DataResponse::Data(payload), false, None),
                data: PyOnceLock::new(),
            };

            assert_eq!(wrapper.correlation_id(), root.to_string());
            assert_eq!(
                wrapper.kind().unwrap().inner(),
                NautilusDataType::Custom {
                    type_name: "Opaque".to_owned()
                }
            );
            assert_eq!(wrapper.client_id(), Some(ClientId::new("HISTORY")));
            assert_eq!(
                (wrapper.start(), wrapper.end(), wrapper.ts_init()),
                (Some(11), Some(23), Some(37))
            );
            assert!(!wrapper.completed());
            assert!(wrapper.data.get(py).is_none());
            assert!(wrapper.data(py).is_err());
            assert!(wrapper.data.get(py).is_none());

            let terminal = PyDataResponse {
                envelope: DataResponseEnvelope::terminal(root, Some("source failed".to_owned())),
                data: PyOnceLock::new(),
            };

            assert!(terminal.data(py).unwrap().is_none(py));
            assert!(terminal.completed());
            assert_eq!(terminal.failure_reason(), Some("source failed"));
            assert!(terminal.kind().is_none());
            assert!(terminal.client_id().is_none());
        });
    }

    #[rstest]
    fn test_response_data_is_an_ordinary_cached_python_list() {
        Python::initialize();
        Python::attach(|py| {
            let payload = CustomDataResponse::new(
                UUID4::new(),
                ClientId::new("HISTORY"),
                None,
                DataType::new("Custom", None, None),
                Vec::<CustomData>::new(),
                None,
                None,
                UnixNanos::from(37),
                None,
            );

            let wrapper = PyDataResponse {
                envelope: DataResponseEnvelope::new(DataResponse::Data(payload), true, None),
                data: PyOnceLock::new(),
            };

            let first = wrapper.data(py).unwrap();
            let second = wrapper.data(py).unwrap();
            assert!(first.is(&second));
            assert_eq!(first.bind(py).cast::<PyList>().unwrap().len(), 0);
        });
    }

    #[rstest]
    #[case::visible(1..2)]
    #[case::empty(1..1)]
    fn test_response_data_list_contains_only_visible_rows(#[case] range: std::ops::Range<usize>) {
        Python::initialize();
        Python::attach(|py| {
            let rows = [11, 23, 37].map(|ts| QuoteTick {
                ts_init: UnixNanos::from(ts),
                ..Default::default()
            });

            let expected = rows[range.clone()].to_vec();

            let payload = QuotesResponse::new(
                UUID4::new(),
                ClientId::new("HISTORY"),
                rows[1].instrument_id,
                BatchView::new(std::sync::Arc::new(rows.to_vec()), range),
                None,
                None,
                UnixNanos::from(41),
                None,
            );

            let wrapper = PyDataResponse {
                envelope: DataResponseEnvelope::new(DataResponse::Quotes(payload), true, None),
                data: PyOnceLock::new(),
            };

            let first = wrapper.data(py).unwrap();
            let second = wrapper.data(py).unwrap();
            let list = first.bind(py).cast::<PyList>().unwrap();

            assert!(first.is(&second));
            assert_eq!(list.extract::<Vec<QuoteTick>>().unwrap(), expected);
        });
    }
}
