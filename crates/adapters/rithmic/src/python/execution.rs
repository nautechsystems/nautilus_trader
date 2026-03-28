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

//! Python bindings for execution client.

#![allow(
    clippy::needless_pass_by_value,
    reason = "PyO3 execution APIs accept owned Python values at the FFI boundary"
)]

#[cfg(feature = "python")]
use pyo3::prelude::*;

#[cfg(feature = "python")]
use pyo3_async_runtimes::tokio::future_into_py;

use nautilus_common::live::get_runtime;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use std::sync::Arc;
use tokio::task::JoinHandle;

use rithmic_rs::{
    OrderSide, OrderType, RithmicBracketOrder, RithmicCancelOrder, RithmicOcoOrderLeg,
    RithmicOrder, TimeInForce, TrailingStop, api::RithmicResponse, rithmic_to_unix_nanos,
    rti::messages::RithmicMessage,
};

use crate::execution::{ExecutionEvent, OrderRejected, OrderSubmitted};
use crate::gateway::RithmicGateway;

use super::enums::{PyOrderSide, PyOrderType, PyTimeInForce};
use super::events::PyExecutionEvent;
use super::gateway::PyRithmicGateway;

/// Python wrapper for RithmicExecutionClient.
///
/// The execution client manages order submission, modification, and cancellation
/// through the Rithmic order plant.
///
/// Example
/// -------
/// ```python
/// gateway = RithmicGateway.from_env()
/// await gateway.connect()
///
/// client = RithmicExecutionClient(gateway, "ACCOUNT123")
/// client.set_execution_callback(on_execution_event)
///
/// await client.submit_order(
///     symbol="ESH5",
///     exchange="CME",
///     side=OrderSide.BUY,
///     order_type=OrderType.LIMIT,
///     quantity=1,
///     price=5000.00,
///     client_order_id="order_001",
/// )
/// ```
#[cfg(feature = "python")]
#[pyclass(name = "RithmicExecutionClient")]
pub struct PyRithmicExecutionClient {
    /// Reference to the gateway for async operations.
    gateway: Arc<tokio::sync::RwLock<RithmicGateway>>,
    /// Trading account ID.
    account_id: String,
    /// Local order tracking (Arc for sharing with async futures).
    orders: Arc<parking_lot::Mutex<std::collections::HashMap<String, OrderInfo>>>,
    /// Python callback for execution events.
    execution_callback: Arc<parking_lot::Mutex<Option<Py<PyAny>>>>,
    event_task: Arc<parking_lot::Mutex<Option<JoinHandle<()>>>>,
    shutdown_tx: Arc<parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

/// Local order tracking info.
#[derive(Clone, Debug)]
struct OrderInfo {
    symbol: String,
    exchange: String,
    venue_order_id: Option<String>,
    side: Option<OrderSide>,
}

fn first_response_error(responses: &[RithmicResponse]) -> Option<String> {
    responses.iter().find_map(|response| response.error.clone())
}

#[cfg(feature = "python")]
#[pymethods]
impl PyRithmicExecutionClient {
    /// Creates a new execution client from a connected gateway.
    ///
    /// Parameters
    /// ----------
    /// gateway : RithmicGateway
    ///     The connected gateway instance.
    /// account_id : str
    ///     The trading account ID.
    #[new]
    fn new(gateway: &PyRithmicGateway, account_id: String) -> Self {
        Self {
            gateway: Arc::clone(&gateway.inner),
            account_id,
            orders: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
            execution_callback: Arc::new(parking_lot::Mutex::new(None)),
            event_task: Arc::new(parking_lot::Mutex::new(None)),
            shutdown_tx: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    /// Returns the account ID.
    #[getter]
    fn account_id(&self) -> &str {
        &self.account_id
    }

    /// Returns count of tracked orders.
    #[getter]
    fn orders_count(&self) -> usize {
        self.orders.lock().len()
    }

    /// Returns true if the gateway is connected.
    #[getter]
    fn is_connected(&self) -> bool {
        self.gateway
            .try_read()
            .map(|g| g.is_connected())
            .unwrap_or(false)
    }

    /// Sets the callback for execution events.
    ///
    /// The callback will be called with each execution event (order submitted,
    /// accepted, filled, cancelled, etc.).
    ///
    /// Parameters
    /// ----------
    /// callback : callable
    ///     A Python callable that accepts a single argument (the event).
    ///
    /// Example
    /// -------
    /// ```python
    /// def on_execution(event):
    ///     if event.is_filled():
    ///         fill = event.as_filled()
    ///         print(f"Filled: {fill.client_order_id} @ {fill.fill_price}")
    ///
    /// client.set_execution_callback(on_execution)
    /// ```
    fn set_execution_callback(&self, callback: Py<PyAny>) {
        *self.execution_callback.lock() = Some(callback);
    }

    /// Clears the execution callback.
    fn clear_execution_callback(&self) {
        *self.execution_callback.lock() = None;
    }

    /// Starts the background event loop for execution events.
    ///
    /// This takes ownership of the gateway's execution receiver and dispatches
    /// events to the Python callback set via `set_execution_callback`.
    ///
    /// This is an async method - use `await client.start_event_loop()`.
    fn start_event_loop<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let gateway = Arc::clone(&self.gateway);
        let orders = Arc::clone(&self.orders);
        let callback = Arc::clone(&self.execution_callback);
        let event_task = Arc::clone(&self.event_task);
        let shutdown_tx = Arc::clone(&self.shutdown_tx);

        future_into_py(py, async move {
            // Check if already running
            if event_task.lock().is_some() {
                return Err(to_pyruntime_err("Execution event loop already running"));
            }

            // Take the receiver from gateway
            let mut gw = gateway.write().await;
            let rx = gw.take_execution_receiver().ok_or_else(|| {
                to_pyruntime_err("Execution receiver already taken or not available")
            })?;

            // Create shutdown channel
            let (tx, rx_shutdown) = tokio::sync::oneshot::channel();
            *shutdown_tx.lock() = Some(tx);

            // Spawn event processing task
            let handle = get_runtime().spawn(Self::event_loop(rx, rx_shutdown, orders, callback));

            // Store task handle
            *event_task.lock() = Some(handle);

            Ok(())
        })
    }

    /// Stops the background event loop for execution events.
    fn stop_event_loop(&self) {
        // Send shutdown signal first, then abort
        // Using take() ensures idempotent cleanup
        if let Some(tx) = self.shutdown_tx.lock().take() {
            let _ = tx.send(());
        }

        if let Some(handle) = self.event_task.lock().take() {
            handle.abort();
        }
    }

    /// Submits an order to Rithmic.
    ///
    /// This is an async method - use `await client.submit_order(...)`.
    ///
    /// Parameters
    /// ----------
    /// symbol : str
    ///     The instrument symbol (e.g., "ESH5").
    /// exchange : str
    ///     The exchange code (e.g., "CME").
    /// side : OrderSide
    ///     Buy or Sell.
    /// order_type : OrderType
    ///     Market, Limit, StopMarket, or StopLimit.
    /// quantity : int
    ///     Number of contracts.
    /// client_order_id : str
    ///     Your unique order identifier.
    /// price : float, optional
    ///     Limit price (required for Limit/StopLimit orders).
    /// stop_price : float, optional
    ///     Stop/trigger price (required for Stop orders).
    /// time_in_force : TimeInForce, optional
    ///     Order duration (default: Day).
    /// trailing_stop_ticks : int, optional
    ///     Enable trailing stop with specified tick offset.
    ///
    /// Returns
    /// -------
    /// None
    ///     On successful submission.
    ///
    /// Raises
    /// ------
    /// RuntimeError
    ///     If submission fails.
    /// ValueError
    ///     If order parameters are invalid.
    #[pyo3(signature = (
        symbol,
        exchange,
        side,
        order_type,
        quantity,
        client_order_id,
        price=None,
        stop_price=None,
        time_in_force=None,
        trailing_stop_ticks=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn submit_order<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
        side: PyOrderSide,
        order_type: PyOrderType,
        quantity: i32,
        client_order_id: String,
        price: Option<f64>,
        stop_price: Option<f64>,
        time_in_force: Option<PyTimeInForce>,
        trailing_stop_ticks: Option<i32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Validate inputs
        Self::validate_symbol_exchange(&symbol, &exchange)?;

        if quantity <= 0 {
            return Err(to_pyvalue_err("Quantity must be positive"));
        }

        if client_order_id.trim().is_empty() {
            return Err(to_pyvalue_err("client_order_id cannot be empty"));
        }

        let rs_order_type: OrderType = order_type.into();
        let rs_side: OrderSide = side.into();
        let rs_tif: TimeInForce = time_in_force.map_or(TimeInForce::Day, |t| t.into());

        // Validate limit orders have price
        if (rs_order_type == OrderType::Limit || rs_order_type == OrderType::StopLimit)
            && price.is_none()
        {
            return Err(to_pyvalue_err("Limit/StopLimit orders require a price"));
        }

        // Validate stop orders have stop price
        if (rs_order_type == OrderType::StopMarket || rs_order_type == OrderType::StopLimit)
            && stop_price.is_none()
        {
            return Err(to_pyvalue_err("Stop orders require a stop_price"));
        }

        let gateway = Arc::clone(&self.gateway);
        let orders = Arc::clone(&self.orders);
        let callback = Arc::clone(&self.execution_callback);
        let order_price = match rs_order_type {
            OrderType::Market | OrderType::StopMarket => 0.0,
            OrderType::Limit | OrderType::StopLimit => price.unwrap_or(0.0),
            _ => price.unwrap_or(0.0),
        };

        let trailing_stop = trailing_stop_ticks.map(|ticks| TrailingStop {
            trail_by_ticks: ticks,
        });

        // Clone values needed for tracking after successful submission
        let tracking_symbol = symbol.clone();
        let tracking_exchange = exchange.clone();
        let tracking_client_order_id = client_order_id.clone();

        future_into_py(py, async move {
            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let order = RithmicOrder {
                symbol,
                exchange,
                quantity,
                price: order_price,
                transaction_type: rs_side.into(),
                price_type: rs_order_type.into(),
                user_tag: client_order_id,
                duration: Some(rs_tif.into()),
                trigger_price: stop_price,
                trailing_stop,
            };

            let responses = handle
                .place_order(order)
                .await
                .map_err(|e| to_pyruntime_err(format!("Order submission failed: {e}")))?;

            let mut venue_order_id: Option<String> = None;
            let mut submitted_event: Option<ExecutionEvent> = None;

            for response in &responses {
                if let RithmicMessage::ResponseNewOrder(resp) = &response.message {
                    if let Some(error) = &response.error {
                        let rejected_event = ExecutionEvent::Rejected(OrderRejected {
                            client_order_id: tracking_client_order_id.clone(),
                            reason: error.clone(),
                            ts_event: rithmic_to_unix_nanos(
                                resp.ssboe.unwrap_or(0),
                                resp.usecs.unwrap_or(0),
                            ),
                            context: crate::execution::OrderContext {
                                symbol: Some(tracking_symbol.clone()),
                                exchange: Some(tracking_exchange.clone()),
                                side: Some(rs_side),
                                order_type: Some(rs_order_type),
                                time_in_force: Some(rs_tif),
                                quantity: Some(quantity as f64),
                                filled_qty: Some(0.0),
                                leaves_qty: Some(quantity as f64),
                                price,
                                trigger_price: stop_price,
                                avg_price: None,
                                ..Default::default()
                            },
                        });
                        Self::dispatch_callback_event(&callback, rejected_event);
                        return Err(to_pyruntime_err(format!(
                            "Order submission failed: {error}"
                        )));
                    }

                    let matches_request =
                        resp.user_tag.as_deref() == Some(tracking_client_order_id.as_str());
                    let has_venue_identity = resp.basket_id.is_some();

                    if matches_request || has_venue_identity {
                        venue_order_id = resp.basket_id.clone();
                        submitted_event = Some(ExecutionEvent::Submitted(OrderSubmitted {
                            client_order_id: tracking_client_order_id.clone(),
                            venue_order_id: venue_order_id.clone(),
                            account_id: gw.config().account_id.clone(),
                            ts_event: rithmic_to_unix_nanos(
                                resp.ssboe.unwrap_or(0),
                                resp.usecs.unwrap_or(0),
                            ),
                            context: crate::execution::OrderContext {
                                symbol: Some(tracking_symbol.clone()),
                                exchange: Some(tracking_exchange.clone()),
                                side: Some(rs_side),
                                order_type: Some(rs_order_type),
                                time_in_force: Some(rs_tif),
                                quantity: Some(quantity as f64),
                                filled_qty: Some(0.0),
                                leaves_qty: Some(quantity as f64),
                                price,
                                trigger_price: stop_price,
                                avg_price: None,
                                ..Default::default()
                            },
                        }));
                    }
                }
            }

            if let Some(error) = first_response_error(&responses) {
                return Err(to_pyruntime_err(format!(
                    "Order submission failed: {error}"
                )));
            }

            orders.lock().insert(
                tracking_client_order_id,
                OrderInfo {
                    symbol: tracking_symbol,
                    exchange: tracking_exchange,
                    venue_order_id,
                    side: Some(rs_side),
                },
            );

            if let Some(event) = submitted_event {
                Self::dispatch_callback_event(&callback, event);
            }

            Ok(())
        })
    }

