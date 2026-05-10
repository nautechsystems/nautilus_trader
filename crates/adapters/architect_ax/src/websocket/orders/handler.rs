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

//! Orders WebSocket message handler for Ax.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_core::{
    UUID4,
    nanos::UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide as NautilusOrderSide, OrderStatus, OrderType, TimeInForce},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderExpired, OrderFilled, OrderRejected,
    },
    identifiers::{AccountId, ClientOrderId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};
use nautilus_network::websocket::{AuthTracker, WebSocketClient};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use crate::{
    common::{
        consts::AX_POST_ONLY_REJECT,
        enums::{AxOrderRequestType, AxOrderSide, AxTimeInForce},
        parse::{ax_timestamp_s_to_unix_nanos, cid_to_client_order_id},
    },
    http::models::AxOrderRejectReason,
    websocket::{
        messages::{
            AxOrdersWsMessage, AxWsCancelOrder, AxWsCancelRejected, AxWsError, AxWsGetOpenOrders,
            AxWsOrder, AxWsOrderAcknowledged, AxWsOrderCanceled, AxWsOrderDoneForDay,
            AxWsOrderEvent, AxWsOrderExpired, AxWsOrderFilled, AxWsOrderPartiallyFilled,
            AxWsOrderRejected, AxWsOrderReplaced, AxWsOrderResponse, AxWsPlaceOrder,
            AxWsRawMessage, AxWsTradeExecution, NautilusExecWsMessage, OrderMetadata,
        },
        parse::parse_order_message,
    },
};

fn map_time_in_force(tif: AxTimeInForce) -> TimeInForce {
    match tif {
        AxTimeInForce::Gtc => TimeInForce::Gtc,
        AxTimeInForce::Ioc => TimeInForce::Ioc,
        AxTimeInForce::Fok => TimeInForce::Fok,
        AxTimeInForce::Day => TimeInForce::Day,
        AxTimeInForce::Gtd => TimeInForce::Gtd,
        AxTimeInForce::Ato => TimeInForce::AtTheOpen,
        AxTimeInForce::Atc => TimeInForce::AtTheClose,
    }
}

fn map_order_side(side: AxOrderSide) -> NautilusOrderSide {
    match side {
        AxOrderSide::Buy => NautilusOrderSide::Buy,
        AxOrderSide::Sell => NautilusOrderSide::Sell,
    }
}

/// Simple tracking info for pending WebSocket orders.
#[derive(Clone, Debug)]
pub struct WsOrderInfo {
    /// Client order ID for correlation.
    pub client_order_id: ClientOrderId,
    /// Instrument symbol.
    pub symbol: Ustr,
}

/// Commands sent from the outer client to the inner orders handler.
#[derive(Debug)]
pub enum HandlerCommand {
    /// Set the WebSocket client for this handler.
    SetClient(WebSocketClient),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Authenticate with the provided token.
    Authenticate {
        /// Bearer token for authentication.
        token: String,
    },
    /// Place an order.
    PlaceOrder {
        /// Request ID for correlation.
        request_id: i64,
        /// Order placement message.
        order: AxWsPlaceOrder,
        /// Order info for tracking.
        order_info: WsOrderInfo,
    },
    /// Cancel an order.
    CancelOrder {
        /// Request ID for correlation.
        request_id: i64,
        /// Order ID to cancel.
        order_id: String,
    },
    /// Get open orders.
    GetOpenOrders {
        /// Request ID for correlation.
        request_id: i64,
    },
    /// Initialize the instrument cache with instruments.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(Box<InstrumentAny>),
    /// Store order metadata for a pending order.
    StoreOrderMetadata {
        /// Client order ID.
        client_order_id: ClientOrderId,
        /// Order metadata.
        metadata: OrderMetadata,
    },
}

