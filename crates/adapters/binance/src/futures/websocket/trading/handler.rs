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

//! Binance Futures WebSocket Trading API message handler.
//!
//! The handler runs in a dedicated Tokio task as the I/O boundary between the client
//! orchestrator and the network layer. It exclusively owns the `WebSocketClient` and
//! processes commands from the client via an unbounded channel.
//!
//! Unlike the Spot handler which decodes SBE binary responses, the Futures handler
//! works with JSON text responses throughout.

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
    client::BINANCE_FUTURES_WS_RATE_LIMIT_KEY_ORDER,
    error::{BinanceFuturesWsApiError, BinanceFuturesWsApiResult},
    messages::{
        BinanceFuturesWsTradingCommand, BinanceFuturesWsTradingMessage,
        BinanceFuturesWsTradingRequest, BinanceFuturesWsTradingRequestMeta,
        BinanceFuturesWsTradingResponse, method,
    },
};
use crate::{
    common::credential::SigningCredential,
    futures::http::query::{
        BinanceCancelOrderParams, BinanceModifyOrderParams, BinanceNewOrderParams,
    },
};

/// Binance Futures WebSocket Trading API handler.
///
/// Runs in a dedicated Tokio task, processing commands from the client
/// and transforming raw WebSocket JSON messages into Nautilus domain events.
pub struct BinanceFuturesWsTradingHandler {
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceFuturesWsTradingCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<BinanceFuturesWsTradingMessage>,
    credential: Arc<SigningCredential>,
    pending_requests: AHashMap<String, BinanceFuturesWsTradingRequestMeta>,
}

impl Debug for BinanceFuturesWsTradingHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceFuturesWsTradingHandler))
            .field("inner", &self.inner.as_ref().map(|_| "<client>"))
            .field(
                "pending_requests",
                &format!("{} pending", self.pending_requests.len()),
            )
            .finish_non_exhaustive()
    }
}

