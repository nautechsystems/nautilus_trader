//! Rithmic execution client implementation.

use dashmap::DashMap;
use std::sync::Arc;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, warn};

use rithmic_rs::{
    OrderSide, OrderStatus, OrderType, RithmicCancelOrder, RithmicModifyOrder, RithmicOrder,
    TimeInForce, TrailingStop, api::RithmicResponse, rithmic_to_unix_nanos,
    rti::messages::RithmicMessage,
};

use crate::common::enums::ConnectionState;
use crate::common::types::{ClientOrderIdStr, RithmicAccountId, RithmicOrderId, UnixNanos};
use crate::error::{Result, RithmicError};

use rithmic_rs::plants::order_plant::RithmicOrderPlantHandle;

/// Order submitted event.
#[derive(Debug, Clone)]
pub struct OrderSubmitted {
    /// Client order ID.
    pub client_order_id: ClientOrderIdStr,
    /// Venue order ID (from Rithmic).
    pub venue_order_id: Option<RithmicOrderId>,
    /// Account ID.
    pub account_id: RithmicAccountId,
    /// Timestamp.
    pub ts_event: UnixNanos,
    /// Venue order context captured from the notification payload.
    pub context: OrderContext,
}

/// Order accepted event.
#[derive(Debug, Clone)]
pub struct OrderAccepted {
    /// Client order ID.
    pub client_order_id: ClientOrderIdStr,
    /// Venue order ID.
    pub venue_order_id: RithmicOrderId,
    /// Account ID.
    pub account_id: RithmicAccountId,
    /// Timestamp.
    pub ts_event: UnixNanos,
    /// Venue order context captured from the notification payload.
    pub context: OrderContext,
}

/// Order rejected event.
#[derive(Debug, Clone)]
pub struct OrderRejected {
    /// Client order ID.
    pub client_order_id: ClientOrderIdStr,
    /// Rejection reason.
    pub reason: String,
    /// Timestamp.
    pub ts_event: UnixNanos,
    /// Venue order context captured from the notification payload.
    pub context: OrderContext,
}

/// Order filled event.
#[derive(Debug, Clone)]
pub struct OrderFilled {
    /// Client order ID.
    pub client_order_id: ClientOrderIdStr,
    /// Venue order ID.
    pub venue_order_id: RithmicOrderId,
    /// Fill price.
    pub fill_price: f64,
    /// Fill quantity.
    pub fill_qty: f64,
    /// Remaining quantity.
    pub leaves_qty: f64,
    /// Commission.
    pub commission: f64,
    /// Timestamp.
    pub ts_event: UnixNanos,
    /// Venue trade identifier, when provided by Rithmic.
    pub trade_id: Option<String>,
    /// Fill currency, when provided by Rithmic.
    pub currency: Option<String>,
    /// Venue order context captured from the notification payload.
    pub context: OrderContext,
}

/// Order cancelled event.
#[derive(Debug, Clone)]
pub struct OrderCancelled {
    /// Client order ID.
    pub client_order_id: ClientOrderIdStr,
    /// Venue order ID.
    pub venue_order_id: RithmicOrderId,
    /// Timestamp.
    pub ts_event: UnixNanos,
    /// Venue order context captured from the notification payload.
    pub context: OrderContext,
}

/// Order modified event.
#[derive(Debug, Clone)]
pub struct OrderModified {
    /// Client order ID.
    pub client_order_id: ClientOrderIdStr,
    /// Venue order ID.
    pub venue_order_id: RithmicOrderId,
    /// New price (if modified).
    pub new_price: Option<f64>,
    /// New quantity (if modified).
    pub new_qty: Option<f64>,
    /// Timestamp.
    pub ts_event: UnixNanos,
    /// Venue order context captured from the notification payload.
    pub context: OrderContext,
}

/// Venue order context used to rebuild Python-side order state after reconnect.
#[derive(Debug, Clone, Default)]
pub struct OrderContext {
    /// Instrument symbol.
    pub symbol: Option<String>,
    /// Exchange code.
    pub exchange: Option<String>,
    /// Order side.
    pub side: Option<OrderSide>,
    /// Order type.
    pub order_type: Option<OrderType>,
    /// Time in force.
    pub time_in_force: Option<TimeInForce>,
    /// Original order quantity.
    pub quantity: Option<f64>,
    /// Cumulative filled quantity.
    pub filled_qty: Option<f64>,
    /// Remaining open quantity.
    pub leaves_qty: Option<f64>,
    /// Order price.
    pub price: Option<f64>,
    /// Stop or trigger price.
    pub trigger_price: Option<f64>,
    /// Average fill price.
    pub avg_price: Option<f64>,
    /// Parent venue basket ID for bracket child notifications.
    pub original_basket_id: Option<String>,
    /// Linked venue basket IDs for contingent orders.
    pub linked_basket_ids: Vec<String>,
    /// Venue bracket type when provided.
    pub bracket_type: Option<String>,
}

