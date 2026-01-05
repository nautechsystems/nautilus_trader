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

//! Binance Spot WebSocket API message handler.
//!
//! The handler runs in a dedicated Tokio task as the I/O boundary between the client
//! orchestrator and the network layer. It exclusively owns the `WebSocketClient` and
//! processes commands from the client via an unbounded channel.
//!
//! ## Key Responsibilities
//!
//! - Command processing: Receives `HandlerCommand` from client, serializes to JSON requests.
//! - Response decoding: Parses SBE binary responses using schema 3 decoders.
//! - Request correlation: Matches responses to pending requests by ID.
//! - Message transformation: Emits `NautilusWsApiMessage` events to client via channel.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_network::{RECONNECTED, websocket::WebSocketClient};
use tokio_tungstenite::tungstenite::Message;

use super::{
    error::{BinanceWsApiError, BinanceWsApiResult},
    messages::{HandlerCommand, NautilusWsApiMessage, RequestMeta, WsApiRequest, method},
};
use crate::{
    common::{
        credential::Credential,
        sbe::spot::{
            ReadBuf, message_header_codec,
            web_socket_response_codec::{SBE_TEMPLATE_ID, WebSocketResponseDecoder},
        },
    },
    spot::http::{models::BinanceCancelOrderResponse, parse},
};

/// Binance Spot WebSocket API handler.
///
/// Runs in a dedicated Tokio task, processing commands from the client
/// and transforming raw WebSocket messages into Nautilus domain events.
/// Messages are sent to the client via the output channel.
pub struct BinanceSpotWsApiHandler {
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsApiMessage>,
    credential: Arc<Credential>,
    pending_requests: AHashMap<String, RequestMeta>,
}

impl Debug for BinanceSpotWsApiHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceSpotWsApiHandler))
            .field("inner", &self.inner.as_ref().map(|_| "<client>"))
            .field(
                "pending_requests",
                &format!("{} pending", self.pending_requests.len()),
            )
            .finish_non_exhaustive()
    }
}

