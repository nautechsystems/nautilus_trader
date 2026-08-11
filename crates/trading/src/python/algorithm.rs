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

//! Python bindings for execution algorithms.

use std::{cell::UnsafeCell, collections::HashMap, fmt::Debug, rc::Rc};

use jiff::Timestamp;
use nautilus_common::{
    actor::{DataActor, DataActorNative, data_actor::DataActorCore},
    component::Component,
    enums::ComponentState,
    messages::system::{QueueStateChanged, SocketStateChanged},
    python::{cache::PyCache, clock::PyClock, logging::PyLogger},
    signal::Signal,
    timer::TimeEvent,
};
use nautilus_core::{
    UnixNanos,
    python::{to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{
    data::{CustomData, DataType},
    enums::{TimeInForce, TriggerType},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderFillVoided, OrderFilled, OrderInitialized,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased,
        OrderSubmitted, OrderTriggered, OrderUpdated, PositionChanged, PositionClosed,
        PositionEvent, PositionOpened,
    },
    identifiers::{ActorId, ClientId, ExecAlgorithmId, PositionId, TraderId},
    orders::{LimitOrder, MarketOrder, MarketToLimitOrder, Order, OrderAny, OrderList},
    python::{events::order::order_event_to_pyobject, orders::pyobject_to_order_any},
    types::{Price, Quantity},
};
use nautilus_portfolio::python::PyPortfolio;
use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    types::{PyDict, PyList},
};
use ustr::Ustr;

use crate::algorithm::{
    ExecutionAlgorithm, ExecutionAlgorithmConfig, ExecutionAlgorithmCore, ExecutionAlgorithmNative,
    ImportableExecAlgorithmConfig,
};

/// Inner state of `PyExecutionAlgorithm`, shared between Python and Rust registries.
pub struct PyExecutionAlgorithmInner {
    core: ExecutionAlgorithmCore,
    py_self: Option<Py<PyAny>>,
    config: Option<Py<PyAny>>,
    logger: PyLogger,
}

impl Debug for PyExecutionAlgorithmInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyExecutionAlgorithmInner))
            .field("core", &self.core)
            .field("py_self", &self.py_self.as_ref().map(|_| "<Py<PyAny>>"))
            .field("config", &self.config.as_ref().map(|_| "<Py<PyAny>>"))
            .field("logger", &self.logger)
            .finish()
    }
}

/// Python-facing wrapper for execution algorithms.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.trading",
    name = "ExecutionAlgorithm",
    unsendable,
    subclass,
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.trading")]
#[derive(Clone)]
pub struct PyExecutionAlgorithm {
    inner: Rc<UnsafeCell<PyExecutionAlgorithmInner>>,
}

impl Debug for PyExecutionAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyExecutionAlgorithm))
            .field("inner", &self.inner())
            .finish()
    }
}

impl PyExecutionAlgorithm {
    #[inline]
    #[allow(unsafe_code)]
    pub(crate) fn inner(&self) -> &PyExecutionAlgorithmInner {
        // SAFETY: `PyExecutionAlgorithm` is `unsendable` so access is single-threaded, and
        // callers never hold a mutable and shared reference simultaneously.
        unsafe { &*self.inner.get() }
    }

    #[inline]
    #[allow(unsafe_code, clippy::mut_from_ref)]
    pub(crate) fn inner_mut(&self) -> &mut PyExecutionAlgorithmInner {
        // SAFETY: `PyExecutionAlgorithm` is `unsendable` so access is single-threaded, and
        // callers never hold a mutable and shared reference simultaneously.
        unsafe { &mut *self.inner.get() }
    }
}

impl PyExecutionAlgorithm {
    /// Creates a new `PyExecutionAlgorithm` instance.
    #[must_use]
    pub fn new(config: Option<ExecutionAlgorithmConfig>) -> Self {
        let mut config = config.unwrap_or_default();
        if config.exec_algorithm_id.is_none() {
            config.exec_algorithm_id = Some(ExecAlgorithmId::new(stringify!(ExecutionAlgorithm)));
        }

        let core = ExecutionAlgorithmCore::new(config);
        let logger = PyLogger::new(core.actor.actor_id.as_str());

        let inner = PyExecutionAlgorithmInner {
            core,
            py_self: None,
            config: None,
            logger,
        };

        Self {
            inner: Rc::new(UnsafeCell::new(inner)),
        }
    }

    /// Sets the Python instance reference for method dispatch.
    pub fn set_python_instance(&mut self, py_obj: Py<PyAny>) {
        self.inner_mut().py_self = Some(py_obj);
    }

    /// Stores the original Python config object passed at construction.
    pub fn set_config(&mut self, config: Option<Py<PyAny>>) {
        self.inner_mut().config = config;
    }

    /// Updates the runtime execution algorithm ID before registration.
    pub fn set_exec_algorithm_id(&mut self, exec_algorithm_id: ExecAlgorithmId) {
        let actor_id = ActorId::from(exec_algorithm_id.inner().as_str());
        let inner = self.inner_mut();

        inner.core.config.exec_algorithm_id = Some(exec_algorithm_id);
        inner.core.exec_algorithm_id = exec_algorithm_id;
        inner.core.actor.actor_id = actor_id;
        inner.core.actor.config.actor_id = Some(actor_id);
        inner.logger = PyLogger::new(inner.core.actor.actor_id.as_str());
    }

    /// Updates the runtime `log_events` setting.
    pub fn set_log_events(&mut self, log_events: bool) {
        let inner = self.inner_mut();
        inner.core.config.log_events = log_events;
        inner.core.actor.config.log_events = log_events;
    }

    /// Updates the runtime `log_commands` setting.
    pub fn set_log_commands(&mut self, log_commands: bool) {
        let inner = self.inner_mut();
        inner.core.config.log_commands = log_commands;
        inner.core.actor.config.log_commands = log_commands;
    }

    /// Returns the execution algorithm ID.
    #[must_use]
    pub fn exec_algorithm_id(&self) -> ExecAlgorithmId {
        self.inner().core.exec_algorithm_id
    }

    fn dispatch_no_args(&self, method_name: &str) -> PyResult<()> {
        if let Some(ref py_self) = self.inner().py_self {
            Python::attach(|py| py_self.call_method0(py, method_name))?;
        }
        Ok(())
    }

