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

//! Python bindings for live node.

use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    collections::HashMap,
    fmt::Debug,
    future::Future,
    pin::Pin,
    rc::Rc,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll, Waker},
    thread,
    time::{Duration, Instant},
};

use nautilus_common::{
    actor::data_actor::ImportableActorConfig,
    cache::CacheConfig,
    enums::Environment,
    live::get_runtime,
    logging::logger::LoggerConfig,
    msgbus::MessageBusConfig,
    python::{
        actor::{PyDataActor, prepare_python_actor},
        cache::{PyCache, get_global_cache_database_factory_registry},
        msgbus::get_global_msgbus_factory_registry,
    },
};
#[cfg(feature = "examples")]
use nautilus_core::python::to_pytype_err;
use nautilus_core::{
    UUID4,
    python::{to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{
    enums::OmsType,
    identifiers::{ActorId, ExecAlgorithmId, InstrumentId, TraderId},
};
use nautilus_portfolio::{config::PortfolioConfig, python::PyPortfolio};
use nautilus_system::get_global_pyo3_registry;
#[cfg(feature = "examples")]
use nautilus_testkit::{DataTester, DataTesterConfig, ExecTester, ExecTesterConfig};
#[cfg(feature = "examples")]
use nautilus_trading::examples::{
    actors::{BookImbalanceActor, BookImbalanceActorConfig},
    strategies::{
        CompositeMarketMaker, CompositeMarketMakerConfig, DeltaNeutralVol, DeltaNeutralVolConfig,
        EmaCross, EmaCrossConfig, GridMarketMaker, GridMarketMakerConfig, HurstVpinDirectional,
        HurstVpinDirectionalConfig,
    },
};
use nautilus_trading::{
    ImportableControllerConfig, ImportableExecutionAlgorithmConfig, ImportableStrategyConfig,
    python::{algorithm::PyExecutionAlgorithm, strategy::PyStrategy},
};
use parking_lot::{Condvar, Mutex};
use pyo3::{
    ffi::c_str,
    intern,
    prelude::*,
    sync::PyOnceLock,
    types::{PyCFunction, PyDict, PyTuple},
};
use serde_json;

// Re-exported so the `live` module registers every Python class through this module.
pub use crate::node::NodeState;
use crate::{
    builder::LiveNodeBuilder,
    config::{
        LiveDataEngineConfig, LiveExecutionEngineConfig, LiveNodeConfig, LiveRiskEngineConfig,
        PluginConfig,
    },
    node::{LiveNode, LiveNodeHandle, NodeRunMode, config::RoutingConfig},
    python::config::coerce_json_config,
};

/// Python-facing wrapper owning a [`LiveNode`].
///
/// `run_async` moves the node into the returned awaitable, so the wrapper is empty for the
/// remainder of a hosted run. Every method then fails with a clear error rather than aliasing the
/// node the run future is driving. Capture `cache`, `portfolio`, and `handle` before starting a
/// hosted run; each is an independent handle that stays usable while the node runs.
#[pyo3::pyclass(module = "nautilus_trader.live", name = "LiveNode", unsendable)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
#[derive(Debug)]
pub struct PyLiveNode {
    inner: Rc<RefCell<Option<LiveNode>>>,
    handle: LiveNodeHandle,
}

impl PyLiveNode {
    /// Wraps an owned node for Python.
    #[must_use]
    pub fn new(node: LiveNode) -> Self {
        Self {
            handle: node.handle(),
            inner: Rc::new(RefCell::new(Some(node))),
        }
    }

    fn node(&self) -> PyResult<Ref<'_, LiveNode>> {
        let borrow = self.inner.try_borrow().map_err(|_| node_busy_err())?;
        if borrow.is_none() {
            return Err(node_consumed_err());
        }

        Ok(Ref::map(borrow, |node| {
            node.as_ref().expect("node presence checked above")
        }))
    }

    fn node_mut(&self) -> PyResult<RefMut<'_, LiveNode>> {
        let borrow = self.inner.try_borrow_mut().map_err(|_| node_busy_err())?;
        if borrow.is_none() {
            return Err(node_consumed_err());
        }

        Ok(RefMut::map(borrow, |node| {
            node.as_mut().expect("node presence checked above")
        }))
    }

    fn is_consumed(&self) -> bool {
        self.inner.try_borrow().is_ok_and(|node| node.is_none())
    }
}

/// Thread-safe control handle for a [`LiveNode`].
///
/// Stays valid for the node's whole lifetime, including while a hosted run owns the node, and is
/// safe to call from any thread or from a signal handler.
#[pyo3::pyclass(
    module = "nautilus_trader.live",
    name = "LiveNodeHandle",
    frozen,
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
#[derive(Clone, Debug)]
pub struct PyLiveNodeHandle {
    inner: LiveNodeHandle,
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyLiveNodeHandle {
    /// Signals the node to stop, returning immediately.
    ///
    /// The node then runs its full shutdown sequence, so callers awaiting `run_async` observe
    /// completion only once shutdown finishes.
    #[pyo3(name = "stop")]
    fn py_stop(&self) {
        self.inner.stop();
    }

    /// Returns whether a stop has been requested.
    #[getter]
    #[pyo3(name = "is_stopping")]
    fn py_is_stopping(&self) -> bool {
        self.inner.should_stop()
    }

    /// Returns whether the node is currently running.
    #[getter]
    #[pyo3(name = "is_running")]
    fn py_is_running(&self) -> bool {
        self.inner.is_running()
    }

    /// Returns the node's current lifecycle state.
    #[getter]
    #[pyo3(name = "state")]
    fn py_state(&self) -> NodeState {
        self.inner.state()
    }

    fn __repr__(&self) -> String {
        format!("LiveNodeHandle(state={:?})", self.inner.state())
    }
}

/// How long `close` drives a still-running node before giving up.
///
/// A closed host loop can no longer resume the run, so the node is driven to completion inline.
/// Bounded so interpreter teardown cannot hang indefinitely on an unresponsive venue.
const CLOSE_DRIVE_TIMEOUT: Duration = Duration::from_secs(30);

thread_local! {
    /// Guards against a second hosted run on the same thread.
    ///
    /// The runner binds its senders into thread-local storage and the msgbus is thread-local, so
    /// two interleaved hosted nodes would cross-wire each other's events rather than fail.
    static HOSTED_RUN_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Waker for driving a run to completion without an event loop.
///
/// The host loop is gone by the time this is used, so waking must not touch Python: a tokio worker
/// that blocked acquiring the GIL could not then wake the thread waiting here.
#[derive(Debug, Default)]
struct BlockingWake {
    woken: Mutex<bool>,
    signal: Condvar,
}

impl BlockingWake {
    /// Waits for a wake, returning `false` once the deadline passes.
    fn wait_until(&self, deadline: Instant) -> bool {
        let mut woken = self.woken.lock();
        while !*woken {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };

            let timeout = self.signal.wait_for(&mut woken, remaining);

            if timeout.timed_out() && !*woken {
                return false;
            }
        }

        *woken = false;
        true
    }
}

impl std::task::Wake for BlockingWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        *self.woken.lock() = true;
        self.signal.notify_all();
    }
}

/// Wake state shared between a hosted run and the callback that resumes it.
#[derive(Debug, Default)]
struct RunWakeState {
    pending: Mutex<Option<RunSuspension>>,
    closed: AtomicBool,
}

#[derive(Debug)]
struct RunSuspension {
    generation: u64,
    future: Py<PyAny>,
}

impl RunWakeState {
    fn suspend(&self, generation: u64, future: Py<PyAny>) {
        *self.pending.lock() = Some(RunSuspension { generation, future });
    }

    fn resume(&self, py: Python<'_>, generation: u64) -> PyResult<()> {
        let mut pending = self.pending.lock();
        let Some(suspension) = pending.as_ref() else {
            return Ok(());
        };

        if self.closed.load(Ordering::Acquire) || suspension.generation != generation {
            return Ok(());
        }

        let future = pending.take().expect("suspension presence checked").future;
        drop(pending);

        let future = future.bind(py);
        if !future
            .call_method0(intern!(py, "done"))?
            .extract::<bool>()?
        {
            future.call_method1(intern!(py, "set_result"), (py.None(),))?;
        }

        Ok(())
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.pending.lock().take();
    }
}

/// Callable scheduled on the host event loop to resume a hosted run.
#[pyo3::pyclass(name = "NodeRunWake", frozen)]
struct PyNodeRunWake {
    state: Arc<RunWakeState>,
}

#[pymethods]
impl PyNodeRunWake {
    fn __call__(&self, py: Python<'_>, generation: u64) -> PyResult<()> {
        self.state.resume(py, generation)
    }
}

enum HostWakeSignal {
    Resume(u64),
    Shutdown,
}

struct HostWakeControl {
    sender: mpsc::Sender<HostWakeSignal>,
    active: AtomicBool,
    handle: LiveNodeHandle,
}

impl HostWakeControl {
    fn resume(&self, generation: u64) {
        if !self.active.load(Ordering::Acquire) {
            return;
        }

        if self
            .sender
            .send(HostWakeSignal::Resume(generation))
            .is_err()
            && self.active.swap(false, Ordering::AcqRel)
        {
            // Nothing can resume a suspended run once the wake pump is gone. Request a stop,
            // but the awaiting task may remain suspended.
            log::error!("Hosted run wake pump stopped unexpectedly, stopping node");
            self.handle.stop();
        }
    }

    fn close(&self) {
        if self.active.swap(false, Ordering::AcqRel) {
            let _ = self.sender.send(HostWakeSignal::Shutdown);
        }
    }
}

/// Waker that signals the host wake pump from whichever thread completed the work.
struct HostLoopWaker {
    generation: u64,
    scheduled: AtomicBool,
    control: Arc<HostWakeControl>,
}

impl std::task::Wake for HostLoopWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            self.control.resume(self.generation);
        }
    }
}

/// Moves Python scheduling off the dependency thread that invoked the waker.
struct HostWakePump {
    control: Arc<HostWakeControl>,
    thread: Option<thread::JoinHandle<()>>,
}

impl HostWakePump {
    fn start(
        event_loop: Py<PyAny>,
        wake_callback: Py<PyAny>,
        handle: LiveNodeHandle,
    ) -> PyResult<Self> {
        let (sender, receiver) = mpsc::channel();
        let control = Arc::new(HostWakeControl {
            sender,
            active: AtomicBool::new(true),
            handle: handle.clone(),
        });

        let control_for_thread = control.clone();

        let thread = thread::Builder::new()
            .name("nautilus-host-wake".to_string())
            .spawn(move || {
                while let Ok(signal) = receiver.recv() {
                    match signal {
                        HostWakeSignal::Resume(generation) => {
                            if !control_for_thread.active.load(Ordering::Acquire) {
                                continue;
                            }

                            let Some(result) = Python::try_attach(|py| {
                                event_loop.bind(py).call_method1(
                                    intern!(py, "call_soon_threadsafe"),
                                    (wake_callback.bind(py), generation),
                                )?;
                                Ok::<(), PyErr>(())
                            }) else {
                                log::error!(
                                    "Python unavailable while scheduling hosted run wake-up, stopping node"
                                );
                                handle.stop();
                                break;
                            };

                            if let Err(e) = result {
                                log::error!(
                                    "Failed to schedule hosted run wake-up, stopping node: {e}"
                                );
                                handle.stop();
                                break;
                            }
                        }
                        HostWakeSignal::Shutdown => break,
                    }
                }

                control_for_thread.active.store(false, Ordering::Release);
            })
            .map_err(|e| to_pyruntime_err(format!("failed to start hosted run wake pump: {e}")))?;

        Ok(Self {
            control,
            thread: Some(thread),
        })
    }

    fn waker(&self, generation: u64) -> Waker {
        Waker::from(Arc::new(HostLoopWaker {
            generation,
            scheduled: AtomicBool::new(false),
            control: self.control.clone(),
        }))
    }

    fn close(&self) {
        self.control.close();
    }

    fn join(&mut self, py: Option<Python<'_>>) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        let mut thread = Some(thread);

        let result = match py {
            Some(py) => py.detach(|| thread.take().expect("thread presence checked").join()),
            None => Python::try_attach(|py| {
                py.detach(|| thread.take().expect("thread presence checked").join())
            })
            .unwrap_or_else(|| thread.take().expect("thread presence checked").join()),
        };

        if result.is_err() {
            log::error!("Hosted run wake pump panicked while stopping");
        }
    }
}

impl Drop for HostWakePump {
    fn drop(&mut self) {
        self.close();
        self.join(None);
    }
}

struct SendPtr<T>(*mut T);

// SAFETY: the owner of a `SendPtr` holds the only reference to the pointee for the pointer's
// lifetime, and the pointee outlives every use.
#[allow(unsafe_code)]
unsafe impl<T> Send for SendPtr<T> {}

/// Awaitable driving a [`LiveNode`] on the host's asyncio event loop.
///
/// Polls the node's run future from loop callbacks, so the node shares the host's thread and never
/// blocks it. Awaiting resolves once the node has fully stopped.
#[pyo3::pyclass(name = "NodeRun", unsendable)]
pub struct PyNodeRun {
    // Declared before `node` so the future is dropped first; it borrows the node.
    future: Option<Pin<Box<dyn Future<Output = anyhow::Result<()>>>>>,
    node: Option<Box<LiveNode>>,
    owner: Rc<RefCell<Option<LiveNode>>>,
    handle: LiveNodeHandle,
    event_loop: Py<PyAny>,
    wake_pump: HostWakePump,
    state: Arc<RunWakeState>,
    generation: u64,
    pending_throw: Option<PyErr>,
}

#[allow(
    clippy::missing_fields_in_debug,
    reason = "the future and node fields have no useful debug representation"
)]
impl Debug for PyNodeRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyNodeRun))
            .field("state", &self.handle.state())
            .field("completed", &self.future.is_none())
            .finish()
    }
}

impl Drop for PyNodeRun {
    fn drop(&mut self) {
        self.restore_node(None);
    }
}

impl PyNodeRun {
    #[allow(
        unsafe_code,
        reason = "the run future borrows the boxed node this type owns"
    )]
    fn new(
        node: LiveNode,
        owner: Rc<RefCell<Option<LiveNode>>>,
        event_loop: Bound<'_, PyAny>,
        state: Arc<RunWakeState>,
        wake_pump: HostWakePump,
    ) -> Self {
        let handle = node.handle();
        let mut node = Box::new(node);
        let node_ptr = SendPtr(std::ptr::from_mut::<LiveNode>(node.as_mut()));

        let future: Pin<Box<dyn Future<Output = anyhow::Result<()>>>> = Box::pin(async move {
            let ptr = node_ptr;
            // SAFETY: the node is boxed, so its address is stable for as long as this type owns
            // it. `run_async` moved the node out of the wrapper, so no other reference exists,
            // and `future` is declared before `node` so it is dropped first.
            unsafe { (*ptr.0).run_with_mode(NodeRunMode::Hosted).await }
        });

        Self {
            future: Some(future),
            node: Some(node),
            owner,
            handle,
            event_loop: event_loop.unbind(),
            wake_pump,
            state,
            generation: 0,
            pending_throw: None,
        }
    }

    /// Returns the node to the wrapper, dropping the future that borrows it first.
    ///
    /// Releases the per-thread run guard here rather than only on drop, so a completed run does
    /// not block the next one until the coroutine object happens to be collected.
    fn restore_node(&mut self, py: Option<Python<'_>>) {
        self.state.close();
        self.wake_pump.close();
        self.future = None;
        self.wake_pump.join(py);

        if let Some(node) = self.node.take() {
            *self.owner.borrow_mut() = Some(*node);

            // Release inside the take arm so this runs exactly once, and only for the run that
            // set the guard. Clearing unconditionally would let a late drop release a newer run's.
            HOSTED_RUN_ACTIVE.set(false);
        }
    }

    /// Returns whether the host event loop is currently running.
    ///
    /// Treats an unavailable answer as running, because blocking a live loop is the worse error.
    fn host_loop_is_running(&self, py: Python<'_>) -> bool {
        self.event_loop
            .bind(py)
            .call_method0(intern!(py, "is_running"))
            .and_then(|running| running.extract::<bool>())
            .unwrap_or(true)
    }

    /// Drives the run to completion on this thread, bounded by [`CLOSE_DRIVE_TIMEOUT`].
    fn drive_to_completion(&mut self, py: Python<'_>) {
        let signal = Arc::new(BlockingWake::default());
        let waker = Waker::from(signal.clone());
        let deadline = Instant::now() + CLOSE_DRIVE_TIMEOUT;

        loop {
            let Some(future) = self.future.as_mut() else {
                return;
            };

            let poll = {
                let _guard = get_runtime().enter();
                future.as_mut().poll(&mut Context::from_waker(&waker))
            };

            if let Poll::Ready(result) = poll {
                if let Err(e) = result {
                    log::error!("Hosted run failed during inline shutdown: {e}");
                }

                self.restore_node(Some(py));
                return;
            }

            // Release the GIL so tokio workers that need it can make progress.
            if !py.detach(|| signal.wait_until(deadline)) {
                log::error!(
                    "Hosted run did not stop within {}s of the host loop closing; abandoning it \
                     with resources still held",
                    CLOSE_DRIVE_TIMEOUT.as_secs()
                );
                return;
            }
        }
    }

    /// Polls the run future once, returning the asyncio future to suspend on when still running.
    fn step(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let Some(future) = self.future.as_mut() else {
            return Err(to_pyruntime_err("Hosted run has already completed"));
        };

        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| to_pyruntime_err("Hosted run wake generation exhausted"))?;
        self.generation = generation;

        // A distinct waker prevents a late wake from an older poll resuming this suspension
        let waker = self.wake_pump.waker(generation);

        // Timers and the reactor are only reachable inside the runtime context
        let poll = {
            let _guard = get_runtime().enter();
            future.as_mut().poll(&mut Context::from_waker(&waker))
        };

        match poll {
            Poll::Ready(result) => {
                self.restore_node(Some(py));

                if let Err(e) = &result {
                    log::error!("Hosted run failed: {e}");
                }

                if let Some(raised) = self.pending_throw.take() {
                    // Shutdown finished, so the injected exception is now honoured. Reporting
                    // success here would break `asyncio.timeout`, `wait_for`, and task groups.
                    return Err(raised);
                }

                result.map(|()| None).map_err(to_pyruntime_err)
            }
            Poll::Pending => {
                let suspended = self
                    .event_loop
                    .bind(py)
                    .call_method0(intern!(py, "create_future"))?;
                suspended.setattr(intern!(py, "_asyncio_future_blocking"), true)?;

                // `call_soon_threadsafe` queues the callback, so this publishes before it can run
                self.state.suspend(generation, suspended.clone().unbind());

                Ok(Some(suspended.unbind()))
            }
        }
    }
}

