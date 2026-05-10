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
//! ## Responsibilities
//!
//! - Command processing: Receives `BinanceSpotWsTradingCommand` from client, serializes to JSON requests.
//! - Response decoding: Parses SBE binary responses using schema 3 decoders.
//! - Request correlation: Matches responses to pending requests by ID.
//! - Message transformation: Emits `BinanceSpotWsTradingMessage` events to client via channel.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_network::{RECONNECTED, websocket::WebSocketClient};
use tokio_tungstenite::tungstenite::Message;

use super::{
    client::BINANCE_WS_RATE_LIMIT_KEY_ORDER,
    error::{BinanceWsApiError, BinanceWsApiResult},
    messages::{
        BinanceSpotWsTradingCommand, BinanceSpotWsTradingMessage, BinanceSpotWsTradingRequest,
        BinanceSpotWsTradingRequestMeta, method,
    },
};
use crate::{
    common::credential::SigningCredential,
    spot::{
        http::{models::BinanceCancelOrderResponse, parse},
        sbe::spot::{
            ReadBuf,
            error_response_codec::ErrorResponseDecoder,
            message_header_codec,
            web_socket_response_codec::{SBE_TEMPLATE_ID, WebSocketResponseDecoder},
        },
    },
};

/// Binance Spot WebSocket API handler.
///
/// Runs in a dedicated Tokio task, processing commands from the client
/// and transforming raw WebSocket messages into Nautilus domain events.
/// Messages are sent to the client via the output channel.
pub struct BinanceSpotWsTradingHandler {
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceSpotWsTradingCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<BinanceSpotWsTradingMessage>,
    credential: Arc<SigningCredential>,
    pending_requests: AHashMap<String, BinanceSpotWsTradingRequestMeta>,
    request_id_counter: AtomicU64,
}

impl Debug for BinanceSpotWsTradingHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceSpotWsTradingHandler))
            .field("inner", &self.inner.as_ref().map(|_| "<client>"))
            .field(
                "pending_requests",
                &format!("{} pending", self.pending_requests.len()),
            )
            .finish_non_exhaustive()
    }
}

