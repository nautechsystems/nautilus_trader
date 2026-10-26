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

//! Python bindings for latency model types.

use nautilus_common::messages::execution::TradingCommand;
use nautilus_core::{
    DurationNanos, UnixNanos,
    python::{clone_py_object, to_pynotimplemented_err, to_pytype_err},
};
use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    types::{PyBool, PyDict, PyTuple},
};

use crate::models::latency::{LatencyModel, LatencyModelAny, StaticLatencyModel};

/// Python base for command-specific latency models returning effective nanosecond delays.
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")]
#[pyclass(module = "nautilus_trader.execution", name = "LatencyModel", subclass)]
#[derive(Debug)]
pub struct PyLatencyModel;

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyLatencyModel {
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn py_new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Self
    }

    /// Returns the effective insertion delay in nanoseconds at current clock time `ts_now`.
    fn get_insert_latency(&self, _command: &Bound<'_, PyAny>, _ts_now: u64) -> PyResult<u64> {
        Err(to_pynotimplemented_err(
            "Method 'get_insert_latency' must be implemented in a subclass.",
        ))
    }

    /// Returns the effective modification delay in nanoseconds at current clock time `ts_now`.
    fn get_update_latency(&self, _command: &Bound<'_, PyAny>, _ts_now: u64) -> PyResult<u64> {
        Err(to_pynotimplemented_err(
            "Method 'get_update_latency' must be implemented in a subclass.",
        ))
    }

    /// Returns the effective cancellation delay in nanoseconds at current clock time `ts_now`.
    fn get_delete_latency(&self, _command: &Bound<'_, PyAny>, _ts_now: u64) -> PyResult<u64> {
        Err(to_pynotimplemented_err(
            "Method 'get_delete_latency' must be implemented in a subclass.",
        ))
    }
}

/// Retains a Python latency model across configuration and runtime handle clones.
#[derive(Debug)]
pub struct PythonLatencyModel {
    obj: Py<PyAny>,
}

impl Clone for PythonLatencyModel {
    fn clone(&self) -> Self {
        Self {
            obj: clone_py_object(&self.obj),
        }
    }
}

impl LatencyModel for PythonLatencyModel {
    fn get_insert_latency(
        &self,
        command: &TradingCommand,
        ts_now: UnixNanos,
    ) -> anyhow::Result<DurationNanos> {
        self.call_latency("get_insert_latency", command, ts_now)
    }

    fn get_update_latency(
        &self,
        command: &TradingCommand,
        ts_now: UnixNanos,
    ) -> anyhow::Result<DurationNanos> {
        self.call_latency("get_update_latency", command, ts_now)
    }

    fn get_delete_latency(
        &self,
        command: &TradingCommand,
        ts_now: UnixNanos,
    ) -> anyhow::Result<DurationNanos> {
        self.call_latency("get_delete_latency", command, ts_now)
    }

    fn get_base_latency(&self) -> DurationNanos {
        DurationNanos::ZERO
    }
}

impl PythonLatencyModel {
    fn call_latency(
        &self,
        method: &str,
        command: &TradingCommand,
        ts_now: UnixNanos,
    ) -> anyhow::Result<DurationNanos> {
        Python::attach(|py| -> PyResult<DurationNanos> {
            let command = match command {
                TradingCommand::SubmitOrder(cmd) => cmd.clone().into_py_any(py)?,
                TradingCommand::SubmitOrderList(cmd) => cmd.clone().into_py_any(py)?,
                TradingCommand::ModifyOrder(cmd) => cmd.clone().into_py_any(py)?,
                TradingCommand::ModifyOrders(cmd) => cmd.clone().into_py_any(py)?,
                TradingCommand::CancelOrder(cmd) => cmd.clone().into_py_any(py)?,
                TradingCommand::CancelOrders(cmd) => cmd.clone().into_py_any(py)?,
                TradingCommand::CancelAllOrders(cmd) => cmd.clone().into_py_any(py)?,
                TradingCommand::QueryOrder(cmd) => cmd.clone().into_py_any(py)?,
                TradingCommand::QueryAccount(cmd) => cmd.clone().into_py_any(py)?,
            };

            let latency = self
                .obj
                .bind(py)
                .call_method1(method, (command, ts_now.as_u64()))?;

            if latency.is_instance_of::<PyBool>() {
                return Err(to_pytype_err(
                    "Latency must be an integer nanosecond delay, not bool",
                ));
            }

            let nanos = latency.extract::<u64>()?;
            Ok(DurationNanos::new(nanos))
        })
        .map_err(|e| anyhow::anyhow!("Python LatencyModel.{method} failed: {e}"))
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl StaticLatencyModel {
    /// Static latency model with fixed latency values.
    ///
    /// Models the latency for different order operations including base network latency
    /// and specific operation latencies for insert, update, and delete operations.
    ///
    /// The base latency is automatically added to each operation latency, matching
    /// Python's behavior. For example, if `base_latency_nanos = 100ms` and
    /// `insert_latency_nanos = 200ms`, the effective insert latency will be 300ms.
    #[new]
    #[pyo3(signature = (
        base_latency_nanos = 0,
        insert_latency_nanos = 0,
        update_latency_nanos = 0,
        cancel_latency_nanos = 0,
    ))]
    fn py_new(
        base_latency_nanos: u64,
        insert_latency_nanos: u64,
        update_latency_nanos: u64,
        cancel_latency_nanos: u64,
    ) -> Self {
        Self::new(
            DurationNanos::new(base_latency_nanos),
            DurationNanos::new(insert_latency_nanos),
            DurationNanos::new(update_latency_nanos),
            DurationNanos::new(cancel_latency_nanos),
        )
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

/// Extracts a Python latency model object into a Rust [`LatencyModelAny`].
///
/// # Errors
///
/// Returns an error if `obj` is neither a static binding nor a Python `LatencyModel` subclass instance.
pub fn pyobject_to_latency_model_any(obj: &Bound<'_, PyAny>) -> PyResult<LatencyModelAny> {
    if let Ok(m) = obj.extract::<StaticLatencyModel>() {
        return Ok(LatencyModelAny::Static(m));
    }

    if obj.is_exact_instance_of::<PyLatencyModel>() {
        return Err(to_pytype_err(
            "LatencyModel is a base class; use StaticLatencyModel or a LatencyModel subclass",
        ));
    }

    if obj.is_instance_of::<PyLatencyModel>() {
        return Ok(LatencyModelAny::Python(PythonLatencyModel {
            obj: obj.clone().unbind(),
        }));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!(
        "Cannot convert {type_name} to LatencyModel"
    )))
}