#[pymethods]
impl PyNodeRun {
    fn __await__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.step(py)
    }

    #[pyo3(signature = (value=None))]
    fn send(
        &mut self,
        py: Python<'_>,
        value: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Option<Py<PyAny>>> {
        let _ = value;
        self.step(py)
    }

    /// Converts an injected exception into a graceful stop, then re-raises once the node stopped.
    ///
    /// Later throws are absorbed while shutdown runs, so a half-stopped node is never abandoned
    /// with clients connected and channels undrained. The first exception is the one raised.
    #[pyo3(signature = (*args))]
    fn throw(&mut self, py: Python<'_>, args: &Bound<'_, PyTuple>) -> PyResult<Option<Py<PyAny>>> {
        let raised = args.get_item(0)?;

        if self.pending_throw.is_none() {
            let cancelled_type = cancelled_error_type(py)?;
            let is_cancelled = raised.is_instance(&cancelled_type)? || raised.is(&cancelled_type);
            if is_cancelled {
                log::info!("Hosted run cancelled, requesting graceful shutdown");
            } else {
                log::warn!("Exception thrown into hosted run, requesting graceful shutdown");
            }

            self.pending_throw = Some(PyErr::from_value(raised));
            self.handle.stop();
        }

        self.step(py)
    }

    /// Stops the node, and drives shutdown inline only when no host loop can finish it.
    ///
    /// Python calls this when the awaiting task is discarded. With the loop still running, this
    /// requests the stop and returns, because driving inline would block every host callback; the
    /// run is then dropped with shutdown incomplete, which the warning names. With no running loop
    /// nothing else can finish the shutdown, so it is driven here rather than dropping the node
    /// with venue connections still open.
    fn close(&mut self, py: Python<'_>) {
        if self.future.is_none() {
            return;
        }

        self.handle.stop();

        // Driving inline blocks this thread, so only do it when the host loop is not running and
        // therefore cannot finish the shutdown itself.
        if self.host_loop_is_running(py) {
            log::warn!(
                "Hosted run discarded while its event loop is running; shutdown was requested but \
                 not completed, await the run instead of discarding it"
            );
            return;
        }

        log::warn!("Hosted run closed with no running event loop, draining shutdown inline");
        self.drive_to_completion(py);
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

/// Returns the driver that wraps a [`PyNodeRun`] in a native coroutine.
///
/// `asyncio.create_task` accepts only coroutines, so returning the awaitable directly would force
/// every caller to wrap it. Awaiting the coroutine drives the same object, and a cancellation
/// delivered to the task reaches [`PyNodeRun::throw`] unchanged.
fn node_run_driver(py: Python<'_>) -> PyResult<&Py<PyAny>> {
    static DRIVER: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

    DRIVER.get_or_try_init(py, || {
        let module = PyModule::from_code(
            py,
            c_str!("async def drive(run):\n    return await run\n"),
            c_str!("nautilus_trader/live/_hosted_run.py"),
            c_str!("nautilus_trader._hosted_run"),
        )?;
        Ok(module.getattr("drive")?.unbind())
    })
}

/// Returns the `asyncio.CancelledError` type.
fn cancelled_error_type(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    py.import("asyncio")?.getattr(intern!(py, "CancelledError"))
}

/// Reports a borrow that cannot be served because the node is busy inside a call.
fn node_busy_err() -> PyErr {
    to_pyruntime_err(
        "LiveNode is busy servicing another call; do not re-enter the node from a component \
         callback, use the cache, portfolio, and handle captured beforehand",
    )
}

fn node_consumed_err() -> PyErr {
    to_pyruntime_err(
        "LiveNode is being run by `run_async`; use the handle returned by `handle()` to stop it, \
         and the `cache` and `portfolio` captured before the run to read state",
    )
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyLiveNode {
    /// Creates a new `LiveNode` directly from a kernel name and optional configuration.
    ///
    /// This is a convenience method for creating a live node with a pre-configured
    /// kernel configuration, bypassing the builder pattern. If no config is provided,
    /// a default configuration will be used.
    ///
    /// # Errors
    ///
    /// Returns an error if kernel construction fails.
    #[staticmethod]
    #[pyo3(name = "build")]
    #[pyo3(signature = (name, config=None))]
    fn py_build(name: String, config: Option<LiveNodeConfig>) -> PyResult<Self> {
        LiveNode::build(name, config)
            .map(Self::new)
            .map_err(to_pyruntime_err)
    }

    /// Creates a new `LiveNodeBuilder` for fluent configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment is invalid for live trading.
    #[staticmethod]
    #[pyo3(name = "builder")]
    fn py_builder(
        name: String,
        trader_id: TraderId,
        environment: Environment,
    ) -> PyResult<PyLiveNodeBuilder> {
        match LiveNode::builder(trader_id, environment) {
            Ok(builder) => Ok(PyLiveNodeBuilder {
                state: Rc::new(Cell::new(PyLiveNodeBuilderState::Ready(Box::new(
                    builder.with_name(name),
                )))),
            }),
            Err(e) => Err(to_pyruntime_err(e)),
        }
    }

    /// Gets the node's environment.
    #[getter]
    #[pyo3(name = "environment")]
    fn py_environment(&self) -> PyResult<Environment> {
        Ok(self.node()?.environment())
    }

    /// Gets the node's trader ID.
    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> PyResult<TraderId> {
        Ok(self.node()?.trader_id())
    }

    /// Gets the node's instance ID.
    #[getter]
    #[pyo3(name = "instance_id")]
    fn py_instance_id(&self) -> PyResult<UUID4> {
        Ok(self.node()?.instance_id())
    }

    /// Checks if the live node is currently running.
    ///
    /// Answered from the handle, so this stays truthful while a hosted run holds the node.
    #[getter]
    #[pyo3(name = "is_running")]
    fn py_is_running(&self) -> bool {
        self.handle.is_running()
    }

    /// Returns the cache shared with the kernel and registered components.
    #[getter]
    #[pyo3(name = "cache")]
    fn py_cache(&self) -> PyResult<PyCache> {
        Ok(PyCache::from_rc(self.node()?.kernel().cache()))
    }

    /// Returns the portfolio shared with the kernel and registered components.
    #[getter]
    #[pyo3(name = "portfolio")]
    fn py_portfolio(&self) -> PyResult<PyPortfolio> {
        Ok(PyPortfolio::from_rc(
            self.node()?.kernel().portfolio.clone(),
        ))
    }

    /// Returns a thread-safe handle for controlling and observing this node.
    ///
    /// The handle stays valid for the node's whole lifetime, including while `run_async` owns the
    /// node, and is the supported way to stop a hosted run.
    #[pyo3(name = "handle")]
    fn py_handle(&self) -> PyLiveNodeHandle {
        PyLiveNodeHandle {
            inner: self.handle.clone(),
        }
    }

    /// Runs the live node on the caller's asyncio event loop.
    ///
    /// Takes the node and returns an awaitable that resolves once the node has stopped. The host
    /// owns the loop and its signal handling, so this installs no signal handlers. Stop the node
    /// through the handle from `handle()`; cancelling the awaiting task requests the same graceful
    /// shutdown, waits for it to finish, then re-raises the cancellation.
    ///
    /// Capture `cache`, `portfolio`, and `handle()` before calling this. They stay usable while the
    /// node runs, whereas the node itself is owned by the returned awaitable.
    ///
    /// # Limitations
    ///
    /// A node configured with a cache database backing is rejected. Those backings wait for their
    /// worker task by blocking the calling thread, which stalls the host loop rather than merely
    /// slowing it. Use `run()` for a database-backed node until the backings can be driven without
    /// blocking a host loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the node has already been taken, no event loop is running, a cache
    /// database backing is configured, or another node is already running on this loop.
    #[gen_stub(override_return_type(
        type_repr = "collections.abc.Coroutine[typing.Any, typing.Any, None]",
        imports = ("collections.abc", "typing")
    ))]
    #[pyo3(name = "run_async")]
    fn py_run_async(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let event_loop = py
            .import("asyncio")?
            .call_method0("get_running_loop")
            .map_err(|_| {
                to_pyruntime_err(
                    "`run_async` requires a running asyncio event loop; use `run` to have the node \
                     own the thread instead",
                )
            })?;

        // A node runs once. Rejecting anything past `Idle` here turns a second run into an
        // immediate error rather than a coroutine that fails only when awaited.
        let state = self.node()?.state();
        if state != NodeState::Idle {
            return Err(to_pyruntime_err(format!(
                "LiveNode cannot be run from state {state:?}; build a new node to run again"
            )));
        }

        // The Redis and SQL cache backings block the calling thread while awaiting their worker
        // task, which would freeze the host loop inside a poll rather than merely slow it down.
        // Fail before taking the node so the caller gets a clear boundary instead of a hang.
        if self.node()?.has_pending_cache_database() {
            return Err(to_pyruntime_err(
                "a cache database backing is not supported on a host event loop, because its \
                 blocking calls would stall the loop; use `run()` to have the node own the thread, \
                 or configure the node without a cache database",
            ));
        }

        if HOSTED_RUN_ACTIVE.get() {
            return Err(to_pyruntime_err(
                "another LiveNode is already running on this event loop; run one concurrent \
                 LiveNode per process, or run additional nodes in separate processes",
            ));
        }

        // Everything fallible happens before the node is taken, so a failure here cannot strand
        // it. `PyNodeRun` restores the node on drop, covering the paths after this point.
        let driver = node_run_driver(py)?.clone_ref(py);
        let state = Arc::new(RunWakeState::default());

        let wake_callback = Py::new(
            py,
            PyNodeRunWake {
                state: state.clone(),
            },
        )?
        .into_any();

        let wake_pump = HostWakePump::start(
            event_loop.clone().unbind(),
            wake_callback,
            self.handle.clone(),
        )?;

        let node = self
            .inner
            .borrow_mut()
            .take()
            .ok_or_else(node_consumed_err)?;
        let run = PyNodeRun::new(node, self.inner.clone(), event_loop, state, wake_pump);
        HOSTED_RUN_ACTIVE.set(true);

        driver.call1(py, (Py::new(py, run)?,))
    }

    /// Run the live node with automatic shutdown handling.
    ///
    /// This method starts the node, runs indefinitely, and handles graceful shutdown
    /// on interrupt signals.
    ///
    /// # Thread Safety
    ///
    /// The event loop runs directly on the current thread (not spawned) because the
    /// msgbus uses thread-local storage. Endpoints registered by the kernel are only
    /// accessible from the same thread.
    ///
    /// # Shutdown Sequence
    ///
    /// 1. Signal received (SIGINT, SIGTERM, or handle stop).
    /// 2. Trader components stopped (triggers order cancellations, etc.).
    /// 3. Event loop continues processing residual events for the configured grace period.
    /// 4. Kernel finalized, clients disconnected, remaining events drained.
    ///
    /// # Errors
    ///
    /// Returns an error if the node fails to start or encounters a runtime error.
    #[pyo3(name = "run")]
    fn py_run(&self, py: Python) -> PyResult<()> {
        if self.node()?.is_running() {
            return Err(to_pyruntime_err("LiveNode is already running"));
        }

        // Get a handle for coordinating with the signal checker
        let handle = self.node()?.handle();

        // Import signal module
        let signal_module = py.import("signal")?;
        let original_handler =
            signal_module.call_method1("signal", (2, signal_module.getattr("SIG_DFL")?))?; // Save original SIGINT handler (signal 2)

        // Set up a custom signal handler that uses our handle
        let handle_for_signal = handle;
        let signal_callback = new_sync_py_callback(
            py,
            move |_args: &pyo3::Bound<'_, PyTuple>,
                  _kwargs: Option<&pyo3::Bound<'_, PyDict>>|
                  -> PyResult<()> {
                log::info!("Python signal handler called");
                handle_for_signal.stop();
                Ok(())
            },
        )?;

        // Install our signal handler
        signal_module.call_method1("signal", (2, signal_callback))?;

        // Run the node and restore signal handler afterward
        let mut node = self.node_mut()?;
        let result = run_live_node_detached(py, &mut node);

        // Restore original signal handler
        signal_module.call_method1("signal", (2, original_handler))?;

        result
    }

    /// Stop the live node.
    ///
    /// This method stops the trader, waits for the configured grace period to allow
    /// residual events to be processed, then finalizes the shutdown sequence.
    ///
    /// # Errors
    ///
    /// Returns an error if shutdown fails.
    #[pyo3(name = "stop")]
    fn py_stop(&self, py: Python<'_>) -> PyResult<()> {
        let mut node = self.node_mut()?;
        if !node.is_running() {
            return Err(to_pyruntime_err("LiveNode is not running"));
        }

        stop_live_node_detached(py, &mut node)
    }

    /// Disposes the live node kernel and releases resources.
    ///
    /// Does nothing while a hosted run holds the node. The run returns the node when it finishes,
    /// so a `try`/`finally` cleanup that outlives the run disposes it as usual.
    #[pyo3(name = "dispose")]
    fn py_dispose(&self, py: Python<'_>) -> PyResult<()> {
        if self.is_consumed() {
            return Ok(());
        }

        let mut node = self.node_mut()?;
        let stop_result = if node.is_running() {
            stop_live_node_detached(py, &mut node)
        } else {
            Ok(())
        };

        if let Err(ref err) = stop_result {
            log::error!("Failed to stop LiveNode during dispose: {err}");
        }

        node.dispose();
        stop_result
    }

    /// Adds a constructed Python actor to the trader.
    ///
    /// # Errors
    ///
    /// Returns an error if the node is running, the actor is invalid, or registration fails.
    #[pyo3(name = "add_actor")]
    fn py_add_actor(&self, actor: &Bound<'_, PyAny>) -> PyResult<()> {
        if self.node()?.state() != NodeState::Idle {
            return Err(to_pyruntime_err(
                "Cannot add actor while node is running, add actors before running the node",
            ));
        }

        log::debug!("`add_actor` with a constructed instance");

        let actor = actor.clone().unbind();
        let actor_id = Python::attach(|py| {
            let actor = actor.bind(py);
            let config = actor
                .getattr("config")
                .ok()
                .filter(|config| !config.is_none());
            prepare_python_actor(actor, config.as_ref())
        })
        .map_err(to_pyruntime_err)?;

        self.register_python_actor(&actor, actor_id)
    }

    #[pyo3(name = "add_actor_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_actor_from_config(&self, _py: Python, config: ImportableActorConfig) -> PyResult<()> {
        log::debug!("`add_actor_from_config` with: {config:?}");

        // Extract module and class name from actor_path
        let parts: Vec<&str> = config.actor_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "actor_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing actor from module: {module_name} class: {class_name}");

        let (python_actor, actor_id) =
            Python::attach(|py| -> anyhow::Result<(Py<PyAny>, ActorId)> {
                let actor_module = py
                    .import(module_name)
                    .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
                let actor_class = actor_module
                    .getattr(class_name)
                    .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;

                let config_instance =
                    create_config_instance(py, &config.config_path, &config.config)?;

                let python_actor = if let Some(config_obj) = config_instance.as_ref() {
                    actor_class.call1((config_obj,))?
                } else {
                    actor_class.call0()?
                };

                log::debug!("Created Python actor instance: {python_actor:?}");

                let actor_id = prepare_python_actor(&python_actor, config_instance.as_ref())?;

                Ok((python_actor.unbind(), actor_id))
            })
            .map_err(to_pyruntime_err)?;

        self.register_python_actor(&python_actor, actor_id)
    }

    /// Adds a strategy to the trader.
    ///
    /// Strategies are registered in both the component registry (for lifecycle management)
    /// and the actor registry (for data callbacks via msgbus).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The node is currently running.
    /// - A strategy with the same ID is already registered.
    /// - The configured external order instrument IDs repeat an instrument or the cache already
    ///   contains a requested claim.
    /// - The strategy configures one or more external order instrument IDs and the cache is already
    ///   borrowed.
    /// - The strategy configures an OMS type override and the execution engine is already borrowed.
    #[allow(
        unsafe_code,
        reason = "Required for Python strategy component registration"
    )]
    #[pyo3(name = "add_strategy")]
    fn py_add_strategy(&self, strategy: &Bound<'_, PyAny>) -> PyResult<()> {
        if self.node()?.state() != NodeState::Idle {
            return Err(to_pyruntime_err(
                "Cannot add strategy while node is running, add strategies before running the node",
            ));
        }

        log::debug!("`add_strategy` with a constructed instance");

        let strategy = strategy.clone().unbind();

        let strategy_id = self
            .node_mut()?
            .kernel_mut()
            .trader
            .borrow_mut()
            .prepare_python_strategy_instance(&strategy)
            .map_err(to_pyruntime_err)?;

        let (external_order_instrument_ids, oms_type) = Python::attach(
            |py| -> anyhow::Result<(Option<Vec<InstrumentId>>, Option<OmsType>)> {
                let bound = strategy.bind(py);
                let config_obj = bound
                    .getattr("config")
                    .ok()
                    .filter(|config| !config.is_none());

                let mut py_strategy_ref = bound
                    .extract::<PyRefMut<PyStrategy>>()
                    .map_err(Into::<PyErr>::into)
                    .map_err(|e| anyhow::anyhow!("Failed to extract PyStrategy: {e}"))?;

                if let Some(config_obj) = config_obj.as_ref()
                    && let Some(claims) =
                        extract_external_order_instrument_ids_config_attr(config_obj)?
                {
                    py_strategy_ref.set_external_order_instrument_ids(Some(claims));
                }

                let claims = py_strategy_ref.external_order_instrument_ids();
                let oms_type = config_obj
                    .as_ref()
                    .and_then(|cfg| cfg.getattr("oms_type").ok())
                    .filter(|value| !value.is_none())
                    .and_then(|value| value.extract::<OmsType>().ok());

                Ok((claims, oms_type))
            },
        )
        .map_err(to_pyruntime_err)?;

        let external_order_instrument_ids =
            external_order_instrument_ids.filter(|claims| !claims.is_empty());
        if let Some(claims) = &external_order_instrument_ids {
            self.node_mut()?
                .register_external_order_claims(strategy_id, claims)
                .map_err(to_pyruntime_err)?;
        }

        let commit_result = self
            .node_mut()?
            .kernel_mut()
            .trader
            .borrow_mut()
            .commit_python_strategy_instance(&strategy);

        if let Err(commit_error) = commit_result {
            if let Some(instrument_ids) = external_order_instrument_ids.as_deref()
                && let Err(rollback_error) = self
                    .node_mut()?
                    .rollback_external_order_claims(strategy_id, instrument_ids)
            {
                return Err(to_pyruntime_err(format!(
                    "Failed to add strategy {strategy_id}: {commit_error}; failed to roll back external order claims: {rollback_error}"
                )));
            }
            return Err(to_pyruntime_err(commit_error));
        }

        if let Some(oms_type) = oms_type {
            self.node_mut()?
                .kernel()
                .exec_engine
                .borrow_mut()
                .register_oms_type(strategy_id, oms_type);
        }

        log::info!("Registered Python strategy {strategy_id}");
        Ok(())
    }

    #[pyo3(name = "add_strategy_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_strategy_from_config(
        &self,
        _py: Python,
        config: ImportableStrategyConfig,
    ) -> PyResult<()> {
        log::debug!("`add_strategy_from_config` with: {config:?}");

        // Extract module and class name from strategy_path
        let parts: Vec<&str> = config.strategy_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "strategy_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing strategy from module: {module_name} class: {class_name}");

        // Phase 1: Create the Python strategy, then prepare it for registration so the strategy ID,
        // order ID tag, and logging flags are sourced from the same config as the instance path
        let python_strategy = Python::attach(|py| -> anyhow::Result<Py<PyAny>> {
            let strategy_module = py
                .import(module_name)
                .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
            let strategy_class = strategy_module
                .getattr(class_name)
                .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;

            let config_instance = create_config_instance(py, &config.config_path, &config.config)?;

            let python_strategy = if let Some(config_obj) = config_instance {
                strategy_class.call1((config_obj,))?
            } else {
                strategy_class.call0()?
            };

            log::debug!("Created Python strategy instance: {python_strategy:?}");

            Ok(python_strategy.unbind())
        })
        .map_err(to_pyruntime_err)?;

        let strategy_id = self
            .node_mut()?
            .kernel_mut()
            .trader
            .borrow_mut()
            .prepare_python_strategy_instance(&python_strategy)
            .map_err(to_pyruntime_err)?;

        Python::attach(|py| -> anyhow::Result<()> {
            let bound = python_strategy.bind(py);
            let config_obj = bound
                .getattr("config")
                .ok()
                .filter(|config| !config.is_none());

            let mut py_strategy_ref = bound
                .extract::<PyRefMut<PyStrategy>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyStrategy: {e}"))?;

            if let Some(config_obj) = config_obj.as_ref()
                && let Some(claims) = extract_external_order_instrument_ids_config_attr(config_obj)?
            {
                py_strategy_ref.set_external_order_instrument_ids(Some(claims));
            }

            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        // Phase 2: Claim external orders before committing, matching the instance path
        let external_order_instrument_ids =
            Python::attach(|py| -> anyhow::Result<Option<Vec<_>>> {
                let py_strategy = python_strategy.bind(py);
                let py_strategy_ref = py_strategy
                    .extract::<PyRef<PyStrategy>>()
                    .map_err(Into::<PyErr>::into)
                    .map_err(|e| anyhow::anyhow!("Failed to extract PyStrategy: {e}"))?;

                Ok(py_strategy_ref.external_order_instrument_ids())
            })
            .map_err(to_pyruntime_err)?;

        let external_order_instrument_ids =
            external_order_instrument_ids.filter(|claims| !claims.is_empty());
        if let Some(claims) = &external_order_instrument_ids {
            self.node_mut()?
                .register_external_order_claims(strategy_id, claims)
                .map_err(to_pyruntime_err)?;
        }

        // Phase 3: Register the strategy through the trader's single Python registration path
        let commit_result = self
            .node_mut()?
            .kernel_mut()
            .trader
            .borrow_mut()
            .commit_python_strategy_instance(&python_strategy);

        if let Err(commit_error) = commit_result {
            if let Some(instrument_ids) = external_order_instrument_ids.as_deref()
                && let Err(rollback_error) = self
                    .node_mut()?
                    .rollback_external_order_claims(strategy_id, instrument_ids)
            {
                return Err(to_pyruntime_err(format!(
                    "Failed to add strategy {strategy_id}: {commit_error}; failed to roll back external order claims: {rollback_error}"
                )));
            }
            return Err(to_pyruntime_err(commit_error));
        }

        log::info!("Registered Python strategy {strategy_id}");
        Ok(())
    }

    /// Adds an execution algorithm to the trader.
    ///
    /// Execution algorithms are registered in both the component registry (for lifecycle
    /// management) and the actor registry (for data callbacks via msgbus).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The node is currently running.
    /// - An execution algorithm with the same ID is already registered.
    #[pyo3(name = "add_exec_algorithm")]
    fn py_add_exec_algorithm(&self, exec_algorithm: &Bound<'_, PyAny>) -> PyResult<()> {
        if self.node()?.state() != NodeState::Idle {
            return Err(to_pyruntime_err(
                "Cannot add exec algorithm while node is running, add exec algorithms before running the node",
            ));
        }

        log::debug!("`add_exec_algorithm` with a constructed instance");

        let exec_algorithm = exec_algorithm.clone().unbind();
        let py_exec_algorithm = Python::attach(|py| -> anyhow::Result<PyExecutionAlgorithm> {
            let bound = exec_algorithm.bind(py);
            let config = bound
                .getattr("config")
                .ok()
                .filter(|config| !config.is_none());
            let mut py_exec_algorithm_ref = bound
                .extract::<PyRefMut<PyExecutionAlgorithm>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "LiveNode.add_exec_algorithm requires a Python v2 ExecutionAlgorithm instance; use add_exec_algorithm_from_config for DataActor-based algorithms: {e}"
                    )
                })?;

            if let Some(config) = config.as_ref() {
                py_exec_algorithm_ref.configure_from_py_config(config)?;
            }

            py_exec_algorithm_ref.set_python_instance(bound)?;
            Ok(py_exec_algorithm_ref.clone())
        })
        .map_err(to_pyruntime_err)?;

        let exec_algorithm_id = self
            .node_mut()?
            .kernel_mut()
            .trader
            .borrow_mut()
            .add_py_execution_algorithm_instance(py_exec_algorithm, &exec_algorithm)
            .map_err(to_pyruntime_err)?;

        log::info!("Registered Python exec algorithm {exec_algorithm_id}");
        Ok(())
    }

    #[pyo3(name = "add_exec_algorithm_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_exec_algorithm_from_config(
        &self,
        _py: Python,
        config: ImportableExecutionAlgorithmConfig,
    ) -> PyResult<()> {
        if self.node()?.is_running() {
            return Err(to_pyruntime_err(
                "Cannot add exec algorithm while node is running",
            ));
        }

        log::debug!("`add_exec_algorithm_from_config` with: {config:?}");

        let parts: Vec<&str> = config.exec_algorithm_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "exec_algorithm_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing exec algorithm from module: {module_name} class: {class_name}");

        // Phase 1: Create and configure the Python exec algorithm.
        let (python_exec_algorithm, py_execution_algorithm, actor_id) = Python::attach(
            |py| -> anyhow::Result<(Py<PyAny>, Option<PyExecutionAlgorithm>, ActorId)> {
                let algo_module = py
                    .import(module_name)
                    .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
                let algo_class = algo_module
                    .getattr(class_name)
                    .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;

                let config_instance =
                    create_config_instance(py, &config.config_path, &config.config)?;

                let python_exec_algorithm = if let Some(config_obj) = config_instance.clone() {
                    algo_class.call1((config_obj,))?
                } else {
                    algo_class.call0()?
                };

                log::debug!("Created Python exec algorithm instance: {python_exec_algorithm:?}");

                if let Ok(mut py_exec_algorithm_ref) =
                    python_exec_algorithm.extract::<PyRefMut<PyExecutionAlgorithm>>()
                {
                    if let Some(config_obj) = config_instance.as_ref() {
                        py_exec_algorithm_ref.configure_from_py_config(config_obj)?;
                    }

                    py_exec_algorithm_ref.set_python_instance(&python_exec_algorithm)?;
                    let actor_id =
                        ActorId::from(py_exec_algorithm_ref.exec_algorithm_id().inner().as_str());

                    return Ok((
                        python_exec_algorithm.unbind(),
                        Some(py_exec_algorithm_ref.clone()),
                        actor_id,
                    ));
                }

                let mut py_data_actor_ref = python_exec_algorithm
                    .extract::<PyRefMut<PyDataActor>>()
                    .map_err(Into::<PyErr>::into)
                    .map_err(|e| anyhow::anyhow!("Failed to extract PyDataActor: {e}"))?;

                // Extract ID from config: prefer exec_algorithm_id, fall back to actor_id
                if let Some(config_obj) = config_instance.as_ref() {
                    let id_attr = config_obj
                        .getattr("exec_algorithm_id")
                        .ok()
                        .filter(|v| !v.is_none())
                        .or_else(|| config_obj.getattr("actor_id").ok().filter(|v| !v.is_none()));

                    if let Some(id_value) = id_attr {
                        let actor_id_val = if let Ok(eaid) = id_value.extract::<ExecAlgorithmId>() {
                            ActorId::new(eaid.inner().as_str())
                        } else if let Ok(aid) = id_value.extract::<ActorId>() {
                            aid
                        } else if let Ok(aid_str) = id_value.extract::<String>() {
                            ActorId::new_checked(&aid_str)?
                        } else {
                            anyhow::bail!("Invalid `exec_algorithm_id`/`actor_id` type");
                        };
                        py_data_actor_ref.set_actor_id(actor_id_val);
                    }

                    if let Some(val) = extract_bool_config_attr(config_obj, "log_events") {
                        py_data_actor_ref.set_log_events(val);
                    }

                    if let Some(val) = extract_bool_config_attr(config_obj, "log_commands") {
                        py_data_actor_ref.set_log_commands(val);
                    }
                }

                py_data_actor_ref.set_python_instance(&python_exec_algorithm)?;

                let actor_id = py_data_actor_ref.actor_id();

                Ok((python_exec_algorithm.unbind(), None, actor_id))
            },
        )
        .map_err(to_pyruntime_err)?;

        let exec_algorithm_id = if let Some(py_execution_algorithm) = py_execution_algorithm {
            // This branch registered through `LiveNode::add_exec_algorithm` before the trader owned
            // the path, so it keeps that method's stricter state requirement
            if self.node()?.state() != NodeState::Idle {
                return Err(to_pyruntime_err(
                    "Cannot add exec algorithm while node is running, add exec algorithms before running the node",
                ));
            }

            self.node_mut()?
                .kernel_mut()
                .trader
                .borrow_mut()
                .add_py_execution_algorithm_instance(py_execution_algorithm, &python_exec_algorithm)
                .map_err(to_pyruntime_err)?
        } else {
            // Phase 2: Register the DataActor-backed algorithm through the trader's single Python
            // registration path
            self.node_mut()?
                .kernel_mut()
                .trader
                .borrow_mut()
                .add_python_exec_algorithm_instance(&python_exec_algorithm, actor_id)
                .map_err(to_pyruntime_err)?
        };

        log::info!("Registered Python exec algorithm {exec_algorithm_id}");
        Ok(())
    }

    /// Loads and registers one plug-in instance.
    ///
    /// # Errors
    ///
    /// Returns an error because dynamic plug-in hosting lives in the host-side integration.
    #[pyo3(name = "add_plugin", signature = (path, type_name, config=None, sha256=None))]
    fn py_add_plugin(
        &self,
        path: String,
        type_name: String,
        config: Option<HashMap<String, Py<PyAny>>>,
        sha256: Option<String>,
    ) -> PyResult<()> {
        let config = PluginConfig {
            path,
            type_name,
            config: match config {
                Some(config) => coerce_json_config(config)?,
                None => HashMap::new(),
            },
            sha256,
        };

        self.node_mut()?
            .add_plugin(config)
            .map_err(to_pyruntime_err)
    }

    /// Adds a built-in example actor from its type name and config.
    ///
    /// This method exists only to single-source bundled example actor code across
    /// Rust and Python tests/examples. It is not a first-class extension path for
    /// adding native actors.
    #[cfg(feature = "examples")]
    #[pyo3(name = "add_builtin_actor")]
    fn py_add_builtin_actor(&self, type_name: &str, config: &Bound<'_, PyAny>) -> PyResult<()> {
        let register = builtin_actor_register(type_name).ok_or_else(|| {
            to_pytype_err(format!("Unsupported built-in actor type: {type_name}"))
        })?;
        let mut node = self.node_mut()?;
        register(&mut node, config)
    }

    /// Adds a built-in example strategy from its type name and config.
    ///
    /// This method exists only to single-source bundled example strategy code across
    /// Rust and Python tests/examples. It is not a first-class extension path for
    /// adding native strategies.
    #[cfg(feature = "examples")]
    #[pyo3(name = "add_builtin_strategy")]
    fn py_add_builtin_strategy(&self, type_name: &str, config: &Bound<'_, PyAny>) -> PyResult<()> {
        let register = builtin_strategy_register(type_name).ok_or_else(|| {
            to_pytype_err(format!("Unsupported built-in strategy type: {type_name}"))
        })?;
        let mut node = self.node_mut()?;
        register(&mut node, config)
    }

    fn __repr__(&self) -> String {
        format!(
            "LiveNode(trader_id={}, environment={}, running={})",
            self.node()
                .map_or_else(|_| "<running>".to_string(), |n| n.trader_id().to_string()),
            self.node().map_or_else(
                |_| "<running>".to_string(),
                |n| format!("{:?}", n.environment())
            ),
            self.py_is_running()
        )
    }
}

