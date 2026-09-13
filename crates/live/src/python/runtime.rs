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

//! Owns Python adapter operations on the node's asyncio event loop.
//!
//! Admission, command ordering, lifecycle transitions, and terminal task ownership live here.
//! Asyncio executes coroutines through a small driver; it does not own client lifecycle policy.

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
    thread::{self, ThreadId},
};

use ahash::AHashMap;
use nautilus_core::python::{to_pyruntime_err, to_pytype_err};
use parking_lot::Mutex;
use pyo3::{
    prelude::*,
    sync::PyOnceLock,
    types::{PyCFunction, PyDict, PyTuple},
};

const COMMAND_CAPACITY: usize = 1024;

/// Supervises a Python client's operations until their terminal results are retrieved.
#[pyclass(
    module = "nautilus_trader.live",
    name = "_ClientRuntime",
    frozen,
    weakref
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
#[derive(Debug)]
pub struct ClientRuntime {
    owner: ThreadId,
    client: Py<PyAny>,
    name: String,
    logger: Py<PyAny>,
    state: Mutex<RuntimeState>,
}

// These references deliberately root pending work across Python garbage collection. Completion
// removes each task record and releases the active client, breaking the cycle without a global set.
#[derive(Debug, Default)]
struct RuntimeState {
    event_loop: Option<Py<PyAny>>,
    capacity: usize,
    queue: VecDeque<(String, Py<PyTuple>)>,
    tasks: AHashMap<usize, TaskEntry>,
    worker: Option<Py<PyAny>>,
    client_active: Option<Py<PyAny>>,
    accepting: bool,
    connected: bool,
    disposed: bool,
}

