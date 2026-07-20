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
use nautilus_model::identifiers::{ClientOrderId, VenueOrderId};
use nautilus_network::websocket::{AuthTracker, WebSocketClient};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use crate::{
    common::enums::AxOrderRequestType,
    websocket::{
        messages::{
            AxOrdersWsFrame, AxOrdersWsMessage, AxWsCancelOrder, AxWsError, AxWsGetOpenOrders,
            AxWsOrderEvent, AxWsOrderResponse, AxWsPlaceOrder, OrderMetadata,
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
    /// Numeric AX client ID.
    pub cid: u64,
}

/// Commands sent from the outer client to the inner orders handler.
#[derive(Debug)]
pub enum HandlerCommand {
    /// Set the WebSocket client for this handler.
    SetClient(WebSocketClient),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Mark the current handshake-authenticated session as ready.
    SessionAuthenticated,
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
pub(crate) struct AxOrdersWsFeedHandler {
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    auth_tracker: AuthTracker,
    pending_orders: AHashMap<i64, WsOrderInfo>,
    message_queue: VecDeque<AxOrdersWsMessage>,
    orders_metadata: Arc<DashMap<ClientOrderId, OrderMetadata>>,
    venue_to_client_order_id: Arc<DashMap<VenueOrderId, ClientOrderId>>,
    cid_to_client_order_id: Arc<DashMap<u64, ClientOrderId>>,
    has_authenticated_session: bool,
    needs_session_restore: bool,
}

impl AxOrdersWsFeedHandler {
    /// Creates a new [`AxOrdersWsFeedHandler`] instance.
    #[must_use]
    pub(crate) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        auth_tracker: AuthTracker,
        orders_metadata: Arc<DashMap<ClientOrderId, OrderMetadata>>,
        venue_to_client_order_id: Arc<DashMap<VenueOrderId, ClientOrderId>>,
        cid_to_client_order_id: Arc<DashMap<u64, ClientOrderId>>,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            auth_tracker,
            pending_orders: AHashMap::new(),
            message_queue: VecDeque::new(),
            orders_metadata,
            venue_to_client_order_id,
            cid_to_client_order_id,
            has_authenticated_session: false,
            needs_session_restore: false,
        }
    }

    fn restore_authenticated_session(&mut self) {
        if self.has_authenticated_session {
            log::debug!("Restoring authenticated session after reconnection");

            // The reconnect handshake has already succeeded with the current Bearer header.
            self.auth_tracker.succeed();
            self.message_queue
                .push_back(AxOrdersWsMessage::Authenticated);
            log::debug!("Authenticated session restored");
        } else {
            log::warn!("Cannot restore authentication before the initial session succeeds");
        }
    }

    /// Returns the next message from the handler.
    ///
    /// This method blocks until a message is available or the handler is stopped.
    pub(crate) async fn next(&mut self) -> Option<AxOrdersWsMessage> {
        loop {
            if self.needs_session_restore && self.message_queue.is_empty() {
                self.needs_session_restore = false;
                self.restore_authenticated_session();
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

                        if let Some(client) = &self.inner
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
                self.inner = Some(client);
            }
            HandlerCommand::Disconnect => {
                log::debug!("Disconnect command received");
                self.auth_tracker.fail("Disconnected");

                if let Some(inner) = self.inner.take() {
                    inner.disconnect().await;
                }
            }
            HandlerCommand::SessionAuthenticated => {
                log::debug!("Session authenticated command received");
                self.has_authenticated_session = true;
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
                    self.cid_to_client_order_id.remove(&order_info.cid);
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
        let Some(inner) = &self.inner else {
            return Err("No WebSocket client available".to_string());
        };

        let payload = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        log::trace!("Sending: {payload}");

        inner
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
                    self.needs_session_restore = true;
                    return Some(vec![AxOrdersWsMessage::Reconnected]);
                }

                log::trace!("Raw websocket message: {text}");

                let raw_msg: AxOrdersWsFrame = match parse_order_message(&text) {
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

    fn handle_raw_message(&mut self, raw_msg: AxOrdersWsFrame) -> Option<Vec<AxOrdersWsMessage>> {
        match raw_msg {
            AxOrdersWsFrame::Error(err) => {
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
            AxOrdersWsFrame::Response(resp) => self.handle_response(resp),
            AxOrdersWsFrame::Event(event) => self.handle_event(*event),
        }
    }

    fn handle_response(&mut self, resp: AxWsOrderResponse) -> Option<Vec<AxOrdersWsMessage>> {
        match resp {
            AxWsOrderResponse::PlaceOrder(msg) => {
                log::debug!("Place order response: rid={} oid={}", msg.rid, msg.res.oid);
                let Some(order_info) = self.pending_orders.remove(&msg.rid) else {
                    log::warn!("Ignoring unsolicited place order response: rid={}", msg.rid);
                    return Some(vec![AxOrdersWsMessage::PlaceOrderResponse(msg)]);
                };

                let venue_order_id = match VenueOrderId::new_checked(&msg.res.oid) {
                    Ok(venue_order_id) => venue_order_id,
                    Err(e) => {
                        log::warn!(
                            "Invalid venue order ID in place response for {}: {e}",
                            order_info.client_order_id,
                        );
                        return Some(vec![AxOrdersWsMessage::PlaceOrderResponse(msg)]);
                    }
                };

                if let Some(mut metadata) =
                    self.orders_metadata.get_mut(&order_info.client_order_id)
                {
                    metadata.venue_order_id = Some(venue_order_id);
                    self.venue_to_client_order_id
                        .insert(venue_order_id, order_info.client_order_id);
                } else {
                    log::debug!(
                        "Order tracking already cleared before place response: {}",
                        order_info.client_order_id,
                    );
                }

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
                log::debug!("Open orders response: {} orders", msg.res.orders.len());
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

    fn handle_event(&self, event: AxWsOrderEvent) -> Option<Vec<AxOrdersWsMessage>> {
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
    use nautilus_model::{
        identifiers::{InstrumentId, StrategyId, TraderId},
        types::Currency,
    };
    use nautilus_network::websocket::AuthTracker;
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::websocket::messages::{
        AxWsOrderError, AxWsOrderErrorResponse, AxWsPlaceOrderResponse, AxWsPlaceOrderResult,
    };

    fn test_handler() -> AxOrdersWsFeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        AxOrdersWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            AuthTracker::default(),
            Arc::new(DashMap::new()),
            Arc::new(DashMap::new()),
            Arc::new(DashMap::new()),
        )
    }

    #[rstest]
    fn test_place_order_response_records_venue_identity() {
        let mut handler = test_handler();
        let request_id = 11;
        let cid = 1011;
        let client_order_id = ClientOrderId::from("CID-11");
        let venue_order_id = VenueOrderId::from("OID-11");
        handler
            .orders_metadata
            .insert(client_order_id, test_order_metadata(client_order_id));
        handler.pending_orders.insert(
            request_id,
            WsOrderInfo {
                client_order_id,
                symbol: Ustr::from("EURUSD-PERP"),
                cid,
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
        assert_eq!(
            handler
                .orders_metadata
                .get(&client_order_id)
                .and_then(|metadata| metadata.venue_order_id),
            Some(venue_order_id),
        );
        assert_eq!(
            handler
                .venue_to_client_order_id
                .get(&venue_order_id)
                .map(|client_order_id| *client_order_id),
            Some(client_order_id),
        );
    }

    #[rstest]
    fn test_late_place_order_response_does_not_restore_cleared_tracking() {
        let mut handler = test_handler();
        let request_id = 12;
        let client_order_id = ClientOrderId::from("CID-12");
        handler.pending_orders.insert(
            request_id,
            WsOrderInfo {
                client_order_id,
                symbol: Ustr::from("EURUSD-PERP"),
                cid: 1012,
            },
        );

        let response = AxWsOrderResponse::PlaceOrder(AxWsPlaceOrderResponse {
            rid: request_id,
            res: AxWsPlaceOrderResult {
                oid: "OID-12".to_string(),
            },
        });

        let messages = handler.handle_response(response).unwrap();

        assert_eq!(messages.len(), 1);
        assert!(handler.pending_orders.get(&request_id).is_none());
        assert!(!handler.orders_metadata.contains_key(&client_order_id));
        assert!(handler.venue_to_client_order_id.is_empty());
    }

    #[rstest]
    fn test_place_order_error_preserves_cid_for_reconciliation() {
        let mut handler = test_handler();
        let request_id = 13;
        let cid = 1013;
        let client_order_id = ClientOrderId::from("CID-13");
        handler
            .orders_metadata
            .insert(client_order_id, test_order_metadata(client_order_id));
        handler.cid_to_client_order_id.insert(cid, client_order_id);
        handler.pending_orders.insert(
            request_id,
            WsOrderInfo {
                client_order_id,
                symbol: Ustr::from("EURUSD-PERP"),
                cid,
            },
        );

        let messages = handler
            .handle_raw_message(AxOrdersWsFrame::Error(AxWsOrderErrorResponse {
                rid: request_id,
                err: AxWsOrderError {
                    code: 400,
                    msg: "invalid order".to_string(),
                },
            }))
            .unwrap();

        assert_eq!(messages.len(), 1);
        assert!(handler.pending_orders.get(&request_id).is_none());
        assert!(!handler.orders_metadata.contains_key(&client_order_id));
        assert_eq!(
            handler
                .cid_to_client_order_id
                .get(&cid)
                .map(|client_order_id| *client_order_id),
            Some(client_order_id),
        );
    }

    #[rstest]
    fn test_handle_event_forwards_venue_event() {
        let handler = test_handler();

        let event = AxWsOrderEvent::Heartbeat;
        let result = handler.handle_event(event);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_authenticated_session_is_restored_after_reconnect() {
        let mut handler = test_handler();

        handler
            .handle_command(HandlerCommand::SessionAuthenticated)
            .await;
        let initial = handler.message_queue.pop_front();
        handler.auth_tracker.fail("Reconnecting");
        handler.restore_authenticated_session();
        let restored = handler.message_queue.pop_front();

        assert!(handler.has_authenticated_session);
        assert!(matches!(initial, Some(AxOrdersWsMessage::Authenticated)));
        assert!(matches!(restored, Some(AxOrdersWsMessage::Authenticated)));
        assert!(handler.auth_tracker.is_authenticated());
    }

    fn test_order_metadata(client_order_id: ClientOrderId) -> OrderMetadata {
        OrderMetadata {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("EURUSD-PERP.AX"),
            client_order_id,
            venue_order_id: None,
            ts_init: 0.into(),
            size_precision: 0,
            price_precision: 2,
            quote_currency: Currency::USD(),
        }
    }
}