impl PyLiveNode {
    fn register_python_actor(&self, actor: &Py<PyAny>, actor_id: ActorId) -> PyResult<()> {
        if self
            .node()?
            .kernel()
            .trader
            .borrow()
            .actor_ids()
            .contains(&actor_id)
        {
            return Err(to_pyruntime_err(format!(
                "Actor '{actor_id}' is already registered"
            )));
        }

        self.node_mut()?
            .kernel_mut()
            .trader
            .borrow_mut()
            .add_python_actor_instance(actor, actor_id)
            .map_err(to_pyruntime_err)?;

        log::info!("Registered Python actor {actor_id}");
        Ok(())
    }
}

fn new_sync_py_callback<F>(py: Python<'_>, closure: F) -> PyResult<Bound<'_, PyCFunction>>
where
    F: Fn(&Bound<'_, PyTuple>, Option<&Bound<'_, PyDict>>) -> PyResult<()> + Send + Sync + 'static,
{
    PyCFunction::new_closure(py, None, None, closure)
}

#[allow(unsafe_code)]
fn run_live_node_detached(py: Python<'_>, node: &mut LiveNode) -> PyResult<()> {
    let node_ptr = SendPtr(std::ptr::from_mut::<LiveNode>(node));

    // SAFETY: `py_run` holds the only mutable reference to `LiveNode` until
    // `run()` returns, and the detached closure completes before `py_run` can
    // access `node` again.
    unsafe {
        py.detach(move || {
            let ptr = node_ptr;
            get_runtime().block_on(async { (*ptr.0).run().await })
        })
    }
    .map_err(to_pyruntime_err)
}

#[allow(unsafe_code)]
fn stop_live_node_detached(py: Python<'_>, node: &mut LiveNode) -> PyResult<()> {
    let node_ptr = SendPtr(std::ptr::from_mut::<LiveNode>(node));

    // SAFETY: the Python binding holds the only mutable reference to `LiveNode`
    // until `stop()` returns, and the detached closure completes before the
    // caller can access `node` again.
    unsafe {
        py.detach(move || {
            let ptr = node_ptr;
            get_runtime().block_on(async { (*ptr.0).stop().await })
        })
    }
    .map_err(to_pyruntime_err)
}

/// Creates a Python config instance from a config path and config dictionary.
///
/// This helper is shared between `add_actor_from_config` and `add_strategy_from_config`.
/// It handles:
/// 1. Importing the config class from the module path
/// 2. Converting the `HashMap<String, serde_json::Value>` to a Python dict
/// 3. Trying kwargs-first construction, falling back to default + setattr
/// 4. Calling `__post_init__` for dataclasses when using the setattr path
fn create_config_instance<'py>(
    py: Python<'py>,
    config_path: &str,
    config: &HashMap<String, serde_json::Value>,
) -> anyhow::Result<Option<Bound<'py, PyAny>>> {
    if config_path.is_empty() && config.is_empty() {
        log::debug!("No config_path or empty config, using None");
        return Ok(None);
    }

    let config_parts: Vec<&str> = config_path.split(':').collect();
    if config_parts.len() != 2 {
        anyhow::bail!("config_path must be in format 'module.path:ClassName', was {config_path}");
    }
    let (config_module_name, config_class_name) = (config_parts[0], config_parts[1]);

    log::debug!(
        "Importing config class from module: {config_module_name} class: {config_class_name}"
    );

    let config_module = py
        .import(config_module_name)
        .map_err(|e| anyhow::anyhow!("Failed to import config module {config_module_name}: {e}"))?;
    let config_class = config_module
        .getattr(config_class_name)
        .map_err(|e| anyhow::anyhow!("Failed to get config class {config_class_name}: {e}"))?;

    // Convert config dict to Python dict
    let py_dict = PyDict::new(py);

    for (key, value) in config {
        let py_value = config_value_to_py(py, key, value)?;
        py_dict.set_item(key, py_value)?;
    }

    log::debug!("Created config dict: {py_dict:?}");

    // Try kwargs first, then default constructor with setattr
    let config_instance = match config_class.call((), Some(&py_dict)) {
        Ok(instance) => {
            log::debug!("Created config instance with kwargs");
            instance
        }
        Err(kwargs_err) => {
            log::debug!("Failed to create config with kwargs: {kwargs_err}");

            match config_class.call0() {
                Ok(instance) => {
                    log::debug!("Created default config instance, setting attributes");
                    for (key, value) in config {
                        let py_value = config_value_to_py(py, key, value)?;

                        if let Err(setattr_err) = instance.setattr(key, py_value) {
                            log::warn!("Failed to set attribute {key}: {setattr_err}");
                        }
                    }

                    // Only call __post_init__ if it exists (setattr path
                    // needs it, kwargs path already triggered it via __init__)
                    if instance.hasattr("__post_init__")? {
                        instance.call_method0("__post_init__")?;
                    }

                    instance
                }
                Err(default_err) => {
                    anyhow::bail!(
                        "Failed to create config instance. \
                         Tried kwargs: {kwargs_err}, default: {default_err}"
                    );
                }
            }
        }
    };

    log::debug!("Created config instance: {config_instance:?}");

    Ok(Some(config_instance))
}