/// Execution event emitted by the execution client.
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    /// Order was submitted.
    Submitted(OrderSubmitted),
    /// Order was accepted by venue.
    Accepted(OrderAccepted),
    /// Order was rejected.
    Rejected(OrderRejected),
    /// Order was filled (partial or complete).
    Filled(OrderFilled),
    /// Order was cancelled.
    Cancelled(OrderCancelled),
    /// Order was modified.
    Modified(OrderModified),
    /// Connection state change.
    ConnectionState(ConnectionState),
    /// Successfully reconnected after disconnect.
    Reconnected,
    /// Successfully authenticated with venue.
    Authenticated,
    /// Error event.
    Error(String),
}

/// Trailing stop configuration for orders.
#[derive(Debug, Clone)]
pub struct TrailingStopConfig {
    /// Number of ticks to trail behind the market price.
    pub trail_by_ticks: i32,
}

/// Order request to submit.
#[derive(Debug, Clone)]
pub struct OrderRequest {
    /// Client order ID.
    pub client_order_id: ClientOrderIdStr,
    /// Instrument symbol.
    pub symbol: String,
    /// Exchange.
    pub exchange: String,
    /// Order side.
    pub side: OrderSide,
    /// Order type.
    pub order_type: OrderType,
    /// Time in force.
    pub time_in_force: TimeInForce,
    /// Quantity.
    pub quantity: f64,
    /// Limit price (for limit orders).
    pub price: Option<f64>,
    /// Stop/trigger price (for stop orders).
    pub stop_price: Option<f64>,
    /// Trailing stop configuration (optional).
    pub trailing_stop: Option<TrailingStopConfig>,
}

/// Order state tracking.
#[derive(Debug, Clone)]
pub struct OrderState {
    /// Client order ID.
    pub client_order_id: ClientOrderIdStr,
    /// Venue order ID (basket_id from Rithmic).
    pub venue_order_id: Option<RithmicOrderId>,
    /// Instrument symbol.
    pub symbol: String,
    /// Exchange.
    pub exchange: String,
    /// Order type (needed for modify).
    pub order_type: OrderType,
    /// Current status.
    pub status: OrderStatus,
    /// Original quantity.
    pub quantity: f64,
    /// Filled quantity.
    pub filled_qty: f64,
    /// Remaining quantity.
    pub leaves_qty: f64,
    /// Average fill price.
    pub avg_price: f64,
}

/// Rithmic execution client for order management.
///
/// This client uses a `RithmicOrderPlantHandle` to send order commands
/// to the Rithmic order plant. Events (fills, cancels, etc.) are received
/// via the gateway's execution event channel.
///
/// # Example
///
/// ```rust,ignore
/// use nautilus_rithmic::RithmicExecutionClient;
///
/// // Get handle from gateway after connection
/// let handle = gateway.order_handle().unwrap().clone();
/// let client = RithmicExecutionClient::new(handle, "ACCOUNT123".to_string());
///
/// // Submit an order
/// client.submit_order(request).await?;
/// ```
pub struct RithmicExecutionClient {
    handle: RithmicOrderPlantHandle,
    account_id: String,
    orders: DashMap<ClientOrderIdStr, OrderState>,
    venue_to_client: DashMap<RithmicOrderId, ClientOrderIdStr>,
    event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
}

fn first_response_error(responses: &[RithmicResponse]) -> Option<String> {
    responses.iter().find_map(|response| response.error.clone())
}

impl RithmicExecutionClient {
    /// Creates a new execution client with the given order plant handle.
    ///
    /// # Arguments
    /// * `handle` - Order plant handle from the gateway
    /// * `account_id` - Trading account ID
    pub fn new(handle: RithmicOrderPlantHandle, account_id: String) -> Self {
        Self {
            handle,
            account_id,
            orders: DashMap::new(),
            venue_to_client: DashMap::new(),
            event_tx: None,
        }
    }