    fn dispatch_time_event(&self, event: &TimeEvent) -> PyResult<()> {
        if let Some(ref py_self) = self.inner().py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_time_event", (event.clone().into_py_any(py)?,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_signal(&self, signal: &Signal) -> PyResult<()> {
        if let Some(ref py_self) = self.inner().py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_signal", (signal.clone().into_py_any(py)?,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_queue_state(&self, event: &QueueStateChanged) -> PyResult<()> {
        if let Some(ref py_self) = self.inner().py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_queue_state", (event.clone().into_py_any(py)?,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_socket_state(&self, event: &SocketStateChanged) -> PyResult<()> {
        if let Some(ref py_self) = self.inner().py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_socket_state", (event.clone().into_py_any(py)?,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order(&self, order: OrderAny) -> PyResult<()> {
        if let Some(ref py_self) = self.inner().py_self {
            Python::attach(|py| {
                let py_order = nautilus_model::python::orders::order_any_to_pyobject(py, order)?;
                py_self.call_method1(py, "on_order", (py_order,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_list(&self, order_list: OrderList, orders: Vec<OrderAny>) -> PyResult<()> {
        if let Some(ref py_self) = self.inner().py_self {
            Python::attach(|py| -> PyResult<()> {
                let py_order_list = order_list.into_py_any(py)?;
                let py_orders: Vec<_> = orders
                    .into_iter()
                    .map(|order| nautilus_model::python::orders::order_any_to_pyobject(py, order))
                    .collect::<PyResult<Vec<_>>>()?;
                let py_orders = PyList::new(py, py_orders)?;
                py_self.call_method1(py, "on_order_list", (py_order_list, py_orders))?;
                Ok(())
            })?;
        }
        Ok(())
    }

    fn has_python_override(&self, method_name: &str) -> PyResult<bool> {
        Python::attach(|py| -> PyResult<bool> {
            let Some(ref py_self) = self.inner().py_self else {
                return Ok(false);
            };

            let instance_type = py_self.bind(py).get_type();
            let instance_method = instance_type.getattr(method_name)?;
            let base_method = py.get_type::<Self>().getattr(method_name)?;

            Ok(!instance_method.is(&base_method))
        })
    }

    fn dispatch_order_event(&self, method_name: &str, event: OrderEventAny) -> PyResult<()> {
        if let Some(ref py_self) = self.inner().py_self {
            Python::attach(|py| {
                let py_event = order_event_to_pyobject(py, event)?;
                py_self.call_method1(py, method_name, (py_event,))
            })?;
        }
        Ok(())
    }

    fn dispatch_position_event(&self, method_name: &str, event: PositionEvent) -> PyResult<()> {
        if let Some(ref py_self) = self.inner().py_self {
            Python::attach(|py| {
                let py_event = match event {
                    PositionEvent::PositionOpened(event) => event.into_py_any(py)?,
                    PositionEvent::PositionChanged(event) => event.into_py_any(py)?,
                    PositionEvent::PositionClosed(event) => event.into_py_any(py)?,
                    PositionEvent::PositionAdjusted(event) => event.into_py_any(py)?,
                };
                py_self.call_method1(py, method_name, (py_event,))
            })?;
        }
        Ok(())
    }

    fn tags_to_ustr(tags: Option<Vec<String>>) -> Option<Vec<Ustr>> {
        tags.map(|tags| tags.into_iter().map(|s| Ustr::from(&s)).collect())
    }

    fn primary_order_for_spawn(
        &self,
        py: Python<'_>,
        primary: Py<PyAny>,
        quantity: Quantity,
        reduce_primary: bool,
    ) -> PyResult<OrderAny> {
        if !self.inner().core.actor.is_registered() {
            return Err(to_pyruntime_err(
                "ExecutionAlgorithm must be registered before spawning orders",
            ));
        }

        let primary = pyobject_to_order_any(py, primary)?;
        let cached_primary = {
            let cache = self.inner().core.actor.cache_ref();
            cache
                .order(&primary.client_order_id())
                .map(|order| order.clone())
        };

        let primary = if reduce_primary {
            cached_primary.ok_or_else(|| {
                to_pyruntime_err(format!(
                    "Cannot reduce primary order {}: order not found in cache",
                    primary.client_order_id()
                ))
            })?
        } else {
            cached_primary.unwrap_or(primary)
        };

        if reduce_primary && quantity > primary.leaves_qty() {
            return Err(to_pyvalue_err(format!(
                "Spawn quantity {quantity} exceeds primary leaves_qty {}",
                primary.leaves_qty()
            )));
        }

        if reduce_primary && primary.is_closed() {
            return Err(to_pyvalue_err(format!(
                "Cannot reduce closed primary order {}",
                primary.client_order_id()
            )));
        }

        Ok(primary)
    }
}

impl DataActorNative for PyExecutionAlgorithm {
    fn core(&self) -> &DataActorCore {
        DataActorNative::core(&self.inner().core)
    }

    fn core_mut(&mut self) -> &mut DataActorCore {
        DataActorNative::core_mut(&mut self.inner_mut().core)
    }
}

impl ExecutionAlgorithmNative for PyExecutionAlgorithm {
    fn exec_algorithm_core(&self) -> &ExecutionAlgorithmCore {
        &self.inner().core
    }

    fn exec_algorithm_core_mut(&mut self) -> &mut ExecutionAlgorithmCore {
        &mut self.inner_mut().core
    }
}

impl ExecutionAlgorithm for PyExecutionAlgorithm {
    fn on_order(&mut self, order: OrderAny) -> anyhow::Result<()> {
        self.dispatch_on_order(order)
            .map_err(|e| anyhow::anyhow!("Python on_order failed: {e}"))
    }

    fn on_order_list(
        &mut self,
        order_list: OrderList,
        orders: Vec<OrderAny>,
    ) -> anyhow::Result<()> {
        if self
            .has_python_override("on_order_list")
            .map_err(|e| anyhow::anyhow!("Python override lookup failed: {e}"))?
        {
            return self
                .dispatch_on_order_list(order_list, orders)
                .map_err(|e| anyhow::anyhow!("Python on_order_list failed: {e}"));
        }

        for order in orders {
            self.on_order(order)?;
        }
        Ok(())
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting {}", self.exec_algorithm_id());
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.unsubscribe_all_strategy_events();
        self.inner_mut().core.reset();
        Ok(())
    }

    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        Ok(())
    }

    fn on_order_initialized(&mut self, event: OrderInitialized) {
        let _ =
            self.dispatch_order_event("on_order_initialized", OrderEventAny::Initialized(event));
    }

    fn on_order_denied(&mut self, event: OrderDenied) {
        let _ = self.dispatch_order_event("on_order_denied", OrderEventAny::Denied(event));
    }

    fn on_order_emulated(&mut self, event: OrderEmulated) {
        let _ = self.dispatch_order_event("on_order_emulated", OrderEventAny::Emulated(event));
    }

    fn on_order_released(&mut self, event: OrderReleased) {
        let _ = self.dispatch_order_event("on_order_released", OrderEventAny::Released(event));
    }

    fn on_order_submitted(&mut self, event: OrderSubmitted) {
        let _ = self.dispatch_order_event("on_order_submitted", OrderEventAny::Submitted(event));
    }

    fn on_order_rejected(&mut self, event: OrderRejected) {
        let _ = self.dispatch_order_event("on_order_rejected", OrderEventAny::Rejected(event));
    }

    fn on_order_accepted(&mut self, event: OrderAccepted) {
        let _ = self.dispatch_order_event("on_order_accepted", OrderEventAny::Accepted(event));
    }

    fn on_algo_order_canceled(&mut self, event: OrderCanceled) {
        let _ = self.dispatch_order_event("on_order_canceled", OrderEventAny::Canceled(event));
    }

    fn on_order_expired(&mut self, event: OrderExpired) {
        let _ = self.dispatch_order_event("on_order_expired", OrderEventAny::Expired(event));
    }

    fn on_order_triggered(&mut self, event: OrderTriggered) {
        let _ = self.dispatch_order_event("on_order_triggered", OrderEventAny::Triggered(event));
    }

    fn on_order_pending_update(&mut self, event: OrderPendingUpdate) {
        let _ = self.dispatch_order_event(
            "on_order_pending_update",
            OrderEventAny::PendingUpdate(event),
        );
    }

    fn on_order_pending_cancel(&mut self, event: OrderPendingCancel) {
        let _ = self.dispatch_order_event(
            "on_order_pending_cancel",
            OrderEventAny::PendingCancel(event),
        );
    }

    fn on_order_modify_rejected(&mut self, event: OrderModifyRejected) {
        let _ = self.dispatch_order_event(
            "on_order_modify_rejected",
            OrderEventAny::ModifyRejected(event),
        );
    }

    fn on_order_cancel_rejected(&mut self, event: OrderCancelRejected) {
        let _ = self.dispatch_order_event(
            "on_order_cancel_rejected",
            OrderEventAny::CancelRejected(event),
        );
    }

    fn on_order_updated(&mut self, event: OrderUpdated) {
        let _ = self.dispatch_order_event("on_order_updated", OrderEventAny::Updated(event));
    }

    fn on_algo_order_filled(&mut self, event: OrderFilled) {
        let _ = self.dispatch_order_event("on_order_filled", OrderEventAny::Filled(event));
    }

    fn on_order_fill_voided(&mut self, event: &OrderFillVoided) {
        let _ = self.dispatch_order_event(
            "on_order_fill_voided",
            OrderEventAny::FillVoided(event.clone()),
        );
    }

    fn on_order_event(&mut self, event: OrderEventAny) {
        let _ = self.dispatch_order_event("on_order_event", event);
    }

    fn on_position_opened(&mut self, event: PositionOpened) {
        let _ = self
            .dispatch_position_event("on_position_opened", PositionEvent::PositionOpened(event));
    }

    fn on_position_changed(&mut self, event: PositionChanged) {
        let _ = self
            .dispatch_position_event("on_position_changed", PositionEvent::PositionChanged(event));
    }

    fn on_position_closed(&mut self, event: PositionClosed) {
        let _ = self
            .dispatch_position_event("on_position_closed", PositionEvent::PositionClosed(event));
    }

    fn on_position_event(&mut self, event: PositionEvent) {
        let _ = self.dispatch_position_event("on_position_event", event);
    }
}

impl DataActor for PyExecutionAlgorithm {
    fn on_start(&mut self) -> anyhow::Result<()> {
        ExecutionAlgorithm::on_start(self)?;
        self.dispatch_no_args("on_start")
            .map_err(|e| anyhow::anyhow!("Python on_start failed: {e}"))
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        ExecutionAlgorithm::on_stop(self)?;
        self.dispatch_no_args("on_stop")
            .map_err(|e| anyhow::anyhow!("Python on_stop failed: {e}"))
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        self.dispatch_no_args("on_resume")
            .map_err(|e| anyhow::anyhow!("Python on_resume failed: {e}"))
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        ExecutionAlgorithm::on_reset(self)?;
        self.dispatch_no_args("on_reset")
            .map_err(|e| anyhow::anyhow!("Python on_reset failed: {e}"))
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        self.dispatch_no_args("on_dispose")
            .map_err(|e| anyhow::anyhow!("Python on_dispose failed: {e}"))
    }

    fn on_degrade(&mut self) -> anyhow::Result<()> {
        self.dispatch_no_args("on_degrade")
            .map_err(|e| anyhow::anyhow!("Python on_degrade failed: {e}"))
    }

    fn on_fault(&mut self) -> anyhow::Result<()> {
        self.dispatch_no_args("on_fault")
            .map_err(|e| anyhow::anyhow!("Python on_fault failed: {e}"))
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        ExecutionAlgorithm::on_time_event(self, event)?;
        self.dispatch_time_event(event)
            .map_err(|e| anyhow::anyhow!("Python on_time_event failed: {e}"))
    }

    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        self.dispatch_on_signal(signal)
            .map_err(|e| anyhow::anyhow!("Python on_signal failed: {e}"))
    }

    fn on_queue_state(&mut self, event: &QueueStateChanged) -> anyhow::Result<()> {
        self.dispatch_on_queue_state(event)
            .map_err(|e| anyhow::anyhow!("Python on_queue_state failed: {e}"))
    }

    fn on_socket_state(&mut self, event: &SocketStateChanged) -> anyhow::Result<()> {
        self.dispatch_on_socket_state(event)
            .map_err(|e| anyhow::anyhow!("Python on_socket_state failed: {e}"))
    }
}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[allow(
    clippy::large_types_passed_by_value,
    reason = "PyO3 callbacks accept Python-owned event values"
)]
#[expect(
    clippy::unused_self,
    reason = "default PyO3 callbacks must remain instance methods"
)]
impl PyExecutionAlgorithm {
    /// Creates a new [`PyExecutionAlgorithm`] instance.
    #[new]
    #[pyo3(signature = (config=None))]
    fn py_new(config: Option<Py<PyAny>>) -> Self {
        let algorithm_config = config
            .as_ref()
            .and_then(|obj| Python::attach(|py| obj.extract::<ExecutionAlgorithmConfig>(py).ok()));
        let mut algorithm = Self::new(algorithm_config);
        algorithm.set_config(config);
        algorithm
    }

    /// Captures the Python self reference for Rust→Python event dispatch.
    #[pyo3(signature = (config=None))]
    fn __init__(slf: &Bound<'_, Self>, config: Option<Py<PyAny>>) -> PyResult<()> {
        let retained_config = if config.is_none() {
            Python::attach(|py| {
                slf.borrow()
                    .inner()
                    .config
                    .as_ref()
                    .map(|config| config.clone_ref(py))
            })
        } else {
            None
        };
        let has_configured_id = if let Some(config) = config.as_ref().or(retained_config.as_ref()) {
            Python::attach(|py| slf.borrow_mut().configure_from_py_config(config.bind(py)))
                .map_err(to_pyvalue_err)?
        } else {
            false
        };

        if !has_configured_id {
            let py_type = slf.get_type();
            let type_name = py_type.name()?;
            let exec_algorithm_id =
                ExecAlgorithmId::new_checked(type_name.to_str()?).map_err(to_pyvalue_err)?;
            slf.borrow_mut().set_exec_algorithm_id(exec_algorithm_id);
        }
        let py_self: Py<PyAny> = slf.clone().unbind().into_any();
        let mut borrowed = slf.borrow_mut();
        borrowed.set_python_instance(py_self);
        if config.is_some() {
            borrowed.set_config(config);
        }
        Ok(())
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> Option<TraderId> {
        self.inner().core.actor.trader_id()
    }

    #[getter]
    #[pyo3(name = "exec_algorithm_id")]
    fn py_exec_algorithm_id(&self) -> ExecAlgorithmId {
        self.exec_algorithm_id()
    }

    #[getter]
    #[pyo3(name = "config")]
    fn py_config(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.inner()
            .config
            .as_ref()
            .map(|config| config.clone_ref(py))
    }

    /// Returns an importable configuration for this execution algorithm.
    #[pyo3(name = "to_importable_config")]
    fn py_to_importable_config(&self, py: Python<'_>) -> PyResult<ImportableExecAlgorithmConfig> {
        let py_self = self
            .inner()
            .py_self
            .as_ref()
            .ok_or_else(|| to_pyruntime_err("Python execution algorithm instance is not set"))?
            .bind(py);
        let exec_algorithm_path = py_type_path(py_self)?;

        let Some(config) = self.inner().config.as_ref() else {
            return Ok(ImportableExecAlgorithmConfig {
                exec_algorithm_path,
                config_path: String::new(),
                config: HashMap::new(),
            });
        };
        let config = config.bind(py);

        Ok(ImportableExecAlgorithmConfig {
            exec_algorithm_path,
            config_path: py_type_path(config)?,
            config: py_config_to_json(config)?,
        })
    }

    #[getter]
    #[pyo3(name = "clock")]
    fn py_clock(&self) -> Option<PyClock> {
        self.inner()
            .core
            .actor
            .is_registered()
            .then(|| PyClock::from_rc(self.inner().core.actor.clock_rc()))
    }

    #[getter]
    #[pyo3(name = "cache")]
    fn py_cache(&self) -> Option<PyCache> {
        self.inner()
            .core
            .actor
            .is_registered()
            .then(|| PyCache::from_rc(self.inner().core.actor.cache_rc()))
    }

    #[getter]
    #[pyo3(name = "portfolio")]
    fn py_portfolio(&self) -> PyResult<PyPortfolio> {
        if self.inner().core.actor.is_registered() {
            Ok(PyPortfolio::from_rc(self.portfolio_rc()))
        } else {
            Err(to_pyruntime_err(
                "ExecutionAlgorithm must be registered with a trader before accessing portfolio",
            ))
        }
    }

    #[getter]
    #[pyo3(name = "log")]
    fn py_log(&self) -> PyLogger {
        self.inner().logger.clone()
    }

    #[getter]
    #[pyo3(name = "state")]
    fn py_state(&self) -> ComponentState {
        self.inner().core.actor.state()
    }

    #[pyo3(name = "is_registered")]
    fn py_is_registered(&self) -> bool {
        self.inner().core.actor.is_registered()
    }

    #[pyo3(name = "is_ready")]
    fn py_is_ready(&self) -> bool {
        Component::is_ready(self)
    }

    #[pyo3(name = "is_running")]
    fn py_is_running(&self) -> bool {
        Component::is_running(self)
    }

    #[pyo3(name = "is_stopped")]
    fn py_is_stopped(&self) -> bool {
        Component::is_stopped(self)
    }

    #[pyo3(name = "is_disposed")]
    fn py_is_disposed(&self) -> bool {
        Component::is_disposed(self)
    }

    #[pyo3(name = "is_degraded")]
    fn py_is_degraded(&self) -> bool {
        Component::is_degraded(self)
    }

    #[pyo3(name = "is_faulted")]
    fn py_is_faulted(&self) -> bool {
        Component::is_faulted(self)
    }

    #[pyo3(name = "start")]
    fn py_start(slf: PyRef<'_, Self>) -> PyResult<()> {
        let mut exec_algorithm = slf.clone();
        drop(slf);
        Component::start(&mut exec_algorithm).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "stop")]
    fn py_stop(slf: PyRef<'_, Self>) -> PyResult<()> {
        let mut exec_algorithm = slf.clone();
        drop(slf);
        Component::stop(&mut exec_algorithm).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "resume")]
    fn py_resume(slf: PyRef<'_, Self>) -> PyResult<()> {
        let mut exec_algorithm = slf.clone();
        drop(slf);
        Component::resume(&mut exec_algorithm).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "reset")]
    fn py_reset(slf: PyRef<'_, Self>) -> PyResult<()> {
        let mut exec_algorithm = slf.clone();
        drop(slf);
        Component::reset(&mut exec_algorithm).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "dispose")]
    fn py_dispose(slf: PyRef<'_, Self>) -> PyResult<()> {
        let mut exec_algorithm = slf.clone();
        drop(slf);
        Component::dispose(&mut exec_algorithm).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "degrade")]
    fn py_degrade(slf: PyRef<'_, Self>) -> PyResult<()> {
        let mut exec_algorithm = slf.clone();
        drop(slf);
        Component::degrade(&mut exec_algorithm).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "fault")]
    fn py_fault(slf: PyRef<'_, Self>) -> PyResult<()> {
        let mut exec_algorithm = slf.clone();
        drop(slf);
        Component::fault(&mut exec_algorithm).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "publish_data")]
    fn py_publish_data(&self, data_type: &DataType, data: &CustomData) {
        DataActor::publish_data(self, data_type, data);
    }

    #[pyo3(name = "publish_signal")]
    #[pyo3(signature = (name, value, ts_event=0))]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 accepts an owned PyAny handle for Python signal values"
    )]
    fn py_publish_signal(
        &self,
        py: Python<'_>,
        name: &str,
        value: Py<PyAny>,
        ts_event: u64,
    ) -> PyResult<()> {
        let value_str: String = value.bind(py).str()?.extract()?;
        DataActor::publish_signal(self, name, value_str, UnixNanos::from(ts_event));
        Ok(())
    }

    #[pyo3(name = "subscribe_signal")]
    #[pyo3(signature = (name="", priority=None))]
    fn py_subscribe_signal(&mut self, name: &str, priority: Option<u32>) {
        DataActor::subscribe_signal(self, name, priority);
    }

    #[pyo3(name = "subscribe_queue_state")]
    #[pyo3(signature = (priority=None))]
    fn py_subscribe_queue_state(&mut self, priority: Option<u32>) {
        DataActor::subscribe_queue_state(self, priority);
    }

    #[pyo3(name = "subscribe_socket_state")]
    #[pyo3(signature = (priority=None))]
    fn py_subscribe_socket_state(&mut self, priority: Option<u32>) {
        DataActor::subscribe_socket_state(self, priority);
    }

    #[pyo3(name = "unsubscribe_signal")]
    #[pyo3(signature = (name=""))]
    fn py_unsubscribe_signal(&mut self, name: &str) {
        DataActor::unsubscribe_signal(self, name);
    }

    #[pyo3(name = "unsubscribe_queue_state")]
    fn py_unsubscribe_queue_state(&mut self) {
        DataActor::unsubscribe_queue_state(self);
    }

    #[pyo3(name = "unsubscribe_socket_state")]
    fn py_unsubscribe_socket_state(&mut self) {
        DataActor::unsubscribe_socket_state(self);
    }

    #[pyo3(name = "on_start")]
    fn py_on_start(&mut self) {}

    #[pyo3(name = "on_stop")]
    fn py_on_stop(&mut self) {}

    #[pyo3(name = "on_resume")]
    fn py_on_resume(&mut self) {}

    #[pyo3(name = "on_reset")]
    fn py_on_reset(&mut self) {}

    #[pyo3(name = "on_dispose")]
    fn py_on_dispose(&mut self) {}

    #[pyo3(name = "on_degrade")]
    fn py_on_degrade(&mut self) {}

    #[pyo3(name = "on_fault")]
    fn py_on_fault(&mut self) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_time_event")]
    fn py_on_time_event(&mut self, event: TimeEvent) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_signal")]
    fn py_on_signal(&mut self, signal: &Signal) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_queue_state")]
    fn py_on_queue_state(&mut self, event: QueueStateChanged) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_socket_state")]
    fn py_on_socket_state(&mut self, event: SocketStateChanged) {}

    #[allow(clippy::needless_pass_by_value)]
    #[pyo3(name = "execute")]
    fn py_execute(&mut self, command: Py<PyAny>) -> PyResult<()> {
        let _ = command;
        Err(to_pyruntime_err(
            "ExecutionAlgorithm.execute is invoked by the v2 runtime endpoint",
        ))
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_order")]
    fn py_on_order(&mut self, order: Py<PyAny>) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_order_list")]
    fn py_on_order_list(&mut self, order_list: Py<PyAny>, orders: Py<PyAny>) {}

    #[pyo3(name = "spawn_market")]
    #[pyo3(signature = (
        primary,
        quantity,
        time_in_force = TimeInForce::Gtc,
        reduce_only = false,
        tags = None,
        reduce_primary = true
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_spawn_market(
        &mut self,
        py: Python<'_>,
        primary: Py<PyAny>,
        quantity: Quantity,
        time_in_force: TimeInForce,
        reduce_only: bool,
        tags: Option<Vec<String>>,
        reduce_primary: bool,
    ) -> PyResult<MarketOrder> {
        let mut primary = self.primary_order_for_spawn(py, primary, quantity, reduce_primary)?;
        Ok(ExecutionAlgorithm::spawn_market(
            self,
            &mut primary,
            quantity,
            time_in_force,
            reduce_only,
            Self::tags_to_ustr(tags),
            reduce_primary,
        ))
    }

    #[pyo3(name = "spawn_limit")]
    #[pyo3(signature = (
        primary,
        quantity,
        price,
        time_in_force = TimeInForce::Gtc,
        expire_time = None,
        post_only = false,
        reduce_only = false,
        display_qty = None,
        emulation_trigger = None,
        tags = None,
        reduce_primary = true
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_spawn_limit(
        &mut self,
        py: Python<'_>,
        primary: Py<PyAny>,
        quantity: Quantity,
        price: Price,
        time_in_force: TimeInForce,
        expire_time: Option<Timestamp>,
        post_only: bool,
        reduce_only: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        tags: Option<Vec<String>>,
        reduce_primary: bool,
    ) -> PyResult<LimitOrder> {
        let mut primary = self.primary_order_for_spawn(py, primary, quantity, reduce_primary)?;
        Ok(ExecutionAlgorithm::spawn_limit(
            self,
            &mut primary,
            quantity,
            price,
            time_in_force,
            expire_time.map(UnixNanos::from),
            post_only,
            reduce_only,
            display_qty,
            emulation_trigger,
            Self::tags_to_ustr(tags),
            reduce_primary,
        ))
    }

    #[pyo3(name = "spawn_market_to_limit")]
    #[pyo3(signature = (
        primary,
        quantity,
        time_in_force = TimeInForce::Gtc,
        expire_time = None,
        reduce_only = false,
        display_qty = None,
        emulation_trigger = None,
        tags = None,
        reduce_primary = true
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_spawn_market_to_limit(
        &mut self,
        py: Python<'_>,
        primary: Py<PyAny>,
        quantity: Quantity,
        time_in_force: TimeInForce,
        expire_time: Option<Timestamp>,
        reduce_only: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        tags: Option<Vec<String>>,
        reduce_primary: bool,
    ) -> PyResult<MarketToLimitOrder> {
        let mut primary = self.primary_order_for_spawn(py, primary, quantity, reduce_primary)?;
        Ok(ExecutionAlgorithm::spawn_market_to_limit(
            self,
            &mut primary,
            quantity,
            time_in_force,
            expire_time.map(UnixNanos::from),
            reduce_only,
            display_qty,
            emulation_trigger,
            Self::tags_to_ustr(tags),
            reduce_primary,
        ))
    }

    #[pyo3(name = "deny_order")]
    fn py_deny_order(&mut self, py: Python<'_>, order: Py<PyAny>, reason: &str) -> PyResult<()> {
        let order = pyobject_to_order_any(py, order)?;
        ExecutionAlgorithm::deny_order(self, &order, Ustr::from(reason)).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (order, position_id=None, client_id=None))]
    fn py_submit_order(
        &mut self,
        py: Python<'_>,
        order: Py<PyAny>,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
    ) -> PyResult<()> {
        let order = pyobject_to_order_any(py, order)?;
        ExecutionAlgorithm::submit_order(self, order, position_id, client_id)
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (order, quantity=None, price=None, trigger_price=None, client_id=None))]
    fn py_modify_order(
        &mut self,
        py: Python<'_>,
        order: Py<PyAny>,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        client_id: Option<ClientId>,
    ) -> PyResult<()> {
        let mut order = pyobject_to_order_any(py, order)?;
        ExecutionAlgorithm::modify_order(
            self,
            &mut order,
            quantity,
            price,
            trigger_price,
            client_id,
        )
        .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "modify_order_in_place")]
    #[pyo3(signature = (order, quantity=None, price=None, trigger_price=None))]
    fn py_modify_order_in_place(
        &mut self,
        py: Python<'_>,
        order: Py<PyAny>,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) -> PyResult<()> {
        let mut order = pyobject_to_order_any(py, order)?;
        ExecutionAlgorithm::modify_order_in_place(self, &mut order, quantity, price, trigger_price)
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (order, client_id=None))]
    fn py_cancel_order(
        &mut self,
        py: Python<'_>,
        order: Py<PyAny>,
        client_id: Option<ClientId>,
    ) -> PyResult<()> {
        let mut order = pyobject_to_order_any(py, order)?;
        ExecutionAlgorithm::cancel_order(self, &mut order, client_id).map_err(to_pyruntime_err)
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_order_initialized")]
    fn py_on_order_initialized(&mut self, event: OrderInitialized) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_order_event")]
    fn py_on_order_event(&mut self, event: Py<PyAny>) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_denied")]
    fn py_on_order_denied(&mut self, event: OrderDenied) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_emulated")]
    fn py_on_order_emulated(&mut self, event: OrderEmulated) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_released")]
    fn py_on_order_released(&mut self, event: OrderReleased) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_submitted")]
    fn py_on_order_submitted(&mut self, event: OrderSubmitted) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_rejected")]
    fn py_on_order_rejected(&mut self, event: OrderRejected) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_accepted")]
    fn py_on_order_accepted(&mut self, event: OrderAccepted) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_canceled")]
    fn py_on_order_canceled(&mut self, event: OrderCanceled) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_expired")]
    fn py_on_order_expired(&mut self, event: OrderExpired) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_triggered")]
    fn py_on_order_triggered(&mut self, event: OrderTriggered) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_pending_update")]
    fn py_on_order_pending_update(&mut self, event: OrderPendingUpdate) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_pending_cancel")]
    fn py_on_order_pending_cancel(&mut self, event: OrderPendingCancel) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_modify_rejected")]
    fn py_on_order_modify_rejected(&mut self, event: OrderModifyRejected) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_cancel_rejected")]
    fn py_on_order_cancel_rejected(&mut self, event: OrderCancelRejected) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_updated")]
    fn py_on_order_updated(&mut self, event: OrderUpdated) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_order_filled")]
    fn py_on_order_filled(&mut self, event: OrderFilled) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_order_fill_voided")]
    fn py_on_order_fill_voided(&mut self, event: OrderFillVoided) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_position_opened")]
    fn py_on_position_opened(&mut self, event: PositionOpened) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_position_event")]
    fn py_on_position_event(&mut self, event: Py<PyAny>) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_position_changed")]
    fn py_on_position_changed(&mut self, event: PositionChanged) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_position_closed")]
    fn py_on_position_closed(&mut self, event: PositionClosed) {}
}

