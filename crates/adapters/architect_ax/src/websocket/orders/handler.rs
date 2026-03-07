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
use nautilus_model::identifiers::ClientOrderId;
use nautilus_network::websocket::{AuthTracker, WebSocketClient};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use crate::{
    common::enums::AxOrderRequestType,
    websocket::{
        messages::{
            AxOrdersWsMessage, AxWsCancelOrder, AxWsError, AxWsGetOpenOrders, AxWsOrderEvent,
            AxWsOrderResponse, AxWsPlaceOrder, AxWsRawMessage, OrderMetadata,
        },
        parse::parse_order_message,
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
}

/// Orders feed handler that processes WebSocket messages.
///
/// Runs in a dedicated Tokio task and owns the WebSocket client exclusively.
/// Emits raw venue types for downstream consumers to parse into domain events.
pub(crate) struct FeedHandler {
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    auth_tracker: AuthTracker,
    pending_orders: AHashMap<i64, WsOrderInfo>,
    message_queue: VecDeque<AxOrdersWsMessage>,
    orders_metadata: Arc<DashMap<ClientOrderId, OrderMetadata>>,
    cid_to_client_order_id: Arc<DashMap<u64, ClientOrderId>>,
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
        orders_metadata: Arc<DashMap<ClientOrderId, OrderMetadata>>,
        cid_to_client_order_id: Arc<DashMap<u64, ClientOrderId>>,
    ) -> Self {
        Self {
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            auth_tracker,
            pending_orders: AHashMap::new(),
            message_queue: VecDeque::new(),
            orders_metadata,
            cid_to_client_order_id,
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
        if matches!(event, AxWsOrderEvent::Heartbeat) {
            log::trace!("Received heartbeat");
            return None;
        }
        Some(vec![AxOrdersWsMessage::Event(Box::new(event))])
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use dashmap::DashMap;
    use nautilus_network::websocket::AuthTracker;
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::websocket::messages::{AxWsPlaceOrderResponse, AxWsPlaceOrderResult};

    fn test_handler() -> FeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            AuthTracker::default(),
            Arc::new(DashMap::new()),
            Arc::new(DashMap::new()),
        )
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
    fn test_handle_event_forwards_venue_event() {
        let mut handler = test_handler();

        let event = AxWsOrderEvent::Heartbeat;
        let result = handler.handle_event(event);
        assert!(result.is_none());
    }
}