    /// Returns the account ID.
    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    /// Submits an order to Rithmic.
    ///
    /// The order is tracked locally and submitted via the order plant.
    /// Order events (submitted, accepted, filled, etc.) will be received
    /// through the gateway's execution event channel.
    ///
    /// # Supported Order Types
    ///
    /// - **Market**: Execute immediately at market price
    /// - **Limit**: Execute at specified price or better
    /// - **StopMarket**: Trigger at stop_price, then execute as market order
    /// - **StopLimit**: Trigger at stop_price, then execute as limit order at price
    ///
    /// # Trailing Stops
    ///
    /// Set `trailing_stop` to enable trailing stop functionality. The stop price
    /// will trail the market by the specified number of ticks.
    pub async fn submit_order(&self, request: OrderRequest) -> Result<()> {
        // Validate quantity is a positive whole number (Rithmic uses i32 for contracts)
        if request.quantity <= 0.0 {
            return Err(RithmicError::Order(format!(
                "Quantity must be positive, got: {}",
                request.quantity
            )));
        }
        if request.quantity.fract() != 0.0 {
            return Err(RithmicError::Order(format!(
                "Quantity must be a whole number (contracts), got: {}",
                request.quantity
            )));
        }
        if request.quantity > i32::MAX as f64 {
            return Err(RithmicError::Order(format!(
                "Quantity exceeds maximum: {}",
                request.quantity
            )));
        }

        // Validate limit orders have a price
        if (request.order_type == OrderType::Limit || request.order_type == OrderType::StopLimit)
            && request.price.is_none()
        {
            return Err(RithmicError::Order(
                "Limit/StopLimit order requires price".to_string(),
            ));
        }

        // Validate stop orders have a stop price
        if (request.order_type == OrderType::StopMarket
            || request.order_type == OrderType::StopLimit)
            && request.stop_price.is_none()
        {
            return Err(RithmicError::Order(
                "Stop order requires stop_price".to_string(),
            ));
        }

        // Track order locally
        let order_state = OrderState {
            client_order_id: request.client_order_id.clone(),
            venue_order_id: None,
            symbol: request.symbol.clone(),
            exchange: request.exchange.clone(),
            order_type: request.order_type,
            status: OrderStatus::Pending,
            quantity: request.quantity,
            filled_qty: 0.0,
            leaves_qty: request.quantity,
            avg_price: 0.0,
        };
        self.orders
            .insert(request.client_order_id.clone(), order_state);

        // Determine prices based on order type
        // Note: For Limit/StopLimit, price is guaranteed to exist due to prior validation
        let price = match request.order_type {
            OrderType::Market | OrderType::StopMarket => 0.0,
            OrderType::Limit | OrderType::StopLimit => {
                request.price.expect("price validated above")
            }
            _ => request.price.unwrap_or(0.0),
        };

        let trigger_price = match request.order_type {
            OrderType::StopMarket | OrderType::StopLimit => request.stop_price,
            _ => None,
        };

        // Convert trailing stop config
        let trailing_stop = request.trailing_stop.map(|ts| TrailingStop {
            trail_by_ticks: ts.trail_by_ticks,
        });

        debug!(
            "Submitting order: client_id={}, symbol={}, exchange={}, qty={}, price={}, trigger={:?}, side={:?}, type={:?}, trailing={:?}",
            request.client_order_id,
            request.symbol,
            request.exchange,
            request.quantity,
            price,
            trigger_price,
            request.side,
            request.order_type,
            trailing_stop
        );

        let tracking_symbol = request.symbol.clone();
        let tracking_exchange = request.exchange.clone();

        // Build RithmicOrder - use Into traits for automatic conversion
        let order = RithmicOrder {
            symbol: request.symbol,
            exchange: request.exchange,
            quantity: request.quantity as i32,
            price,
            transaction_type: request.side.into(),
            price_type: request.order_type.into(),
            user_tag: request.client_order_id.clone(),
            duration: Some(request.time_in_force.into()),
            trigger_price,
            trailing_stop,
        };

        // Submit to Rithmic using the new place_order API
        let responses = self
            .handle
            .place_order(order)
            .await
            .map_err(|e| RithmicError::Order(e.to_string()))?;

        let mut submitted_event: Option<ExecutionEvent> = None;

        for response in &responses {
            match &response.message {
                RithmicMessage::ResponseNewOrder(resp) => {
                    debug!(
                        request_id = %response.request_id,
                        source = %response.source,
                        error = ?response.error,
                        user_tag = ?resp.user_tag,
                        basket_id = ?resp.basket_id,
                        rp_code = ?resp.rp_code,
                        rq_handler_rp_code = ?resp.rq_handler_rp_code,
                        user_msg = ?resp.user_msg,
                        "place_order returned response_new_order"
                    );

                    if let Some(error) = &response.error {
                        let event = ExecutionEvent::Rejected(OrderRejected {
                            client_order_id: request.client_order_id.clone(),
                            reason: error.clone(),
                            ts_event: rithmic_to_unix_nanos(
                                resp.ssboe.unwrap_or(0),
                                resp.usecs.unwrap_or(0),
                            ),
                            context: OrderContext {
                                symbol: Some(tracking_symbol.clone()),
                                exchange: Some(tracking_exchange.clone()),
                                side: Some(request.side),
                                order_type: Some(request.order_type),
                                time_in_force: Some(request.time_in_force),
                                quantity: Some(request.quantity),
                                filled_qty: Some(0.0),
                                leaves_qty: Some(request.quantity),
                                price: request.price,
                                trigger_price: request.stop_price,
                                avg_price: None,
                                ..Default::default()
                            },
                        });
                        self.apply_event(&event);
                        return Err(RithmicError::Order(error.clone()));
                    }

                    let matches_request =
                        resp.user_tag.as_deref() == Some(request.client_order_id.as_str());
                    let has_venue_identity = resp.basket_id.is_some();

                    if matches_request || has_venue_identity {
                        submitted_event = Some(ExecutionEvent::Submitted(OrderSubmitted {
                            client_order_id: request.client_order_id.clone(),
                            venue_order_id: resp.basket_id.clone(),
                            account_id: self.account_id.clone(),
                            ts_event: rithmic_to_unix_nanos(
                                resp.ssboe.unwrap_or(0),
                                resp.usecs.unwrap_or(0),
                            ),
                            context: OrderContext {
                                symbol: Some(tracking_symbol.clone()),
                                exchange: Some(tracking_exchange.clone()),
                                side: Some(request.side),
                                order_type: Some(request.order_type),
                                time_in_force: Some(request.time_in_force),
                                quantity: Some(request.quantity),
                                filled_qty: Some(0.0),
                                leaves_qty: Some(request.quantity),
                                price: request.price,
                                trigger_price: request.stop_price,
                                avg_price: None,
                                ..Default::default()
                            },
                        }));
                    } else {
                        debug!(
                            request_id = %response.request_id,
                            "ignoring response_new_order without matching user_tag or basket_id"
                        );
                    }
                }
                other => {
                    debug!(
                        request_id = %response.request_id,
                        source = %response.source,
                        error = ?response.error,
                        message_kind = ?std::mem::discriminant(other),
                        "place_order returned non-new-order response"
                    );
                }
            }
        }

        if let Some(event) = submitted_event {
            self.apply_event(&event);
        }

        debug!("Order submitted: {}", request.client_order_id);
        Ok(())
    }