impl BinanceFuturesWsTradingHandler {
    /// Creates a new handler instance.
    #[must_use]
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceFuturesWsTradingCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<BinanceFuturesWsTradingMessage>,
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
        }
    }

    /// Runs the main event loop for commands and raw messages.
    ///
    /// Returns `false` when disconnected or the signal is set.
    pub async fn run(&mut self) -> bool {
        loop {
            if self.signal.load(Ordering::Relaxed) {
                return false;
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        BinanceFuturesWsTradingCommand::SetClient(client) => {
                            log::debug!("Handler received WebSocket client");
                            self.inner = Some(client);
                            self.emit(BinanceFuturesWsTradingMessage::Connected);
                        }
                        BinanceFuturesWsTradingCommand::Disconnect => {
                            log::debug!("Handler disconnecting WebSocket client");
                            self.inner = None;
                            return false;
                        }
                        BinanceFuturesWsTradingCommand::PlaceOrder { id, params } => {
                            if let Err(e) = self.handle_place_order(id.clone(), params).await {
                                log::error!("Failed to handle place order command: {e}");
                                self.emit(BinanceFuturesWsTradingMessage::OrderRejected {
                                    request_id: id,
                                    code: -1,
                                    msg: e.to_string(),
                                });
                            }
                        }
                        BinanceFuturesWsTradingCommand::CancelOrder { id, params } => {
                            if let Err(e) = self.handle_cancel_order(id.clone(), params).await {
                                log::error!("Failed to handle cancel order command: {e}");
                                self.emit(BinanceFuturesWsTradingMessage::CancelRejected {
                                    request_id: id,
                                    code: -1,
                                    msg: e.to_string(),
                                });
                            }
                        }
                        BinanceFuturesWsTradingCommand::ModifyOrder { id, params } => {
                            if let Err(e) = self.handle_modify_order(id.clone(), params).await {
                                log::error!("Failed to handle modify order command: {e}");
                                self.emit(BinanceFuturesWsTradingMessage::ModifyRejected {
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
                        self.fail_pending_requests();
                        self.emit(BinanceFuturesWsTradingMessage::Reconnected);
                        continue;
                    }

                    self.handle_message(msg);
                }
                else => {
                    return false;
                }
            }
        }
    }

    fn emit(&self, msg: BinanceFuturesWsTradingMessage) {
        if let Err(e) = self.out_tx.send(msg) {
            log::error!("Failed to send message to output channel: {e}");
        }
    }

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
        params: BinanceNewOrderParams,
    ) -> BinanceFuturesWsApiResult<()> {
        let params_json = serde_json::to_value(&params)
            .map_err(|e| BinanceFuturesWsApiError::JsonError(e.to_string()))?;
        let signed_params = self.sign_params(params_json)?;

        let request = BinanceFuturesWsTradingRequest::new(&id, method::ORDER_PLACE, signed_params);
        self.pending_requests
            .insert(id.clone(), BinanceFuturesWsTradingRequestMeta::PlaceOrder);
        self.send_request(request).await
    }

    async fn handle_cancel_order(
        &mut self,
        id: String,
        params: BinanceCancelOrderParams,
    ) -> BinanceFuturesWsApiResult<()> {
        let params_json = serde_json::to_value(&params)
            .map_err(|e| BinanceFuturesWsApiError::JsonError(e.to_string()))?;
        let signed_params = self.sign_params(params_json)?;

        let request = BinanceFuturesWsTradingRequest::new(&id, method::ORDER_CANCEL, signed_params);
        self.pending_requests
            .insert(id.clone(), BinanceFuturesWsTradingRequestMeta::CancelOrder);
        self.send_request(request).await
    }

    async fn handle_modify_order(
        &mut self,
        id: String,
        params: BinanceModifyOrderParams,
    ) -> BinanceFuturesWsApiResult<()> {
        let params_json = serde_json::to_value(&params)
            .map_err(|e| BinanceFuturesWsApiError::JsonError(e.to_string()))?;
        let signed_params = self.sign_params(params_json)?;

        let request = BinanceFuturesWsTradingRequest::new(&id, method::ORDER_MODIFY, signed_params);
        self.pending_requests
            .insert(id.clone(), BinanceFuturesWsTradingRequestMeta::ModifyOrder);
        self.send_request(request).await
    }

    fn sign_params(
        &self,
        mut params: serde_json::Value,
    ) -> BinanceFuturesWsApiResult<serde_json::Value> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| BinanceFuturesWsApiError::ClientError(e.to_string()))?
            .as_millis() as i64;

        if let Some(obj) = params.as_object_mut() {
            obj.insert("timestamp".to_string(), serde_json::json!(timestamp));
            obj.insert(
                "apiKey".to_string(),
                serde_json::json!(self.credential.api_key()),
            );
        }

        let query_string = serde_urlencoded::to_string(&params)
            .map_err(|e| BinanceFuturesWsApiError::ClientError(e.to_string()))?;
        let signature = self.credential.sign(&query_string);

        if let Some(obj) = params.as_object_mut() {
            obj.insert("signature".to_string(), serde_json::json!(signature));
        }

        Ok(params)
    }

    async fn send_request(
        &mut self,
        request: BinanceFuturesWsTradingRequest,
    ) -> BinanceFuturesWsApiResult<()> {
        let client = self.inner.as_mut().ok_or_else(|| {
            BinanceFuturesWsApiError::ConnectionError("WebSocket not connected".to_string())
        })?;

        let json = serde_json::to_string(&request)
            .map_err(|e| BinanceFuturesWsApiError::JsonError(e.to_string()))?;

        log::debug!(
            "Sending Futures WS Trading API request id={} method={}",
            request.id,
            request.method
        );

        client
            .send_text(
                json,
                Some(BINANCE_FUTURES_WS_RATE_LIMIT_KEY_ORDER.as_slice()),
            )
            .await
            .map_err(|e| {
                BinanceFuturesWsApiError::ConnectionError(format!("Failed to send request: {e}"))
            })?;

        Ok(())
    }

    fn handle_message(&mut self, msg: Message) {
        match msg {
            Message::Text(text) => self.handle_text_response(&text),
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(frame) => {
                log::debug!("WebSocket closed: {frame:?}");
            }
            Message::Binary(_) | Message::Frame(_) => {}
        }
    }

    fn handle_text_response(&mut self, text: &str) {
        let response: BinanceFuturesWsTradingResponse = match serde_json::from_str(text) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Failed to parse WS Trading API response: {e}");
                return;
            }
        };

        let Some(meta) = self.pending_requests.remove(&response.id) else {
            log::warn!("Received response for unknown request ID: {}", response.id);
            return;
        };

        if response.status != 200 {
            let (code, msg) = response.error.map(|e| (e.code, e.msg)).unwrap_or((
                -1,
                format!("Request failed with status {}", response.status),
            ));
            let rejection = self.create_rejection(response.id, code, msg, meta);
            self.emit(rejection);
            return;
        }

        let Some(result) = response.result else {
            log::warn!(
                "Missing result in success response for request {}",
                response.id
            );
            return;
        };

        match meta {
            BinanceFuturesWsTradingRequestMeta::PlaceOrder => {
                match serde_json::from_value(result) {
                    Ok(order) => {
                        self.emit(BinanceFuturesWsTradingMessage::OrderAccepted {
                            request_id: response.id,
                            response: Box::new(order),
                        });
                    }
                    Err(e) => {
                        log::error!("Failed to deserialize order response: {e}");
                        self.emit(BinanceFuturesWsTradingMessage::Error(e.to_string()));
                    }
                }
            }
            BinanceFuturesWsTradingRequestMeta::CancelOrder => match serde_json::from_value(result)
            {
                Ok(order) => {
                    self.emit(BinanceFuturesWsTradingMessage::OrderCanceled {
                        request_id: response.id,
                        response: Box::new(order),
                    });
                }
                Err(e) => {
                    log::error!("Failed to deserialize cancel response: {e}");
                    self.emit(BinanceFuturesWsTradingMessage::Error(e.to_string()));
                }
            },
            BinanceFuturesWsTradingRequestMeta::ModifyOrder => match serde_json::from_value(result)
            {
                Ok(order) => {
                    self.emit(BinanceFuturesWsTradingMessage::OrderModified {
                        request_id: response.id,
                        response: Box::new(order),
                    });
                }
                Err(e) => {
                    log::error!("Failed to deserialize modify response: {e}");
                    self.emit(BinanceFuturesWsTradingMessage::Error(e.to_string()));
                }
            },
        }
    }

    fn create_rejection(
        &self,
        request_id: String,
        code: i32,
        msg: String,
        meta: BinanceFuturesWsTradingRequestMeta,
    ) -> BinanceFuturesWsTradingMessage {
        match meta {
            BinanceFuturesWsTradingRequestMeta::PlaceOrder => {
                BinanceFuturesWsTradingMessage::OrderRejected {
                    request_id,
                    code,
                    msg,
                }
            }
            BinanceFuturesWsTradingRequestMeta::CancelOrder => {
                BinanceFuturesWsTradingMessage::CancelRejected {
                    request_id,
                    code,
                    msg,
                }
            }
            BinanceFuturesWsTradingRequestMeta::ModifyOrder => {
                BinanceFuturesWsTradingMessage::ModifyRejected {
                    request_id,
                    code,
                    msg,
                }
            }
        }
    }
}