impl PyExecutionAlgorithm {
    /// Applies Python configuration overrides.
    ///
    /// Returns whether the config supplied an execution algorithm ID.
    ///
    /// # Errors
    ///
    /// Returns an error if an ID has an unsupported type or invalid value.
    pub fn configure_from_py_config(&mut self, config: &Bound<'_, PyAny>) -> anyhow::Result<bool> {
        let id = config
            .getattr("exec_algorithm_id")
            .ok()
            .filter(|id| !id.is_none())
            .or_else(|| config.getattr("actor_id").ok().filter(|id| !id.is_none()));
        let has_id = if let Some(id) = id {
            let exec_algorithm_id = if let Ok(exec_algorithm_id) = id.extract::<ExecAlgorithmId>() {
                exec_algorithm_id
            } else if let Ok(actor_id) = id.extract::<ActorId>() {
                ExecAlgorithmId::new_checked(actor_id.inner().as_str())?
            } else if let Ok(id) = id.extract::<String>() {
                ExecAlgorithmId::new_checked(&id)?
            } else {
                anyhow::bail!("Invalid `exec_algorithm_id`/`actor_id` type");
            };
            self.set_exec_algorithm_id(exec_algorithm_id);
            true
        } else {
            false
        };

        if let Ok(log_events) = config.getattr("log_events")
            && let Ok(log_events) = log_events.extract::<bool>()
        {
            self.set_log_events(log_events);
        }

        if let Ok(log_commands) = config.getattr("log_commands")
            && let Ok(log_commands) = log_commands.extract::<bool>()
        {
            self.set_log_commands(log_commands);
        }

        Ok(has_id)
    }
}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ExecutionAlgorithmConfig {
    /// Configuration for an execution algorithm.
    #[new]
    #[pyo3(signature = (
        exec_algorithm_id=None,
        log_events=true,
        log_commands=true,
        **_kwargs
    ))]
    fn py_new(
        #[gen_stub(override_type(type_repr = "model.ExecAlgorithmId | str | None"))]
        exec_algorithm_id: Option<&Bound<'_, PyAny>>,
        log_events: bool,
        log_commands: bool,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let exec_algorithm_id = exec_algorithm_id
            .map(|value| -> PyResult<ExecAlgorithmId> {
                if let Ok(exec_algorithm_id) = value.extract::<ExecAlgorithmId>() {
                    Ok(exec_algorithm_id)
                } else {
                    let value: String = value.extract()?;
                    ExecAlgorithmId::new_checked(&value).map_err(to_pyvalue_err)
                }
            })
            .transpose()?;

        Ok(Self {
            exec_algorithm_id,
            log_events,
            log_commands,
        })
    }

    #[getter]
    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id
    }

    #[getter]
    fn log_events(&self) -> bool {
        self.log_events
    }

    #[getter]
    fn log_commands(&self) -> bool {
        self.log_commands
    }
}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ImportableExecAlgorithmConfig {
    /// Configuration for creating execution algorithms from importable paths.
    #[new]
    #[expect(clippy::needless_pass_by_value)]
    fn py_new(
        exec_algorithm_path: String,
        config_path: String,
        config: Py<PyDict>,
    ) -> PyResult<Self> {
        let json_config = Python::attach(|py| py_dict_to_json(config.bind(py)))?;

        Ok(Self {
            exec_algorithm_path,
            config_path,
            config: json_config,
        })
    }

    #[getter]
    fn exec_algorithm_path(&self) -> &String {
        &self.exec_algorithm_path
    }

    #[getter]
    fn config_path(&self) -> &String {
        &self.config_path
    }

    #[getter]
    fn config(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let py_dict = PyDict::new(py);

        for (key, value) in &self.config {
            let json_str = serde_json::to_string(value).map_err(to_pyvalue_err)?;
            let py_value = PyModule::import(py, "json")?.call_method("loads", (json_str,), None)?;
            py_dict.set_item(key, py_value)?;
        }
        Ok(py_dict.unbind())
    }
}