    /// Modifies an existing order.
    ///
    /// The order must exist locally and have a venue_order_id (must be accepted).
    ///
    /// # Arguments
    /// * `client_order_id` - The client order ID
    /// * `new_qty` - New quantity (optional)
    /// * `new_price` - New price (optional)
    pub async fn modify_order(
        &self,
        client_order_id: &str,
        new_qty: Option<f64>,
        new_price: Option<f64>,
    ) -> Result<()> {
        // Validate new_qty if provided
        if let Some(qty) = new_qty {
            if qty <= 0.0 {
                return Err(RithmicError::Order(format!(
                    "Quantity must be positive, got: {qty}"
                )));
            }
            if qty.fract() != 0.0 {
                return Err(RithmicError::Order(format!(
                    "Quantity must be a whole number (contracts), got: {qty}"
                )));
            }
        }

        let order = self
            .orders
            .get(client_order_id)
            .ok_or_else(|| RithmicError::Order(format!("Order not found: {client_order_id}")))?;

        let venue_order_id = order
            .venue_order_id
            .clone()
            .ok_or_else(|| RithmicError::Order("Order not yet accepted by venue".to_string()))?;

        let modify_request = RithmicModifyOrder {
            id: venue_order_id.clone(),
            exchange: order.exchange.clone(),
            symbol: order.symbol.clone(),
            qty: new_qty.map(|q| q as i32).unwrap_or(order.leaves_qty as i32),
            price: new_price.unwrap_or(0.0),
            price_type: order.order_type.into(),
        };

        debug!(
            "Modifying order: client_id={}, venue_id={}, new_qty={:?}, new_price={:?}",
            client_order_id, venue_order_id, new_qty, new_price
        );

        let responses = self
            .handle
            .modify_order(modify_request)
            .await
            .map_err(|e| RithmicError::Order(e.to_string()))?;

        for response in &responses {
            match &response.message {
                RithmicMessage::ResponseModifyOrder(resp) => {
                    debug!(
                        request_id = %response.request_id,
                        source = %response.source,
                        error = ?response.error,
                        basket_id = ?resp.basket_id,
                        rp_code = ?resp.rp_code,
                        rq_handler_rp_code = ?resp.rq_handler_rp_code,
                        user_msg = ?resp.user_msg,
                        "modify_order returned response_modify_order"
                    );
                }
                other => {
                    debug!(
                        request_id = %response.request_id,
                        source = %response.source,
                        error = ?response.error,
                        message_kind = ?std::mem::discriminant(other),
                        "modify_order returned non-modify-order response"
                    );
                }
            }
        }

        if let Some(error) = first_response_error(&responses) {
            return Err(RithmicError::Order(error));
        }

        Ok(())
    }

