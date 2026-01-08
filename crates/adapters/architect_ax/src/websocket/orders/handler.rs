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
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::websocket::{AuthTracker, WebSocketClient};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use crate::websocket::messages::{
    AxOrdersWsMessage, AxWsCancelOrder, AxWsCancelOrderResponse, AxWsCancelRejected, AxWsError,
    AxWsGetOpenOrders, AxWsOpenOrdersResponse, AxWsOrderAcknowledged, AxWsOrderCanceled,
    AxWsOrderDoneForDay, AxWsOrderExpired, AxWsOrderFilled, AxWsOrderPartiallyFilled,
    AxWsOrderRejected, AxWsOrderReplaced, AxWsPlaceOrder, AxWsPlaceOrderResponse, OrderMetadata,
};

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
        /// Metadata for response correlation.
        metadata: OrderMetadata,
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
}

/// Orders feed handler that processes WebSocket messages.
///
/// Runs in a dedicated Tokio task and owns the WebSocket client exclusively.
pub(crate) struct FeedHandler {
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    #[allow(dead_code)] // TODO: Use for sending parsed messages
    out_tx: tokio::sync::mpsc::UnboundedSender<AxOrdersWsMessage>,
    auth_tracker: AuthTracker,
    instruments: AHashMap<Ustr, InstrumentAny>,
    pending_orders: AHashMap<i64, OrderMetadata>,
    message_queue: VecDeque<AxOrdersWsMessage>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    #[must_use]
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<AxOrdersWsMessage>,
        auth_tracker: AuthTracker,
    ) -> Self {
        Self {
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            out_tx,
            auth_tracker,
            instruments: AHashMap::new(),
            pending_orders: AHashMap::new(),
            message_queue: VecDeque::new(),
        }
    }

    /// Returns the next message from the handler.
    ///
    /// This method blocks until a message is available or the handler is stopped.
    pub async fn next(&mut self) -> Option<AxOrdersWsMessage> {
        loop {
            if let Some(msg) = self.message_queue.pop_front() {
                return Some(msg);
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await;
                }

                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.signal.load(Ordering::Relaxed) {
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

                    if self.signal.load(Ordering::Relaxed) {
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
                if let Some(client) = self.client.take() {
                    client.disconnect().await;
                }
            }
            HandlerCommand::Authenticate { token: _ } => {
                log::debug!("Authenticate command received");
                // Ax uses Bearer token in connection headers, not a message
                // This is handled at connection time, so we just mark as authenticated
                self.auth_tracker.succeed();
                self.message_queue
                    .push_back(AxOrdersWsMessage::Authenticated);
            }
            HandlerCommand::PlaceOrder {
                request_id,
                order,
                metadata,
            } => {
                log::debug!(
                    "PlaceOrder command received: request_id={request_id}, symbol={}",
                    order.s
                );
                self.pending_orders.insert(request_id, metadata);

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
                    return Some(vec![AxOrdersWsMessage::Reconnected]);
                }

                log::trace!("Raw websocket message: {text}");

                let value: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        log::error!("Failed to parse WebSocket message: {e}: {text}");
                        return None;
                    }
                };

                self.classify_and_parse_message(value)
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

    fn classify_and_parse_message(
        &mut self,
        value: serde_json::Value,
    ) -> Option<Vec<AxOrdersWsMessage>> {
        let obj = value.as_object()?;

        // Response messages have "rid" + "res", event messages have "t"
        if obj.contains_key("rid") && obj.contains_key("res") {
            return self.parse_response_message(value);
        }

        let msg_type = obj.get("t").and_then(|v| v.as_str())?;

        match msg_type {
            "h" => {
                log::trace!("Received heartbeat");
                None
            }
            "n" => match serde_json::from_value::<AxWsOrderAcknowledged>(value) {
                Ok(msg) => {
                    log::debug!("Order acknowledged: {} {}", msg.o.oid, msg.o.s);
                    Some(vec![AxOrdersWsMessage::OrderAcknowledged(msg)])
                }
                Err(e) => {
                    log::error!("Failed to parse order acknowledged: {e}");
                    None
                }
            },
            "p" => match serde_json::from_value::<AxWsOrderPartiallyFilled>(value) {
                Ok(msg) => {
                    log::debug!(
                        "Order partially filled: {} {} @ {}",
                        msg.o.oid,
                        msg.xs.q,
                        msg.xs.p
                    );
                    Some(vec![AxOrdersWsMessage::OrderPartiallyFilled(msg)])
                }
                Err(e) => {
                    log::error!("Failed to parse order partially filled: {e}");
                    None
                }
            },
            "f" => match serde_json::from_value::<AxWsOrderFilled>(value) {
                Ok(msg) => {
                    log::debug!("Order filled: {} {} @ {}", msg.o.oid, msg.xs.q, msg.xs.p);
                    Some(vec![AxOrdersWsMessage::OrderFilled(msg)])
                }
                Err(e) => {
                    log::error!("Failed to parse order filled: {e}");
                    None
                }
            },
            "c" => match serde_json::from_value::<AxWsOrderCanceled>(value) {
                Ok(msg) => {
                    log::debug!("Order canceled: {} reason={}", msg.o.oid, msg.xr);
                    Some(vec![AxOrdersWsMessage::OrderCanceled(msg)])
                }
                Err(e) => {
                    log::error!("Failed to parse order canceled: {e}");
                    None
                }
            },
            "j" => match serde_json::from_value::<AxWsOrderRejected>(value) {
                Ok(msg) => {
                    log::debug!("Order rejected: {} reason={}", msg.o.oid, msg.r);
                    Some(vec![AxOrdersWsMessage::OrderRejectedRaw(msg)])
                }
                Err(e) => {
                    log::error!("Failed to parse order rejected: {e}");
                    None
                }
            },
            "x" => match serde_json::from_value::<AxWsOrderExpired>(value) {
                Ok(msg) => {
                    log::debug!("Order expired: {}", msg.o.oid);
                    Some(vec![AxOrdersWsMessage::OrderExpired(msg)])
                }
                Err(e) => {
                    log::error!("Failed to parse order expired: {e}");
                    None
                }
            },
            "r" => match serde_json::from_value::<AxWsOrderReplaced>(value) {
                Ok(msg) => {
                    log::debug!("Order replaced: {}", msg.o.oid);
                    Some(vec![AxOrdersWsMessage::OrderReplaced(msg)])
                }
                Err(e) => {
                    log::error!("Failed to parse order replaced: {e}");
                    None
                }
            },
            "d" => match serde_json::from_value::<AxWsOrderDoneForDay>(value) {
                Ok(msg) => {
                    log::debug!("Order done for day: {}", msg.o.oid);
                    Some(vec![AxOrdersWsMessage::OrderDoneForDay(msg)])
                }
                Err(e) => {
                    log::error!("Failed to parse order done for day: {e}");
                    None
                }
            },
            "e" => match serde_json::from_value::<AxWsCancelRejected>(value) {
                Ok(msg) => {
                    log::debug!("Cancel rejected: {} reason={}", msg.oid, msg.r);
                    Some(vec![AxOrdersWsMessage::CancelRejected(msg)])
                }
                Err(e) => {
                    log::error!("Failed to parse cancel rejected: {e}");
                    None
                }
            },
            _ => {
                log::warn!("Unknown message type: {msg_type}");
                Some(vec![AxOrdersWsMessage::Error(AxWsError::new(format!(
                    "Unknown message type: {msg_type}"
                )))])
            }
        }
    }

    fn parse_response_message(
        &mut self,
        value: serde_json::Value,
    ) -> Option<Vec<AxOrdersWsMessage>> {
        let obj = value.as_object()?;
        let res = obj.get("res")?;

        if res.is_object() {
            if res.get("oid").is_some() {
                match serde_json::from_value::<AxWsPlaceOrderResponse>(value) {
                    Ok(msg) => {
                        log::debug!("Place order response: rid={} oid={}", msg.rid, msg.res.oid);
                        Some(vec![AxOrdersWsMessage::PlaceOrderResponse(msg)])
                    }
                    Err(e) => {
                        log::error!("Failed to parse place order response: {e}");
                        None
                    }
                }
            } else if res.get("cxl_rx").is_some() {
                match serde_json::from_value::<AxWsCancelOrderResponse>(value) {
                    Ok(msg) => {
                        log::debug!(
                            "Cancel order response: rid={} cxl_rx={}",
                            msg.rid,
                            msg.res.cxl_rx
                        );
                        Some(vec![AxOrdersWsMessage::CancelOrderResponse(msg)])
                    }
                    Err(e) => {
                        log::error!("Failed to parse cancel order response: {e}");
                        None
                    }
                }
            } else {
                log::warn!("Unknown response object format");
                None
            }
        } else if res.is_array() {
            match serde_json::from_value::<AxWsOpenOrdersResponse>(value) {
                Ok(msg) => {
                    log::debug!(
                        "Open orders response: rid={} count={}",
                        msg.rid,
                        msg.res.len()
                    );
                    Some(vec![AxOrdersWsMessage::OpenOrdersResponse(msg)])
                }
                Err(e) => {
                    log::error!("Failed to parse open orders response: {e}");
                    None
                }
            }
        } else {
            log::warn!("Unknown response format");
            None
        }
    }
}