    /// Submits a native venue bracket order.
    ///
    /// The bracket request places a single entry order with venue-managed
    /// profit-target and stop-loss offsets.
    #[pyo3(signature = (
        *,
        symbol,
        exchange,
        side,
        order_type,
        quantity,
        client_order_id,
        profit_ticks,
        stop_ticks,
        price=None,
        time_in_force=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn submit_bracket_order<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
        side: PyOrderSide,
        order_type: PyOrderType,
        quantity: i32,
        client_order_id: String,
        profit_ticks: i32,
        stop_ticks: i32,
        price: Option<f64>,
        time_in_force: Option<PyTimeInForce>,
    ) -> PyResult<Bound<'py, PyAny>> {
        Self::validate_symbol_exchange(&symbol, &exchange)?;

        if quantity <= 0 {
            return Err(to_pyvalue_err("Quantity must be positive"));
        }

        if client_order_id.trim().is_empty() {
            return Err(to_pyvalue_err("client_order_id cannot be empty"));
        }

        if profit_ticks <= 0 || stop_ticks <= 0 {
            return Err(to_pyvalue_err(
                "profit_ticks and stop_ticks must be positive",
            ));
        }

        let rs_order_type: OrderType = order_type.into();
        let rs_side: OrderSide = side.into();
        let rs_tif: TimeInForce = time_in_force.map_or(TimeInForce::Day, |t| t.into());