    /// Cancels an order.
    ///
    /// The order must exist locally and have a venue_order_id (must be accepted).
    pub async fn cancel_order(&self, client_order_id: &str) -> Result<()> {
        let order = self
            .orders
            .get(client_order_id)
            .ok_or_else(|| RithmicError::Order(format!("Order not found: {client_order_id}")))?;

        let venue_order_id = order
            .venue_order_id
            .clone()
            .ok_or_else(|| RithmicError::Order("Order not yet accepted by venue".to_string()))?;

        debug!(
            "Cancelling order: client_id={}, venue_id={}",
            client_order_id, venue_order_id
        );

        let cancel_request = RithmicCancelOrder { id: venue_order_id };

        let responses = self
            .handle
            .cancel_order(cancel_request)
            .await
            .map_err(|e| RithmicError::Order(e.to_string()))?;

        for response in &responses {
            match &response.message {
                RithmicMessage::ResponseCancelOrder(resp) => {
                    debug!(
                        request_id = %response.request_id,
                        source = %response.source,
                        error = ?response.error,
                        basket_id = ?resp.basket_id,
                        rp_code = ?resp.rp_code,
                        rq_handler_rp_code = ?resp.rq_handler_rp_code,
                        user_msg = ?resp.user_msg,
                        "cancel_order returned response_cancel_order"
                    );
                }
                other => {
                    debug!(
                        request_id = %response.request_id,
                        source = %response.source,
                        error = ?response.error,
                        message_kind = ?std::mem::discriminant(other),
                        "cancel_order returned non-cancel-order response"
                    );
                }
            }
        }

        if let Some(error) = first_response_error(&responses) {
            return Err(RithmicError::Order(error));
        }

        Ok(())
    }

    /// Cancels all open orders.
    pub async fn cancel_all_orders(&self) -> Result<()> {
        debug!("Cancelling all orders");

        let response = self
            .handle
            .cancel_all_orders()
            .await
            .map_err(|e| RithmicError::Order(e.to_string()))?;

        if let Some(error) = response.error {
            return Err(RithmicError::Order(error));
        }

        Ok(())
    }

    /// Cancels a batch of orders by client order ID.
    ///
    /// This iterates through the provided order IDs and cancels each one.
    /// Orders that don't exist or haven't been accepted are skipped.
    ///
    /// # Returns
    ///
    /// Returns the number of orders successfully submitted for cancellation.
    /// Note: This doesn't guarantee the cancels were accepted by the venue.
    pub async fn batch_cancel_orders(&self, client_order_ids: &[&str]) -> Result<usize> {
        debug!("Batch cancelling {} orders", client_order_ids.len());

        let mut cancelled = 0;
        for client_order_id in client_order_ids {
            match self.cancel_order(client_order_id).await {
                Ok(()) => cancelled += 1,
                Err(e) => {
                    warn!(
                        "Failed to cancel order {}: {} (continuing with batch)",
                        client_order_id, e
                    );
                }
            }
        }

        debug!(
            "Batch cancel submitted {} of {} orders",
            cancelled,
            client_order_ids.len()
        );
        Ok(cancelled)
    }