fn config_value_to_py<'py>(
    py: Python<'py>,
    key: &str,
    value: &serde_json::Value,
) -> anyhow::Result<Bound<'py, PyAny>> {
    if key == "actor_id"
        && let Some(actor_id) = value.as_str()
    {
        return Ok(ActorId::new_checked(actor_id)?
            .into_pyobject(py)?
            .into_any());
    }

    let json_str = serde_json::to_string(value)
        .map_err(|e| anyhow::anyhow!("Failed to serialize config value: {e}"))?;
    Ok(PyModule::import(py, "json")?
        .call_method("loads", (json_str,), None)?
        .into_any())
}

/// Extracts an optional boolean attribute from a Python config object.
///
/// Returns `None` if the attribute doesn't exist or isn't a bool,
/// without raising an error (config fields are optional overrides).
fn extract_bool_config_attr(config_obj: &Bound<'_, PyAny>, attr: &str) -> Option<bool> {
    config_obj
        .getattr(attr)
        .ok()
        .and_then(|val| val.extract::<bool>().ok())
}

fn extract_external_order_instrument_ids_config_attr(
    config_obj: &Bound<'_, PyAny>,
) -> anyhow::Result<Option<Vec<InstrumentId>>> {
    let Ok(claims) = config_obj.getattr("external_order_instrument_ids") else {
        return Ok(None);
    };

    if claims.is_none() {
        return Ok(None);
    }

    if let Ok(claims) = claims.extract::<Vec<InstrumentId>>() {
        return Ok(Some(claims));
    }

    let claim_strings = claims
        .extract::<Vec<String>>()
        .map_err(|e| anyhow::anyhow!("Invalid `external_order_instrument_ids` type: {e}"))?;
    let claims = claim_strings
        .into_iter()
        .map(|claim| {
            InstrumentId::from_str(&claim).map_err(|e| {
                anyhow::anyhow!(
                    "Invalid `external_order_instrument_ids` instrument ID {claim}: {e}"
                )
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(Some(claims))
}

#[cfg(feature = "examples")]
type BuiltinActorRegister = for<'py> fn(&mut LiveNode, &Bound<'py, PyAny>) -> PyResult<()>;

#[cfg(feature = "examples")]
type BuiltinStrategyRegister = for<'py> fn(&mut LiveNode, &Bound<'py, PyAny>) -> PyResult<()>;

#[cfg(feature = "examples")]
fn builtin_actor_register(type_name: &str) -> Option<BuiltinActorRegister> {
    match type_name {
        "BookImbalanceActor" => Some(register_book_imbalance_actor),
        "DataTester" => Some(register_data_tester),
        _ => None,
    }
}

#[cfg(feature = "examples")]
fn builtin_strategy_register(type_name: &str) -> Option<BuiltinStrategyRegister> {
    match type_name {
        "CompositeMarketMaker" => Some(register_composite_market_maker),
        "DeltaNeutralVol" => Some(register_delta_neutral_vol),
        "EmaCross" => Some(register_ema_cross),
        "ExecTester" => Some(register_exec_tester),
        "GridMarketMaker" => Some(register_grid_market_maker),
        "HurstVpinDirectional" => Some(register_hurst_vpin_directional),
        _ => None,
    }
}

#[cfg(feature = "examples")]
fn register_composite_market_maker(node: &mut LiveNode, config: &Bound<'_, PyAny>) -> PyResult<()> {
    let config = config.extract::<CompositeMarketMakerConfig>()?;
    node.add_strategy(CompositeMarketMaker::new(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_delta_neutral_vol(node: &mut LiveNode, config: &Bound<'_, PyAny>) -> PyResult<()> {
    let config = config.extract::<DeltaNeutralVolConfig>()?;
    node.add_strategy(DeltaNeutralVol::new(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_ema_cross(node: &mut LiveNode, config: &Bound<'_, PyAny>) -> PyResult<()> {
    let config = config.extract::<EmaCrossConfig>()?;
    node.add_strategy(EmaCross::from_config(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_exec_tester(node: &mut LiveNode, config: &Bound<'_, PyAny>) -> PyResult<()> {
    let config = config.extract::<ExecTesterConfig>()?;
    node.add_strategy(ExecTester::new(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_grid_market_maker(node: &mut LiveNode, config: &Bound<'_, PyAny>) -> PyResult<()> {
    let config = config.extract::<GridMarketMakerConfig>()?;
    node.add_strategy(GridMarketMaker::new(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_hurst_vpin_directional(node: &mut LiveNode, config: &Bound<'_, PyAny>) -> PyResult<()> {
    let config = config.extract::<HurstVpinDirectionalConfig>()?;
    node.add_strategy(HurstVpinDirectional::new(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_book_imbalance_actor(node: &mut LiveNode, config: &Bound<'_, PyAny>) -> PyResult<()> {
    let config = config.extract::<BookImbalanceActorConfig>()?;
    node.add_actor(BookImbalanceActor::from_config(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_data_tester(node: &mut LiveNode, config: &Bound<'_, PyAny>) -> PyResult<()> {
    let config = config.extract::<DataTesterConfig>()?;
    node.add_actor(DataTester::new(config))
        .map_err(to_pyruntime_err)
}

/// Python wrapper for `LiveNodeBuilder` that uses interior mutability
/// to work around PyO3's shared ownership model.
#[pyclass(name = "LiveNodeBuilder", module = "nautilus_trader.live", unsendable)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
pub struct PyLiveNodeBuilder {
    state: Rc<Cell<PyLiveNodeBuilderState>>,
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyLiveNodeBuilder {
    #[pyo3(name = "with_instance_id")]
    fn py_with_instance_id(&self, instance_id: UUID4) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_instance_id(instance_id))
    }

    #[pyo3(name = "with_load_state")]
    fn py_with_load_state(&self, load_state: bool) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_load_state(load_state))
    }

    #[pyo3(name = "with_save_state")]
    fn py_with_save_state(&self, save_state: bool) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_save_state(save_state))
    }

    #[pyo3(name = "with_timeout_connection")]
    fn py_with_timeout_connection(&self, timeout_secs: u64) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_timeout_connection(timeout_secs))
    }

    #[pyo3(name = "with_timeout_reconciliation")]
    fn py_with_timeout_reconciliation(&self, timeout_secs: u64) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_timeout_reconciliation(timeout_secs))
    }

    #[pyo3(name = "with_timeout_portfolio")]
    fn py_with_timeout_portfolio(&self, timeout_secs: u64) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_timeout_portfolio(timeout_secs))
    }

    #[pyo3(name = "with_timeout_disconnection_secs")]
    fn py_with_timeout_disconnection_secs(&self, timeout_secs: u64) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_timeout_disconnection_secs(timeout_secs))
    }

    #[pyo3(name = "with_delay_post_stop_secs")]
    fn py_with_delay_post_stop_secs(&self, delay_secs: u64) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_delay_post_stop_secs(delay_secs))
    }

    #[pyo3(name = "with_delay_shutdown_secs")]
    fn py_with_delay_shutdown_secs(&self, delay_secs: u64) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_delay_shutdown_secs(delay_secs))
    }

    #[pyo3(name = "with_reconciliation")]
    fn py_with_reconciliation(&self, reconciliation: bool) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_reconciliation(reconciliation))
    }

    #[pyo3(name = "with_controller")]
    fn py_with_controller(&self, controller: ImportableControllerConfig) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_controller(controller))
    }

    #[pyo3(name = "with_reconciliation_lookback_mins")]
    fn py_with_reconciliation_lookback_mins(&self, mins: u32) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_reconciliation_lookback_mins(mins))
    }

    #[pyo3(name = "with_cache_config")]
    fn py_with_cache_config(&self, config: CacheConfig) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_cache_config(config))
    }

    #[pyo3(name = "with_cache_database_factory")]
    fn py_with_cache_database_factory(&self, factory: Py<PyAny>) -> PyResult<Self> {
        let mut operation = self.begin_operation()?;
        let factory =
            Python::attach(|py| get_global_cache_database_factory_registry().extract(py, factory))?;
        let builder = operation.take_builder()?;
        operation.complete(builder.with_cache_database_factory(factory));
        Ok(self.shared())
    }

    #[pyo3(name = "with_msgbus_config")]
    fn py_with_msgbus_config(&self, config: MessageBusConfig) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_msgbus_config(config))
    }

    #[pyo3(name = "with_external_msgbus_factory")]
    fn py_with_external_msgbus_factory(&self, factory: Py<PyAny>) -> PyResult<Self> {
        let mut operation = self.begin_operation()?;
        let factory =
            Python::attach(|py| get_global_msgbus_factory_registry().extract(py, factory))?;
        let builder = operation.take_builder()?;
        operation.complete(builder.with_external_msgbus_factory(factory));
        Ok(self.shared())
    }

    #[pyo3(name = "with_portfolio_config")]
    fn py_with_portfolio_config(&self, config: PortfolioConfig) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_portfolio_config(config))
    }

    #[pyo3(name = "with_data_engine_config")]
    fn py_with_data_engine_config(&self, config: LiveDataEngineConfig) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_data_engine_config(config))
    }

    #[pyo3(name = "with_risk_engine_config")]
    fn py_with_risk_engine_config(&self, config: LiveRiskEngineConfig) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_risk_engine_config(config))
    }

    #[pyo3(name = "with_exec_engine_config")]
    fn py_with_exec_engine_config(&self, config: LiveExecutionEngineConfig) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_exec_engine_config(config))
    }

    #[pyo3(name = "with_logging")]
    fn py_with_logging(&self, logging: LoggerConfig) -> PyResult<Self> {
        self.update_builder(|builder| builder.with_logging(logging))
    }

    #[pyo3(name = "add_data_client", signature = (name, factory, config, routing=None))]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_data_client(
        &self,
        name: Option<String>,
        factory: Py<PyAny>,
        config: Py<PyAny>,
        routing: Option<RoutingConfig>,
    ) -> PyResult<Self> {
        let mut operation = self.begin_operation()?;
        Python::attach(|py| -> PyResult<Self> {
            let registry = get_global_pyo3_registry();
            let boxed_factory = registry.extract_factory(py, factory.clone_ref(py))?;
            let boxed_config = registry.extract_config(py, config.clone_ref(py))?;
            let factory_name = factory
                .getattr(py, "name")?
                .call0(py)?
                .extract::<String>(py)?;
            let client_name = name.unwrap_or(factory_name);
            let builder = operation.take_builder()?;
            let updated_builder = match routing {
                Some(routing) => builder.add_data_client_with_routing(
                    Some(client_name),
                    boxed_factory,
                    boxed_config,
                    routing,
                ),
                None => builder.add_data_client(Some(client_name), boxed_factory, boxed_config),
            }
            .map_err(|e| to_pyruntime_err(format!("Failed to add data client: {e}")))?;
            operation.complete(updated_builder);
            Ok(self.shared())
        })
    }

    #[pyo3(name = "add_exec_client", signature = (name, factory, config, routing=None))]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_exec_client(
        &self,
        name: Option<String>,
        factory: Py<PyAny>,
        config: Py<PyAny>,
        routing: Option<RoutingConfig>,
    ) -> PyResult<Self> {
        let mut operation = self.begin_operation()?;
        Python::attach(|py| -> PyResult<Self> {
            let registry = get_global_pyo3_registry();
            let boxed_factory = registry.extract_exec_factory(py, factory.clone_ref(py))?;
            let boxed_config = registry.extract_config(py, config.clone_ref(py))?;
            let factory_name = factory
                .getattr(py, "name")?
                .call0(py)?
                .extract::<String>(py)?;
            let client_name = name.unwrap_or(factory_name);
            let builder = operation.take_builder()?;
            let updated_builder = match routing {
                Some(routing) => builder.add_exec_client_with_routing(
                    Some(client_name),
                    boxed_factory,
                    boxed_config,
                    routing,
                ),
                None => builder.add_exec_client(Some(client_name), boxed_factory, boxed_config),
            }
            .map_err(|e| to_pyruntime_err(format!("Failed to add exec client: {e}")))?;
            operation.complete(updated_builder);
            Ok(self.shared())
        })
    }

    #[pyo3(name = "add_simulated_exec_client")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_simulated_exec_client(
        &self,
        name: Option<String>,
        factory: Py<PyAny>,
        config: Py<PyAny>,
    ) -> PyResult<Self> {
        let mut operation = self.begin_operation()?;
        Python::attach(|py| -> PyResult<Self> {
            let registry = get_global_pyo3_registry();
            let boxed_factory = registry.extract_sim_exec_factory(py, factory.clone_ref(py))?;
            let boxed_config = registry.extract_config(py, config.clone_ref(py))?;
            let factory_name = factory
                .getattr(py, "name")?
                .call0(py)?
                .extract::<String>(py)?;
            let client_name = name.unwrap_or(factory_name);
            let builder = operation.take_builder()?;
            let updated_builder = builder
                .add_simulated_exec_client(Some(client_name), boxed_factory, boxed_config)
                .map_err(|e| {
                    to_pyruntime_err(format!("Failed to add simulated exec client: {e}"))
                })?;
            operation.complete(updated_builder);
            Ok(self.shared())
        })
    }

    #[pyo3(name = "build")]
    fn py_build(&self) -> PyResult<PyLiveNode> {
        let mut operation = self.begin_operation()?;
        let builder = operation.take_builder()?;
        builder
            .build()
            .map(PyLiveNode::new)
            .map_err(to_pyruntime_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

const BUILDER_OPERATION_IN_PROGRESS: &str = "Builder operation already in progress";
const BUILDER_OPERATION_VALUE_TAKEN: &str = "Builder operation value already taken";

enum PyLiveNodeBuilderState {
    Ready(Box<LiveNodeBuilder>),
    InProgress,
    Consumed,
}

struct PyLiveNodeBuilderOperation<'a> {
    state: &'a Cell<PyLiveNodeBuilderState>,
    builder: Option<LiveNodeBuilder>,
}

impl PyLiveNodeBuilder {
    fn begin_operation(&self) -> PyResult<PyLiveNodeBuilderOperation<'_>> {
        match self.state.replace(PyLiveNodeBuilderState::InProgress) {
            PyLiveNodeBuilderState::Ready(builder) => Ok(PyLiveNodeBuilderOperation {
                state: &self.state,
                builder: Some(*builder),
            }),
            PyLiveNodeBuilderState::InProgress => {
                self.state.set(PyLiveNodeBuilderState::InProgress);
                Err(to_pyruntime_err(BUILDER_OPERATION_IN_PROGRESS))
            }
            PyLiveNodeBuilderState::Consumed => {
                self.state.set(PyLiveNodeBuilderState::Consumed);
                Err(to_pyruntime_err("Builder already consumed"))
            }
        }
    }

    fn update_builder<F>(&self, update: F) -> PyResult<Self>
    where
        F: FnOnce(LiveNodeBuilder) -> LiveNodeBuilder,
    {
        let mut operation = self.begin_operation()?;
        let builder = operation.take_builder()?;
        operation.complete(update(builder));
        Ok(self.shared())
    }

    fn shared(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl Debug for PyLiveNodeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Preserve the existing Python repr despite the internal state change
        let state = self.state.replace(PyLiveNodeBuilderState::InProgress);
        let result = match &state {
            PyLiveNodeBuilderState::Ready(builder) => write!(
                f,
                "PyLiveNodeBuilder {{ inner: RefCell {{ value: Some({builder:?}) }} }}"
            ),
            PyLiveNodeBuilderState::InProgress => {
                f.write_str("PyLiveNodeBuilder { inner: <operation active> }")
            }
            PyLiveNodeBuilderState::Consumed => {
                f.write_str("PyLiveNodeBuilder { inner: RefCell { value: None } }")
            }
        };
        self.state.set(state);
        result
    }
}