#[derive(Debug)]
struct TaskEntry {
    task: Py<PyAny>,
    operation: String,
    driver: Py<RuntimeOperation>,
    awaited: bool,
    cancellation_requested: bool,
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl ClientRuntime {
    #[new]
    #[pyo3(signature = (client, capacity=COMMAND_CAPACITY))]
    fn py_new(py: Python<'_>, client: &Bound<'_, PyAny>, capacity: usize) -> PyResult<Self> {
        let name = client.getattr("client_id")?.str()?.to_string();
        let logger = py
            .import("nautilus_trader.common")?
            .getattr("Logger")?
            .call1((&name,))?
            .unbind();
        let client = py
            .import("weakref")?
            .getattr("ref")?
            .call1((client,))?
            .unbind();
        Ok(Self {
            owner: thread::current().id(),
            client,
            name,
            logger,
            state: Mutex::new(RuntimeState {
                capacity,
                ..RuntimeState::default()
            }),
        })
    }

    #[getter]
    fn connected(&self) -> bool {
        self.state.lock().connected
    }

    #[getter]
    fn complete(&self) -> bool {
        let state = self.state.lock();
        state.tasks.is_empty() && state.queue.is_empty()
    }

    #[getter]
    fn event_loop(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.state
            .lock()
            .event_loop
            .as_ref()
            .map(|value| value.clone_ref(py))
    }

    fn bind(&self, py: Python<'_>, event_loop: Py<PyAny>) -> PyResult<()> {
        self.check_thread()?;
        {
            let state = self.state.lock();
            if state.event_loop.is_some() || state.disposed {
                return Err(to_pyruntime_err(format!(
                    "Client {} can only run once",
                    self.name
                )));
            }
        }

        if !event_loop
            .bind(py)
            .is(&py.import("asyncio")?.call_method0("get_running_loop")?)
        {
            return Err(to_pyruntime_err(
                "Client must bind to the running owner loop",
            ));
        }

        let mut state = self.state.lock();
        state.event_loop = Some(event_loop);
        state.accepting = true;
        Ok(())
    }

    #[pyo3(signature = (operation, args=None))]
    fn call(
        slf: &Bound<'_, Self>,
        operation: String,
        args: Option<Py<PyTuple>>,
    ) -> PyResult<Py<PyAny>> {
        Self::invoke_task(slf, operation, args, false)
    }

    #[pyo3(signature = (operation, args=None))]
    fn call_awaited(
        slf: &Bound<'_, Self>,
        operation: String,
        args: Option<Py<PyTuple>>,
    ) -> PyResult<Py<PyAny>> {
        Self::invoke_task(slf, operation, args, true)
    }

    #[pyo3(signature = (operation, args=None))]
    fn admit(slf: &Bound<'_, Self>, operation: String, args: Option<Py<PyTuple>>) -> PyResult<()> {
        let py = slf.py();
        slf.get().check_bound(py)?;
        let args = args.unwrap_or_else(|| PyTuple::empty(py).unbind());

        let needs_worker = {
            let mut state = slf.get().state.lock();
            if !state.accepting {
                return Err(slf.get().shutting_down());
            }

            if state.queue.len() >= state.capacity {
                return Err(to_pyruntime_err(format!(
                    "Client {} command queue is full",
                    slf.get().name
                )));
            }

            state.queue.push_back((operation, args));
            state.worker.is_none()
        };

        if needs_worker {
            match Self::schedule(slf, OperationKind::Dispatch, "commands".into(), false) {
                Ok(task) => slf.get().state.lock().worker = Some(task),
                Err(e) => {
                    let rejected = slf.get().state.lock().queue.pop_back();
                    drop(rejected);
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    #[pyo3(signature = (coroutine, operation="background"))]
    fn create_task(
        slf: &Bound<'_, Self>,
        coroutine: Py<PyAny>,
        operation: &str,
    ) -> PyResult<Py<PyAny>> {
        Self::schedule(
            slf,
            OperationKind::Coroutine(coroutine),
            operation.into(),
            false,
        )
    }

    fn lifecycle(slf: &Bound<'_, Self>, operation: &str) -> PyResult<Py<PyAny>> {
        let kind = match operation {
            "connect" => OperationKind::Connect,
            "disconnect" => OperationKind::Disconnect,
            _ => return Err(to_pytype_err("Expected connect or disconnect")),
        };

        Self::schedule(slf, kind, operation.into(), true)
    }

    fn connect(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        Self::coroutine(slf, OperationKind::Connect)
    }

    fn abandon(&self, py: Python<'_>, task: &Bound<'_, PyAny>) -> PyResult<()> {
        self.check_thread()?;

        let owned = {
            let mut state = self.state.lock();
            if let Some(entry) = state.tasks.get_mut(&(task.as_ptr() as usize)) {
                entry.awaited = false;
                true
            } else {
                false
            }
        };

        if task.call_method0("done")?.extract::<bool>()? {
            if owned {
                self.completed(py, task)?;
            } else {
                self.retrieve(
                    py,
                    task,
                    &task.call_method0("get_name")?.extract::<String>()?,
                    true,
                )?;
            }
        } else if owned && !self.loop_closed(py)? {
            self.cancel_task(task)?;
        }

        Ok(())
    }

    fn dispose(&self, py: Python<'_>) -> PyResult<()> {
        self.check_thread()?;

        let (already_disposed, tasks) = {
            let mut state = self.state.lock();
            let disposed = state.disposed;
            state.disposed = true;
            state.accepting = false;
            (
                disposed,
                state
                    .tasks
                    .values()
                    .map(|entry| entry.task.clone_ref(py))
                    .collect::<Vec<_>>(),
            )
        };

        self.discard_commands(py)?;
        for task in tasks {
            if task.call_method0(py, "done")?.extract::<bool>(py)? {
                self.completed(py, task.bind(py))?;
            } else if !already_disposed && !self.loop_closed(py)? {
                self.cancel_task(task.bind(py))?;
            }
        }

        if !already_disposed && !self.complete() {
            let count = self.state.lock().tasks.len();
            self.log(
                py,
                &format!("Client {} cleanup is incomplete ({count} tasks)", self.name),
            )?;
        }

        Ok(())
    }
}

impl ClientRuntime {
    fn invoke_task(
        slf: &Bound<'_, Self>,
        operation: String,
        args: Option<Py<PyTuple>>,
        awaited: bool,
    ) -> PyResult<Py<PyAny>> {
        let args = args.unwrap_or_else(|| PyTuple::empty(slf.py()).unbind());
        let name = operation.clone();
        Self::schedule(slf, OperationKind::Invoke(operation, args), name, awaited)
    }

    fn schedule(
        slf: &Bound<'_, Self>,
        kind: OperationKind,
        operation: String,
        awaited: bool,
    ) -> PyResult<Py<PyAny>> {
        let py = slf.py();

        let validation = (|| {
            slf.get().check_bound(py)?;
            if !slf.get().state.lock().accepting {
                return Err(slf.get().shutting_down());
            }

            if let OperationKind::Coroutine(ref coroutine) = kind
                && !py
                    .import("inspect")?
                    .call_method1("iscoroutine", (coroutine,))?
                    .extract::<bool>()?
            {
                return Err(to_pytype_err("Expected a coroutine"));
            }

            slf.get().get_client(py)
        })();

        let client = match validation {
            Ok(client) => client,
            Err(e) => {
                if let OperationKind::Coroutine(ref coroutine) = kind
                    && py
                        .import("inspect")?
                        .call_method1("iscoroutine", (coroutine,))?
                        .extract::<bool>()?
                {
                    coroutine.call_method0(py, "close")?;
                }

                return Err(e);
            }
        };

        let driver = Py::new(py, RuntimeOperation::new(slf.clone().unbind(), kind))?;
        let wrapper = coroutine_driver(py)?.call1(py, (driver.clone_ref(py),))?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("name", format!("{}:{operation}", slf.get().name))?;
        let event_loop = slf.get().event_loop(py).expect("validated event loop");

        let task = match event_loop.call_method(py, "create_task", (&wrapper,), Some(&kwargs)) {
            Ok(task) => task,
            Err(e) => {
                wrapper.call_method0(py, "close")?;
                driver.borrow_mut(py).close(py)?;
                return Err(e);
            }
        };

        {
            let mut state = slf.get().state.lock();
            state.tasks.insert(
                task.as_ptr() as usize,
                TaskEntry {
                    task: task.clone_ref(py),
                    operation,
                    driver,
                    awaited,
                    cancellation_requested: false,
                },
            );

            state.client_active = Some(client);
        }

        Self::track_cancellation(slf, task.bind(py))?;

        // Release ownership while attached, before the callback capsule is destroyed
        let runtime = Mutex::new(Some(slf.clone().unbind()));

        let callback = pyo3::types::PyCFunction::new_closure(
            py,
            None,
            None,
            move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| {
                let owner = runtime.lock().take();
                if let Some(owner) = owner {
                    owner.get().completed(args.py(), &args.get_item(0)?)?;
                }

                Ok::<(), PyErr>(())
            },
        )?;

        task.call_method1(py, "add_done_callback", (callback,))?;
        Ok(task)
    }

    fn track_cancellation(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<()> {
        let py = slf.py();
        let owner = py
            .import("weakref")?
            .getattr("ref")?
            .call1((slf,))?
            .unbind();
        let task_id = task.as_ptr() as usize;

        let notify = PyCFunction::new_closure(
            py,
            None,
            None,
            move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| {
                let runtime = owner.call0(args.py())?;
                if !runtime.is_none(args.py()) {
                    let runtime = runtime.bind(args.py()).cast::<Self>()?;
                    if let Some(entry) = runtime.get().state.lock().tasks.get_mut(&task_id) {
                        if args.get_item(0)?.extract::<bool>()? && entry.cancellation_requested {
                            return Ok(false);
                        }

                        entry.cancellation_requested = true;
                    }
                }

                Ok::<bool, PyErr>(true)
            },
        )?;

        py.import("nautilus_trader.live._coroutine")?
            .call_method1("track_cancellation", (task, notify))?;
        Ok(())
    }

    fn cancel_task(&self, task: &Bound<'_, PyAny>) -> PyResult<()> {
        let request = self
            .state
            .lock()
            .tasks
            .get(&(task.as_ptr() as usize))
            .is_some_and(|entry| !entry.cancellation_requested);

        if request && !task.call_method0("done")?.extract::<bool>()? {
            task.py()
                .import("nautilus_trader.live._coroutine")?
                .call_method1("cancel_supervised", (task,))?;
        }

        Ok(())
    }

    fn coroutine(slf: &Bound<'_, Self>, kind: OperationKind) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let operation = Py::new(py, RuntimeOperation::new(slf.clone().unbind(), kind))?;
        coroutine_driver(py)?.call1(py, (operation,))
    }

    fn completed(&self, py: Python<'_>, task: &Bound<'_, PyAny>) -> PyResult<()> {
        let entry = self.state.lock().tasks.remove(&(task.as_ptr() as usize));
        if let Some(entry) = entry {
            entry.driver.borrow_mut(py).close(py)?;
            self.retrieve(py, task, &entry.operation, !entry.awaited)?;

            // Release Python references outside the lock: finalizers can re-enter the runtime
            let released = {
                let mut state = self.state.lock();
                if state.tasks.is_empty() {
                    state.client_active.take()
                } else {
                    None
                }
            };

            drop(released);
        }

        Ok(())
    }

    fn retrieve(
        &self,
        py: Python<'_>,
        task: &Bound<'_, PyAny>,
        operation: &str,
        log_failure: bool,
    ) -> PyResult<()> {
        // Task.exception() consumes the cancellation message needed by a subsequent awaiter
        if task.call_method0("cancelled")?.extract::<bool>()? {
            return Ok(());
        }

        // Inspect without re-raising: repeated Task.result() calls can replace the traceback
        let exception = task.call_method0("exception")?;

        if log_failure && !exception.is_none() {
            self.log_error(py, operation, &PyErr::from_value(exception))?;
        }

        Ok(())
    }

    fn log_error(&self, py: Python<'_>, operation: &str, e: &PyErr) -> PyResult<()> {
        let traceback = py
            .import("traceback")?
            .call_method1(
                "format_exception",
                (e.get_type(py), e.value(py), e.traceback(py)),
            )?
            .extract::<Vec<String>>()?
            .concat();
        self.log(
            py,
            &format!(
                "Client {} operation {operation} failed\n{traceback}",
                self.name
            ),
        )
    }

    fn log(&self, py: Python<'_>, message: &str) -> PyResult<()> {
        self.logger.call_method1(py, "error", (message,))?;
        Ok(())
    }

    fn discard_commands(&self, py: Python<'_>) -> PyResult<()> {
        let queue = std::mem::take(&mut self.state.lock().queue);
        for (operation, _) in queue {
            self.log(
                py,
                &format!(
                    "Client {} abandoned queued operation {operation}",
                    self.name
                ),
            )?;
        }

        Ok(())
    }

    fn get_client(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let client = self.client.call0(py)?;
        if client.is_none(py) {
            return Err(to_pyruntime_err(format!(
                "Client {} no longer exists",
                self.name
            )));
        }

        Ok(client)
    }

    fn invoke(
        &self,
        py: Python<'_>,
        operation: &str,
        args: &Bound<'_, PyTuple>,
    ) -> PyResult<Py<PyAny>> {
        self.get_client(py)?.call_method1(py, operation, args)
    }

    fn shutting_down(&self) -> PyErr {
        to_pyruntime_err(format!("Client {} is shutting down", self.name))
    }

    fn check_bound(&self, py: Python<'_>) -> PyResult<()> {
        self.check_thread()?;

        let event_loop = {
            let state = self.state.lock();
            if state.event_loop.is_none() || state.disposed {
                return Err(to_pyruntime_err(format!(
                    "Client {} is not bound to an active node",
                    self.name
                )));
            }

            state.event_loop.as_ref().expect("bound loop").clone_ref(py)
        };

        if event_loop
            .call_method0(py, "is_closed")?
            .extract::<bool>(py)?
            || !event_loop
                .bind(py)
                .is(&py.import("asyncio")?.call_method0("get_running_loop")?)
        {
            return Err(to_pyruntime_err(format!(
                "Client {} must use its bound event loop",
                self.name
            )));
        }

        Ok(())
    }

    fn check_thread(&self) -> PyResult<()> {
        if self.owner != thread::current().id() {
            return Err(to_pyruntime_err(format!(
                "Client {} must be used on its owner thread",
                self.name
            )));
        }

        Ok(())
    }

    fn loop_closed(&self, py: Python<'_>) -> PyResult<bool> {
        match self.event_loop(py) {
            Some(event_loop) => event_loop.call_method0(py, "is_closed")?.extract(py),
            None => Ok(true),
        }
    }
}

enum OperationKind {
    Coroutine(Py<PyAny>),
    Invoke(String, Py<PyTuple>),
    Dispatch,
    Connect,
    Disconnect,
}

#[derive(Clone, Copy)]
enum Phase {
    Start,
    Awaiting,
    Draining,
    Done,
}

#[pyclass(module = "nautilus_trader.live", name = "_ClientOperation")]
struct RuntimeOperation {
    runtime: Py<ClientRuntime>,
    kind: OperationKind,
    phase: Phase,
    current: Option<Py<PyAny>>,
    pending_error: Option<PyErr>,
    command: Option<String>,
    closed: bool,
}

#[pymethods]
impl RuntimeOperation {
    #[pyo3(signature = (value=None, error=None))]
    fn advance(
        &mut self,
        py: Python<'_>,
        value: Option<Py<PyAny>>,
        error: Option<Py<PyAny>>,
    ) -> PyResult<(bool, Py<PyAny>)> {
        self.runtime.get().check_thread()?;

        let result = match error {
            Some(e) => Err(PyErr::from_value(e.into_bound(py))),
            None => Ok(value.unwrap_or_else(|| py.None())),
        };

        if matches!(self.phase, Phase::Start) {
            self.phase = Phase::Awaiting;
            let runtime = self.runtime.get();

            let next = match &self.kind {
                OperationKind::Coroutine(coroutine) => Ok(coroutine.clone_ref(py)),
                OperationKind::Invoke(name, args) => runtime.invoke(py, name, args.bind(py)),
                OperationKind::Connect => runtime.invoke(py, "_connect", &PyTuple::empty(py)),
                OperationKind::Disconnect => {
                    let worker = {
                        let mut state = runtime.state.lock();
                        state.accepting = false;
                        state.worker.as_ref().map(|task| task.clone_ref(py))
                    };

                    runtime.discard_commands(py)?;
                    if let Some(worker) = worker {
                        runtime.cancel_task(worker.bind(py))?;
                    }

                    runtime.invoke(py, "_disconnect", &PyTuple::empty(py))
                }
                OperationKind::Dispatch => return self.next_command(py),
            };

            return match next {
                Ok(next) => Ok(self.suspend(py, next)),
                Err(e) => self.finish_step(py, Err(e)),
            };
        }

        self.current.take();
        self.finish_step(py, result)
    }

    fn close(&mut self, py: Python<'_>) -> PyResult<()> {
        if self.closed {
            return Ok(());
        }

        self.closed = true;
        self.pending_error.take();

        if let Some(current) = self.current.take()
            && current.bind(py).hasattr("close")?
        {
            current.call_method0(py, "close")?;
        }

        if let OperationKind::Coroutine(coroutine) = &self.kind {
            coroutine.call_method0(py, "close")?;
        }

        if matches!(self.kind, OperationKind::Dispatch) {
            let worker = self.runtime.get().state.lock().worker.take();
            drop(worker);
        }

        self.phase = Phase::Done;
        Ok(())
    }
}

impl RuntimeOperation {
    fn new(runtime: Py<ClientRuntime>, kind: OperationKind) -> Self {
        Self {
            runtime,
            kind,
            phase: Phase::Start,
            current: None,
            pending_error: None,
            command: None,
            closed: false,
        }
    }

    fn finish_step(
        &mut self,
        py: Python<'_>,
        result: PyResult<Py<PyAny>>,
    ) -> PyResult<(bool, Py<PyAny>)> {
        let runtime = self.runtime.get();

        match self.kind {
            OperationKind::Dispatch => {
                if let Err(e) = result {
                    self.command_failed(py, e)?;
                }

                self.next_command(py)
            }
            OperationKind::Connect => {
                result?;
                let mut state = runtime.state.lock();
                if !state.accepting || state.disposed {
                    return Err(to_pyruntime_err(format!(
                        "Client {} connect finished after shutdown",
                        runtime.name
                    )));
                }

                state.connected = true;
                self.phase = Phase::Done;
                Ok((true, py.None()))
            }
            OperationKind::Disconnect if !matches!(self.phase, Phase::Draining) => {
                self.pending_error = result.err();
                self.phase = Phase::Draining;
                let asyncio = py.import("asyncio")?;
                let current = asyncio.call_method0("current_task")?;
                let tasks = runtime
                    .state
                    .lock()
                    .tasks
                    .values()
                    .filter(|entry| !entry.task.bind(py).is(&current))
                    .map(|entry| entry.task.clone_ref(py))
                    .collect::<Vec<_>>();

                for task in &tasks {
                    runtime.cancel_task(task.bind(py))?;
                }

                if tasks.is_empty() {
                    return self.finish_step(py, Ok(py.None()));
                }

                let kwargs = PyDict::new(py);
                kwargs.set_item("return_exceptions", true)?;
                // Shield each owned task so cancelling the drain cannot interrupt its cleanup
                let shielded = tasks
                    .iter()
                    .map(|task| asyncio.call_method1("shield", (task,)))
                    .collect::<PyResult<Vec<_>>>()?;
                let gathered = asyncio
                    .call_method("gather", PyTuple::new(py, shielded)?, Some(&kwargs))?
                    .unbind();
                Ok(self.suspend(py, gathered))
            }
            OperationKind::Disconnect => {
                result?;

                if let Some(e) = self.pending_error.take() {
                    return Err(e);
                }

                runtime.state.lock().connected = false;
                self.phase = Phase::Done;
                Ok((true, py.None()))
            }
            _ => {
                self.phase = Phase::Done;
                result.map(|value| (true, value))
            }
        }
    }

    fn next_command(&mut self, py: Python<'_>) -> PyResult<(bool, Py<PyAny>)> {
        loop {
            // A reused dispatcher starts the next command with no consumed cancellation requests
            let task = py.import("asyncio")?.call_method0("current_task")?;
            while task.call_method0("cancelling")?.extract::<usize>()? != 0 {
                task.call_method0("uncancel")?;
            }

            if let Some(entry) = self
                .runtime
                .get()
                .state
                .lock()
                .tasks
                .get_mut(&(task.as_ptr() as usize))
            {
                entry.cancellation_requested = false;
            }

            let entry = self.runtime.get().state.lock().queue.pop_front();

            let Some((name, args)) = entry else {
                self.phase = Phase::Done;
                return Ok((true, py.None()));
            };

            self.command = Some(name.clone());
            match self.runtime.get().invoke(py, &name, args.bind(py)) {
                Ok(coroutine) => return Ok(self.suspend(py, coroutine)),
                Err(e) => {
                    self.command_failed(py, e)?;
                }
            }
        }
    }

    fn command_failed(&self, py: Python<'_>, e: PyErr) -> PyResult<()> {
        if is_cancelled(py, &e)? {
            if !self.runtime.get().state.lock().accepting {
                return Err(e);
            }
        } else if !e.is_instance_of::<pyo3::exceptions::PyException>(py) {
            return Err(e);
        }

        self.runtime
            .get()
            .log_error(py, self.command.as_deref().unwrap_or("commands"), &e)
    }

    fn suspend(&mut self, py: Python<'_>, value: Py<PyAny>) -> (bool, Py<PyAny>) {
        self.current = Some(value.clone_ref(py));
        (false, value)
    }
}

pub(crate) struct PythonOperation {
    runtime: Py<PyAny>,
    task: Py<PyAny>,
    waker: Arc<Mutex<Option<Waker>>>,
    complete: bool,
}

impl PythonOperation {
    pub(crate) fn new(py: Python<'_>, task: Py<PyAny>, runtime: Py<PyAny>) -> PyResult<Self> {
        let waker: Arc<Mutex<Option<Waker>>> = Arc::default();
        let callback_waker = waker.clone();

        let callback = PyCFunction::new_closure(
            py,
            None,
            None,
            move |_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| {
                if let Some(waker) = callback_waker.lock().take() {
                    waker.wake();
                }

                Ok::<(), PyErr>(())
            },
        )?;

        task.call_method1(py, "add_done_callback", (callback,))?;
        Ok(Self {
            runtime,
            task,
            waker,
            complete: false,
        })
    }
}

impl Future for PythonOperation {
    type Output = PyResult<Py<PyAny>>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        *self.waker.lock() = Some(cx.waker().clone());
        Python::attach(|py| {
            match self
                .task
                .call_method0(py, "done")
                .and_then(|done| done.extract::<bool>(py))
            {
                Ok(false) => Poll::Pending,
                Ok(true) => {
                    self.complete = true;
                    Poll::Ready(self.task.call_method0(py, "result"))
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        })
    }
}

impl Drop for PythonOperation {
    fn drop(&mut self) {
        if !self.complete {
            Python::attach(|py| {
                if let Err(e) = self
                    .runtime
                    .call_method1(py, "abandon", (self.task.clone_ref(py),))
                {
                    log::error!("Failed to request Python operation cancellation: {e}");
                }
            });
        }
    }
}

fn is_cancelled(py: Python<'_>, e: &PyErr) -> PyResult<bool> {
    e.value(py)
        .is_instance(&py.import("asyncio")?.getattr("CancelledError")?)
}

fn coroutine_driver(py: Python<'_>) -> PyResult<&Py<PyAny>> {
    static DRIVER: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
    DRIVER.get_or_try_init(py, || {
        let module = py.import("nautilus_trader.live._coroutine")?;
        Ok(module.getattr("drive")?.unbind())
    })
}