    /// Queries all open orders from Rithmic.
    ///
    /// This triggers an order reconciliation - the response will come
    /// through the gateway's execution event channel.
    pub async fn query_orders(&self) -> Result<()> {
        debug!("Querying open orders");

        let response = self
            .handle
            .show_orders()
            .await
            .map_err(|e| RithmicError::Order(e.to_string()))?;

        if let Some(error) = response.error {
            return Err(RithmicError::Order(error));
        }

        Ok(())
    }

    /// Replays execution history from Rithmic for the requested time window.
    ///
    /// The replayed venue events are emitted through the gateway execution
    /// channel and must be applied in timestamp order by the consumer.
    pub async fn replay_executions(
        &self,
        start_index_sec: i32,
        finish_index_sec: i32,
    ) -> Result<()> {
        debug!(
            start_index_sec,
            finish_index_sec, "Replaying execution history"
        );

        let responses = self
            .handle
            .replay_executions(start_index_sec, finish_index_sec)
            .await
            .map_err(|e| RithmicError::Order(e.to_string()))?;

        if let Some(error) = first_response_error(&responses) {
            return Err(RithmicError::Order(error));
        }

        Ok(())
    }

    /// Returns locally tracked order state.
    pub fn get_order(&self, client_order_id: &str) -> Option<OrderState> {
        self.orders.get(client_order_id).map(|r| r.clone())
    }

    /// Returns all locally tracked orders.
    pub fn orders(&self) -> Vec<OrderState> {
        self.orders.iter().map(|r| r.clone()).collect()
    }

    /// Returns open orders count.
    pub fn open_orders_count(&self) -> usize {
        self.orders
            .iter()
            .filter(|r| !r.status.is_terminal())
            .count()
    }

