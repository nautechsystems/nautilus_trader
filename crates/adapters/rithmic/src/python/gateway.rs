//! Python bindings for the Rithmic gateway.

#[cfg(feature = "python")]
use pyo3::prelude::*;

#[cfg(feature = "python")]
use pyo3_async_runtimes::tokio::future_into_py;

use std::sync::Arc;

use nautilus_common::live::get_runtime;
use tokio::sync::{RwLock, oneshot};
use tokio::task::JoinHandle;

use crate::gateway::{GatewayConfig, PnlEvent, RithmicGateway};
use crate::python::events::{PyAccountEvent, PyPositionEvent};

use super::config::PyRithmicEnv;

/// Python wrapper for RithmicGateway.
///
/// The gateway manages all Rithmic plant connections and provides handles
/// for data and execution clients.
#[cfg(feature = "python")]
#[pyclass(name = "RithmicGateway")]
pub struct PyRithmicGateway {
    /// The inner gateway wrapped in RwLock for safe async access.
    /// We use RwLock because connect/disconnect need &mut self.
    pub(crate) inner: Arc<RwLock<RithmicGateway>>,
    pnl_task: Arc<parking_lot::Mutex<Option<JoinHandle<()>>>>,
    pnl_shutdown: Arc<parking_lot::Mutex<Option<oneshot::Sender<()>>>>,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyRithmicGateway {
    /// Creates a new gateway with the given configuration.
    #[new]
    #[pyo3(signature = (
        environment,
        username,
        password,
        system_name,
        fcm_id,
        ib_id,
        account_id,
        app_name="NautilusTrader",
        app_version="1.0",
        enable_ticker=true,
        enable_order=true,
        enable_pnl=true,
        enable_history=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        environment: PyRithmicEnv,
        username: String,
        password: String,
        system_name: String,
        fcm_id: String,
        ib_id: String,
        account_id: String,
        app_name: &str,
        app_version: &str,
        enable_ticker: bool,
        enable_order: bool,
        enable_pnl: bool,
        enable_history: bool,
    ) -> Self {
        let config = GatewayConfig::new(
            environment.into(),
            username,
            password,
            system_name,
            fcm_id,
            ib_id,
            account_id,
        )
        .with_app_name(app_name)
        .with_app_version(app_version)
        .with_ticker(enable_ticker)
        .with_order(enable_order)
        .with_pnl(enable_pnl)
        .with_history(enable_history);

        Self {
            inner: Arc::new(RwLock::new(RithmicGateway::new(config))),
            pnl_task: Arc::new(parking_lot::Mutex::new(None)),
            pnl_shutdown: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    /// Creates a gateway from environment variables.
    #[staticmethod]
    #[pyo3(signature = (profile=None))]
    fn from_env(profile: Option<String>) -> PyResult<Self> {
        let config = GatewayConfig::from_env_with_profile(profile.as_deref())
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(RwLock::new(RithmicGateway::new(config))),
            pnl_task: Arc::new(parking_lot::Mutex::new(None)),
            pnl_shutdown: Arc::new(parking_lot::Mutex::new(None)),
        })
    }

    /// Returns true if the gateway is connected.
    fn is_connected(&self) -> bool {
        // Use try_read to avoid blocking - if locked, assume not connected
        self.inner
            .try_read()
            .map(|g| g.is_connected())
            .unwrap_or(false)
    }

    /// Returns the current connection state as a string.
    fn connection_state(&self) -> String {
        self.inner
            .try_read()
            .map(|g| format!("{:?}", g.connection_state()))
            .unwrap_or_else(|_| "Unknown".to_string())
    }

    /// Returns the account ID from the configuration.
    fn account_id(&self) -> Option<String> {
        self.inner
            .try_read()
            .ok()
            .map(|g| g.config().account_id.clone())
    }