impl PyLiveNodeBuilderOperation<'_> {
    fn take_builder(&mut self) -> PyResult<LiveNodeBuilder> {
        self.builder
            .take()
            .ok_or_else(|| to_pyruntime_err(BUILDER_OPERATION_VALUE_TAKEN))
    }

    fn complete(&mut self, builder: LiveNodeBuilder) {
        self.builder = Some(builder);
    }
}

impl Drop for PyLiveNodeBuilderOperation<'_> {
    fn drop(&mut self) {
        self.state.set(match self.builder.take() {
            Some(builder) => PyLiveNodeBuilderState::Ready(Box::new(builder)),
            None => PyLiveNodeBuilderState::Consumed,
        });
    }
}

#[cfg(all(test, feature = "python"))]
#[allow(
    clippy::await_holding_refcell_ref,
    reason = "each test owns its node exclusively, so the wrapper borrow cannot contend"
)]
mod tests {
    use std::{
        any::Any,
        cell::RefCell,
        collections::HashMap,
        ffi::CString,
        fmt::Debug,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };

    use async_trait::async_trait;
    use indexmap::IndexMap;
    use nautilus_common::{
        actor::DataActor,
        cache::{
            CacheConfig, CacheView,
            database::{CacheDatabaseAdapter, CacheDatabaseFactory},
        },
        clients::DataClient,
        clock::Clock,
        enums::Environment,
        factories::{ClientConfig, DataClientFactory},
        live::{runner::get_data_event_sender, runtime::get_runtime},
        messages::{
            DataEvent, DataResponse,
            data::{BarsResponse, RequestBars},
            execution::{CancelAllOrders, SubmitOrder, TradingCommand},
        },
        msgbus::{
            BusMessage, MessageBusBacking, MessageBusBackingFactory, MessageBusConfig,
            MessagingSwitchboard, get_message_bus,
        },
        python::{
            actor::PyDataActor, cache::get_global_cache_database_factory_registry,
            msgbus::get_global_msgbus_factory_registry,
        },
        runner::{TradingCommandMessage, get_trading_cmd_sender},
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_execution::engine::stubs::StubExecutionClient;
    use nautilus_model::{
        data::{Bar, BarType},
        enums::{OmsType, OrderStatus, OrderType},
        identifiers::{
            AccountId, ActorId, ClientId, InstrumentId, PositionId, StrategyId, TraderId, Venue,
        },
        instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
        orders::{Order, OrderTestBuilder},
        types::{Price, Quantity},
    };
    use nautilus_testkit::cache::{TestCacheDatabase, TestCacheDatabaseControl};
    use nautilus_trading::{
        ImportableStrategyConfig, nautilus_strategy,
        python::strategy::PyStrategy,
        strategy::{StrategyConfig, StrategyCore},
    };
    use parking_lot::Mutex;
    use pyo3::{
        Py, PyRef, Python,
        ffi::c_str,
        types::{PyAnyMethods, PyDict, PyModule, PyModuleMethods},
    };
    use rstest::rstest;

    use super::{
        BUILDER_OPERATION_IN_PROGRESS, LiveNode, PyLiveNode, PyLiveNodeBuilder,
        PyLiveNodeBuilderState, get_global_pyo3_registry,
    };
    use crate::node::config::RoutingConfig;

    #[derive(Clone, Copy, Debug)]
    enum ShutdownRunPath {
        Native,
        PyO3,
    }

    static TEST_MSGBUS_FACTORY_CALLS: AtomicUsize = AtomicUsize::new(0);

    #[derive(Debug, Clone)]
    #[pyo3::pyclass(name = "TestMessageBusFactory", from_py_object)]
    struct TestMessageBusFactory;

    impl MessageBusBackingFactory for TestMessageBusFactory {
        fn create(
            &self,
            trader_id: TraderId,
            _instance_id: UUID4,
            config: MessageBusConfig,
        ) -> anyhow::Result<Box<dyn MessageBusBacking>> {
            TEST_MSGBUS_FACTORY_CALLS.fetch_add(1, Ordering::SeqCst);

            anyhow::ensure!(
                trader_id == TraderId::from("TESTER-001"),
                "unexpected trader ID: {trader_id}"
            );
            anyhow::ensure!(
                config.external_streams == Some(vec!["external-stream".to_string()]),
                "unexpected external streams: {:?}",
                config.external_streams
            );

            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(Box::new(TestMessageBusBacking {
                rx: Some(rx),
                closed: false,
            }))
        }
    }

    #[derive(Debug)]
    struct TestMessageBusBacking {
        rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
        closed: bool,
    }

    impl MessageBusBacking for TestMessageBusBacking {
        fn is_closed(&self) -> bool {
            self.closed
        }

        fn publish(&self, _message: BusMessage) {}

        fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
            self.rx
                .take()
                .ok_or_else(|| anyhow::anyhow!("Stream receiver already taken"))
        }

        fn close(&mut self) {
            self.closed = true;
        }
    }

    #[expect(clippy::needless_pass_by_value)]
    fn extract_test_msgbus_factory(
        py: Python<'_>,
        factory: Py<pyo3::PyAny>,
    ) -> pyo3::PyResult<Box<dyn MessageBusBackingFactory>> {
        Ok(Box::new(factory.extract::<TestMessageBusFactory>(py)?))
    }

    #[rstest]
    fn test_python_builder_installs_external_msgbus_factory() {
        TEST_MSGBUS_FACTORY_CALLS.store(0, Ordering::SeqCst);
        get_global_msgbus_factory_registry()
            .register(
                "TestMessageBusFactory".to_string(),
                extract_test_msgbus_factory,
            )
            .unwrap();
        Python::initialize();

        Python::attach(|py| {
            let factory = Py::new(py, TestMessageBusFactory).unwrap().into_any();
            let builder = PyLiveNode::py_builder(
                "TEST".to_string(),
                TraderId::from("TESTER-001"),
                Environment::Sandbox,
            )
            .unwrap()
            .py_with_msgbus_config(MessageBusConfig {
                external_streams: Some(vec!["external-stream".to_string()]),
                ..Default::default()
            })
            .unwrap()
            .py_with_external_msgbus_factory(factory)
            .unwrap();

            let node = builder.py_build().unwrap();
            let consumed_error = builder
                .py_with_msgbus_config(MessageBusConfig::default())
                .unwrap_err();

            assert!(!node.node_mut().unwrap().is_running());
            assert_eq!(TEST_MSGBUS_FACTORY_CALLS.load(Ordering::SeqCst), 1);
            assert_eq!(
                consumed_error.to_string(),
                "RuntimeError: Builder already consumed"
            );
            assert_eq!(
                builder.__repr__(),
                "PyLiveNodeBuilder { inner: RefCell { value: None } }"
            );
            get_message_bus().borrow_mut().dispose();
        });
    }

    static TEST_CACHE_DATABASE_FACTORY_CALLS: AtomicUsize = AtomicUsize::new(0);

    #[derive(Debug, Clone)]
    #[pyo3::pyclass(name = "TestCacheDatabaseFactory", from_py_object)]
    struct TestCacheDatabaseFactory {
        database: Arc<parking_lot::Mutex<Option<TestCacheDatabase>>>,
        instance_id: Arc<parking_lot::Mutex<Option<UUID4>>>,
    }

    impl TestCacheDatabaseFactory {
        fn new(database: Option<TestCacheDatabase>) -> Self {
            Self {
                database: Arc::new(parking_lot::Mutex::new(database)),
                instance_id: Arc::new(parking_lot::Mutex::new(None)),
            }
        }

        fn received_instance_id(&self) -> Option<UUID4> {
            *self.instance_id.lock()
        }
    }

    #[async_trait]
    impl CacheDatabaseFactory for TestCacheDatabaseFactory {
        async fn create(
            &self,
            trader_id: TraderId,
            instance_id: UUID4,
            config: CacheConfig,
        ) -> anyhow::Result<Box<dyn CacheDatabaseAdapter>> {
            TEST_CACHE_DATABASE_FACTORY_CALLS.fetch_add(1, Ordering::SeqCst);
            *self.instance_id.lock() = Some(instance_id);

            anyhow::ensure!(
                trader_id == TraderId::from("TESTER-001"),
                "unexpected trader ID: {trader_id}"
            );
            anyhow::ensure!(
                config.buffer_interval_ms == Some(25),
                "unexpected buffer interval: {:?}",
                config.buffer_interval_ms
            );

            let database = self
                .database
                .lock()
                .take()
                .ok_or_else(|| anyhow::anyhow!("Test cache database unavailable"))?;
            Ok(Box::new(database))
        }
    }

    #[expect(clippy::needless_pass_by_value)]
    fn extract_test_cache_database_factory(
        py: Python<'_>,
        factory: Py<pyo3::PyAny>,
    ) -> pyo3::PyResult<Box<dyn CacheDatabaseFactory>> {
        Ok(Box::new(factory.extract::<TestCacheDatabaseFactory>(py)?))
    }

    fn state_factory_builder(factory: &TestCacheDatabaseFactory) -> PyLiveNodeBuilder {
        Python::attach(|py| {
            PyLiveNode::py_builder(
                "TEST".to_string(),
                TraderId::from("TESTER-001"),
                Environment::Sandbox,
            )
            .unwrap()
            .py_with_cache_config(CacheConfig {
                buffer_interval_ms: Some(25),
                ..Default::default()
            })
            .unwrap()
            .py_with_cache_database_factory(Py::new(py, factory.clone()).unwrap().into_any())
            .unwrap()
            .py_with_reconciliation(false)
            .unwrap()
            .py_with_timeout_connection(0)
            .unwrap()
            .py_with_timeout_reconciliation(0)
            .unwrap()
            .py_with_timeout_portfolio(0)
            .unwrap()
            .py_with_timeout_disconnection_secs(0)
            .unwrap()
            .py_with_delay_post_stop_secs(0)
            .unwrap()
            .py_with_delay_shutdown_secs(0)
            .unwrap()
        })
    }

    #[tokio::test]
    async fn test_python_builder_installs_cache_database_factory_on_start() {
        TEST_CACHE_DATABASE_FACTORY_CALLS.store(0, Ordering::SeqCst);
        get_global_cache_database_factory_registry()
            .register(
                "TestCacheDatabaseFactory".to_string(),
                extract_test_cache_database_factory,
            )
            .unwrap();
        Python::initialize();

        let (database, control) = TestCacheDatabaseControl::create();
        let actor_id = ActorId::from("PY-CACHE-FACTORY-ACTOR");
        let actor_state = IndexMap::from([("loaded".to_string(), b"value".to_vec())]);
        control.set_actor_state(actor_id, &actor_state);
        let factory = TestCacheDatabaseFactory::new(Some(database));

        let node = state_factory_builder(&factory).py_build().unwrap();

        assert_eq!(TEST_CACHE_DATABASE_FACTORY_CALLS.load(Ordering::SeqCst), 0);
        assert!(
            !node
                .node_mut()
                .unwrap()
                .kernel()
                .cache()
                .borrow()
                .has_backing()
        );

        node.node_mut().unwrap().start().await.unwrap();

        assert_eq!(TEST_CACHE_DATABASE_FACTORY_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(
            factory.received_instance_id(),
            Some(node.node_mut().unwrap().instance_id())
        );
        assert!(
            node.node_mut()
                .unwrap()
                .kernel()
                .cache()
                .borrow()
                .has_backing()
        );
        assert_eq!(
            node.node_mut()
                .unwrap()
                .kernel()
                .cache()
                .borrow()
                .load_actor_state(&actor_id)
                .unwrap(),
            Some(actor_state)
        );

        node.node_mut().unwrap().stop().await.unwrap();
        node.node_mut().unwrap().dispose();
        get_message_bus().borrow_mut().dispose();
    }

    #[tokio::test]
    async fn test_python_builder_propagates_cache_database_factory_error_on_start() {
        get_global_cache_database_factory_registry()
            .register(
                "TestCacheDatabaseFactory".to_string(),
                extract_test_cache_database_factory,
            )
            .unwrap();
        Python::initialize();

        let factory = TestCacheDatabaseFactory::new(None);
        let node = state_factory_builder(&factory).py_build().unwrap();

        let error = node.node_mut().unwrap().start().await.unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            "failed to create cache database backing: Test cache database unavailable"
        );
        assert!(
            !node
                .node_mut()
                .unwrap()
                .kernel()
                .cache()
                .borrow()
                .has_backing()
        );

        let retry_error = node.node_mut().unwrap().start().await.unwrap_err();

        assert_eq!(
            format!("{retry_error:#}"),
            "failed to create cache database backing: Test cache database unavailable"
        );
        assert!(
            !node
                .node_mut()
                .unwrap()
                .kernel()
                .cache()
                .borrow()
                .has_backing()
        );

        node.node_mut().unwrap().dispose();
        get_message_bus().borrow_mut().dispose();
    }

    #[rstest]
    fn test_python_builder_rejects_unregistered_cache_database_factory() {
        Python::initialize();

        Python::attach(|py| {
            let factory = PyDict::new(py).unbind().into_any();
            let builder = PyLiveNode::py_builder(
                "TEST".to_string(),
                TraderId::from("TESTER-001"),
                Environment::Sandbox,
            )
            .unwrap();

            let error = builder.py_with_cache_database_factory(factory).unwrap_err();

            assert_eq!(
                error.to_string(),
                "NotImplementedError: No cache database factory extractor registered for 'dict'"
            );
            builder
                .py_with_cache_config(CacheConfig::default())
                .unwrap();
        });
    }

    #[rstest]
    fn test_python_builder_restores_state_after_factory_type_reentry() {
        Python::initialize();

        Python::attach(|py| {
            let builder = Py::new(
                py,
                PyLiveNode::py_builder(
                    "TEST".to_string(),
                    TraderId::from("TESTER-001"),
                    Environment::Sandbox,
                )
                .unwrap(),
            )
            .unwrap();
            let locals = PyDict::new(py);
            locals.set_item("builder", &builder).unwrap();
            py.run(
                pyo3::ffi::c_str!(
                    "class ReentrantFactory:\n    def __getattribute__(self, name):\n        if name == '__class__':\n            builder.with_load_state(True)\n        return object.__getattribute__(self, name)\n\nfactory = ReentrantFactory()"
                ),
                Some(&locals),
                None,
            )
            .unwrap();
            let factory = locals.get_item("factory").unwrap();

            let error = builder
                .call_method1(py, "with_cache_database_factory", (factory,))
                .unwrap_err();

            assert_eq!(
                error.to_string(),
                format!("RuntimeError: {BUILDER_OPERATION_IN_PROGRESS}")
            );
            let builder_ref = builder.borrow(py);
            let state = builder_ref
                .state
                .replace(PyLiveNodeBuilderState::InProgress);
            let is_ready = matches!(&state, PyLiveNodeBuilderState::Ready(_));
            builder_ref.state.set(state);
            assert!(is_ready);
            locals.call_method0("clear").unwrap();
        });
    }

    #[rstest]
    fn test_python_builder_rejects_unregistered_external_msgbus_factory() {
        Python::initialize();

        Python::attach(|py| {
            let factory = PyDict::new(py).unbind().into_any();
            let builder = PyLiveNode::py_builder(
                "TEST".to_string(),
                TraderId::from("TESTER-001"),
                Environment::Sandbox,
            )
            .unwrap();

            let error = builder
                .py_with_external_msgbus_factory(factory)
                .unwrap_err();

            assert_eq!(
                error.to_string(),
                "NotImplementedError: No message bus factory extractor registered for 'dict'"
            );
            builder
                .py_with_msgbus_config(MessageBusConfig::default())
                .unwrap();
        });
    }