fn py_type_path(value: &Bound<'_, PyAny>) -> PyResult<String> {
    let value_type = value.get_type();
    let module: String = value_type.getattr("__module__")?.extract()?;
    let qualname: String = value_type.getattr("__qualname__")?.extract()?;
    Ok(format!("{module}:{qualname}"))
}

fn py_config_to_json(config: &Bound<'_, PyAny>) -> PyResult<HashMap<String, serde_json::Value>> {
    let py = config.py();
    let config_dict = PyDict::new(py);

    if let Ok(attributes) = config.getattr("__dict__")
        && let Ok(attributes) = attributes.cast::<PyDict>()
    {
        for (key, value) in attributes.iter() {
            config_dict.set_item(key, value)?;
        }
    }

    for field in [
        "exec_algorithm_id",
        "actor_id",
        "log_events",
        "log_commands",
    ] {
        if let Ok(value) = config.getattr(field) {
            config_dict.set_item(field, value)?;
        }
    }

    py_dict_to_json(&config_dict)
}

fn py_dict_to_json(config: &Bound<'_, PyDict>) -> PyResult<HashMap<String, serde_json::Value>> {
    let py = config.py();
    let kwargs = PyDict::new(py);
    kwargs.set_item("default", py.eval(pyo3::ffi::c_str!("str"), None, None)?)?;
    let json_str: String = PyModule::import(py, "json")?
        .call_method("dumps", (config,), Some(&kwargs))?
        .extract()?;

    let json_value: serde_json::Value = serde_json::from_str(&json_str).map_err(to_pyvalue_err)?;

    if let serde_json::Value::Object(map) = json_value {
        Ok(map.into_iter().collect())
    } else {
        Err(to_pyvalue_err("Config must be a dictionary"))
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::{Clock, TestClock},
        messages::system::{
            QueueCondition, QueueState, QueueStateChanged, SocketState, SocketStateChanged,
        },
        msgbus::{MessageBus, MessagingSwitchboard, get_message_bus},
        runner::SystemChannel,
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::OrderType,
        identifiers::{
            ClientId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TraderId, Venue,
        },
        orders::OrderTestBuilder,
    };
    use pyo3::ffi::c_str;
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    fn sample_queue_state_changed() -> QueueStateChanged {
        QueueStateChanged::new(
            TraderId::from("TRADER-001"),
            SystemChannel::ExecCommands,
            QueueCondition::Backlogged,
            QueueState::Triggered,
            17,
            23,
            UUID4::from("00000000-0000-4000-8000-000000000001"),
            UnixNanos::from(1_700_000_000_000_000_001),
            UnixNanos::from(1_700_000_000_000_000_002),
        )
    }

    fn sample_socket_state_changed() -> SocketStateChanged {
        SocketStateChanged::new(
            TraderId::from("TRADER-001"),
            ClientId::from("BINANCE"),
            Some(Venue::from("BINANCE")),
            Ustr::from("binance-futures-market-streams"),
            SocketState::Connected,
            UUID4::from("00000000-0000-4000-8000-000000000001"),
            UnixNanos::from(1_700_000_000_000_000_001),
            UnixNanos::from(1_700_000_000_000_000_002),
        )
    }

    #[rstest]
    fn test_python_queue_state_dispatches_exact_event() {
        Python::initialize();

        let tracker = Python::attach(|py| {
            py.run(
                c_str!(
                    r#"
class QueueStateTracker:
    def __init__(self):
        self.event = None

    def on_queue_state(self, event):
        self.event = event
"#
                ),
                None,
                None,
            )
            .unwrap();
            py.eval(c_str!("QueueStateTracker()"), None, None)
                .unwrap()
                .unbind()
        });
        let mut algorithm = PyExecutionAlgorithm::new(None);
        algorithm.set_python_instance(tracker);
        let event = sample_queue_state_changed();

        DataActor::on_queue_state(&mut algorithm, &event).unwrap();

        let received = Python::attach(|py| {
            algorithm
                .inner()
                .py_self
                .as_ref()
                .unwrap()
                .getattr(py, "event")
                .unwrap()
                .extract::<QueueStateChanged>(py)
                .unwrap()
        });
        assert_eq!(received, event);
    }

    #[rstest]
    fn test_python_socket_state_dispatches_exact_event() {
        Python::initialize();

        let tracker = Python::attach(|py| {
            py.run(
                c_str!(
                    r#"
class SocketStateTracker:
    def __init__(self):
        self.event = None

    def on_socket_state(self, event):
        self.event = event
"#
                ),
                None,
                None,
            )
            .unwrap();
            py.eval(c_str!("SocketStateTracker()"), None, None)
                .unwrap()
                .unbind()
        });
        let mut algorithm = PyExecutionAlgorithm::new(None);
        algorithm.set_python_instance(tracker);
        let event = sample_socket_state_changed();

        DataActor::on_socket_state(&mut algorithm, &event).unwrap();

        let received = Python::attach(|py| {
            algorithm
                .inner()
                .py_self
                .as_ref()
                .unwrap()
                .getattr(py, "event")
                .unwrap()
                .extract::<SocketStateChanged>(py)
                .unwrap()
        });
        assert_eq!(received, event);
    }

    #[rstest]
    fn test_python_subscribe_and_unsubscribe_queue_state_update_msgbus() {
        *get_message_bus().borrow_mut() = MessageBus::default();

        let mut algorithm = PyExecutionAlgorithm::new(None);
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        Component::register(&mut algorithm, TraderId::from("TRADER-001"), clock, cache).unwrap();

        algorithm.py_subscribe_queue_state(Some(50));

        let topic = MessagingSwitchboard::queue_state_changed_topic();
        let subscriptions = get_message_bus().borrow_mut().matching_subscriptions(topic);
        assert_eq!(subscriptions.len(), 1);
        assert_eq!(subscriptions[0].priority, 50);

        algorithm.py_unsubscribe_queue_state();

        let subscriptions = get_message_bus().borrow_mut().matching_subscriptions(topic);
        assert!(subscriptions.is_empty());
    }

    #[rstest]
    fn test_python_subscribe_and_unsubscribe_socket_state_update_msgbus() {
        *get_message_bus().borrow_mut() = MessageBus::default();

        let mut algorithm = PyExecutionAlgorithm::new(None);
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        Component::register(&mut algorithm, TraderId::from("TRADER-001"), clock, cache).unwrap();

        algorithm.py_subscribe_socket_state(Some(50));

        let topic = MessagingSwitchboard::socket_state_changed_topic();
        let subscriptions = get_message_bus().borrow_mut().matching_subscriptions(topic);
        assert_eq!(subscriptions.len(), 1);
        assert_eq!(subscriptions[0].priority, 50);

        algorithm.py_unsubscribe_socket_state();

        let subscriptions = get_message_bus().borrow_mut().matching_subscriptions(topic);
        assert!(subscriptions.is_empty());
    }

    #[rstest]
    fn test_python_order_list_override_receives_resolved_orders_without_fanout() {
        Python::initialize();

        let tracker = Python::attach(|py| {
            py.run(
                c_str!(
                    r#"
class OrderListTracker:
    def __init__(self):
        self.list_calls = 0
        self.list_ids = []
        self.resolved_ids = []
        self.order_ids = []

    def on_order_list(self, order_list, orders):
        self.list_calls += 1
        self.list_ids = [str(value) for value in order_list.client_order_ids()]
        self.resolved_ids = [str(order.client_order_id) for order in orders]

    def on_order(self, order):
        self.order_ids.append(str(order.client_order_id))

    def observations(self):
        return self.list_calls, self.list_ids, self.resolved_ids, self.order_ids
"#
                ),
                None,
                None,
            )
            .unwrap();
            py.eval(c_str!("OrderListTracker()"), None, None)
                .unwrap()
                .unbind()
        });
        let mut algorithm = PyExecutionAlgorithm::new(None);
        algorithm.set_python_instance(tracker);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let strategy_id = StrategyId::from("STRAT-LIST-OVERRIDE");
        let first = OrderTestBuilder::new(OrderType::Market)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-LIST-OVERRIDE-001"))
            .quantity(Quantity::from("1.0"))
            .build();
        let second = OrderTestBuilder::new(OrderType::Market)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-LIST-OVERRIDE-002"))
            .quantity(Quantity::from("2.0"))
            .build();
        let order_list = OrderList::new(
            OrderListId::from("OL-OVERRIDE-001"),
            instrument_id,
            strategy_id,
            vec![first.client_order_id(), second.client_order_id()],
            0.into(),
        );

        ExecutionAlgorithm::on_order_list(&mut algorithm, order_list, vec![first, second]).unwrap();

        let observations = Python::attach(|py| {
            algorithm
                .inner()
                .py_self
                .as_ref()
                .unwrap()
                .call_method0(py, "observations")
                .unwrap()
                .extract::<(usize, Vec<String>, Vec<String>, Vec<String>)>(py)
                .unwrap()
        });
        assert_eq!(
            observations,
            (
                1,
                vec![
                    "O-LIST-OVERRIDE-001".to_string(),
                    "O-LIST-OVERRIDE-002".to_string(),
                ],
                vec![
                    "O-LIST-OVERRIDE-001".to_string(),
                    "O-LIST-OVERRIDE-002".to_string(),
                ],
                Vec::new(),
            ),
        );
    }
}