impl BinanceSpotWsTradingHandler {
    /// Creates a new handler instance.
    #[must_use]
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceSpotWsTradingCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<BinanceSpotWsTradingMessage>,
        credential: Arc<SigningCredential>,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            out_tx,
            credential,
            pending_requests: AHashMap::new(),
            request_id_counter: AtomicU64::new(1000),
        }
    }

    /// Runs the main event loop for commands and raw messages.
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
                        BinanceSpotWsTradingCommand::SetClient(client) => {
                            log::debug!("Handler received WebSocket client");
                            self.inner = Some(client);
                            self.emit(BinanceSpotWsTradingMessage::Connected);
                        }
                        BinanceSpotWsTradingCommand::Disconnect => {
                            log::debug!("Handler disconnecting WebSocket client");
                            self.inner = None;
                            return false;
                        }
                        BinanceSpotWsTradingCommand::PlaceOrder { id, params } => {
                            if let Err(e) = self.handle_place_order(id.clone(), params).await {
                                log::error!("Failed to handle place order command: {e}");
                                self.emit(BinanceSpotWsTradingMessage::OrderRejected {
                                    request_id: id,
                                    code: -1,
                                    msg: e.to_string(),
                                });
                            }
                        }
                        BinanceSpotWsTradingCommand::CancelOrder { id, params } => {
                            if let Err(e) = self.handle_cancel_order(id.clone(), params).await {
                                log::error!("Failed to handle cancel order command: {e}");
                                self.emit(BinanceSpotWsTradingMessage::CancelRejected {
                                    request_id: id,
                                    code: -1,
                                    msg: e.to_string(),
                                });
                            }
                        }
                        BinanceSpotWsTradingCommand::CancelReplaceOrder { id, params } => {
                            if let Err(e) = self.handle_cancel_replace_order(id.clone(), params).await {
                                log::error!("Failed to handle cancel replace command: {e}");
                                self.emit(BinanceSpotWsTradingMessage::CancelReplaceRejected {
                                    request_id: id,
                                    code: -1,
                                    msg: e.to_string(),
                                });
                            }
                        }
                        BinanceSpotWsTradingCommand::CancelAllOrders { id, symbol } => {
                            if let Err(e) = self.handle_cancel_all_orders(id.clone(), symbol).await {
                                log::error!("Failed to handle cancel all command: {e}");
                                self.emit(BinanceSpotWsTradingMessage::CancelRejected {
                                    request_id: id,
                                    code: -1,
                                    msg: e.to_string(),
                                });
                            }
                        }
                        BinanceSpotWsTradingCommand::SessionLogon => {
                            if let Err(e) = self.handle_session_logon().await {
                                log::error!("Session logon failed: {e}");
                                self.emit(BinanceSpotWsTradingMessage::Error(
                                    format!("Session logon failed: {e}"),
                                ));
                            }
                        }
                        BinanceSpotWsTradingCommand::SubscribeUserData => {
                            if let Err(e) = self.handle_subscribe_user_data().await {
                                log::error!("User data subscribe failed: {e}");
                                self.emit(BinanceSpotWsTradingMessage::Error(
                                    format!("User data subscribe failed: {e}"),
                                ));
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

                        self.emit(BinanceSpotWsTradingMessage::Reconnected);
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
    fn emit(&self, msg: BinanceSpotWsTradingMessage) {
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

        let request = BinanceSpotWsTradingRequest::new(&id, method::ORDER_PLACE, signed_params);
        self.pending_requests
            .insert(id.clone(), BinanceSpotWsTradingRequestMeta::PlaceOrder);
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

        let request = BinanceSpotWsTradingRequest::new(&id, method::ORDER_CANCEL, signed_params);
        self.pending_requests
            .insert(id.clone(), BinanceSpotWsTradingRequestMeta::CancelOrder);
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

        let request =
            BinanceSpotWsTradingRequest::new(&id, method::ORDER_CANCEL_REPLACE, signed_params);
        self.pending_requests.insert(
            id.clone(),
            BinanceSpotWsTradingRequestMeta::CancelReplaceOrder,
        );
        self.send_request(request).await
    }

    async fn handle_cancel_all_orders(
        &mut self,
        id: String,
        symbol: String,
    ) -> BinanceWsApiResult<()> {
        let params_json = serde_json::json!({ "symbol": symbol });
        let signed_params = self.sign_params(params_json)?;

        let request =
            BinanceSpotWsTradingRequest::new(&id, method::OPEN_ORDERS_CANCEL_ALL, signed_params);
        self.pending_requests
            .insert(id.clone(), BinanceSpotWsTradingRequestMeta::CancelAllOrders);
        self.send_request(request).await
    }

    async fn handle_session_logon(&mut self) -> BinanceWsApiResult<()> {
        let id = self.next_request_id();
        let params_json = serde_json::json!({});
        let signed_params = self.sign_params(params_json)?;

        let request = BinanceSpotWsTradingRequest::new(&id, "session.logon", signed_params);
        self.pending_requests
            .insert(id, BinanceSpotWsTradingRequestMeta::SessionLogon);
        self.send_request(request).await
    }

    async fn handle_subscribe_user_data(&mut self) -> BinanceWsApiResult<()> {
        let id = self.next_request_id();
        let request = BinanceSpotWsTradingRequest::new(
            &id,
            "userDataStream.subscribe",
            serde_json::json!({}),
        );
        self.pending_requests
            .insert(id, BinanceSpotWsTradingRequestMeta::SubscribeUserData);
        self.send_request(request).await
    }

    fn next_request_id(&self) -> String {
        let id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);
        format!("ws-{id}")
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

    async fn send_request(
        &mut self,
        request: BinanceSpotWsTradingRequest,
    ) -> BinanceWsApiResult<()> {
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
        client
            .send_text(json, Some(BINANCE_WS_RATE_LIMIT_KEY_ORDER.as_slice()))
            .await
            .map_err(|e| {
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
                self.emit(BinanceSpotWsTradingMessage::Error(e.to_string()));
            }
        }
    }

    fn handle_text_response(&mut self, text: &str) {
        let json: serde_json::Value = match serde_json::from_str(text) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Failed to parse text response as JSON: {e}");
                return;
            }
        };

        // User data events arrive wrapped: {"subscriptionId": N, "event": {...}}
        if let Some(event) = json.get("event") {
            self.handle_user_data_event(event);
            return;
        }

        // WS API responses have an "id" field for request correlation
        if let Some(id) = json.get("id") {
            let id_str = match id {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                _ => return,
            };

            if let Some(meta) = self.pending_requests.remove(&id_str) {
                // Check for error: nested {"error": {"code": N, "msg": "..."}}
                // or top-level {"code": N, "msg": "..."}
                let error_info = json
                    .get("error")
                    .map(|e| {
                        (
                            e.get("code").and_then(|v| v.as_i64()).unwrap_or(-1),
                            e.get("msg")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error")
                                .to_string(),
                        )
                    })
                    .or_else(|| {
                        json.get("code").and_then(|c| c.as_i64()).map(|code| {
                            let msg = json
                                .get("msg")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error")
                                .to_string();
                            (code, msg)
                        })
                    });

                if let Some((code, msg)) = error_info {
                    let rejection = self.create_rejection(id_str, code as i32, msg, meta);
                    self.emit(rejection);
                    return;
                }

                // Success response
                match meta {
                    BinanceSpotWsTradingRequestMeta::SessionLogon => {
                        log::info!("Session authenticated");
                        self.emit(BinanceSpotWsTradingMessage::Authenticated);
                    }
                    BinanceSpotWsTradingRequestMeta::SubscribeUserData => {
                        let subscription_id = json
                            .get("result")
                            .and_then(|r| r.get("subscriptionId"))
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        log::info!("User data stream subscribed: id={subscription_id}");
                        self.emit(BinanceSpotWsTradingMessage::UserDataSubscribed {
                            subscription_id,
                        });
                    }
                    _ => {
                        // Order operation responses come as SBE binary, not JSON text.
                        // If we get a JSON success for an order operation, log it.
                        log::debug!("Unexpected JSON success for request {id_str}: {json}");
                    }
                }
                return;
            }

            // Error response without matching pending request
            if let Some(code) = json.get("code").and_then(|v| v.as_i64()) {
                let msg = json
                    .get("msg")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                log::warn!(
                    "Received error response without matching request ID: code={code} msg={msg}"
                );
            }
            return;
        }

        // Stream termination event
        if json.get("eventStreamTerminated").is_some() {
            log::warn!("User data stream terminated, resubscribe needed");
            return;
        }

        log::debug!("Unhandled text message: {text}");
    }

    fn handle_user_data_event(&self, event: &serde_json::Value) {
        let event_type = event.get("e").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "executionReport" => {
                match serde_json::from_value::<super::user_data::BinanceSpotExecutionReport>(
                    event.clone(),
                ) {
                    Ok(report) => {
                        log::debug!(
                            "Execution report: symbol={}, order_id={}, exec={:?}, status={:?}",
                            report.symbol,
                            report.order_id,
                            report.execution_type,
                            report.order_status
                        );
                        self.emit(BinanceSpotWsTradingMessage::ExecutionReport(Box::new(
                            report,
                        )));
                    }
                    Err(e) => log::warn!("Failed to parse execution report: {e}"),
                }
            }
            "outboundAccountPosition" => {
                match serde_json::from_value::<super::user_data::BinanceSpotAccountPositionMsg>(
                    event.clone(),
                ) {
                    Ok(msg) => {
                        log::debug!("Account position update: {} balance(s)", msg.balances.len());
                        self.emit(BinanceSpotWsTradingMessage::AccountPosition(msg));
                    }
                    Err(e) => log::warn!("Failed to parse account position: {e}"),
                }
            }
            "balanceUpdate" => {
                match serde_json::from_value::<super::user_data::BinanceSpotBalanceUpdateMsg>(
                    event.clone(),
                ) {
                    Ok(msg) => {
                        log::debug!("Balance update: asset={}, delta={}", msg.asset, msg.delta);
                        self.emit(BinanceSpotWsTradingMessage::BalanceUpdate(msg));
                    }
                    Err(e) => log::warn!("Failed to parse balance update: {e}"),
                }
            }
            _ => {
                log::debug!("Unhandled user data event type: {event_type}");
            }
        }
    }

    fn decode_ws_api_response(
        &mut self,
        data: &[u8],
    ) -> Result<BinanceSpotWsTradingMessage, BinanceWsApiError> {
        // Check template ID before parsing
        if data.len() >= message_header_codec::ENCODED_LENGTH {
            let buf = ReadBuf::new(data);
            let template_id = buf.get_u16_at(2);

            // User data stream events arrive as SBE with their own template IDs
            // (not wrapped in WebSocketResponse template 50).
            match template_id {
                601 => {
                    log::debug!("Received SBE BalanceUpdateEvent ({} bytes)", data.len());
                    match super::decode_sbe::decode_balance_update(data) {
                        Ok(msg) => {
                            log::debug!(
                                "SBE balance update: asset={}, delta={}",
                                msg.asset,
                                msg.delta
                            );
                            return Ok(BinanceSpotWsTradingMessage::BalanceUpdate(msg));
                        }
                        Err(e) => {
                            log::error!("Failed to decode SBE BalanceUpdateEvent: {e}");
                            return Ok(BinanceSpotWsTradingMessage::Error(format!(
                                "SBE BalanceUpdateEvent decode failed: {e}"
                            )));
                        }
                    }
                }
                603 => {
                    log::debug!("Received SBE ExecutionReportEvent ({} bytes)", data.len());
                    match super::decode_sbe::decode_execution_report(data) {
                        Ok(report) => {
                            log::debug!(
                                "SBE execution report: symbol={}, order_id={}, exec={:?}, status={:?}",
                                report.symbol,
                                report.order_id,
                                report.execution_type,
                                report.order_status
                            );
                            return Ok(BinanceSpotWsTradingMessage::ExecutionReport(Box::new(
                                report,
                            )));
                        }
                        Err(e) => {
                            log::error!("Failed to decode SBE ExecutionReportEvent: {e}");
                            return Ok(BinanceSpotWsTradingMessage::Error(format!(
                                "SBE ExecutionReportEvent decode failed: {e}"
                            )));
                        }
                    }
                }
                606 => {
                    log::debug!(
                        "Received SBE ListStatusEvent ({} bytes), not yet decoded",
                        data.len()
                    );
                    return Ok(BinanceSpotWsTradingMessage::Error(
                        "SBE ListStatusEvent decoding not yet implemented".to_string(),
                    ));
                }
                607 => {
                    log::debug!(
                        "Received SBE OutboundAccountPositionEvent ({} bytes)",
                        data.len()
                    );

                    match super::decode_sbe::decode_account_position(data) {
                        Ok(msg) => {
                            log::debug!("SBE account position: {} balance(s)", msg.balances.len());
                            return Ok(BinanceSpotWsTradingMessage::AccountPosition(msg));
                        }
                        Err(e) => {
                            log::error!("Failed to decode SBE OutboundAccountPositionEvent: {e}");
                            return Ok(BinanceSpotWsTradingMessage::Error(format!(
                                "SBE OutboundAccountPositionEvent decode failed: {e}"
                            )));
                        }
                    }
                }
                _ => {} // Fall through to WebSocketResponse parsing
            }
        }

        // Standard WebSocketResponse envelope (template 50)
        let (request_id, status, result_data) = self.parse_envelope(data)?;

        // Look up the pending request by ID
        let meta = self.pending_requests.remove(&request_id).ok_or_else(|| {
            BinanceWsApiError::UnknownRequestId(format!("No pending request for ID: {request_id}"))
        })?;

        // Check for error status (non-200)
        if status != 200 {
            let (code, msg) = Self::try_decode_sbe_error(&result_data).unwrap_or((
                status as i32,
                format!("Request failed with status {status}"),
            ));
            return Ok(self.create_rejection(request_id, code, msg, meta));
        }

        // Decode the inner payload based on request type
        match meta {
            BinanceSpotWsTradingRequestMeta::PlaceOrder => {
                let response = parse::decode_new_order_full(&result_data)?;
                Ok(BinanceSpotWsTradingMessage::OrderAccepted {
                    request_id,
                    response,
                })
            }
            BinanceSpotWsTradingRequestMeta::CancelOrder => {
                let response = parse::decode_cancel_order(&result_data)?;
                Ok(BinanceSpotWsTradingMessage::OrderCanceled {
                    request_id,
                    response,
                })
            }
            BinanceSpotWsTradingRequestMeta::CancelReplaceOrder => {
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
                    status: crate::spot::sbe::spot::order_status::OrderStatus::Canceled,
                    time_in_force: new_order_response.time_in_force,
                    order_type: new_order_response.order_type,
                    side: new_order_response.side,
                    self_trade_prevention_mode: new_order_response.self_trade_prevention_mode,
                    client_order_id: String::new(),
                    orig_client_order_id: String::new(),
                    symbol: new_order_response.symbol.clone(),
                };
                Ok(BinanceSpotWsTradingMessage::CancelReplaceAccepted {
                    request_id,
                    cancel_response,
                    new_order_response,
                })
            }
            BinanceSpotWsTradingRequestMeta::CancelAllOrders => {
                let responses = parse::decode_cancel_open_orders(&result_data)?;
                Ok(BinanceSpotWsTradingMessage::AllOrdersCanceled {
                    request_id,
                    responses,
                })
            }
            BinanceSpotWsTradingRequestMeta::SessionLogon => {
                log::info!("Session authenticated (SBE response)");
                Ok(BinanceSpotWsTradingMessage::Authenticated)
            }
            BinanceSpotWsTradingRequestMeta::SubscribeUserData => {
                log::info!("User data stream subscribed (SBE response)");
                Ok(BinanceSpotWsTradingMessage::UserDataSubscribed {
                    subscription_id: request_id,
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
                crate::spot::sbe::error::SbeDecodeError::BufferTooShort {
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
                crate::spot::sbe::error::SbeDecodeError::UnknownTemplateId(template_id),
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
        meta: BinanceSpotWsTradingRequestMeta,
    ) -> BinanceSpotWsTradingMessage {
        match meta {
            BinanceSpotWsTradingRequestMeta::PlaceOrder => {
                BinanceSpotWsTradingMessage::OrderRejected {
                    request_id,
                    code,
                    msg,
                }
            }
            BinanceSpotWsTradingRequestMeta::CancelOrder => {
                BinanceSpotWsTradingMessage::CancelRejected {
                    request_id,
                    code,
                    msg,
                }
            }
            BinanceSpotWsTradingRequestMeta::CancelReplaceOrder => {
                BinanceSpotWsTradingMessage::CancelReplaceRejected {
                    request_id,
                    code,
                    msg,
                }
            }
            BinanceSpotWsTradingRequestMeta::CancelAllOrders => {
                BinanceSpotWsTradingMessage::CancelRejected {
                    request_id,
                    code,
                    msg,
                }
            }
            BinanceSpotWsTradingRequestMeta::SessionLogon
            | BinanceSpotWsTradingRequestMeta::SubscribeUserData => {
                BinanceSpotWsTradingMessage::Error(format!("code={code}: {msg}"))
            }
        }
    }

    // Decodes the SBE error response to extract the Binance error code and message
    fn try_decode_sbe_error(data: &[u8]) -> Option<(i32, String)> {
        const HEADER_LEN: usize = 8;

        if data.len()
            < HEADER_LEN + crate::spot::sbe::spot::error_response_codec::SBE_BLOCK_LENGTH as usize
        {
            return None;
        }

        let buf = ReadBuf::new(data);
        let header = message_header_codec::MessageHeaderDecoder::default().wrap(buf, 0);
        if header.template_id() != crate::spot::sbe::spot::error_response_codec::SBE_TEMPLATE_ID {
            return None;
        }

        let mut decoder = ErrorResponseDecoder::default().header(header, 0);
        let code = i32::from(decoder.code());
        let msg_coords = decoder.msg_decoder();
        let msg_bytes = decoder.msg_slice(msg_coords);
        let msg = String::from_utf8_lossy(msg_bytes).into_owned();

        Some((code, msg))
    }
}