    #[derive(Debug)]
    struct ShutdownCancelStrategy {
        core: StrategyCore,
        instrument_id: InstrumentId,
    }

    impl ShutdownCancelStrategy {
        fn new(instrument_id: InstrumentId) -> Self {
            Self {
                core: StrategyCore::new(StrategyConfig {
                    strategy_id: Some(StrategyId::from("SHUTDOWN-CANCEL-001")),
                    ..Default::default()
                }),
                instrument_id,
            }
        }
    }

    nautilus_strategy!(ShutdownCancelStrategy);

    impl DataActor for ShutdownCancelStrategy {
        fn on_stop(&mut self) -> anyhow::Result<()> {
            get_trading_cmd_sender().execute(TradingCommandMessage::new(
                MessagingSwitchboard::exec_engine_execute(),
                TradingCommand::CancelAllOrders(CancelAllOrders::new(
                    TraderId::from("TESTER-001"),
                    None,
                    StrategyId::from("SHUTDOWN-CANCEL-001"),
                    self.instrument_id,
                    None,
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )),
            ));
            Ok(())
        }
    }
    #[derive(Debug, Default)]
    struct TestDataClientConfig;

    impl ClientConfig for TestDataClientConfig {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[derive(Debug)]
    #[expect(
        clippy::struct_field_names,
        reason = "test counters intentionally share the count postfix"
    )]
    struct TestHistoricalBarsDataClientFactory {
        request_count: Arc<AtomicUsize>,
        response_sent_count: Arc<AtomicUsize>,
        handler_visible_count: Arc<AtomicUsize>,
    }

    impl TestHistoricalBarsDataClientFactory {
        fn new(
            request_count: Arc<AtomicUsize>,
            response_sent_count: Arc<AtomicUsize>,
            handler_visible_count: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                request_count,
                response_sent_count,
                handler_visible_count,
            }
        }
    }

    impl DataClientFactory for TestHistoricalBarsDataClientFactory {
        fn create(
            &self,
            name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
            _clock: Rc<RefCell<dyn Clock>>,
        ) -> anyhow::Result<Box<dyn DataClient>> {
            Ok(Box::new(TestHistoricalBarsDataClient::new(
                ClientId::from(name),
                Venue::from("SIM"),
                self.request_count.clone(),
                self.response_sent_count.clone(),
                self.handler_visible_count.clone(),
            )))
        }

        fn name(&self) -> &'static str {
            "TEST_DATA"
        }

        fn config_type(&self) -> &'static str {
            "TestDataClientConfig"
        }
    }

    #[derive(Debug)]
    struct TestDisconnectFailureDataClientFactory {
        dispose_count: Arc<AtomicUsize>,
    }

    impl TestDisconnectFailureDataClientFactory {
        fn new(dispose_count: Arc<AtomicUsize>) -> Self {
            Self { dispose_count }
        }
    }

    impl DataClientFactory for TestDisconnectFailureDataClientFactory {
        fn create(
            &self,
            name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
            _clock: Rc<RefCell<dyn Clock>>,
        ) -> anyhow::Result<Box<dyn DataClient>> {
            Ok(Box::new(TestDisconnectFailureDataClient::new(
                ClientId::from(name),
                Venue::from("SIM"),
                self.dispose_count.clone(),
            )))
        }

        fn name(&self) -> &'static str {
            "TEST_DISCONNECT_FAILURE"
        }

        fn config_type(&self) -> &'static str {
            "TestDataClientConfig"
        }
    }

    #[derive(Debug)]
    struct TestDisconnectFailureDataClient {
        client_id: ClientId,
        venue: Venue,
        connected: Arc<AtomicBool>,
        dispose_count: Arc<AtomicUsize>,
    }

    impl TestDisconnectFailureDataClient {
        fn new(client_id: ClientId, venue: Venue, dispose_count: Arc<AtomicUsize>) -> Self {
            Self {
                client_id,
                venue,
                connected: Arc::new(AtomicBool::new(false)),
                dispose_count,
            }
        }
    }

    #[async_trait(?Send)]
    impl DataClient for TestDisconnectFailureDataClient {
        fn client_id(&self) -> ClientId {
            self.client_id
        }

        fn venue(&self) -> Option<Venue> {
            Some(self.venue)
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn reset(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn dispose(&mut self) -> anyhow::Result<()> {
            self.dispose_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn is_connected(&self) -> bool {
            self.connected.load(Ordering::Relaxed)
        }

        fn is_disconnected(&self) -> bool {
            !self.is_connected()
        }

        async fn connect(&mut self) -> anyhow::Result<()> {
            self.connected.store(true, Ordering::Relaxed);
            Ok(())
        }

        async fn disconnect(&mut self) -> anyhow::Result<()> {
            self.connected.store(false, Ordering::Relaxed);
            anyhow::bail!("test disconnect failed")
        }
    }

    struct VenueLessDataClient {
        client_id: ClientId,
    }

    impl VenueLessDataClient {
        fn new(client_id: ClientId) -> Self {
            Self { client_id }
        }
    }

    #[async_trait(?Send)]
    impl DataClient for VenueLessDataClient {
        fn client_id(&self) -> ClientId {
            self.client_id
        }

        fn venue(&self) -> Option<Venue> {
            None
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn reset(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn dispose(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            true
        }

        fn is_disconnected(&self) -> bool {
            false
        }

        async fn connect(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn disconnect(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct VenueLessDataClientFactory;

    impl DataClientFactory for VenueLessDataClientFactory {
        fn create(
            &self,
            name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
            _clock: Rc<RefCell<dyn Clock>>,
        ) -> anyhow::Result<Box<dyn DataClient>> {
            Ok(Box::new(VenueLessDataClient::new(ClientId::from(name))))
        }

        fn name(&self) -> &'static str {
            "VENUE_LESS"
        }

        fn config_type(&self) -> &'static str {
            "TestDataClientConfig"
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "signature must match the factory extractor function pointer"
    )]
    fn extract_reentrant_data_client_factory(
        _py: Python<'_>,
        _factory: Py<pyo3::PyAny>,
    ) -> pyo3::PyResult<Box<dyn DataClientFactory>> {
        Ok(Box::new(VenueLessDataClientFactory))
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "signature must match the config extractor function pointer"
    )]
    fn extract_reentrant_data_client_config(
        _py: Python<'_>,
        _config: Py<pyo3::PyAny>,
    ) -> pyo3::PyResult<Box<dyn ClientConfig>> {
        Ok(Box::new(TestDataClientConfig))
    }

    #[rstest]
    fn test_python_builder_blocks_factory_name_reentry() {
        get_global_pyo3_registry()
            .register_factory_extractor(
                "REENTRANT_DATA".to_string(),
                extract_reentrant_data_client_factory,
            )
            .unwrap();
        get_global_pyo3_registry()
            .register_config_extractor(
                "ReentrantDataClientConfig".to_string(),
                extract_reentrant_data_client_config,
            )
            .unwrap();
        Python::initialize();

        Python::attach(|py| {
            let builder = Py::new(
                py,
                PyLiveNode::py_builder(
                    "TEST".to_string(),
                    TraderId::from("TESTER-001"),
                    Environment::Sandbox,
                )
                .unwrap(),
            )
            .unwrap();
            let locals = PyDict::new(py);
            locals.set_item("builder", &builder).unwrap();
            py.run(
                pyo3::ffi::c_str!(
                    "class ReentrantDataClientFactory:\n    def __init__(self):\n        self.reprs = []\n        self.results = []\n\n    def name(self):\n        self.reprs.append(repr(builder))\n        try:\n            builder.with_save_state(True)\n        except RuntimeError as e:\n            self.results.append((type(e).__name__, str(e)))\n        return 'REENTRANT_DATA'\n\nclass ReentrantDataClientConfig:\n    pass\n\nfactory = ReentrantDataClientFactory()\nconfig = ReentrantDataClientConfig()"
                ),
                Some(&locals),
                None,
            )
            .unwrap();
            let factory = locals.get_item("factory").unwrap();
            let config = locals.get_item("config").unwrap();

            builder
                .call_method1(py, "add_data_client", (py.None(), &factory, &config))
                .unwrap();

            let results = factory
                .getattr("results")
                .unwrap()
                .extract::<Vec<(String, String)>>()
                .unwrap();
            assert_eq!(
                results,
                vec![
                    (
                        "RuntimeError".to_string(),
                        BUILDER_OPERATION_IN_PROGRESS.to_string(),
                    ),
                    (
                        "RuntimeError".to_string(),
                        BUILDER_OPERATION_IN_PROGRESS.to_string(),
                    ),
                ]
            );
            assert_eq!(
                factory
                    .getattr("reprs")
                    .unwrap()
                    .extract::<Vec<String>>()
                    .unwrap(),
                vec!["PyLiveNodeBuilder { inner: <operation active> }"; 2]
            );
            let builder_ref = builder.borrow(py);
            let state = builder_ref
                .state
                .replace(PyLiveNodeBuilderState::InProgress);
            let is_ready = matches!(&state, PyLiveNodeBuilderState::Ready(_));
            builder_ref.state.set(state);
            assert!(is_ready);
            drop(builder_ref);

            let duplicate_error = builder
                .call_method1(py, "add_data_client", (py.None(), factory, config))
                .unwrap_err();

            assert_eq!(
                duplicate_error.to_string(),
                "RuntimeError: Failed to add data client: Data client 'REENTRANT_DATA' is already registered"
            );
            let builder_ref = builder.borrow(py);
            let state = builder_ref
                .state
                .replace(PyLiveNodeBuilderState::InProgress);
            let is_consumed = matches!(&state, PyLiveNodeBuilderState::Consumed);
            builder_ref.state.set(state);
            assert!(is_consumed);
            locals.call_method0("clear").unwrap();
        });
    }

    #[derive(Debug)]
    struct TestHistoricalBarsDataClient {
        client_id: ClientId,
        venue: Venue,
        connected: Arc<AtomicBool>,
        request_count: Arc<AtomicUsize>,
        response_sent_count: Arc<AtomicUsize>,
        handler_visible_count: Arc<AtomicUsize>,
    }

    impl TestHistoricalBarsDataClient {
        fn new(
            client_id: ClientId,
            venue: Venue,
            request_count: Arc<AtomicUsize>,
            response_sent_count: Arc<AtomicUsize>,
            handler_visible_count: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                client_id,
                venue,
                connected: Arc::new(AtomicBool::new(false)),
                request_count,
                response_sent_count,
                handler_visible_count,
            }
        }

        fn make_bar(bar_type: BarType) -> Bar {
            Bar::new(
                bar_type,
                Price::from("1.0000"),
                Price::from("1.1000"),
                Price::from("0.9000"),
                Price::from("1.0500"),
                Quantity::from("1000"),
                UnixNanos::from(1_700_000_000_000_000_000u64),
                UnixNanos::from(1_700_000_000_000_000_001u64),
            )
        }
    }

    #[async_trait(?Send)]
    impl DataClient for TestHistoricalBarsDataClient {
        fn client_id(&self) -> ClientId {
            self.client_id
        }

        fn venue(&self) -> Option<Venue> {
            Some(self.venue)
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn reset(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn dispose(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            self.connected.load(Ordering::Relaxed)
        }

        fn is_disconnected(&self) -> bool {
            !self.is_connected()
        }

        async fn connect(&mut self) -> anyhow::Result<()> {
            self.connected.store(true, Ordering::Relaxed);
            Ok(())
        }

        async fn disconnect(&mut self) -> anyhow::Result<()> {
            self.connected.store(false, Ordering::Relaxed);
            Ok(())
        }

        fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
            self.request_count.fetch_add(1, Ordering::Relaxed);

            if get_message_bus()
                .borrow()
                .get_response_handler(&request.request_id)
                .is_some()
            {
                self.handler_visible_count.fetch_add(1, Ordering::Relaxed);
            }

            let sender = get_data_event_sender();
            let client_id = self.client_id;
            let response_sent_count = self.response_sent_count.clone();
            let response = BarsResponse::new(
                request.request_id,
                client_id,
                request.bar_type,
                vec![Self::make_bar(request.bar_type)],
                None,
                None,
                UnixNanos::from(1_700_000_000_000_000_002u64),
                None,
            );

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                response_sent_count.fetch_add(1, Ordering::Relaxed);
                sender
                    .send(DataEvent::Response(DataResponse::Bars(response)))
                    .expect("test bars response should send");
            });

            Ok(())
        }
    }

    fn install_tracking_strategy_module(py: Python<'_>, module_name: &str) {
        let module = PyModule::new(py, module_name).expect("test module should create");
        module
            .setattr("Strategy", py.get_type::<PyStrategy>())
            .expect("Strategy type should bind");
        module
            .setattr("BarType", py.get_type::<BarType>())
            .expect("BarType type should bind");
        module
            .setattr("RESULTS", PyDict::new(py))
            .expect("RESULTS should bind");

        let code = CString::new(
            r#"
RESULTS["on_start"] = 0
RESULTS["on_historical_bars"] = 0
RESULTS["historical_bar_count"] = 0
RESULTS["last_request_id"] = ""

class HistoricalBarsStrategy(Strategy):
    def __init__(self):
        super().__init__()
        self.bar_type = BarType.from_str("AUDUSD.SIM-1-MINUTE-LAST-EXTERNAL")

    def on_start(self):
        RESULTS["on_start"] += 1
        RESULTS["last_request_id"] = self.request_bars(self.bar_type)

    def on_stop(self):
        pass

    def on_historical_bars(self, bars):
        RESULTS["on_historical_bars"] += 1
        RESULTS["historical_bar_count"] += len(bars)
"#,
        )
        .expect("python test code should be valid CString");

        py.run(code.as_c_str(), Some(&module.dict()), None)
            .expect("test strategy code should execute");

        let sys_modules = py
            .import("sys")
            .expect("sys should import")
            .getattr("modules")
            .expect("sys.modules should exist");
        sys_modules
            .set_item(module_name, module)
            .expect("test strategy module should register");
    }

    fn get_results(py: Python<'_>, module_name: &str) -> (usize, usize, usize) {
        let module = py
            .import(module_name)
            .expect("test strategy module should import");
        let results_obj = module.getattr("RESULTS").expect("RESULTS should exist");
        let results = results_obj
            .cast::<PyDict>()
            .expect("RESULTS should be a dict");

        let on_start = results
            .get_item("on_start")
            .expect("on_start key should exist")
            .extract::<usize>()
            .expect("on_start should extract");
        let on_historical_bars = results
            .get_item("on_historical_bars")
            .expect("on_historical_bars key should exist")
            .extract::<usize>()
            .expect("on_historical_bars should extract");
        let historical_bar_count = results
            .get_item("historical_bar_count")
            .expect("historical_bar_count key should exist")
            .extract::<usize>()
            .expect("historical_bar_count should extract");

        (on_start, on_historical_bars, historical_bar_count)
    }

    fn install_timer_strategy_module(py: Python<'_>, module_name: &str) {
        let module = PyModule::new(py, module_name).expect("test module should create");
        module
            .setattr("Strategy", py.get_type::<PyStrategy>())
            .expect("Strategy type should bind");
        module
            .setattr("RESULTS", PyDict::new(py))
            .expect("RESULTS should bind");

        let code = CString::new(
            r#"
RESULTS["on_start"] = 0
RESULTS["callback_timer_count"] = 0
RESULTS["default_timer_count"] = 0
RESULTS["callback_event_type"] = ""
RESULTS["default_event_type"] = ""
RESULTS["callback_event_name"] = ""
RESULTS["default_event_name"] = ""

class LiveTimerStrategy(Strategy):
    def __init__(self):
        super().__init__()

    def on_start(self):
        RESULTS["on_start"] += 1
        self.clock.set_timer_ns(
            "explicit_timer",
            1_000_000,
            callback=self._on_timer,
            fire_immediately=True,
        )
        self.clock.set_timer_ns(
            "default_timer",
            1_000_000,
            fire_immediately=True,
        )

    def on_stop(self):
        pass

    def _on_timer(self, event):
        RESULTS["callback_timer_count"] += 1
        RESULTS["callback_event_type"] = type(event).__name__
        RESULTS["callback_event_name"] = event.name

    def on_time_event(self, event):
        RESULTS["default_timer_count"] += 1
        RESULTS["default_event_type"] = type(event).__name__
        RESULTS["default_event_name"] = event.name
"#,
        )
        .expect("python test code should be valid CString");

        py.run(code.as_c_str(), Some(&module.dict()), None)
            .expect("test strategy code should execute");

        let sys_modules = py
            .import("sys")
            .expect("sys should import")
            .getattr("modules")
            .expect("sys.modules should exist");
        sys_modules
            .set_item(module_name, module)
            .expect("test strategy module should register");
    }

    fn install_claim_strategy_module(py: Python<'_>, module_name: &str) {
        let module = PyModule::new(py, module_name).expect("test module should create");
        module
            .setattr("Strategy", py.get_type::<PyStrategy>())
            .expect("Strategy type should bind");

        let code = CString::new(
            "
class ClaimsConfig:
    def __init__(
        self,
        strategy_id=None,
        order_id_tag=None,
        external_order_instrument_ids=None,
        oms_type=None,
    ):
        self.strategy_id = strategy_id
        self.order_id_tag = order_id_tag
        self.external_order_instrument_ids = external_order_instrument_ids
        self.oms_type = oms_type

class ClaimsStrategy(Strategy):
    def __init__(self, config):
        super().__init__(config)
",
        )
        .expect("python test code should be valid CString");

        py.run(code.as_c_str(), Some(&module.dict()), None)
            .expect("test strategy code should execute");

        let sys_modules = py
            .import("sys")
            .expect("sys should import")
            .getattr("modules")
            .expect("sys.modules should exist");
        sys_modules
            .set_item(module_name, module)
            .expect("test strategy module should register");
    }

    #[derive(Debug)]
    struct TimerStrategyResults {
        on_start: usize,
        callback_timer_count: usize,
        default_timer_count: usize,
        callback_event_type: String,
        default_event_type: String,
        callback_event_name: String,
        default_event_name: String,
    }

    fn get_timer_results(py: Python<'_>, module_name: &str) -> TimerStrategyResults {
        let module = py
            .import(module_name)
            .expect("test strategy module should import");
        let results_obj = module.getattr("RESULTS").expect("RESULTS should exist");
        let results = results_obj
            .cast::<PyDict>()
            .expect("RESULTS should be a dict");

        TimerStrategyResults {
            on_start: results
                .get_item("on_start")
                .expect("on_start key should exist")
                .extract::<usize>()
                .expect("on_start should extract"),
            callback_timer_count: results
                .get_item("callback_timer_count")
                .expect("callback_timer_count key should exist")
                .extract::<usize>()
                .expect("callback_timer_count should extract"),
            default_timer_count: results
                .get_item("default_timer_count")
                .expect("default_timer_count key should exist")
                .extract::<usize>()
                .expect("default_timer_count should extract"),
            callback_event_type: results
                .get_item("callback_event_type")
                .expect("callback_event_type key should exist")
                .extract::<String>()
                .expect("callback_event_type should extract"),
            default_event_type: results
                .get_item("default_event_type")
                .expect("default_event_type key should exist")
                .extract::<String>()
                .expect("default_event_type should extract"),
            callback_event_name: results
                .get_item("callback_event_name")
                .expect("callback_event_name key should exist")
                .extract::<String>()
                .expect("callback_event_name should extract"),
            default_event_name: results
                .get_item("default_event_name")
                .expect("default_event_name key should exist")
                .extract::<String>()
                .expect("default_event_name should extract"),
        }
    }

    #[cfg(feature = "examples")]
    #[rstest]
    #[case("CompositeMarketMaker")]
    #[case("DeltaNeutralVol")]
    #[case("EmaCross")]
    #[case("ExecTester")]
    #[case("GridMarketMaker")]
    #[case("HurstVpinDirectional")]
    fn test_builtin_strategy_register_accepts_supported_names(#[case] type_name: &str) {
        assert!(super::builtin_strategy_register(type_name).is_some());
    }

    #[cfg(feature = "examples")]
    #[rstest]
    #[case("BookImbalanceActor")]
    #[case("DataTester")]
    fn test_builtin_actor_register_accepts_supported_names(#[case] type_name: &str) {
        assert!(super::builtin_actor_register(type_name).is_some());
    }

    #[cfg(feature = "examples")]
    #[rstest]
    fn test_builtin_register_rejects_unknown_names() {
        assert!(super::builtin_strategy_register("UnknownStrategy").is_none());
        assert!(super::builtin_actor_register("UnknownActor").is_none());
    }

    #[rstest]
    fn test_inspection_wrappers_share_kernel_state() {
        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        let cache = node.py_cache().unwrap();
        let portfolio = node.py_portfolio().unwrap();

        assert!(Rc::ptr_eq(
            &cache.cache_rc(),
            &node.node_mut().unwrap().kernel().cache
        ));
        assert!(Rc::ptr_eq(
            &portfolio.portfolio_rc(),
            &node.node_mut().unwrap().kernel().portfolio
        ));
    }

    #[cfg(feature = "examples")]
    #[rstest]
    fn test_builtin_strategy_register_rejects_mismatched_config() {
        Python::initialize();

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        Python::attach(|py| {
            let register = super::builtin_strategy_register("EmaCross").unwrap();
            let config = PyDict::new(py);
            let error = register(&mut node.node_mut().unwrap(), config.as_any()).unwrap_err();

            assert!(error.is_instance_of::<pyo3::exceptions::PyTypeError>(py));
        });
    }

    #[cfg(feature = "examples")]
    #[rstest]
    fn test_builtin_actor_register_rejects_mismatched_config() {
        Python::initialize();

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        Python::attach(|py| {
            let register = super::builtin_actor_register("DataTester").unwrap();
            let config = PyDict::new(py);
            let error = register(&mut node.node_mut().unwrap(), config.as_any()).unwrap_err();

            assert!(error.is_instance_of::<pyo3::exceptions::PyTypeError>(py));
        });
    }

    #[rstest]
    #[case(ShutdownRunPath::Native)]
    #[case(ShutdownRunPath::PyO3)]
    fn test_native_and_python_shutdown_paths_drain_cancel_command(
        #[case] run_path: ShutdownRunPath,
    ) {
        Python::initialize();

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(1)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();
        node.node_mut()
            .unwrap()
            .add_strategy(ShutdownCancelStrategy::new(InstrumentId::from(
                "TEST.POLYMARKET",
            )))
            .unwrap();

        let handle = node.node_mut().unwrap().handle();
        let stop_handle = handle.clone();

        let stop_thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !stop_handle.is_running() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            stop_handle.stop();
        });

        match run_path {
            ShutdownRunPath::Native => get_runtime()
                .block_on(node.node_mut().unwrap().run())
                .expect("native LiveNode run should stop cleanly"),
            ShutdownRunPath::PyO3 => Python::attach(|py| {
                super::run_live_node_detached(py, &mut node.node_mut().unwrap())
                    .expect("Python LiveNode run should stop cleanly");
            }),
        }

        stop_thread.join().expect("stop thread should join");
        let metrics = handle.metrics_snapshot();

        assert_eq!(metrics.exec_commands.dispatched, 1);
        assert_eq!(metrics.exec_commands.queue_depth, 0);
        assert!(!handle.is_running());
    }

    #[rstest]
    fn test_run_live_node_detached_releases_gil() {
        Python::initialize();

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        let handle = node.node_mut().unwrap().handle();
        let (gil_tx, gil_rx) = mpsc::channel();
        let acquired_before_stop = Arc::new(AtomicBool::new(false));
        let acquired_before_stop_for_thread = acquired_before_stop.clone();

        let stop_thread = thread::spawn(move || {
            if gil_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
                acquired_before_stop_for_thread.store(true, Ordering::SeqCst);
            }
            handle.stop();
        });

        let gil_thread = thread::spawn(move || {
            Python::attach(|_| {});
            let _ = gil_tx.send(());
        });

        Python::attach(|py| {
            super::run_live_node_detached(py, &mut node.node_mut().unwrap())
                .expect("node should run cleanly");
        });

        stop_thread.join().expect("stop thread should join");
        gil_thread.join().expect("GIL thread should join");

        assert!(
            acquired_before_stop.load(Ordering::SeqCst),
            "worker thread should acquire the GIL while LiveNode::run is blocked"
        );
    }

    #[rstest]
    fn test_host_loop_waker_coalesces_redundant_wakes() {
        let (waker, receiver, _) = host_loop_waker_probe();

        std::task::Wake::wake(waker.clone());
        let consuming_wake = receiver.recv_timeout(Duration::from_secs(1));
        std::task::Wake::wake_by_ref(&waker);
        let redundant_wake = receiver.try_recv();

        assert!(matches!(
            consuming_wake,
            Ok(super::HostWakeSignal::Resume(7))
        ));
        assert!(matches!(redundant_wake, Err(mpsc::TryRecvError::Empty)));
    }

    #[rstest]
    fn test_host_loop_waker_ignores_wake_after_close() {
        let (waker, receiver, handle) = host_loop_waker_probe();
        waker.control.close();

        std::task::Wake::wake_by_ref(&waker);

        assert!(matches!(
            receiver.recv().unwrap(),
            super::HostWakeSignal::Shutdown
        ));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(!handle.should_stop());
    }

    #[rstest]
    fn test_host_loop_waker_stops_node_when_pump_is_gone() {
        let (waker, receiver, handle) = host_loop_waker_probe();
        drop(receiver);

        std::task::Wake::wake_by_ref(&waker);

        assert!(!waker.control.active.load(Ordering::Acquire));
        assert!(handle.should_stop());
    }

    fn host_loop_waker_probe() -> (
        Arc<super::HostLoopWaker>,
        mpsc::Receiver<super::HostWakeSignal>,
        crate::node::LiveNodeHandle,
    ) {
        let (sender, receiver) = mpsc::channel();
        let handle = crate::node::LiveNodeHandle::new();
        let control = Arc::new(super::HostWakeControl {
            sender,
            active: AtomicBool::new(true),
            handle: handle.clone(),
        });
        let waker = Arc::new(super::HostLoopWaker {
            generation: 7,
            scheduled: AtomicBool::new(false),
            control,
        });

        (waker, receiver, handle)
    }

    #[rstest]
    fn test_host_loop_waker_breaks_gil_dependency_lock_cycle() {
        Python::initialize();

        let (waker, mut wake_pump, event_loop) = Python::attach(|py| {
            let event_loop = py
                .import("asyncio")
                .unwrap()
                .call_method0("new_event_loop")
                .unwrap();
            let state = Arc::new(super::RunWakeState::default());
            let wake_callback = Py::new(py, super::PyNodeRunWake { state })
                .unwrap()
                .into_any();
            let event_loop = event_loop.unbind();
            let wake_pump = super::HostWakePump::start(
                event_loop.clone_ref(py),
                wake_callback,
                crate::node::LiveNodeHandle::new(),
            )
            .unwrap();
            let waker = wake_pump.waker(1);

            (waker, wake_pump, event_loop)
        });
        let (start_tx, start_rx) = mpsc::channel();
        let (locked_tx, locked_rx) = mpsc::channel();
        let dependency_lock = Arc::new(Mutex::new(()));
        let dependency_lock_for_thread = dependency_lock.clone();
        let wake_thread = thread::spawn(move || {
            let _guard = dependency_lock_for_thread.lock();
            locked_tx.send(()).unwrap();
            start_rx.recv().unwrap();
            waker.wake_by_ref();
        });
        locked_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("wake thread should hold the dependency lock");

        let dependency_released_while_gil_held = Python::attach(|_| {
            start_tx.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);

            loop {
                match dependency_lock.try_lock() {
                    Some(_) => break true,
                    None if Instant::now() < deadline => {
                        thread::yield_now();
                    }
                    None => break false,
                }
            }
        });
        wake_thread.join().unwrap();
        Python::attach(|py| {
            wake_pump.close();
            wake_pump.join(Some(py));
            event_loop.bind(py).call_method0("close").unwrap();
        });

        assert!(dependency_released_while_gil_held);
    }

    #[rstest]
    fn test_run_wake_state_ignores_stale_generation() {
        Python::initialize();

        Python::attach(|py| {
            let event_loop = py
                .import("asyncio")
                .unwrap()
                .call_method0("new_event_loop")
                .unwrap();
            let future = event_loop.call_method0("create_future").unwrap();
            let state = super::RunWakeState::default();
            state.suspend(2, future.clone().unbind());

            state.resume(py, 1).unwrap();
            let done_after_stale: bool = future.call_method0("done").unwrap().extract().unwrap();
            state.resume(py, 2).unwrap();
            let done_after_current: bool = future.call_method0("done").unwrap().extract().unwrap();
            event_loop.call_method0("close").unwrap();

            assert!(!done_after_stale);
            assert!(done_after_current);
        });
    }

    #[rstest]
    fn test_host_wake_pump_resumes_current_suspension() {
        Python::initialize();

        Python::attach(|py| {
            let asyncio = py.import("asyncio").unwrap();
            let event_loop = asyncio.call_method0("new_event_loop").unwrap();
            let future = event_loop.call_method0("create_future").unwrap();
            let state = Arc::new(super::RunWakeState::default());
            state.suspend(7, future.clone().unbind());
            let wake_callback = Py::new(py, super::PyNodeRunWake { state })
                .unwrap()
                .into_any();
            let mut wake_pump = super::HostWakePump::start(
                event_loop.clone().unbind(),
                wake_callback,
                crate::node::LiveNodeHandle::new(),
            )
            .unwrap();

            wake_pump.waker(7).wake_by_ref();
            let wait_for = asyncio.call_method1("wait_for", (&future, 1.0)).unwrap();
            let result = event_loop
                .call_method1("run_until_complete", (wait_for,))
                .unwrap();
            wake_pump.close();
            wake_pump.join(Some(py));
            event_loop.call_method0("close").unwrap();

            assert!(result.is_none());
        });
    }

    #[rstest]
    fn test_host_wake_pump_stops_node_when_loop_is_closed() {
        Python::initialize();

        Python::attach(|py| {
            let event_loop = py
                .import("asyncio")
                .unwrap()
                .call_method0("new_event_loop")
                .unwrap();
            event_loop.call_method0("close").unwrap();
            let state = Arc::new(super::RunWakeState::default());
            let wake_callback = Py::new(py, super::PyNodeRunWake { state })
                .unwrap()
                .into_any();
            let handle = crate::node::LiveNodeHandle::new();
            let mut wake_pump =
                super::HostWakePump::start(event_loop.unbind(), wake_callback, handle.clone())
                    .unwrap();

            wake_pump.waker(1).wake_by_ref();
            wake_pump.join(Some(py));

            assert!(handle.should_stop());
        });
    }

    #[rstest]
    fn test_hosted_run_completes_after_stop() {
        Python::initialize();

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_delay_shutdown_secs(0)
            .with_timeout_connection(0)
            .with_timeout_disconnection_secs(0)
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        Python::attach(|py| {
            let locals = PyDict::new(py);
            let py_node = Py::new(py, node).unwrap();
            locals.set_item("node", &py_node).unwrap();
            py.run(
                pyo3::ffi::c_str!(
                    "import asyncio\n\nasync def run_node():\n    handle = node.handle()\n    asyncio.get_running_loop().call_soon(handle.stop)\n    await asyncio.wait_for(node.run_async(), timeout=5)\n\nasyncio.run(run_node())"
                ),
                Some(&locals),
                None,
            )
            .unwrap();

            let node = py_node.borrow(py);
            assert!(!node.is_consumed());
            assert_eq!(
                node.node().unwrap().state(),
                crate::node::NodeState::Stopped
            );
        });
        get_message_bus().borrow_mut().dispose();
    }

    #[rstest]
    fn test_build_routes_venue_less_data_client_with_venue_routing() {
        Python::initialize();

        let routing = RoutingConfig::builder()
            .venues(vec!["IBIS".to_string()])
            .build();
        let node = LiveNode::builder(TraderId::from("TEST-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_timeout_connection(1)
            .add_data_client_with_routing(
                Some("IB".to_string()),
                Box::new(VenueLessDataClientFactory),
                Box::new(TestDataClientConfig),
                routing,
            )
            .unwrap()
            .build();

        assert!(node.is_ok(), "build should succeed: {:?}", node.err());
    }

    #[rstest]
    fn test_build_routes_venue_less_data_client_with_default_and_venues() {
        Python::initialize();

        let routing = RoutingConfig::builder()
            .default(true)
            .venues(vec!["IBIS".to_string()])
            .build();
        let node = LiveNode::builder(TraderId::from("TEST-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_timeout_connection(1)
            .add_data_client_with_routing(
                Some("IB".to_string()),
                Box::new(VenueLessDataClientFactory),
                Box::new(TestDataClientConfig),
                routing,
            )
            .unwrap()
            .build();

        assert!(node.is_ok(), "build should succeed: {:?}", node.err());
    }

    #[rstest]
    fn test_stop_live_node_detached_releases_gil() {
        Python::initialize();

        let node = LiveNode::builder(TraderId::from("TESTER-002"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(1)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        get_runtime()
            .block_on(async { node.node_mut().unwrap().start().await })
            .expect("node should start");

        let (attempt_tx, attempt_rx) = mpsc::channel();
        let acquired_before_stop_return = Arc::new(AtomicBool::new(false));
        let acquired_before_stop_return_for_thread = acquired_before_stop_return.clone();
        let stop_returned = Arc::new(AtomicBool::new(false));
        let stop_returned_for_thread = stop_returned.clone();
        let mut gil_thread = None;

        Python::attach(|py| {
            gil_thread = Some(thread::spawn(move || {
                attempt_tx
                    .send(())
                    .expect("GIL acquisition attempt should send");
                Python::attach(|_| {});

                if !stop_returned_for_thread.load(Ordering::SeqCst) {
                    acquired_before_stop_return_for_thread.store(true, Ordering::SeqCst);
                }
            }));

            attempt_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("worker thread should attempt to acquire the GIL");

            super::stop_live_node_detached(py, &mut node.node_mut().unwrap())
                .expect("node should stop cleanly");
            stop_returned.store(true, Ordering::SeqCst);
        });

        gil_thread
            .expect("GIL worker thread should be spawned")
            .join()
            .expect("GIL worker thread should join");

        assert!(
            acquired_before_stop_return.load(Ordering::SeqCst),
            "worker thread should acquire the GIL while LiveNode::stop is blocked"
        );
        assert!(!node.node_mut().unwrap().is_running());
    }

    #[rstest]
    fn test_py_dispose_disposes_kernel_after_stop_error() {
        Python::initialize();

        let dispose_count = Arc::new(AtomicUsize::new(0));
        let factory = TestDisconnectFailureDataClientFactory::new(dispose_count.clone());
        let config = TestDataClientConfig;
        let node = LiveNode::builder(TraderId::from("TESTER-003"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .with_timeout_disconnection_secs(0)
            .add_data_client(
                Some("TEST_DISCONNECT_FAILURE".to_string()),
                Box::new(factory),
                Box::new(config),
            )
            .unwrap()
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        let dispose_result = Python::attach(|py| {
            get_runtime()
                .block_on(async { node.node_mut().unwrap().start().await })
                .expect("node should start");
            assert!(node.node_mut().unwrap().is_running());

            node.py_dispose(py)
        });

        let error = dispose_result.expect_err("dispose should return the stop error");

        assert!(error.to_string().contains("test disconnect failed"));
        assert_eq!(dispose_count.load(Ordering::Relaxed), 1);
        assert!(!node.node_mut().unwrap().is_running());
    }

    #[rstest]
    fn test_live_node_pystrategy_timer_callbacks_run_on_event_loop() {
        Python::initialize();

        let module_name = "test_live_node_timer_strategy";
        Python::attach(|py| install_timer_strategy_module(py, module_name));

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        let importable = ImportableStrategyConfig {
            strategy_path: format!("{module_name}:LiveTimerStrategy"),
            config_path: String::new(),
            config: HashMap::new(),
        };

        Python::attach(|py| {
            node.py_add_strategy_from_config(py, importable)
                .expect("strategy should register");
        });

        let handle = node.node_mut().unwrap().handle();
        let stop_handle = handle.clone();
        let watchdog_handle = handle;
        let (done_tx, done_rx) = mpsc::channel();
        let module_name_for_stop = module_name.to_string();

        let stop_thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);

            loop {
                let fired = Python::attach(|py| {
                    let results = get_timer_results(py, &module_name_for_stop);
                    results.callback_timer_count > 0 && results.default_timer_count > 0
                });

                if fired || Instant::now() >= deadline {
                    break;
                }

                thread::sleep(Duration::from_millis(20));
            }

            stop_handle.stop();
        });

        let watchdog_thread = thread::spawn(move || {
            if done_rx.recv_timeout(Duration::from_secs(5)).is_err() {
                watchdog_handle.stop();
            }
        });

        Python::attach(|py| {
            super::run_live_node_detached(py, &mut node.node_mut().unwrap())
                .expect("node should run cleanly");
        });

        let _ = done_tx.send(());
        stop_thread.join().expect("stop thread should join");
        watchdog_thread.join().expect("watchdog thread should join");

        let results = Python::attach(|py| get_timer_results(py, module_name));

        assert_eq!(results.on_start, 1);
        assert!(results.callback_timer_count > 0);
        assert!(results.default_timer_count > 0);
        assert_eq!(results.callback_event_type, "TimeEvent");
        assert_eq!(results.default_event_type, "TimeEvent");
        assert_eq!(results.callback_event_name, "explicit_timer");
        assert_eq!(results.default_event_name, "default_timer");
    }

    #[rstest]
    fn test_add_actor_registers_constructed_python_instance() {
        Python::initialize();

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();
        let actor_id = ActorId::from("ACTOR-INSTANCE-001");

        Python::attach(|py| {
            let config = py
                .eval(c_str!("type('_Cfg', (), {})()"), None, None)
                .unwrap();
            config.setattr("actor_id", actor_id.to_string()).unwrap();
            let actor = py
                .get_type::<PyDataActor>()
                .as_any()
                .call1((config,))
                .unwrap();

            node.py_add_actor(&actor).expect("actor should register");

            let actor = actor.extract::<PyRef<PyDataActor>>().unwrap();
            assert_eq!(actor.actor_id(), actor_id);
            assert!(actor.is_registered());
        });

        assert_eq!(
            node.node_mut()
                .unwrap()
                .kernel()
                .trader
                .borrow()
                .actor_ids(),
            vec![actor_id]
        );
    }

    #[rstest]
    fn test_add_actor_rejects_non_idle_node() {
        Python::initialize();

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        Python::attach(|py| {
            let actor = py.get_type::<PyDataActor>().call0().unwrap();
            node.handle.set_starting();

            let error = node
                .py_add_actor(&actor)
                .expect_err("a non-idle node should reject actor registration");
            let actor = actor.extract::<PyRef<PyDataActor>>().unwrap();

            assert_eq!(
                error.to_string(),
                "RuntimeError: Cannot add actor while node is running, add actors before running the node"
            );
            assert!(!actor.is_registered());
        });

        assert!(
            node.node()
                .unwrap()
                .kernel()
                .trader
                .borrow()
                .actor_ids()
                .is_empty()
        );
    }

    #[rstest]
    fn test_add_strategy_from_config_registers_external_order_instrument_ids() {
        Python::initialize();

        let module_name = "test_live_node_claim_strategy";
        Python::attach(|py| install_claim_strategy_module(py, module_name));

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        let instrument_id = InstrumentId::from("AUDUSD.SIM");
        let strategy_id = StrategyId::from("CLAIMS-001");
        let mut config = HashMap::new();
        config.insert(
            "strategy_id".to_string(),
            serde_json::json!(strategy_id.to_string()),
        );
        config.insert(
            "external_order_instrument_ids".to_string(),
            serde_json::json!([instrument_id.to_string()]),
        );
        let importable = ImportableStrategyConfig {
            strategy_path: format!("{module_name}:ClaimsStrategy"),
            config_path: format!("{module_name}:ClaimsConfig"),
            config,
        };

        Python::attach(|py| {
            node.py_add_strategy_from_config(py, importable)
                .expect("strategy should register");
        });

        {
            let guard = node.node_mut().unwrap();
            let exec_engine = guard.kernel().exec_engine.borrow();
            assert_eq!(
                exec_engine.get_external_order_claim(&instrument_id),
                Some(strategy_id)
            );
        }

        let result = node
            .node_mut()
            .unwrap()
            .exec_manager_mut()
            .claim_external_orders(instrument_id, StrategyId::from("OTHER-001"));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("already exists for CLAIMS-001")
        );
    }

    #[rstest]
    fn test_add_strategy_registers_constructed_python_instance() {
        Python::initialize();

        let module_name = "test_live_node_add_strategy_instance";
        Python::attach(|py| install_claim_strategy_module(py, module_name));

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        let instrument_id = InstrumentId::from("AUDUSD.SIM");
        let strategy_id = StrategyId::from("CLAIMS-002");

        Python::attach(|py| {
            let module = py.import(module_name).expect("test module should import");
            let kwargs = PyDict::new(py);
            kwargs
                .set_item("strategy_id", strategy_id.to_string())
                .unwrap();
            kwargs
                .set_item(
                    "external_order_instrument_ids",
                    vec![instrument_id.to_string()],
                )
                .unwrap();
            let config = module
                .getattr("ClaimsConfig")
                .unwrap()
                .call((), Some(&kwargs))
                .unwrap();
            let strategy = module
                .getattr("ClaimsStrategy")
                .unwrap()
                .call1((config,))
                .unwrap();

            node.py_add_strategy(&strategy)
                .expect("strategy should register");
        });

        {
            let guard = node.node_mut().unwrap();
            let exec_engine = guard.kernel().exec_engine.borrow();
            assert_eq!(
                exec_engine.get_external_order_claim(&instrument_id),
                Some(strategy_id)
            );
        }
    }

    #[rstest]
    fn test_add_strategy_constructed_python_instance_registers_oms_type() {
        Python::initialize();

        let module_name = "test_live_node_add_strategy_instance_oms_type";
        Python::attach(|py| install_claim_strategy_module(py, module_name));

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();
        let strategy_id = StrategyId::from("FUNDING_ARBITRAGE-003");

        Python::attach(|py| {
            let module = py.import(module_name).expect("test module should import");
            let kwargs = PyDict::new(py);
            kwargs
                .set_item("strategy_id", strategy_id.to_string())
                .unwrap();
            kwargs.set_item("oms_type", OmsType::Hedging).unwrap();
            let config = module
                .getattr("ClaimsConfig")
                .unwrap()
                .call((), Some(&kwargs))
                .unwrap();
            let strategy = module
                .getattr("ClaimsStrategy")
                .unwrap()
                .call1((config,))
                .unwrap();

            node.py_add_strategy(&strategy)
                .expect("strategy should register");
        });

        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let client_id = ClientId::from("STUB");

        node.node_mut()
            .unwrap()
            .kernel()
            .cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(instrument))
            .unwrap();
        node.node_mut()
            .unwrap()
            .kernel()
            .exec_engine
            .borrow_mut()
            .register_client(Box::new(StubExecutionClient::new(
                client_id,
                AccountId::from("TEST-ACCOUNT"),
                instrument_id.venue,
                OmsType::Netting,
                None,
            )))
            .unwrap();

        let order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(node.node_mut().unwrap().trader_id())
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .quantity(Quantity::from("1.000"))
            .build();
        let position_id = PositionId::new("CUSTOM-POSITION-003");

        node.node_mut()
            .unwrap()
            .kernel()
            .cache
            .borrow_mut()
            .add_order(order.clone(), Some(position_id), Some(client_id), true)
            .unwrap();

        let submit_order = SubmitOrder::new(
            order.trader_id(),
            Some(client_id),
            strategy_id,
            instrument_id,
            order.client_order_id(),
            order.init_event().clone(),
            order.exec_algorithm_id(),
            Some(position_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        node.node_mut()
            .unwrap()
            .kernel()
            .exec_engine
            .borrow()
            .execute(TradingCommand::SubmitOrder(submit_order));

        let guard = node.node_mut().unwrap();
        let exec_engine = guard.kernel().exec_engine.borrow();
        let cache = exec_engine.cache().borrow();
        let cached_order = cache
            .order(&order.client_order_id())
            .expect("Order should be cached");

        assert_eq!(cached_order.status(), OrderStatus::Initialized);
    }

    #[rstest]
    fn test_add_strategy_constructed_python_instance_claim_conflict_does_not_register() {
        Python::initialize();

        let module_name = "test_live_node_add_strategy_instance_claim_conflict";
        Python::attach(|py| install_claim_strategy_module(py, module_name));

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();
        let instrument_id = InstrumentId::from("AUDUSD.SIM");
        let first_strategy_id = StrategyId::from("CLAIMS-PRIMARY-001");
        let conflicting_strategy_id = StrategyId::from("CLAIMS-CONFLICT-002");

        let (error, conflicting_strategy_registered) = Python::attach(|py| {
            let module = py.import(module_name).expect("test module should import");
            let first_kwargs = PyDict::new(py);
            first_kwargs
                .set_item("strategy_id", first_strategy_id.to_string())
                .unwrap();
            first_kwargs
                .set_item(
                    "external_order_instrument_ids",
                    vec![instrument_id.to_string()],
                )
                .unwrap();
            let first_config = module
                .getattr("ClaimsConfig")
                .unwrap()
                .call((), Some(&first_kwargs))
                .unwrap();
            let first_strategy = module
                .getattr("ClaimsStrategy")
                .unwrap()
                .call1((first_config,))
                .unwrap();
            node.py_add_strategy(&first_strategy)
                .expect("first strategy should register");

            let conflicting_kwargs = PyDict::new(py);
            conflicting_kwargs
                .set_item("strategy_id", conflicting_strategy_id.to_string())
                .unwrap();
            conflicting_kwargs
                .set_item(
                    "external_order_instrument_ids",
                    vec![instrument_id.to_string()],
                )
                .unwrap();
            let conflicting_config = module
                .getattr("ClaimsConfig")
                .unwrap()
                .call((), Some(&conflicting_kwargs))
                .unwrap();
            let conflicting_strategy = module
                .getattr("ClaimsStrategy")
                .unwrap()
                .call1((conflicting_config,))
                .unwrap();
            let error = node
                .py_add_strategy(&conflicting_strategy)
                .expect_err("conflicting claim should fail");
            let is_registered = conflicting_strategy
                .extract::<PyRef<PyStrategy>>()
                .unwrap()
                .is_registered();

            (error, is_registered)
        });

        let strategy_ids = node
            .node_mut()
            .unwrap()
            .kernel()
            .trader
            .borrow()
            .strategy_ids();
        let manager_claim = node
            .node_mut()
            .unwrap()
            .exec_manager()
            .get_external_order_claim(&instrument_id);
        let engine_claim = node
            .node_mut()
            .unwrap()
            .kernel()
            .exec_engine
            .borrow()
            .get_external_order_claim(&instrument_id);

        assert!(
            error
                .to_string()
                .contains("already exists for CLAIMS-PRIMARY-001")
        );
        assert!(!conflicting_strategy_registered);
        assert_eq!(strategy_ids, vec![first_strategy_id]);
        assert_eq!(manager_claim, Some(first_strategy_id));
        assert_eq!(engine_claim, Some(first_strategy_id));
    }

    #[rstest]
    fn test_add_strategy_constructed_python_instance_duplicate_tag_does_not_register() {
        Python::initialize();

        let module_name = "test_live_node_add_strategy_instance_duplicate_tag";
        Python::attach(|py| install_claim_strategy_module(py, module_name));

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .build()
            .map(PyLiveNode::new)
            .unwrap();
        let first_strategy_id = StrategyId::from("TAGGED-FIRST-777");
        let instrument_id = InstrumentId::from("AUDUSD.SIM");

        let (error, duplicate_strategy_registered) = Python::attach(|py| {
            let module = py.import(module_name).expect("test module should import");
            let first_kwargs = PyDict::new(py);
            first_kwargs
                .set_item("strategy_id", "TAGGED-FIRST")
                .unwrap();
            first_kwargs.set_item("order_id_tag", "777").unwrap();
            let first_config = module
                .getattr("ClaimsConfig")
                .unwrap()
                .call((), Some(&first_kwargs))
                .unwrap();
            let first_strategy = module
                .getattr("ClaimsStrategy")
                .unwrap()
                .call1((first_config,))
                .unwrap();
            node.py_add_strategy(&first_strategy)
                .expect("first strategy should register");

            let duplicate_kwargs = PyDict::new(py);
            duplicate_kwargs
                .set_item("strategy_id", "TAGGED-SECOND")
                .unwrap();
            duplicate_kwargs.set_item("order_id_tag", "777").unwrap();
            duplicate_kwargs
                .set_item(
                    "external_order_instrument_ids",
                    vec![instrument_id.to_string()],
                )
                .unwrap();
            let duplicate_config = module
                .getattr("ClaimsConfig")
                .unwrap()
                .call((), Some(&duplicate_kwargs))
                .unwrap();
            let duplicate_strategy = module
                .getattr("ClaimsStrategy")
                .unwrap()
                .call1((duplicate_config,))
                .unwrap();
            let error = node
                .py_add_strategy(&duplicate_strategy)
                .expect_err("duplicate order ID tag should fail");
            let is_registered = duplicate_strategy
                .extract::<PyRef<PyStrategy>>()
                .unwrap()
                .is_registered();

            (error, is_registered)
        });

        let strategy_ids = node
            .node_mut()
            .unwrap()
            .kernel()
            .trader
            .borrow()
            .strategy_ids();

        assert!(error.to_string().contains("order_id_tag conflict"));
        assert!(!duplicate_strategy_registered);
        assert_eq!(strategy_ids, vec![first_strategy_id]);
        assert_eq!(
            node.node_mut()
                .unwrap()
                .kernel()
                .cache
                .borrow()
                .external_order_claim(&instrument_id),
            None
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_live_node_pystrategy_request_bars_dispatches_on_historical_bars() {
        Python::initialize();

        let module_name = "test_live_node_historical_bars_strategy";
        Python::attach(|py| install_tracking_strategy_module(py, module_name));

        let request_count = Arc::new(AtomicUsize::new(0));
        let response_sent_count = Arc::new(AtomicUsize::new(0));
        let handler_visible_count = Arc::new(AtomicUsize::new(0));
        let factory = TestHistoricalBarsDataClientFactory::new(
            request_count.clone(),
            response_sent_count.clone(),
            handler_visible_count.clone(),
        );
        let config = TestDataClientConfig;

        let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .add_data_client(
                Some("TEST_DATA".to_string()),
                Box::new(factory),
                Box::new(config),
            )
            .unwrap()
            .build()
            .map(PyLiveNode::new)
            .unwrap();

        let importable = ImportableStrategyConfig {
            strategy_path: format!("{module_name}:HistoricalBarsStrategy"),
            config_path: String::new(),
            config: HashMap::new(),
        };

        Python::attach(|py| {
            node.py_add_strategy_from_config(py, importable)
                .expect("strategy should register");
        });

        let handle = node.node_mut().unwrap().handle();
        let stop_handle = handle.clone();
        let response_sent_count_for_stop = response_sent_count.clone();

        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

            loop {
                if response_sent_count_for_stop.load(Ordering::Relaxed) == 1
                    || tokio::time::Instant::now() >= deadline
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            stop_handle.stop();
        });

        node.node_mut()
            .unwrap()
            .run()
            .await
            .expect("node should run cleanly");

        let (on_start, on_historical_bars, historical_bar_count) =
            Python::attach(|py| get_results(py, module_name));

        assert_eq!(request_count.load(Ordering::Relaxed), 1);
        assert_eq!(handler_visible_count.load(Ordering::Relaxed), 1);
        assert_eq!(response_sent_count.load(Ordering::Relaxed), 1);
        assert_eq!(on_start, 1);
        assert_eq!(on_historical_bars, 1);
        assert_eq!(historical_bar_count, 1);
    }
}