impl BinanceSpotWsApiHandler {
    /// Creates a new handler instance.
    #[must_use]
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsApiMessage>,
        credential: Arc<Credential>,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            out_tx,
            credential,
            pending_requests: AHashMap::new(),
        }
    }

    /// Main event loop - processes commands and raw messages.
    ///
    /// Sends output messages via `out_tx` channel. Returns `false` when disconnected
    /// or the signal is set, indicating the handler should exit.
    pub async fn run(&mut self) -> bool {
        loop {
            if self.signal.load(Ordering::Relaxed) {
                return false;
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("Handler received WebSocket client");
                            self.inner = Some(client);
                            self.emit(NautilusWsApiMessage::Connected);
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Handler disconnecting WebSocket client");
                            self.inner = None;
                            return false;
                        }
                        HandlerCommand::PlaceOrder { id, params } => {
                            if let Err(e) = self.handle_place_order(id.clone(), params).await {
                                log::error!("Failed to handle place order command: {e}");
                                self.emit(NautilusWsApiMessage::OrderRejected {
                                    request_id: id,
                                    code: -1,
                                    msg: e.to_string(),
                                });
                            }
                        }
                        HandlerCommand::CancelOrder { id, params } => {
                            if let Err(e) = self.handle_cancel_order(id.clone(), params).await {
                                log::error!("Failed to handle cancel order command: {e}");
                                self.emit(NautilusWsApiMessage::CancelRejected {
                                    request_id: id,
                                    code: -1,
                                    msg: e.to_string(),
                                });
                            }
                        }
                        HandlerCommand::CancelReplaceOrder { id, params } => {
                            if let Err(e) = self.handle_cancel_replace_order(id.clone(), params).await {
                                log::error!("Failed to handle cancel replace command: {e}");
                                self.emit(NautilusWsApiMessage::CancelReplaceRejected {
                                    request_id: id,
                                    code: -1,
                                    msg: e.to_string(),
                                });
                            }
                        }
                        HandlerCommand::CancelAllOrders { id, symbol } => {
                            if let Err(e) = self.handle_cancel_all_orders(id.clone(), symbol).await {
                                log::error!("Failed to handle cancel all command: {e}");
                                self.emit(NautilusWsApiMessage::CancelRejected {
                                    request_id: id,
                                    code: -1,
                                    msg: e.to_string(),
                                });
                            }
                        }
                    }
                }
                Some(msg) = self.raw_rx.recv() => {
                    if let Message::Text(ref text) = msg
                        && text.as_str() == RECONNECTED
                    {
                        log::info!("Handler received reconnection signal");

                        // Fail any pending requests - they won't get responses on new connection
                        self.fail_pending_requests();

                        self.emit(NautilusWsApiMessage::Reconnected);
                        continue;
                    }

                    self.handle_message(msg);
                }
                else => {
                    // Both channels closed
                    return false;
                }
            }
        }
    }

    /// Sends a message to the output channel.
    fn emit(&self, msg: NautilusWsApiMessage) {
        if let Err(e) = self.out_tx.send(msg) {
            log::error!("Failed to send message to output channel: {e}");
        }
    }

    /// Fails all pending requests after a reconnection.
    fn fail_pending_requests(&mut self) {
        if self.pending_requests.is_empty() {
            return;
        }

        let count = self.pending_requests.len();
        log::warn!("Failing {count} pending requests after reconnection");

        let pending = std::mem::take(&mut self.pending_requests);
        for (request_id, meta) in pending {
            let msg = self.create_rejection(
                request_id,
                -1,
                "Connection lost before response received".to_string(),
                meta,
            );
            self.emit(msg);
        }
    }

    async fn handle_place_order(
        &mut self,
        id: String,
        params: crate::spot::http::query::NewOrderParams,
    ) -> BinanceWsApiResult<()> {
        let params_json = serde_json::to_value(&params)
            .map_err(|e| BinanceWsApiError::ClientError(e.to_string()))?;
        let signed_params = self.sign_params(params_json)?;

        let request = WsApiRequest::new(&id, method::ORDER_PLACE, signed_params);
        self.pending_requests
            .insert(id.clone(), RequestMeta::PlaceOrder);
        self.send_request(request).await
    }

    async fn handle_cancel_order(
        &mut self,
        id: String,
        params: crate::spot::http::query::CancelOrderParams,
    ) -> BinanceWsApiResult<()> {
        let params_json = serde_json::to_value(&params)
            .map_err(|e| BinanceWsApiError::ClientError(e.to_string()))?;
        let signed_params = self.sign_params(params_json)?;

        let request = WsApiRequest::new(&id, method::ORDER_CANCEL, signed_params);
        self.pending_requests
            .insert(id.clone(), RequestMeta::CancelOrder);
        self.send_request(request).await
    }

    async fn handle_cancel_replace_order(
        &mut self,
        id: String,
        params: crate::spot::http::query::CancelReplaceOrderParams,
    ) -> BinanceWsApiResult<()> {
        let params_json = serde_json::to_value(&params)
            .map_err(|e| BinanceWsApiError::ClientError(e.to_string()))?;
        let signed_params = self.sign_params(params_json)?;

        let request = WsApiRequest::new(&id, method::ORDER_CANCEL_REPLACE, signed_params);
        self.pending_requests
            .insert(id.clone(), RequestMeta::CancelReplaceOrder);
        self.send_request(request).await
    }

    async fn handle_cancel_all_orders(
        &mut self,
        id: String,
        symbol: String,
    ) -> BinanceWsApiResult<()> {
        let params_json = serde_json::json!({ "symbol": symbol });
        let signed_params = self.sign_params(params_json)?;

        let request = WsApiRequest::new(&id, method::OPEN_ORDERS_CANCEL_ALL, signed_params);
        self.pending_requests
            .insert(id.clone(), RequestMeta::CancelAllOrders);
        self.send_request(request).await
    }

    fn sign_params(&self, mut params: serde_json::Value) -> BinanceWsApiResult<serde_json::Value> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| BinanceWsApiError::ClientError(e.to_string()))?
            .as_millis() as i64;

        if let Some(obj) = params.as_object_mut() {
            obj.insert("timestamp".to_string(), serde_json::json!(timestamp));
            obj.insert(
                "apiKey".to_string(),
                serde_json::json!(self.credential.api_key()),
            );
        }

        let query_string = serde_urlencoded::to_string(&params)
            .map_err(|e| BinanceWsApiError::ClientError(e.to_string()))?;
        let signature = self.credential.sign(&query_string);

        if let Some(obj) = params.as_object_mut() {
            obj.insert("signature".to_string(), serde_json::json!(signature));
        }

        Ok(params)
    }

    async fn send_request(&mut self, request: WsApiRequest) -> BinanceWsApiResult<()> {
        use super::client::BINANCE_WS_RATE_LIMIT_KEY_ORDER;

        let client = self.inner.as_mut().ok_or_else(|| {
            BinanceWsApiError::ConnectionError("WebSocket not connected".to_string())
        })?;

        let json = serde_json::to_string(&request)
            .map_err(|e| BinanceWsApiError::ClientError(e.to_string()))?;

        log::debug!(
            "Sending WebSocket API request id={} method={}",
            request.id,
            request.method
        );

        // Apply rate limiting for order operations
        let rate_limit_keys = Some(vec![BINANCE_WS_RATE_LIMIT_KEY_ORDER.to_string()]);

        client.send_text(json, rate_limit_keys).await.map_err(|e| {
            BinanceWsApiError::ConnectionError(format!("Failed to send request: {e}"))
        })?;

        Ok(())
    }

    fn handle_message(&mut self, msg: Message) {
        match msg {
            Message::Binary(data) => self.handle_binary_response(&data),
            Message::Text(text) => self.handle_text_response(&text),
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(frame) => {
                log::debug!("WebSocket closed: {frame:?}");
            }
            Message::Frame(_) => {}
        }
    }

    fn handle_binary_response(&mut self, data: &[u8]) {
        match self.decode_ws_api_response(data) {
            Ok(response) => self.emit(response),
            Err(e) => {
                log::error!("Failed to decode WebSocket API response: {e}");
                self.emit(NautilusWsApiMessage::Error(e.to_string()));
            }
        }
    }

    fn handle_text_response(&mut self, text: &str) {
        // Text responses are typically JSON errors
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(json) => {
                if let Some(code) = json.get("code").and_then(|v| v.as_i64()) {
                    let msg = json
                        .get("msg")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error");
                    let id = json.get("id").and_then(|v| v.as_str()).map(String::from);

                    if let Some(request_id) = id
                        && let Some(meta) = self.pending_requests.remove(&request_id)
                    {
                        let rejection =
                            self.create_rejection(request_id, code as i32, msg.to_string(), meta);
                        self.emit(rejection);
                        return;
                    }
                    log::warn!(
                        "Received error response without matching request ID: code={code} msg={msg}"
                    );
                }
            }
            Err(e) => {
                log::warn!("Failed to parse text response as JSON: {e}");
            }
        }
    }

    fn decode_ws_api_response(
        &mut self,
        data: &[u8],
    ) -> Result<NautilusWsApiMessage, BinanceWsApiError> {
        // Parse SBE envelope to extract request ID and inner payload
        let (request_id, status, result_data) = self.parse_envelope(data)?;

        // Look up the pending request by ID
        let meta = self.pending_requests.remove(&request_id).ok_or_else(|| {
            BinanceWsApiError::UnknownRequestId(format!("No pending request for ID: {request_id}"))
        })?;

        // Check for error status (non-200)
        if status != 200 {
            return Ok(self.create_rejection(
                request_id,
                status as i32,
                format!("Request failed with status {status}"),
                meta,
            ));
        }

        // Decode the inner payload based on request type
        match meta {
            RequestMeta::PlaceOrder => {
                let response = parse::decode_new_order_full(&result_data)?;
                Ok(NautilusWsApiMessage::OrderAccepted {
                    request_id,
                    response,
                })
            }
            RequestMeta::CancelOrder => {
                let response = parse::decode_cancel_order(&result_data)?;
                Ok(NautilusWsApiMessage::OrderCanceled {
                    request_id,
                    response,
                })
            }
            RequestMeta::CancelReplaceOrder => {
                // Cancel-replace returns both cancel and new order info
                let new_order_response = parse::decode_new_order_full(&result_data)?;
                let cancel_response = BinanceCancelOrderResponse {
                    price_exponent: new_order_response.price_exponent,
                    qty_exponent: new_order_response.qty_exponent,
                    order_id: 0,
                    order_list_id: None,
                    transact_time: new_order_response.transact_time,
                    price_mantissa: 0,
                    orig_qty_mantissa: 0,
                    executed_qty_mantissa: 0,
                    cummulative_quote_qty_mantissa: 0,
                    status: crate::common::sbe::spot::order_status::OrderStatus::Canceled,
                    time_in_force: new_order_response.time_in_force,
                    order_type: new_order_response.order_type,
                    side: new_order_response.side,
                    self_trade_prevention_mode: new_order_response.self_trade_prevention_mode,
                    client_order_id: String::new(),
                    orig_client_order_id: String::new(),
                    symbol: new_order_response.symbol.clone(),
                };
                Ok(NautilusWsApiMessage::CancelReplaceAccepted {
                    request_id,
                    cancel_response,
                    new_order_response,
                })
            }
            RequestMeta::CancelAllOrders => {
                let responses = parse::decode_cancel_open_orders(&result_data)?;
                Ok(NautilusWsApiMessage::AllOrdersCanceled {
                    request_id,
                    responses,
                })
            }
        }
    }

    /// Parses the WebSocketResponse SBE envelope.
    ///
    /// Returns (request_id, status, result_payload).
    fn parse_envelope(&self, data: &[u8]) -> Result<(String, u16, Vec<u8>), BinanceWsApiError> {
        if data.len() < message_header_codec::ENCODED_LENGTH {
            return Err(BinanceWsApiError::DecodeError(
                crate::common::sbe::error::SbeDecodeError::BufferTooShort {
                    expected: message_header_codec::ENCODED_LENGTH,
                    actual: data.len(),
                },
            ));
        }

        let buf = ReadBuf::new(data);

        // Parse message header
        let block_length = buf.get_u16_at(0);
        let template_id = buf.get_u16_at(2);

        if template_id != SBE_TEMPLATE_ID {
            return Err(BinanceWsApiError::DecodeError(
                crate::common::sbe::error::SbeDecodeError::UnknownTemplateId(template_id),
            ));
        }

        let version = buf.get_u16_at(6);

        // Create decoder at offset after message header
        let decoder = WebSocketResponseDecoder::default().wrap(
            buf,
            message_header_codec::ENCODED_LENGTH,
            block_length,
            version,
        );

        // Read status from fixed block (offset 1 within block)
        let status = decoder.status();

        // Skip rate_limits group
        let mut rate_limits = decoder.rate_limits_decoder();
        while rate_limits.advance().unwrap_or(None).is_some() {}
        let mut decoder = rate_limits.parent().map_err(|_| {
            BinanceWsApiError::ClientError("Failed to get parent from rate_limits".to_string())
        })?;

        // Extract request ID
        let id_coords = decoder.id_decoder();
        let id_bytes = decoder.id_slice(id_coords);
        let request_id = String::from_utf8_lossy(id_bytes).to_string();

        // Extract result payload - copy to owned Vec to avoid lifetime issues
        let result_coords = decoder.result_decoder();
        let result_data = decoder.result_slice(result_coords).to_vec();

        Ok((request_id, status, result_data))
    }

    fn create_rejection(
        &self,
        request_id: String,
        code: i32,
        msg: String,
        meta: RequestMeta,
    ) -> NautilusWsApiMessage {
        match meta {
            RequestMeta::PlaceOrder => NautilusWsApiMessage::OrderRejected {
                request_id,
                code,
                msg,
            },
            RequestMeta::CancelOrder => NautilusWsApiMessage::CancelRejected {
                request_id,
                code,
                msg,
            },
            RequestMeta::CancelReplaceOrder => NautilusWsApiMessage::CancelReplaceRejected {
                request_id,
                code,
                msg,
            },
            RequestMeta::CancelAllOrders => NautilusWsApiMessage::CancelRejected {
                request_id,
                code,
                msg,
            },
        }
    }
}