        if !matches!(rs_order_type, OrderType::Market | OrderType::Limit) {
            return Err(to_pyvalue_err(
                "Native bracket orders currently support only MARKET and LIMIT entry types",
            ));
        }

        if rs_order_type == OrderType::Limit && price.is_none() {
            return Err(to_pyvalue_err("Limit bracket orders require a price"));
        }

        let gateway = Arc::clone(&self.gateway);
        let orders = Arc::clone(&self.orders);

        future_into_py(py, async move {
            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let bracket_order = RithmicBracketOrder {
                action: rs_side.into(),
                duration: rs_tif.into(),
                exchange: exchange.clone(),
                localid: client_order_id.clone(),
                price_type: rs_order_type.into(),
                price,
                profit_ticks,
                quantity,
                stop_ticks,
                symbol: symbol.clone(),
            };

            let responses = handle
                .place_bracket_order(bracket_order)
                .await
                .map_err(|e| to_pyruntime_err(format!("Bracket submission failed: {e}")))?;

            if let Some(error) = first_response_error(&responses) {
                return Err(to_pyruntime_err(format!(
                    "Bracket submission failed: {error}"
                )));
            }

            let venue_order_id = responses
                .iter()
                .find_map(|response| match &response.message {
                    RithmicMessage::ResponseBracketOrder(resp) => resp.basket_id.clone(),
                    RithmicMessage::ResponseNewOrder(resp) => resp.basket_id.clone(),
                    _ => None,
                });

            orders.lock().insert(
                client_order_id,
                OrderInfo {
                    symbol,
                    exchange,
                    venue_order_id,
                    side: Some(rs_side),
                },
            );

            Ok(())
        })
    }

    /// Submits a native venue OCO order pair.
    #[pyo3(signature = (
        *,
        leg1_symbol,
        leg1_exchange,
        leg1_side,
        leg1_order_type,
        leg1_quantity,
        leg1_client_order_id,
        leg1_price=None,
        leg1_stop_price=None,
        leg1_time_in_force=None,
        leg2_symbol,
        leg2_exchange,
        leg2_side,
        leg2_order_type,
        leg2_quantity,
        leg2_client_order_id,
        leg2_price=None,
        leg2_stop_price=None,
        leg2_time_in_force=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn submit_oco_order<'py>(
        &self,
        py: Python<'py>,
        leg1_symbol: String,
        leg1_exchange: String,
        leg1_side: PyOrderSide,
        leg1_order_type: PyOrderType,
        leg1_quantity: i32,
        leg1_client_order_id: String,
        leg1_price: Option<f64>,
        leg1_stop_price: Option<f64>,
        leg1_time_in_force: Option<PyTimeInForce>,
        leg2_symbol: String,
        leg2_exchange: String,
        leg2_side: PyOrderSide,
        leg2_order_type: PyOrderType,
        leg2_quantity: i32,
        leg2_client_order_id: String,
        leg2_price: Option<f64>,
        leg2_stop_price: Option<f64>,
        leg2_time_in_force: Option<PyTimeInForce>,
    ) -> PyResult<Bound<'py, PyAny>> {
        Self::validate_symbol_exchange(&leg1_symbol, &leg1_exchange)?;
        Self::validate_symbol_exchange(&leg2_symbol, &leg2_exchange)?;

        if leg1_quantity <= 0 || leg2_quantity <= 0 {
            return Err(to_pyvalue_err("Quantities must be positive"));
        }

        if leg1_client_order_id.trim().is_empty() || leg2_client_order_id.trim().is_empty() {
            return Err(to_pyvalue_err("client_order_id cannot be empty"));
        }

        let leg1_order_type_rs: OrderType = leg1_order_type.into();
        let leg2_order_type_rs: OrderType = leg2_order_type.into();
        let leg1_side_rs: OrderSide = leg1_side.into();
        let leg2_side_rs: OrderSide = leg2_side.into();
        let leg1_tif: TimeInForce = leg1_time_in_force.map_or(TimeInForce::Day, |t| t.into());
        let leg2_tif: TimeInForce = leg2_time_in_force.map_or(TimeInForce::Day, |t| t.into());

        for (label, order_type, price, stop_price) in [
            ("leg1", leg1_order_type_rs, leg1_price, leg1_stop_price),
            ("leg2", leg2_order_type_rs, leg2_price, leg2_stop_price),
        ] {
            if matches!(order_type, OrderType::Limit | OrderType::StopLimit) && price.is_none() {
                return Err(to_pyvalue_err(format!("{label} requires a price",)));
            }

            if matches!(order_type, OrderType::StopMarket | OrderType::StopLimit)
                && stop_price.is_none()
            {
                return Err(to_pyvalue_err(format!("{label} requires a stop_price",)));
            }
        }

        let gateway = Arc::clone(&self.gateway);
        let orders = Arc::clone(&self.orders);

        future_into_py(py, async move {
            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let leg1 = RithmicOcoOrderLeg {
                symbol: leg1_symbol.clone(),
                exchange: leg1_exchange.clone(),
                quantity: leg1_quantity,
                price: match leg1_order_type_rs {
                    OrderType::Market | OrderType::StopMarket => 0.0,
                    OrderType::Limit | OrderType::StopLimit => leg1_price.unwrap_or(0.0),
                    _ => leg1_price.unwrap_or(0.0),
                },
                trigger_price: leg1_stop_price,
                transaction_type: leg1_side_rs.into(),
                duration: leg1_tif.into(),
                price_type: leg1_order_type_rs.into(),
                user_tag: leg1_client_order_id.clone(),
            };
            let leg2 = RithmicOcoOrderLeg {
                symbol: leg2_symbol.clone(),
                exchange: leg2_exchange.clone(),
                quantity: leg2_quantity,
                price: match leg2_order_type_rs {
                    OrderType::Market | OrderType::StopMarket => 0.0,
                    OrderType::Limit | OrderType::StopLimit => leg2_price.unwrap_or(0.0),
                    _ => leg2_price.unwrap_or(0.0),
                },
                trigger_price: leg2_stop_price,
                transaction_type: leg2_side_rs.into(),
                duration: leg2_tif.into(),
                price_type: leg2_order_type_rs.into(),
                user_tag: leg2_client_order_id.clone(),
            };

            let responses = handle
                .place_oco_order(leg1, leg2)
                .await
                .map_err(|e| to_pyruntime_err(format!("OCO submission failed: {e}")))?;

            if let Some(error) = first_response_error(&responses) {
                return Err(to_pyruntime_err(format!("OCO submission failed: {error}")));
            }

            let mut venue_ids = std::collections::HashMap::<String, String>::new();
            for response in &responses {
                if let RithmicMessage::ResponseOcoOrder(resp) = &response.message {
                    for (user_tag, basket_id) in resp.user_tag.iter().zip(resp.basket_id.iter()) {
                        venue_ids.insert(user_tag.clone(), basket_id.clone());
                    }
                }
            }

            let mut guard = orders.lock();
            guard.insert(
                leg1_client_order_id.clone(),
                OrderInfo {
                    symbol: leg1_symbol,
                    exchange: leg1_exchange,
                    venue_order_id: venue_ids.get(&leg1_client_order_id).cloned(),
                    side: Some(leg1_side_rs),
                },
            );
            guard.insert(
                leg2_client_order_id.clone(),
                OrderInfo {
                    symbol: leg2_symbol,
                    exchange: leg2_exchange,
                    venue_order_id: venue_ids.get(&leg2_client_order_id).cloned(),
                    side: Some(leg2_side_rs),
                },
            );

            Ok(())
        })
    }

    /// Cancels an order by venue order ID.
    ///
    /// This is an async method.
    ///
    /// Parameters
    /// ----------
    /// venue_order_id : str
    ///     The Rithmic basket ID for the order.
    fn cancel_order<'py>(
        &self,
        py: Python<'py>,
        venue_order_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        if venue_order_id.trim().is_empty() {
            return Err(to_pyvalue_err("venue_order_id cannot be empty"));
        }

        let gateway = Arc::clone(&self.gateway);

        future_into_py(py, async move {
            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let cancel = RithmicCancelOrder { id: venue_order_id };
            let responses = handle
                .cancel_order(cancel)
                .await
                .map_err(|e| to_pyruntime_err(format!("Cancel failed: {e}")))?;

            if let Some(error) = first_response_error(&responses) {
                return Err(to_pyruntime_err(format!("Cancel failed: {error}")));
            }

            Ok(())
        })
    }

    /// Cancels all orders.
    ///
    /// This is an async method.
    fn cancel_all_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let gateway = Arc::clone(&self.gateway);

        future_into_py(py, async move {
            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let response = handle
                .cancel_all_orders()
                .await
                .map_err(|e| to_pyruntime_err(format!("Cancel all failed: {e}")))?;

            if let Some(error) = response.error {
                return Err(to_pyruntime_err(format!("Cancel all failed: {error}")));
            }

            Ok(())
        })
    }

    /// Cancels tracked open orders matching optional symbol/exchange and side filters.
    ///
    /// Returns the number of cancel requests successfully submitted.
    #[pyo3(signature = (*, symbol=None, exchange=None, side=None))]
    fn cancel_orders<'py>(
        &self,
        py: Python<'py>,
        symbol: Option<String>,
        exchange: Option<String>,
        side: Option<PyOrderSide>,
    ) -> PyResult<Bound<'py, PyAny>> {
        Self::validate_symbol_exchange_filter(symbol.as_deref(), exchange.as_deref())?;

        let gateway = Arc::clone(&self.gateway);
        let venue_order_ids = Self::matching_venue_order_ids(
            &self.orders,
            symbol.as_deref(),
            exchange.as_deref(),
            side.map(Into::into),
        );

        future_into_py(py, async move {
            if venue_order_ids.is_empty() {
                return Ok(0usize);
            }

            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let mut cancelled = 0usize;
            for venue_order_id in venue_order_ids {
                let cancel = RithmicCancelOrder {
                    id: venue_order_id.clone(),
                };
                let responses = handle
                    .cancel_order(cancel)
                    .await
                    .map_err(|e| to_pyruntime_err(format!("Cancel failed: {e}")))?;

                if let Some(error) = first_response_error(&responses) {
                    tracing::warn!(
                        venue_order_id = %venue_order_id,
                        error = %error,
                        "scoped cancel failed for tracked Rithmic order",
                    );
                    continue;
                }

                cancelled += 1;
            }

            Ok(cancelled)
        })
    }

    /// Cancels a batch of venue order IDs.
    ///
    /// Returns the number of cancel requests successfully submitted.
    fn batch_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        ids: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        if ids.iter().any(|id| id.trim().is_empty()) {
            return Err(to_pyvalue_err(
                "ids cannot contain empty venue_order_id values",
            ));
        }

        let gateway = Arc::clone(&self.gateway);
        future_into_py(py, async move {
            if ids.is_empty() {
                return Ok(0usize);
            }

            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let mut cancelled = 0usize;
            for venue_order_id in ids {
                let cancel = RithmicCancelOrder {
                    id: venue_order_id.clone(),
                };
                let responses = handle
                    .cancel_order(cancel)
                    .await
                    .map_err(|e| to_pyruntime_err(format!("Cancel failed: {e}")))?;

                if let Some(error) = first_response_error(&responses) {
                    tracing::warn!(
                        venue_order_id = %venue_order_id,
                        error = %error,
                        "batch cancel failed for tracked Rithmic order",
                    );
                    continue;
                }

                cancelled += 1;
            }

            Ok(cancelled)
        })
    }

    /// Returns tracked open orders matching optional symbol/exchange and side filters.
    #[pyo3(signature = (*, symbol=None, exchange=None, side=None))]
    fn open_orders(
        &self,
        symbol: Option<String>,
        exchange: Option<String>,
        side: Option<PyOrderSide>,
    ) -> PyResult<Vec<std::collections::HashMap<String, Option<String>>>> {
        Self::validate_symbol_exchange_filter(symbol.as_deref(), exchange.as_deref())?;

        Ok(Self::matching_orders(
            &self.orders,
            symbol.as_deref(),
            exchange.as_deref(),
            side.map(Into::into),
        )
        .into_iter()
        .map(|(client_order_id, info)| {
            let mut result = std::collections::HashMap::new();
            result.insert("client_order_id".to_string(), Some(client_order_id));
            result.insert("symbol".to_string(), Some(info.symbol));
            result.insert("exchange".to_string(), Some(info.exchange));
            result.insert("venue_order_id".to_string(), info.venue_order_id);
            result.insert("side".to_string(), info.side.map(|value| value.to_string()));
            result
        })
        .collect())
    }

    /// Requests an open-order snapshot from Rithmic.
    ///
    /// The resulting venue events arrive through the normal execution callback.
    fn query_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let gateway = Arc::clone(&self.gateway);

        future_into_py(py, async move {
            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let response = handle
                .show_orders()
                .await
                .map_err(|e| to_pyruntime_err(format!("Show orders failed: {e}")))?;

            if let Some(error) = response.error {
                return Err(to_pyruntime_err(format!("Show orders failed: {error}")));
            }

            Ok(())
        })
    }

    /// Lists active native bracket parents from the venue.
    fn show_brackets<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let gateway = Arc::clone(&self.gateway);

        future_into_py(py, async move {
            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let responses = handle
                .show_brackets()
                .await
                .map_err(|e| to_pyruntime_err(format!("Show brackets failed: {e}")))?;

            if let Some(error) = first_response_error(&responses) {
                return Err(to_pyruntime_err(format!("Show brackets failed: {error}")));
            }

            let mut result = Vec::<std::collections::HashMap<String, Option<String>>>::new();
            for response in responses {
                if let RithmicMessage::ResponseShowBrackets(resp) = response.message {
                    let mut row = std::collections::HashMap::new();
                    row.insert("basket_id".to_string(), resp.basket_id);
                    row.insert("target_quantity".to_string(), resp.target_quantity);
                    row.insert(
                        "target_quantity_released".to_string(),
                        resp.target_quantity_released,
                    );
                    row.insert("target_ticks".to_string(), resp.target_ticks);
                    result.push(row);
                }
            }

            Ok(result)
        })
    }

    /// Lists active native bracket stop metadata from the venue.
    fn show_bracket_stops<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let gateway = Arc::clone(&self.gateway);

        future_into_py(py, async move {
            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let responses = handle
                .show_bracket_stops()
                .await
                .map_err(|e| to_pyruntime_err(format!("Show bracket stops failed: {e}")))?;

            if let Some(error) = first_response_error(&responses) {
                return Err(to_pyruntime_err(format!(
                    "Show bracket stops failed: {error}"
                )));
            }

            let mut result = Vec::<std::collections::HashMap<String, Option<String>>>::new();
            for response in responses {
                if let RithmicMessage::ResponseShowBracketStops(resp) = response.message {
                    let mut row = std::collections::HashMap::new();
                    row.insert("basket_id".to_string(), resp.basket_id);
                    row.insert("stop_quantity".to_string(), resp.stop_quantity);
                    row.insert(
                        "stop_quantity_released".to_string(),
                        resp.stop_quantity_released,
                    );
                    row.insert("stop_ticks".to_string(), resp.stop_ticks);
                    row.insert(
                        "bracket_trailing_field_id".to_string(),
                        resp.bracket_trailing_field_id,
                    );
                    row.insert(
                        "trailing_stop_trigger_ticks".to_string(),
                        resp.trailing_stop_trigger_ticks,
                    );
                    result.push(row);
                }
            }

            Ok(result)
        })
    }

    /// Replays execution history from Rithmic for a bounded time window.
    ///
    /// The replayed venue events arrive through the normal execution callback.
    fn replay_executions<'py>(
        &self,
        py: Python<'py>,
        start_index_sec: i32,
        finish_index_sec: i32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let gateway = Arc::clone(&self.gateway);

        future_into_py(py, async move {
            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let responses = handle
                .replay_executions(start_index_sec, finish_index_sec)
                .await
                .map_err(|e| to_pyruntime_err(format!("Replay executions failed: {e}")))?;

            if let Some(error) = first_response_error(&responses) {
                return Err(to_pyruntime_err(format!(
                    "Replay executions failed: {error}"
                )));
            }

            Ok(())
        })
    }

    /// Modifies an existing order.
    ///
    /// This is an async method.
    ///
    /// Note: Rithmic requires both quantity and price to be specified when
    /// modifying an order. If you only want to change one value, pass the
    /// current value for the other parameter.
    ///
    /// Parameters
    /// ----------
    /// venue_order_id : str
    ///     The Rithmic basket ID for the order.
    /// symbol : str
    ///     The instrument symbol.
    /// exchange : str
    ///     The exchange code.
    /// new_qty : int
    ///     New quantity (must be positive).
    /// new_price : float
    ///     New limit price.
    /// order_type : OrderType, optional
    ///     Order type (defaults to Limit).
    ///
    /// Returns
    /// -------
    /// None
    ///     On successful modification request.
    ///
    /// Raises
    /// ------
    /// RuntimeError
    ///     If modification fails.
    /// ValueError
    ///     If parameters are invalid.
    #[pyo3(signature = (venue_order_id, symbol, exchange, new_qty, new_price, order_type=None))]
    #[allow(clippy::too_many_arguments)]
    fn modify_order<'py>(
        &self,
        py: Python<'py>,
        venue_order_id: String,
        symbol: String,
        exchange: String,
        new_qty: i32,
        new_price: f64,
        order_type: Option<PyOrderType>,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Validate inputs
        Self::validate_symbol_exchange(&symbol, &exchange)?;

        if venue_order_id.trim().is_empty() {
            return Err(to_pyvalue_err("venue_order_id cannot be empty"));
        }

        if new_qty <= 0 {
            return Err(to_pyvalue_err("Quantity must be positive"));
        }

        let gateway = Arc::clone(&self.gateway);
        let rs_order_type: OrderType = order_type.map_or(OrderType::Limit, |t| t.into());

        future_into_py(py, async move {
            let gw = gateway.read().await;
            let handle = gw
                .order_handle()
                .ok_or_else(|| to_pyruntime_err("Order plant not connected"))?;

            let modify = rithmic_rs::RithmicModifyOrder {
                id: venue_order_id,
                exchange,
                symbol,
                qty: new_qty,
                price: new_price,
                price_type: rs_order_type.into(),
            };

            let responses = handle
                .modify_order(modify)
                .await
                .map_err(|e| to_pyruntime_err(format!("Modify failed: {e}")))?;

            if let Some(error) = first_response_error(&responses) {
                return Err(to_pyruntime_err(format!("Modify failed: {error}")));
            }

            Ok(())
        })
    }

    /// Returns information about a tracked order.
    ///
    /// Parameters
    /// ----------
    /// client_order_id : str
    ///     The client order ID.
    ///
    /// Returns
    /// -------
    /// dict or None
    ///     Order info dict with keys: symbol, exchange, venue_order_id.
    ///     Returns None if order not found.
    fn get_order(
        &self,
        client_order_id: &str,
    ) -> Option<std::collections::HashMap<String, Option<String>>> {
        self.orders.lock().get(client_order_id).map(|info| {
            let mut result = std::collections::HashMap::new();
            result.insert("symbol".to_string(), Some(info.symbol.clone()));
            result.insert("exchange".to_string(), Some(info.exchange.clone()));
            result.insert("venue_order_id".to_string(), info.venue_order_id.clone());
            result.insert(
                "side".to_string(),
                info.side.as_ref().map(std::string::ToString::to_string),
            );
            result
        })
    }

    /// Updates the venue order ID for a tracked order.
    ///
    /// This is typically called when an order is accepted by the venue.
    fn update_venue_order_id(&self, client_order_id: &str, venue_order_id: String) {
        if let Some(info) = self.orders.lock().get_mut(client_order_id) {
            info.venue_order_id = Some(venue_order_id);
        }
    }

    /// Removes a tracked order.
    ///
    /// Call this when an order is filled, cancelled, or rejected.
    fn remove_order(&self, client_order_id: &str) {
        self.orders.lock().remove(client_order_id);
    }

    fn __repr__(&self) -> String {
        format!(
            "RithmicExecutionClient(account_id='{}', orders={})",
            self.account_id,
            self.orders_count()
        )
    }
}