    /// Returns the accessible trading accounts for the current order session.
    fn list_accounts<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let gateway = inner.read().await;
            gateway.list_accounts().await.map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!(
                    "Account list request failed: {e}"
                ))
            })
        })
    }

    /// Requests a PnL snapshot for the configured account.
    ///
    /// Snapshot updates are delivered through the running PnL callback loop.
    fn request_pnl_snapshot<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let gateway = inner.read().await;
            gateway.request_pnl_snapshot().await.map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!(
                    "PnL snapshot request failed: {e}"
                ))
            })
        })
    }

    fn __repr__(&self) -> String {
        let connected = self.is_connected();
        let state = self.connection_state();
        format!("RithmicGateway(connected={connected}, state={state})")
    }

    /// Connects to all enabled Rithmic plants.
    ///
    /// This is an async method - use `await gateway.connect()` in Python.
    ///
    /// Returns
    /// -------
    /// None
    ///     On successful connection.
    ///
    /// Raises
    /// ------
    /// RuntimeError
    ///     If connection fails.
    fn connect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let mut gateway = inner.write().await;
            gateway.connect().await.map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Connection failed: {e}"))
            })
        })
    }

    /// Disconnects from all Rithmic plants.
    ///
    /// This is an async method - use `await gateway.disconnect()` in Python.
    ///
    /// Returns
    /// -------
    /// None
    ///     On successful disconnection.
    ///
    /// Raises
    /// ------
    /// RuntimeError
    ///     If disconnection fails.
    fn disconnect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let mut gateway = inner.write().await;
            gateway.disconnect().await.map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Disconnection failed: {e}"))
            })
        })
    }

    /// Starts a background PnL/position event loop that dispatches events to a Python callback.
    ///
    /// This can be called once after connecting. The callback receives `PyAccountEvent` or
    /// `PyPositionEvent` instances.
    fn start_pnl_loop(&self, _py: Python<'_>, callback: Py<PyAny>) -> PyResult<()> {
        // Prevent double-start
        if self.pnl_task.lock().is_some() {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "PnL loop already running",
            ));
        }

        let inner = Arc::clone(&self.inner);
        let callback = callback;
        let shutdown = Arc::clone(&self.pnl_shutdown);
        let task_slot = Arc::clone(&self.pnl_task);

        // Spawn async task
        let handle = get_runtime().spawn(async move {
            let mut gw = inner.write().await;
            let mut rx = match gw.take_pnl_receiver() {
                Some(rx) => rx,
                None => return,
            };
            drop(gw);

            let (tx, mut rx_shutdown) = oneshot::channel();
            *shutdown.lock() = Some(tx);

            loop {
                tokio::select! {
                    _ = &mut rx_shutdown => {
                        break;
                    }
                    maybe_event = rx.recv() => {
                        if let Some(event) = maybe_event {
                            Python::attach(|py| {
                                match event {
                                    PnlEvent::Account(ae) => {
                                        let py_event = PyAccountEvent::from(ae);
                                        let _ = callback.call1(py, (py_event,));
                                    }
                                    PnlEvent::Position(pe) => {
                                        let py_event = PyPositionEvent::from(pe);
                                        let _ = callback.call1(py, (py_event,));
                                    }
                                }
                            });
                        } else {
                            break;
                        }
                    }
                }
            }
            *shutdown.lock() = None;
            *task_slot.lock() = None;
        });

        *self.pnl_task.lock() = Some(handle);
        Ok(())
    }

    /// Stops the background PnL loop if running.
    fn stop_pnl_loop(&self) {
        if let Some(tx) = self.pnl_shutdown.lock().take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.pnl_task.lock().take() {
            handle.abort();
        }
    }

    /// Subscribes to market data for an instrument.
    ///
    /// This is an async method - use `await gateway.subscribe_market_data(symbol, exchange)`.
    ///
    /// Parameters
    /// ----------
    /// symbol : str
    ///     The instrument symbol (e.g., "ESH5").
    /// exchange : str
    ///     The exchange code (e.g., "CME").
    ///
    /// Returns
    /// -------
    /// None
    ///     On successful subscription.
    fn subscribe_market_data<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let gateway = inner.read().await;
            gateway
                .subscribe_market_data(&symbol, &exchange)
                .await
                .map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!("Subscription failed: {e}"))
                })
        })
    }

    /// Unsubscribes from market data for an instrument.
    ///
    /// This is an async method.
    ///
    /// Parameters
    /// ----------
    /// symbol : str
    ///     The instrument symbol.
    /// exchange : str
    ///     The exchange code.
    fn unsubscribe_market_data<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = Arc::clone(&self.inner);
        future_into_py(py, async move {
            let gateway = inner.read().await;
            gateway
                .unsubscribe_market_data(&symbol, &exchange)
                .await
                .map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!("Unsubscribe failed: {e}"))
                })
        })
    }
}

/// Registers gateway types with the Python module.
#[cfg(feature = "python")]
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyRithmicGateway>()?;
    Ok(())
}
