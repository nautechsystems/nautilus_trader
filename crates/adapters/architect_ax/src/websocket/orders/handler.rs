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
use nautilus_core::{UUID4, nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide as NautilusOrderSide, OrderType},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderExpired, OrderFilled, OrderRejected,
    },
    identifiers::{AccountId, ClientOrderId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::websocket::{AuthTracker, WebSocketClient};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use crate::{
    common::enums::AxOrderSide,
    websocket::messages::{
        AxOrdersWsMessage, AxWsCancelOrder, AxWsCancelRejected, AxWsGetOpenOrders, AxWsOrder,
        AxWsOrderAcknowledged, AxWsOrderCanceled, AxWsOrderDoneForDay, AxWsOrderEvent,
        AxWsOrderExpired, AxWsOrderFilled, AxWsOrderPartiallyFilled, AxWsOrderRejected,
        AxWsOrderReplaced, AxWsOrderResponse, AxWsPlaceOrder, AxWsRawMessage, AxWsTradeExecution,
        NautilusExecWsMessage, OrderMetadata,
    },
};

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
    bearer_token: Option<String>,
    needs_reauthentication: bool,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    #[must_use]
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        auth_tracker: AuthTracker,
        account_id: AccountId,
        orders_metadata: Arc<DashMap<ClientOrderId, OrderMetadata>>,
        venue_to_client_id: Arc<DashMap<VenueOrderId, ClientOrderId>>,
    ) -> Self {
        Self {
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
            bearer_token: None,
            needs_reauthentication: false,
        }
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
                    continue;
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
                self.pending_orders.insert(request_id, order_info);

                if let Err(e) = self.send_json(&order).await {
                    log::error!("Failed to send place order message: {e}");
                    self.pending_orders.remove(&request_id);
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

    async fn send_cancel_order(&self, request_id: i64, order_id: &str) {
        let msg = AxWsCancelOrder {
            rid: request_id,
            t: "x".to_string(),
            oid: order_id.to_string(),
        };

        if let Err(e) = self.send_json(&msg).await {
            log::error!("Failed to send cancel order message: {e}");
        }
    }

    async fn send_get_open_orders(&self, request_id: i64) {
        let msg = AxWsGetOpenOrders {
            rid: request_id,
            t: "o".to_string(),
        };

        if let Err(e) = self.send_json(&msg).await {
            log::error!("Failed to send get open orders message: {e}");
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

                let raw_msg: AxWsRawMessage = match serde_json::from_str(&text) {
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
        } else {
            log::warn!("Could not create OrderFilled event for order {}", msg.o.oid);
            None
        }
    }

    fn handle_order_filled(&mut self, msg: AxWsOrderFilled) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!("Order filled: {} {} @ {}", msg.o.oid, msg.xs.q, msg.xs.p);

        if let Some(event) = self.create_order_filled(&msg.o, &msg.xs, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderFilled(Box::new(event)),
            )])
        } else {
            log::warn!("Could not create OrderFilled event for order {}", msg.o.oid);
            None
        }
    }

    fn handle_order_canceled(&mut self, msg: AxWsOrderCanceled) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!("Order canceled: {} reason={}", msg.o.oid, msg.xr);

        if let Some(event) = self.create_order_canceled(&msg.o, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderCanceled(event),
            )])
        } else {
            log::warn!(
                "Could not create OrderCanceled event for order {}",
                msg.o.oid
            );
            None
        }
    }

    fn handle_order_rejected(&mut self, msg: AxWsOrderRejected) -> Option<Vec<AxOrdersWsMessage>> {
        log::warn!("Order rejected: {} reason={}", msg.o.oid, msg.r);

        if let Some(event) = self.create_order_rejected(&msg.o, &msg.r, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderRejected(event),
            )])
        } else {
            log::warn!(
                "Could not create OrderRejected event for order {}",
                msg.o.oid
            );
            None
        }
    }

    fn handle_order_expired(&mut self, msg: AxWsOrderExpired) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!("Order expired: {}", msg.o.oid);

        if let Some(event) = self.create_order_expired(&msg.o, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderExpired(event),
            )])
        } else {
            log::warn!(
                "Could not create OrderExpired event for order {}",
                msg.o.oid
            );
            None
        }
    }

    fn handle_order_replaced(&mut self, msg: AxWsOrderReplaced) -> Option<Vec<AxOrdersWsMessage>> {
        log::debug!("Order replaced: {}", msg.o.oid);

        // Order replaced is treated as accepted with new parameters
        if let Some(event) = self.create_order_accepted(&msg.o, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderAccepted(event),
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

        if let Some(event) = self.create_order_expired(&msg.o, msg.ts) {
            Some(vec![AxOrdersWsMessage::Nautilus(
                NautilusExecWsMessage::OrderExpired(event),
            )])
        } else {
            log::warn!(
                "Could not create OrderExpired event for done-for-day order {}",
                msg.o.oid
            );
            None
        }
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
                msg.r.into(),
                UUID4::new(),
                get_atomic_clock_realtime().get_time_ns(),
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

    // ---- Domain event creation methods ----

    fn extract_client_order_id(&self, order: &AxWsOrder) -> Option<ClientOrderId> {
        order
            .tag
            .as_ref()
            .map(|tag| ClientOrderId::new(tag.as_str()))
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

        // Fall back to tag field
        if let Some(client_order_id) = self.extract_client_order_id(order) {
            return self.orders_metadata.get(&client_order_id);
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

        let ts_event = UnixNanos::from(event_ts as u64 * 1_000_000_000);

        Some(OrderAccepted::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            self.account_id,
            UUID4::new(),
            ts_event,
            get_atomic_clock_realtime().get_time_ns(),
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

        let ts_event = UnixNanos::from(event_ts as u64 * 1_000_000_000);

        // AX uses i64 contracts directly - use instrument precision from metadata
        let last_qty = Quantity::new(execution.q.abs() as f64, metadata.size_precision);
        let last_px = Price::from_decimal_dp(execution.p, metadata.price_precision).ok()?;

        let order_side = match order.d {
            AxOrderSide::Buy => NautilusOrderSide::Buy,
            AxOrderSide::Sell => NautilusOrderSide::Sell,
        };

        // AX primarily uses limit orders
        let order_type = OrderType::Limit;

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
            LiquiditySide::NoLiquiditySide,
            UUID4::new(),
            ts_event,
            get_atomic_clock_realtime().get_time_ns(),
            false,
            None, // position_id
            None, // commission
        ))
    }

    fn create_order_canceled(&mut self, order: &AxWsOrder, event_ts: i64) -> Option<OrderCanceled> {
        let venue_order_id = VenueOrderId::new(&order.oid);
        let metadata = self.lookup_order_metadata(order)?;

        let client_order_id = metadata.client_order_id;
        let trader_id = metadata.trader_id;
        let strategy_id = metadata.strategy_id;
        let instrument_id = metadata.instrument_id;

        // Drop the reference before removing
        drop(metadata);

        // Remove from tracking maps
        self.orders_metadata.remove(&client_order_id);
        self.venue_to_client_id.remove(&venue_order_id);

        let ts_event = UnixNanos::from(event_ts as u64 * 1_000_000_000);

        Some(OrderCanceled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            get_atomic_clock_realtime().get_time_ns(),
            false,
            Some(venue_order_id),
            Some(self.account_id),
        ))
    }

    fn create_order_expired(&mut self, order: &AxWsOrder, event_ts: i64) -> Option<OrderExpired> {
        let venue_order_id = VenueOrderId::new(&order.oid);
        let metadata = self.lookup_order_metadata(order)?;

        let client_order_id = metadata.client_order_id;
        let trader_id = metadata.trader_id;
        let strategy_id = metadata.strategy_id;
        let instrument_id = metadata.instrument_id;

        // Drop the reference before removing
        drop(metadata);

        // Remove from tracking maps
        self.orders_metadata.remove(&client_order_id);
        self.venue_to_client_id.remove(&venue_order_id);

        let ts_event = UnixNanos::from(event_ts as u64 * 1_000_000_000);

        Some(OrderExpired::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            get_atomic_clock_realtime().get_time_ns(),
            false,
            Some(venue_order_id),
            Some(self.account_id),
        ))
    }

    fn create_order_rejected(
        &mut self,
        order: &AxWsOrder,
        reason: &str,
        event_ts: i64,
    ) -> Option<OrderRejected> {
        let client_order_id = self.extract_client_order_id(order)?;
        let metadata = self.orders_metadata.get(&client_order_id)?;

        let trader_id = metadata.trader_id;
        let strategy_id = metadata.strategy_id;
        let instrument_id = metadata.instrument_id;

        // Drop the reference before removing
        drop(metadata);

        // Remove from tracking
        self.orders_metadata.remove(&client_order_id);

        let ts_event = UnixNanos::from(event_ts as u64 * 1_000_000_000);

        Some(OrderRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            self.account_id,
            reason.to_string().into(),
            UUID4::new(),
            ts_event,
            get_atomic_clock_realtime().get_time_ns(),
            false,
            false,
        ))
    }
}