    /// Returns a receiver for execution events.
    ///
    /// Note: In the typical architecture, execution events come from
    /// the gateway's execution event channel (driven by ExecutionHandler).
    /// This method is provided for standalone client usage.
    pub fn event_receiver(&mut self) -> mpsc::UnboundedReceiver<ExecutionEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.event_tx = Some(tx);
        rx
    }

    /// Sends an event to the event channel.
    #[allow(dead_code)] // Will be used when standalone client mode is needed
    pub(crate) fn emit_event(&self, event: ExecutionEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event);
        }
    }

    /// Updates order state from venue message.
    ///
    /// Called by the execution handler when processing order notifications.
    pub fn update_order_state(
        &self,
        client_order_id: &str,
        venue_order_id: Option<&str>,
        status: OrderStatus,
        filled_qty: Option<f64>,
        leaves_qty: Option<f64>,
        avg_price: Option<f64>,
    ) {
        if let Some(mut order) = self.orders.get_mut(client_order_id) {
            if let Some(vid) = venue_order_id
                && order.venue_order_id.is_none()
            {
                order.venue_order_id = Some(vid.to_string());
                self.venue_to_client
                    .insert(vid.to_string(), client_order_id.to_string());
            }
            order.status = status;
            if let Some(fq) = filled_qty {
                order.filled_qty = fq;
            }
            if let Some(lq) = leaves_qty {
                order.leaves_qty = lq;
            }
            if let Some(ap) = avg_price {
                order.avg_price = ap;
            }
        } else {
            warn!(
                "Received update for unknown order: client_id={}",
                client_order_id
            );
        }
    }

    /// Looks up client order ID from venue order ID.
    pub fn get_client_order_id(&self, venue_order_id: &str) -> Option<String> {
        self.venue_to_client.get(venue_order_id).map(|r| r.clone())
    }

    /// Applies an execution event to local order state and re-emits it to any
    /// attached event receiver.
    pub fn apply_event(&self, event: &ExecutionEvent) {
        let mut emit_downstream = true;

        match event {
            ExecutionEvent::Submitted(e) => {
                if let Some(order) = self.orders.get(&e.client_order_id)
                    && order.status == OrderStatus::Pending
                    && order.venue_order_id == e.venue_order_id
                {
                    emit_downstream = false;
                }
                self.update_order_state(
                    &e.client_order_id,
                    e.venue_order_id.as_deref(),
                    OrderStatus::Pending,
                    None,
                    None,
                    None,
                );
            }
            ExecutionEvent::Accepted(e) => {
                self.update_order_state(
                    &e.client_order_id,
                    Some(&e.venue_order_id),
                    OrderStatus::Open,
                    None,
                    None,
                    None,
                );
            }
            ExecutionEvent::Rejected(e) => {
                self.update_order_state(
                    &e.client_order_id,
                    None,
                    OrderStatus::Rejected,
                    None,
                    Some(0.0),
                    None,
                );
            }
            ExecutionEvent::Filled(e) => {
                if let Some(mut order) = self.orders.get_mut(&e.client_order_id) {
                    // Determine new totals using existing quantity and fill details
                    let prev_filled = order.filled_qty;
                    let new_filled = (prev_filled + e.fill_qty).min(order.quantity);
                    order.filled_qty = new_filled;
                    order.leaves_qty = e.leaves_qty;

                    // Update average price with weighted notional
                    if new_filled > 0.0 {
                        let prev_notional = order.avg_price * prev_filled;
                        let new_notional = prev_notional + e.fill_price * e.fill_qty;
                        order.avg_price = new_notional / new_filled;
                    }

                    order.status = if e.leaves_qty > 0.0 {
                        OrderStatus::Partial
                    } else {
                        OrderStatus::Complete
                    };

                    // Record venue mapping if still missing
                    if order.venue_order_id.is_none() {
                        order.venue_order_id = Some(e.venue_order_id.clone());
                        self.venue_to_client
                            .insert(e.venue_order_id.clone(), e.client_order_id.clone());
                    }
                } else {
                    warn!(
                        "Filled event for unknown order: client_id={} venue_id={}",
                        e.client_order_id, e.venue_order_id
                    );
                }
            }
            ExecutionEvent::Cancelled(e) => {
                self.update_order_state(
                    &e.client_order_id,
                    Some(&e.venue_order_id),
                    OrderStatus::Cancelled,
                    None,
                    Some(0.0),
                    None,
                );
            }
            ExecutionEvent::Modified(e) => {
                if let Some(mut order) = self.orders.get_mut(&e.client_order_id) {
                    if let Some(qty) = e.new_qty {
                        order.quantity = qty;
                        // Keep filled_qty unchanged; recompute leaves based on new quantity
                        order.leaves_qty = (order.quantity - order.filled_qty).max(0.0);
                    }
                    if let Some(price) = e.new_price {
                        // Track latest working price in avg_price field if no separate field exists
                        order.avg_price = if order.filled_qty == 0.0 {
                            price
                        } else {
                            order.avg_price
                        };
                    }

                    if order.venue_order_id.is_none() {
                        order.venue_order_id = Some(e.venue_order_id.clone());
                        self.venue_to_client
                            .insert(e.venue_order_id.clone(), e.client_order_id.clone());
                    }
                } else {
                    warn!(
                        "Modify event for unknown order: client_id={} venue_id={}",
                        e.client_order_id, e.venue_order_id
                    );
                }
            }
            ExecutionEvent::ConnectionState(_) => {}
            ExecutionEvent::Reconnected => {}
            ExecutionEvent::Authenticated => {}
            ExecutionEvent::Error(_) => {}
        }

        // Emit downstream if a receiver has been registered.
        if emit_downstream {
            self.emit_event(event.clone());
        }
    }

    /// Consumes execution events from a channel, applying them to local state
    /// and re-emitting to any downstream listener registered via `event_receiver`.
    ///
    /// Caller is responsible for obtaining the receiver, typically from
    /// `RithmicGateway::take_execution_receiver()`.
    pub async fn pump_events(self: Arc<Self>, mut rx: mpsc::UnboundedReceiver<ExecutionEvent>) {
        while let Some(event) = rx.recv().await {
            self.apply_event(&event);
        }
    }

    /// Spawns an async task to process execution events from a receiver.
    ///
    /// Use this as a convenience when wiring the gateway execution channel to
    /// the client state machine.
    pub fn spawn_event_pump(
        self: Arc<Self>,
        rx: mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> JoinHandle<()> {
        tokio::spawn(self.pump_events(rx))
    }
}

impl std::fmt::Debug for RithmicExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RithmicExecutionClient")
            .field("account_id", &self.account_id)
            .field("open_orders", &self.open_orders_count())
            .finish()
    }
}