/// Converts a Rust [`LatencyModelAny`] into its Python binding object.
///
/// # Errors
///
/// Returns an error if conversion to a Python object fails.
pub fn latency_model_any_to_pyobject(
    py: Python<'_>,
    model: &LatencyModelAny,
) -> PyResult<Py<PyAny>> {
    match model {
        LatencyModelAny::Static(model) => model.clone().into_py_any(py),
        LatencyModelAny::Python(model) => Ok(model.obj.clone_ref(py)),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use nautilus_common::messages::execution::CancelAllOrders;
    use nautilus_core::UUID4;
    use nautilus_model::identifiers::{InstrumentId, StrategyId, TraderId};
    use pyo3::ffi::c_str;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_python_latency_model_dispatches_context_and_preserves_instance() {
        Python::initialize();
        Python::attach(|py| {
            let locals = PyDict::new(py);
            locals
                .set_item("LatencyModel", py.get_type::<PyLatencyModel>())
                .unwrap();
            py.run(
                c_str!(
                    r#"
class ReplayLatency(LatencyModel):
    def __init__(self, offset):
        self.offset = offset
        self.calls = []
    def get_insert_latency(self, command, ts_now):
        self.calls.append((str(command.instrument_id), command.ts_init, ts_now, "insert"))
        return self.offset + 11
    def get_update_latency(self, command, ts_now):
        self.calls.append((str(command.instrument_id), command.ts_init, ts_now, "update"))
        return self.offset + 22
    def get_delete_latency(self, command, ts_now):
        self.calls.append((str(command.instrument_id), command.ts_init, ts_now, "delete"))
        return self.offset + 33
model = ReplayLatency(100)
"#
                ),
                Some(&locals),
                None,
            )
            .unwrap();
            let obj = locals.get_item("model").unwrap().unwrap();
            let model = pyobject_to_latency_model_any(&obj).unwrap();
            let cloned = model.clone();
            assert!(
                latency_model_any_to_pyobject(py, &cloned)
                    .unwrap()
                    .bind(py)
                    .is(&obj)
            );
            let command = TradingCommand::CancelAllOrders(CancelAllOrders::new(
                TraderId::from("TEST-001"),
                None,
                StrategyId::from("S-001"),
                InstrumentId::from("AUD/USD.SIM"),
                None,
                UUID4::new(),
                UnixNanos::new(17),
                None,
                None,
            ));
            let insert = model
                .get_insert_latency(&command, UnixNanos::new(41))
                .unwrap();
            let update = cloned
                .get_update_latency(&command, UnixNanos::new(52))
                .unwrap();
            let delete = model
                .get_delete_latency(&command, UnixNanos::new(63))
                .unwrap();
            let calls: Vec<(String, u64, u64, String)> =
                obj.getattr("calls").unwrap().extract().unwrap();
            assert_eq!(
                (insert, update, delete),
                (
                    DurationNanos::new(111),
                    DurationNanos::new(122),
                    DurationNanos::new(133)
                )
            );
            assert_eq!(model.get_base_latency(), DurationNanos::ZERO);
            assert_eq!(
                calls,
                vec![
                    ("AUD/USD.SIM".to_string(), 17, 41, "insert".to_string()),
                    ("AUD/USD.SIM".to_string(), 17, 52, "update".to_string()),
                    ("AUD/USD.SIM".to_string(), 17, 63, "delete".to_string()),
                ]
            );
        });
    }

    #[rstest]
    #[case("-1")]
    #[case("1.5")]
    #[case("None")]
    #[case("True")]
    #[case("2 ** 64")]
    fn test_python_latency_model_rejects_invalid_delay(#[case] value: &str) {
        Python::initialize();
        Python::attach(|py| {
            let locals = PyDict::new(py);
            locals
                .set_item("LatencyModel", py.get_type::<PyLatencyModel>())
                .unwrap();
            let source = format!(
                "type('InvalidLatency', (LatencyModel,), {{'get_delete_latency': lambda self, command, ts_now: {value}}})()"
            );
            let source = CString::new(source).unwrap();
            let obj = py.eval(&source, Some(&locals), None).unwrap();
            let model = pyobject_to_latency_model_any(&obj).unwrap();
            let command = TradingCommand::CancelAllOrders(CancelAllOrders::new(
                TraderId::from("TEST-001"),
                None,
                StrategyId::from("S-001"),
                InstrumentId::from("AUD/USD.SIM"),
                None,
                UUID4::new(),
                UnixNanos::new(17),
                None,
                None,
            ));
            let error = model
                .get_delete_latency(&command, UnixNanos::new(31))
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .starts_with("Python LatencyModel.get_delete_latency failed:")
            );
        });
    }
}