impl PyRithmicExecutionClient {
    /// Validates symbol and exchange are non-empty.
    fn validate_symbol_exchange(symbol: &str, exchange: &str) -> PyResult<()> {
        if symbol.trim().is_empty() {
            return Err(to_pyvalue_err("symbol cannot be empty"));
        }

        if exchange.trim().is_empty() {
            return Err(to_pyvalue_err("exchange cannot be empty"));
        }
        Ok(())
    }

    fn validate_symbol_exchange_filter(
        symbol: Option<&str>,
        exchange: Option<&str>,
    ) -> PyResult<()> {
        match (symbol, exchange) {
            (Some(symbol), Some(exchange)) => Self::validate_symbol_exchange(symbol, exchange),
            (None, None) => Ok(()),
            _ => Err(to_pyvalue_err(
                "symbol and exchange must either both be provided or both be omitted",
            )),
        }
    }

    fn matching_orders(
        orders: &Arc<parking_lot::Mutex<std::collections::HashMap<String, OrderInfo>>>,
        symbol: Option<&str>,
        exchange: Option<&str>,
        side: Option<OrderSide>,
    ) -> Vec<(String, OrderInfo)> {
        orders
            .lock()
            .iter()
            .filter_map(|(client_order_id, info)| {
                if let Some(symbol) = symbol
                    && info.symbol != symbol
                {
                    return None;
                }

                if let Some(exchange) = exchange
                    && info.exchange != exchange
                {
                    return None;
                }

                if let Some(side) = side
                    && info.side != Some(side)
                {
                    return None;
                }

                Some((client_order_id.clone(), info.clone()))
            })
            .collect()
    }