/// Orders feed handler that processes WebSocket messages and produces domain events.
///
/// Runs in a dedicated Tokio task and owns the WebSocket client exclusively.
pub(crate) struct FeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    auth_tracker: AuthTracker,
    account_id: AccountId,
    instruments: AHashMap<Ustr, InstrumentAny>,
    pending_orders: AHashMap<i64, WsOrderInfo>,
    message_queue: VecDeque<AxOrdersWsMessage>,
    orders_metadata: Arc<DashMap<ClientOrderId, OrderMetadata>>,
    venue_to_client_id: Arc<DashMap<VenueOrderId, ClientOrderId>>,
    cid_to_client_order_id: Arc<DashMap<u64, ClientOrderId>>,
    bearer_token: Option<String>,
    needs_reauthentication: bool,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        auth_tracker: AuthTracker,
        account_id: AccountId,
        orders_metadata: Arc<DashMap<ClientOrderId, OrderMetadata>>,
        venue_to_client_id: Arc<DashMap<VenueOrderId, ClientOrderId>>,
        cid_to_client_order_id: Arc<DashMap<u64, ClientOrderId>>,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            auth_tracker,
            account_id,
            instruments: AHashMap::new(),
            pending_orders: AHashMap::new(),
            message_queue: VecDeque::new(),
            orders_metadata,
            venue_to_client_id,
            cid_to_client_order_id,
            bearer_token: None,
            needs_reauthentication: false,
        }
    }

    fn generate_ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    async fn reauthenticate(&mut self) {
        if self.bearer_token.is_some() {
            log::info!("Re-authenticating after reconnection");

            // Ax uses Bearer token in connection headers which persist across reconnect
            self.auth_tracker.succeed();
            self.message_queue
                .push_back(AxOrdersWsMessage::Authenticated);
            log::info!("Re-authentication completed");
        } else {
            log::warn!("Cannot re-authenticate: no bearer token stored");
        }
    }

    /// Returns the next message from the handler.
    ///
    /// This method blocks until a message is available or the handler is stopped.
    pub async fn next(&mut self) -> Option<AxOrdersWsMessage> {
        loop {
            if self.needs_reauthentication && self.message_queue.is_empty() {
                self.needs_reauthentication = false;
                self.reauthenticate().await;
            }

            if let Some(msg) = self.message_queue.pop_front() {
                return Some(msg);
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await;
                }

                () = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.signal.load(Ordering::Acquire) {
                        log::debug!("Stop signal received during idle period");
                        return None;
                    }
                }

                msg = self.raw_rx.recv() => {
                    let msg = match msg {
                        Some(msg) => msg,
                        None => {
                            log::debug!("WebSocket stream closed");
                            return None;
                        }
                    };

                    if let Message::Ping(data) = &msg {
                        log::trace!("Received ping frame with {} bytes", data.len());

                        if let Some(client) = &self.client
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            log::warn!("Failed to send pong frame: {e}");
                        }
                        continue;
                    }

                    if let Some(messages) = self.parse_raw_message(msg) {
                        self.message_queue.extend(messages);
                    }

                    if self.signal.load(Ordering::Acquire) {
                        log::debug!("Stop signal received");
                        return None;
                    }
                }
            }
        }
    }

    async fn handle_command(&mut self, cmd: HandlerCommand) {
        match cmd {
            HandlerCommand::SetClient(client) => {
                log::debug!("WebSocketClient received by handler");
                self.client = Some(client);
            }
            HandlerCommand::Disconnect => {
                log::debug!("Disconnect command received");
                self.auth_tracker.fail("Disconnected");

                if let Some(client) = self.client.take() {
                    client.disconnect().await;
                }
            }
            HandlerCommand::Authenticate { token } => {
                log::debug!("Authenticate command received");
                self.bearer_token = Some(token);

                // Ax uses Bearer token in connection headers (handled at connect time)
                self.auth_tracker.succeed();
                self.message_queue
                    .push_back(AxOrdersWsMessage::Authenticated);
            }
            HandlerCommand::PlaceOrder {
                request_id,
                order,
                order_info,
            } => {
                log::debug!(
                    "PlaceOrder command received: request_id={request_id}, symbol={}",
                    order.s
                );
                self.pending_orders.insert(request_id, order_info.clone());

                if let Err(e) = self.send_json(&order).await {
                    log::error!("Failed to send place order message: {e}");
                    self.pending_orders.remove(&request_id);
                    self.orders_metadata.remove(&order_info.client_order_id);

                    if let Some(cid) = order.cid {
                        self.cid_to_client_order_id.remove(&cid);
                    }
                    self.message_queue
                        .push_back(AxOrdersWsMessage::Error(AxWsError::new(format!(
                            "Failed to send place order for {}: {e}",
                            order_info.client_order_id
                        ))));
                }
            }
            HandlerCommand::CancelOrder {
                request_id,
                order_id,
            } => {
                log::debug!(
                    "CancelOrder command received: request_id={request_id}, order_id={order_id}"
                );
                self.send_cancel_order(request_id, &order_id).await;
            }
            HandlerCommand::GetOpenOrders { request_id } => {
                log::debug!("GetOpenOrders command received: request_id={request_id}");
                self.send_get_open_orders(request_id).await;
            }
            HandlerCommand::InitializeInstruments(instruments) => {
                for inst in instruments {
                    self.instruments.insert(inst.symbol().inner(), inst);
                }
            }
            HandlerCommand::UpdateInstrument(inst) => {
                self.instruments.insert(inst.symbol().inner(), *inst);
            }
            HandlerCommand::StoreOrderMetadata {
                client_order_id,
                metadata,
            } => {
                self.orders_metadata.insert(client_order_id, metadata);
            }
        }
    }

    async fn send_cancel_order(&mut self, request_id: i64, order_id: &str) {
        let msg = AxWsCancelOrder {
            rid: request_id,
            t: AxOrderRequestType::CancelOrder,
            oid: order_id.to_string(),
        };

        if let Err(e) = self.send_json(&msg).await {
            log::error!("Failed to send cancel order message: {e}");
            self.message_queue
                .push_back(AxOrdersWsMessage::Error(AxWsError::new(format!(
                    "Failed to send cancel for order {order_id}: {e}"
                ))));
        }
    }

    async fn send_get_open_orders(&mut self, request_id: i64) {
        let msg = AxWsGetOpenOrders {
            rid: request_id,
            t: AxOrderRequestType::GetOpenOrders,
        };

        if let Err(e) = self.send_json(&msg).await {
            log::error!("Failed to send get open orders message: {e}");
            self.message_queue
                .push_back(AxOrdersWsMessage::Error(AxWsError::new(format!(
                    "Failed to send get open orders request: {e}"
                ))));
        }
    }

    async fn send_json<T: serde::Serialize>(&self, msg: &T) -> Result<(), String> {
        let Some(client) = &self.client else {
            return Err("No WebSocket client available".to_string());
        };

        let payload = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        log::trace!("Sending: {payload}");

        client
            .send_text(payload, None)
            .await
            .map_err(|e| e.to_string())
    }

    fn parse_raw_message(&mut self, msg: Message) -> Option<Vec<AxOrdersWsMessage>> {
        match msg {
            Message::Text(text) => {
                if text == nautilus_network::RECONNECTED {
                    log::info!("Received WebSocket reconnected signal");
                    self.auth_tracker.fail("Reconnecting");
                    self.needs_reauthentication = true;
                    return Some(vec![AxOrdersWsMessage::Reconnected]);
                }

                log::trace!("Raw websocket message: {text}");

                let raw_msg: AxWsRawMessage = match parse_order_message(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        log::error!("Failed to parse WebSocket message: {e}: {text}");
                        return None;
                    }
                };

                self.handle_raw_message(raw_msg)
            }
            Message::Binary(data) => {
                log::debug!("Received binary message with {} bytes", data.len());
                None
            }
            Message::Close(_) => {
                log::debug!("Received close message, waiting for reconnection");
                None
            }
            _ => None,
        }
    }

    fn handle_raw_message(&mut self, raw_msg: AxWsRawMessage) -> Option<Vec<AxOrdersWsMessage>> {
        match raw_msg {
            AxWsRawMessage::Error(err) => {
                log::warn!(
                    "Order error response: rid={} code={} msg={}",
                    err.rid,
                    err.err.code,
                    err.err.msg
                );

                if let Some(order_info) = self.pending_orders.remove(&err.rid) {
                    self.orders_metadata.remove(&order_info.client_order_id);
                    log::debug!(
                        "Cleaned up metadata for failed order: {}",
                        order_info.client_order_id
                    );
                }

                Some(vec![AxOrdersWsMessage::Error(err.into())])
            }
            AxWsRawMessage::Response(resp) => self.handle_response(resp),
            AxWsRawMessage::Event(event) => self.handle_event(*event),
        }
    }

    fn handle_response(&mut self, resp: AxWsOrderResponse) -> Option<Vec<AxOrdersWsMessage>> {
        match resp {
            AxWsOrderResponse::PlaceOrder(msg) => {
                log::debug!("Place order response: rid={} oid={}", msg.rid, msg.res.oid);
                self.pending_orders.remove(&msg.rid);
                Some(vec![AxOrdersWsMessage::PlaceOrderResponse(msg)])
            }
            AxWsOrderResponse::CancelOrder(msg) => {
                log::debug!(
                    "Cancel order response: rid={} accepted={}",
                    msg.rid,
                    msg.res.cxl_rx
                );
                Some(vec![AxOrdersWsMessage::CancelOrderResponse(msg)])
            }
            AxWsOrderResponse::OpenOrders(msg) => {
                log::debug!("Open orders response: {} orders", msg.res.len());
                Some(vec![AxOrdersWsMessage::OpenOrdersResponse(msg)])
            }
            AxWsOrderResponse::List(msg) => {
                let order_count = msg.res.o.as_ref().map_or(0, |o| o.len());
                log::debug!(
                    "List subscription response: rid={} li={} orders={}",
                    msg.rid,
                    msg.res.li,
                    order_count
                );
                None
            }
        }
    }

    fn handle_event(&mut self, event: AxWsOrderEvent) -> Option<Vec<AxOrdersWsMessage>> {
        match event {
            AxWsOrderEvent::Heartbeat => {
                log::trace!("Received heartbeat");
                None
            }
            AxWsOrderEvent::Acknowledged(msg) => self.handle_order_acknowledged(msg),
            AxWsOrderEvent::PartiallyFilled(msg) => self.handle_order_partially_filled(msg),
            AxWsOrderEvent::Filled(msg) => self.handle_order_filled(msg),
            AxWsOrderEvent::Canceled(msg) => self.handle_order_canceled(msg),
            AxWsOrderEvent::Rejected(msg) => self.handle_order_rejected(msg),
            AxWsOrderEvent::Expired(msg) => self.handle_order_expired(msg),
            AxWsOrderEvent::Replaced(msg) => self.handle_order_replaced(msg),
            AxWsOrderEvent::DoneForDay(msg) => self.handle_order_done_for_day(msg),
            AxWsOrderEvent::CancelRejected(msg) => self.handle_cancel_rejected(msg),
        }
    }

    fn handle_order_acknowledged(
        &mut self,
        msg: AxWsOrderAcknowledged,
    ) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!("Order acknowledged: {} {}", msg.o.oid, msg.o.s);

        if let Some(event) = self.create_order_accepted(&msg.o, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderAccepted(event),
            )])
        } else if let Some(report) =
            self.create_order_status_report(&msg.o, OrderStatus::Accepted, msg.ts)
        {
            log::debug!("Created OrderStatusReport for external order {}", msg.o.oid);
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderStatusReports(vec![report]),
            )])
        } else {
            log::warn!(
                "Could not create OrderAccepted event for order {}",
                msg.o.oid
            );
            None
        }
    }

    fn handle_order_partially_filled(
        &mut self,
        msg: AxWsOrderPartiallyFilled,
    ) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!(
            "Order partially filled: {} {} @ {}",
            msg.o.oid,
            msg.xs.q,
            msg.xs.p
        );

        if let Some(event) = self.create_order_filled(&msg.o, &msg.xs, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderFilled(Box::new(event)),
            )])
        } else if let Some(report) = self.create_fill_report(&msg.o, &msg.xs, msg.ts) {
            log::debug!("Created FillReport for external order {}", msg.o.oid);
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::FillReports(vec![report]),
            )])
        } else {
            log::warn!("Could not create OrderFilled event for order {}", msg.o.oid);
            None
        }
    }

    fn handle_order_filled(&mut self, msg: AxWsOrderFilled) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!("Order filled: {} {} @ {}", msg.o.oid, msg.xs.q, msg.xs.p);

        let message = if let Some(event) = self.create_order_filled(&msg.o, &msg.xs, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderFilled(Box::new(event)),
            )])
        } else if let Some(report) = self.create_fill_report(&msg.o, &msg.xs, msg.ts) {
            log::debug!("Created FillReport for external order {}", msg.o.oid);
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::FillReports(vec![report]),
            )])
        } else {
            log::warn!("Could not create OrderFilled event for order {}", msg.o.oid);
            None
        };

        self.cleanup_terminal_order_tracking(&msg.o);
        message
    }

    fn handle_order_canceled(&mut self, msg: AxWsOrderCanceled) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!("Order canceled: {} reason={}", msg.o.oid, msg.xr);

        let message = if let Some(event) = self.create_order_canceled(&msg.o, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderCanceled(event),
            )])
        } else if let Some(report) =
            self.create_order_status_report(&msg.o, OrderStatus::Canceled, msg.ts)
        {
            log::debug!("Created OrderStatusReport for external order {}", msg.o.oid);
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderStatusReports(vec![report]),
            )])
        } else {
            log::warn!(
                "Could not create OrderCanceled event for order {}",
                msg.o.oid
            );
            None
        };

        self.cleanup_terminal_order_tracking(&msg.o);
        message
    }

    fn handle_order_rejected(&mut self, msg: AxWsOrderRejected) -> Option<Vec<AxOrdersWsMessage>> {
        let known_reason = msg.r.filter(|r| !matches!(r, AxOrderRejectReason::Unknown));
        let reason = known_reason
            .as_ref()
            .map(AsRef::as_ref)
            .or(msg.txt.as_deref())
            .unwrap_or("UNKNOWN");

        let message = if let Some(event) = self.create_order_rejected(&msg.o, reason, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderRejected(event),
            )])
        } else {
            log::warn!(
                "Could not create OrderRejected event for order {}",
                msg.o.oid
            );
            None
        };

        self.cleanup_terminal_order_tracking(&msg.o);
        message
    }

    fn handle_order_expired(&mut self, msg: AxWsOrderExpired) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!("Order expired: {}", msg.o.oid);

        let message = if let Some(event) = self.create_order_expired(&msg.o, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderExpired(event),
            )])
        } else if let Some(report) =
            self.create_order_status_report(&msg.o, OrderStatus::Expired, msg.ts)
        {
            log::debug!("Created OrderStatusReport for external order {}", msg.o.oid);
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderStatusReports(vec![report]),
            )])
        } else {
            log::warn!(
                "Could not create OrderExpired event for order {}",
                msg.o.oid
            );
            None
        };

        self.cleanup_terminal_order_tracking(&msg.o);
        message
    }

    fn handle_order_replaced(&mut self, msg: AxWsOrderReplaced) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!("Order replaced: {}", msg.o.oid);

        // Order replaced is treated as accepted with new parameters
        if let Some(event) = self.create_order_accepted(&msg.o, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderAccepted(event),
            )])
        } else if let Some(report) =
            self.create_order_status_report(&msg.o, OrderStatus::Accepted, msg.ts)
        {
            log::debug!(
                "Created OrderStatusReport for external replaced order {}",
                msg.o.oid
            );
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderStatusReports(vec![report]),
            )])
        } else {
            log::warn!(
                "Could not create OrderAccepted event for replaced order {}",
                msg.o.oid
            );
            None
        }
    }

    fn handle_order_done_for_day(
        &mut self,
        msg: AxWsOrderDoneForDay,
    ) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!("Order done for day: {}", msg.o.oid);

        let message = if let Some(event) = self.create_order_expired(&msg.o, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderExpired(event),
            )])
        } else if let Some(report) =
            self.create_order_status_report(&msg.o, OrderStatus::Expired, msg.ts)
        {
            log::debug!(
                "Created OrderStatusReport for external done-for-day order {}",
                msg.o.oid
            );
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderStatusReports(vec![report]),
            )])
        } else {
            log::warn!(
                "Could not create OrderExpired event for done-for-day order {}",
                msg.o.oid
            );
            None
        };

        self.cleanup_terminal_order_tracking(&msg.o);
        message
    }

    fn handle_cancel_rejected(
        &mut self,
        msg: AxWsCancelRejected,
    ) -> Option<Vec<AxOrdersWsMessage>> {
        log::warn!("Cancel rejected: {} reason={}", msg.oid, msg.r);

        let venue_order_id = VenueOrderId::new(&msg.oid);
        if let Some(client_order_id) = self.venue_to_client_id.get(&venue_order_id)
            && let Some(metadata) = self.orders_metadata.get(&client_order_id)
        {
            let event = OrderCancelRejected::new(
                metadata.trader_id,
                metadata.strategy_id,
                metadata.instrument_id,
                metadata.client_order_id,
                Ustr::from(msg.r.as_ref()),
                UUID4::new(),
                self.generate_ts_init(),
                metadata.ts_init,
                false,
                Some(venue_order_id),
                Some(self.account_id),
            );
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderCancelRejected(event),
            )])
        } else {
            log::warn!(
                "Could not find metadata for cancel rejected order {}",
                msg.oid
            );
            None
        }
    }

    fn lookup_order_metadata(
        &self,
        order: &AxWsOrder,
    ) -> Option<dashmap::mapref::one::Ref<'_, ClientOrderId, OrderMetadata>> {
        let venue_order_id = VenueOrderId::new(&order.oid);

        // Try venue_order_id mapping first
        if let Some(client_order_id) = self.venue_to_client_id.get(&venue_order_id)
            && let Some(metadata) = self.orders_metadata.get(&*client_order_id)
        {
            return Some(metadata);
        }

        // Try cid mapping second
        if let Some(cid) = order.cid
            && let Some(client_order_id) = self.cid_to_client_order_id.get(&cid)
            && let Some(metadata) = self.orders_metadata.get(&*client_order_id)
        {
            return Some(metadata);
        }

        None
    }

    fn create_order_accepted(&mut self, order: &AxWsOrder, event_ts: i64) -> Option<OrderAccepted> {
        let venue_order_id = VenueOrderId::new(&order.oid);
        let metadata = self.lookup_order_metadata(order)?;

        // Extract values before dropping the read guard
        let client_order_id = metadata.client_order_id;
        let trader_id = metadata.trader_id;
        let strategy_id = metadata.strategy_id;
        let instrument_id = metadata.instrument_id;

        // Drop the read guard before acquiring write lock
        drop(metadata);

        // Update venue_order_id mapping
        self.venue_to_client_id
            .insert(venue_order_id, client_order_id);

        // Update metadata with venue_order_id
        if let Some(mut entry) = self.orders_metadata.get_mut(&client_order_id) {
            entry.venue_order_id = Some(venue_order_id);
        }

        let ts_event = ax_timestamp_s_to_unix_nanos(event_ts)
            .map_err(|e| log::error!("{e}"))
            .ok()?;

        Some(OrderAccepted::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            self.account_id,
            UUID4::new(),
            ts_event,
            self.generate_ts_init(),
            false,
        ))
    }

    fn create_order_filled(
        &self,
        order: &AxWsOrder,
        execution: &AxWsTradeExecution,
        event_ts: i64,
    ) -> Option<OrderFilled> {
        let venue_order_id = VenueOrderId::new(&order.oid);
        let metadata = self.lookup_order_metadata(order)?;

        let ts_event = ax_timestamp_s_to_unix_nanos(event_ts)
            .map_err(|e| log::error!("{e}"))
            .ok()?;

        // AX uses u64 contracts - use instrument precision from metadata
        let last_qty = Quantity::new(execution.q as f64, metadata.size_precision);
        let last_px = Price::from_decimal_dp(execution.p, metadata.price_precision).ok()?;

        let order_side = match order.d {
            AxOrderSide::Buy => NautilusOrderSide::Buy,
            AxOrderSide::Sell => NautilusOrderSide::Sell,
        };

        // AX primarily uses limit orders
        let order_type = OrderType::Limit;

        // agg=true means aggressor (taker), agg=false means maker
        let liquidity_side = if execution.agg {
            LiquiditySide::Taker
        } else {
            LiquiditySide::Maker
        };

        Some(OrderFilled::new(
            metadata.trader_id,
            metadata.strategy_id,
            metadata.instrument_id,
            metadata.client_order_id,
            venue_order_id,
            self.account_id,
            TradeId::new(&execution.tid),
            order_side,
            order_type,
            last_qty,
            last_px,
            metadata.quote_currency,
            liquidity_side,
            UUID4::new(),
            ts_event,
            self.generate_ts_init(),
            false,
            None, // position_id
            None, // commission
        ))
    }

    fn create_order_canceled(&self, order: &AxWsOrder, event_ts: i64) -> Option<OrderCanceled> {
        let venue_order_id = VenueOrderId::new(&order.oid);
        let metadata = self.lookup_order_metadata(order)?;

        let client_order_id = metadata.client_order_id;
        let trader_id = metadata.trader_id;
        let strategy_id = metadata.strategy_id;
        let instrument_id = metadata.instrument_id;

        let ts_event = ax_timestamp_s_to_unix_nanos(event_ts)
            .map_err(|e| log::error!("{e}"))
            .ok()?;

        Some(OrderCanceled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            self.generate_ts_init(),
            false,
            Some(venue_order_id),
            Some(self.account_id),
        ))
    }

    fn create_order_expired(&self, order: &AxWsOrder, event_ts: i64) -> Option<OrderExpired> {
        let venue_order_id = VenueOrderId::new(&order.oid);
        let metadata = self.lookup_order_metadata(order)?;

        let client_order_id = metadata.client_order_id;
        let trader_id = metadata.trader_id;
        let strategy_id = metadata.strategy_id;
        let instrument_id = metadata.instrument_id;

        let ts_event = ax_timestamp_s_to_unix_nanos(event_ts)
            .map_err(|e| log::error!("{e}"))
            .ok()?;

        Some(OrderExpired::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            self.generate_ts_init(),
            false,
            Some(venue_order_id),
            Some(self.account_id),
        ))
    }

    fn create_order_rejected(
        &self,
        order: &AxWsOrder,
        reason: &str,
        event_ts: i64,
    ) -> Option<OrderRejected> {
        let metadata = self.lookup_order_metadata(order)?;

        let client_order_id = metadata.client_order_id;
        let trader_id = metadata.trader_id;
        let strategy_id = metadata.strategy_id;
        let instrument_id = metadata.instrument_id;

        let ts_event = ax_timestamp_s_to_unix_nanos(event_ts)
            .map_err(|e| log::error!("{e}"))
            .ok()?;
        let due_post_only = reason.contains(AX_POST_ONLY_REJECT);

        Some(OrderRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            self.account_id,
            Ustr::from(reason),
            UUID4::new(),
            ts_event,
            self.generate_ts_init(),
            false,
            due_post_only,
        ))
    }

    fn cleanup_terminal_order_tracking(&mut self, order: &AxWsOrder) {
        let venue_order_id = VenueOrderId::new(&order.oid);
        let client_order_id = self
            .venue_to_client_id
            .remove(&venue_order_id)
            .map(|(_, v)| v)
            .or_else(|| {
                order
                    .cid
                    .and_then(|cid| self.cid_to_client_order_id.remove(&cid).map(|(_, v)| v))
            });

        if let Some(client_order_id) = client_order_id {
            self.orders_metadata.remove(&client_order_id);
        }

        if let Some(cid) = order.cid {
            self.cid_to_client_order_id.remove(&cid);
        }
    }

    fn create_order_status_report(
        &self,
        order: &AxWsOrder,
        order_status: OrderStatus,
        event_ts: i64,
    ) -> Option<OrderStatusReport> {
        let instrument = self.instruments.get(&order.s)?;
        let venue_order_id = VenueOrderId::new(&order.oid);
        let instrument_id = instrument.id();
        let order_side = map_order_side(order.d);
        let time_in_force = map_time_in_force(order.tif);

        let quantity = Quantity::new(order.q as f64, instrument.size_precision());
        let filled_qty = Quantity::new(order.xq as f64, instrument.size_precision());

        let ts_event = ax_timestamp_s_to_unix_nanos(event_ts)
            .map_err(|e| log::error!("{e}"))
            .ok()?;
        let ts_init = self.generate_ts_init();

        let client_order_id = order.cid.map(|cid| {
            self.cid_to_client_order_id
                .get(&cid)
                .map_or_else(|| cid_to_client_order_id(cid), |v| *v)
        });

        let mut report = OrderStatusReport::new(
            self.account_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            order_side,
            OrderType::Limit, // AX primarily uses limit orders
            time_in_force,
            order_status,
            quantity,
            filled_qty,
            ts_event, // ts_accepted
            ts_event, // ts_last
            ts_init,
            Some(UUID4::new()),
        );

        if let Ok(price) = Price::from_decimal_dp(order.p, instrument.price_precision()) {
            report = report.with_price(price);
        }

        Some(report)
    }

    fn create_fill_report(
        &self,
        order: &AxWsOrder,
        execution: &AxWsTradeExecution,
        event_ts: i64,
    ) -> Option<FillReport> {
        let instrument = self.instruments.get(&order.s)?;
        let venue_order_id = VenueOrderId::new(&order.oid);
        let instrument_id = instrument.id();
        let order_side = map_order_side(order.d);

        let last_qty = Quantity::new(execution.q as f64, instrument.size_precision());
        let last_px = Price::from_decimal_dp(execution.p, instrument.price_precision()).ok()?;

        // agg=true means aggressor (taker), agg=false means maker
        let liquidity_side = if execution.agg {
            LiquiditySide::Taker
        } else {
            LiquiditySide::Maker
        };

        let ts_event = ax_timestamp_s_to_unix_nanos(event_ts)
            .map_err(|e| log::error!("{e}"))
            .ok()?;
        let ts_init = self.generate_ts_init();

        let client_order_id = order.cid.map(|cid| {
            self.cid_to_client_order_id
                .get(&cid)
                .map_or_else(|| cid_to_client_order_id(cid), |v| *v)
        });

        // AX doesn't provide commission in WebSocket fill events
        let commission = Money::new(0.0, instrument.quote_currency());

        Some(FillReport::new(
            self.account_id,
            instrument_id,
            venue_order_id,
            TradeId::new(&execution.tid),
            order_side,
            last_qty,
            last_px,
            commission,
            liquidity_side,
            client_order_id,
            None, // venue_position_id
            ts_event,
            ts_init,
            Some(UUID4::new()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use dashmap::DashMap;
    use nautilus_model::{
        identifiers::{InstrumentId, StrategyId, TraderId},
        types::Currency,
    };
    use nautilus_network::websocket::AuthTracker;
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::{AxOrderSide, AxOrderStatus, AxTimeInForce},
        http::models::AxOrderRejectReason,
        websocket::messages::{AxWsOrderRejected, AxWsPlaceOrderResponse, AxWsPlaceOrderResult},
    };

    fn test_handler() -> FeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            AuthTracker::default(),
            AccountId::from("AX-001"),
            Arc::new(DashMap::new()),
            Arc::new(DashMap::new()),
            Arc::new(DashMap::new()),
        )
    }

    fn sample_order(cid: u64) -> AxWsOrder {
        AxWsOrder {
            oid: "OID-1".to_string(),
            u: "user-1".to_string(),
            s: Ustr::from("EURUSD-PERP"),
            p: dec!(50000),
            q: 100,
            xq: 100,
            rq: 0,
            o: AxOrderStatus::Filled,
            d: AxOrderSide::Buy,
            tif: AxTimeInForce::Gtc,
            ts: 1_700_000_000,
            tn: 1,
            cid: Some(cid),
            tag: None,
            txt: None,
        }
    }

    fn sample_metadata(
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> OrderMetadata {
        OrderMetadata {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("EURUSD-PERP.AX"),
            client_order_id,
            venue_order_id: Some(venue_order_id),
            ts_init: UnixNanos::from(1_700_000_000_000_000_000u64),
            size_precision: 8,
            price_precision: 2,
            quote_currency: Currency::USD(),
        }
    }

    #[rstest]
    fn test_place_order_response_cleans_pending_order() {
        let mut handler = test_handler();
        let request_id = 11;
        handler.pending_orders.insert(
            request_id,
            WsOrderInfo {
                client_order_id: ClientOrderId::from("CID-11"),
                symbol: Ustr::from("EURUSD-PERP"),
            },
        );

        let response = AxWsOrderResponse::PlaceOrder(AxWsPlaceOrderResponse {
            rid: request_id,
            res: AxWsPlaceOrderResult {
                oid: "OID-11".to_string(),
            },
        });

        let messages = handler.handle_response(response).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(handler.pending_orders.get(&request_id).is_none());
    }

    #[rstest]
    fn test_handle_order_filled_cleans_tracking_maps() {
        let mut handler = test_handler();

        let client_order_id = ClientOrderId::from("CID-22");
        let venue_order_id = VenueOrderId::new("OID-1");
        let cid = 22_u64;

        handler.orders_metadata.insert(
            client_order_id,
            sample_metadata(client_order_id, venue_order_id),
        );
        handler
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        handler.cid_to_client_order_id.insert(cid, client_order_id);

        let msg = AxWsOrderFilled {
            ts: 1_700_000_001,
            tn: 2,
            eid: "EID-1".to_string(),
            o: sample_order(cid),
            xs: AxWsTradeExecution {
                tid: "T-1".to_string(),
                s: Ustr::from("EURUSD-PERP"),
                q: 100,
                p: dec!(50000),
                d: AxOrderSide::Buy,
                agg: true,
            },
        };

        let messages = handler.handle_order_filled(msg).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(handler.orders_metadata.get(&client_order_id).is_none());
        assert!(handler.venue_to_client_id.get(&venue_order_id).is_none());
        assert!(handler.cid_to_client_order_id.get(&cid).is_none());
    }

    #[rstest]
    fn test_handle_order_rejected_cleans_tracking_maps() {
        let mut handler = test_handler();

        let client_order_id = ClientOrderId::from("CID-33");
        let venue_order_id = VenueOrderId::new("OID-1");
        let cid = 33_u64;

        handler.orders_metadata.insert(
            client_order_id,
            sample_metadata(client_order_id, venue_order_id),
        );
        handler
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        handler.cid_to_client_order_id.insert(cid, client_order_id);

        let msg = AxWsOrderRejected {
            ts: 1_700_000_002,
            tn: 3,
            eid: "EID-3".to_string(),
            o: sample_order(cid),
            r: Some(AxOrderRejectReason::InsufficientMargin),
            txt: None,
        };

        let messages = handler.handle_order_rejected(msg);
        assert!(messages.is_some());
        assert!(handler.orders_metadata.get(&client_order_id).is_none());
        assert!(handler.venue_to_client_id.get(&venue_order_id).is_none());
        assert!(handler.cid_to_client_order_id.get(&cid).is_none());
    }
}