    fn matching_venue_order_ids(
        orders: &Arc<parking_lot::Mutex<std::collections::HashMap<String, OrderInfo>>>,
        symbol: Option<&str>,
        exchange: Option<&str>,
        side: Option<OrderSide>,
    ) -> Vec<String> {
        Self::matching_orders(orders, symbol, exchange, side)
            .into_iter()
            .filter_map(|(_, info)| info.venue_order_id)
            .collect()
    }

    /// Event processing loop that runs in a spawned task.
    ///
    /// This is separated out to make the async flow clearer and testable.
    async fn event_loop(
        mut rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        mut rx_shutdown: tokio::sync::oneshot::Receiver<()>,
        orders: Arc<parking_lot::Mutex<std::collections::HashMap<String, OrderInfo>>>,
        callback: Arc<parking_lot::Mutex<Option<Py<PyAny>>>>,
    ) {
        loop {
            tokio::select! {
                _ = &mut rx_shutdown => {
                    tracing::debug!("Execution event loop received shutdown signal");
                    break;
                }
                event = rx.recv() => {
                    match event {
                        Some(event) => {
                            Self::sync_tracked_order(&orders, &event);
                            // Acquire GIL and dispatch event
                            // Note: Python::attach is blocking but safe here since
                            // we don't hold any Rust locks while waiting for GIL
                            Self::dispatch_callback_event(&callback, event);
                        }
                        None => {
                            tracing::debug!("Execution channel closed");
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Dispatches an execution event to the Python callback.
    #[allow(dead_code)]
    pub(crate) fn dispatch_event(&self, event: ExecutionEvent) {
        Self::sync_tracked_order(&self.orders, &event);
        Self::dispatch_callback_event(&self.execution_callback, event);
    }

    fn upsert_tracked_order(
        orders: &Arc<parking_lot::Mutex<std::collections::HashMap<String, OrderInfo>>>,
        client_order_id: &str,
        venue_order_id: Option<&str>,
        symbol: Option<&str>,
        exchange: Option<&str>,
        side: Option<OrderSide>,
    ) {
        let mut guard = orders.lock();
        if let Some(info) = guard.get_mut(client_order_id) {
            if let Some(venue_order_id) = venue_order_id {
                info.venue_order_id = Some(venue_order_id.to_string());
            }

            if side.is_some() {
                info.side = side;
            }
            return;
        }

        let (Some(symbol), Some(exchange)) = (symbol, exchange) else {
            return;
        };

        guard.insert(
            client_order_id.to_string(),
            OrderInfo {
                symbol: symbol.to_string(),
                exchange: exchange.to_string(),
                venue_order_id: venue_order_id.map(str::to_string),
                side,
            },
        );
    }

    fn sync_tracked_order(
        orders: &Arc<parking_lot::Mutex<std::collections::HashMap<String, OrderInfo>>>,
        event: &ExecutionEvent,
    ) {
        match event {
            ExecutionEvent::Submitted(e) => {
                Self::upsert_tracked_order(
                    orders,
                    &e.client_order_id,
                    e.venue_order_id.as_deref(),
                    e.context.symbol.as_deref(),
                    e.context.exchange.as_deref(),
                    e.context.side,
                );
            }
            ExecutionEvent::Accepted(e) => {
                Self::upsert_tracked_order(
                    orders,
                    &e.client_order_id,
                    Some(&e.venue_order_id),
                    e.context.symbol.as_deref(),
                    e.context.exchange.as_deref(),
                    e.context.side,
                );
            }
            ExecutionEvent::Cancelled(e) => {
                orders.lock().remove(&e.client_order_id);
            }
            ExecutionEvent::Rejected(e) => {
                orders.lock().remove(&e.client_order_id);
            }
            ExecutionEvent::Filled(e) => {
                if e.leaves_qty > 0.0 {
                    Self::upsert_tracked_order(
                        orders,
                        &e.client_order_id,
                        Some(&e.venue_order_id),
                        e.context.symbol.as_deref(),
                        e.context.exchange.as_deref(),
                        e.context.side,
                    );
                } else {
                    orders.lock().remove(&e.client_order_id);
                }
            }
            _ => {}
        }
    }

    fn dispatch_callback_event(
        callback: &Arc<parking_lot::Mutex<Option<Py<PyAny>>>>,
        event: ExecutionEvent,
    ) {
        pyo3::Python::attach(|py| {
            let guard = callback.lock();
            if let Some(ref cb) = *guard {
                let py_event = PyExecutionEvent::from(event);
                if let Err(e) = cb.call1(py, (py_event,)) {
                    tracing::error!("Error in Python execution callback: {e}");
                }
            }
        });
    }
}

#[cfg(feature = "python")]
impl Drop for PyRithmicExecutionClient {
    fn drop(&mut self) {
        // Reuse stop_event_loop logic for consistent cleanup
        if let Some(tx) = self.shutdown_tx.lock().take() {
            let _ = tx.send(());
        }

        if let Some(handle) = self.event_task.lock().take() {
            handle.abort();
        }
    }
}

/// Registers execution client types with the Python module.
#[cfg(feature = "python")]
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyRithmicExecutionClient>()?;
    Ok(())
}
