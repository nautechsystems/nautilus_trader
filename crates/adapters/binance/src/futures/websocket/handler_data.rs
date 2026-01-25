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

//! Binance Futures WebSocket handler for JSON market data streams.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_core::time::AtomicTime;
use nautilus_model::{
    data::Data,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    websocket::{SubscriptionState, WebSocketClient},
};
use ustr::Ustr;

use super::{
    messages::{
        BinanceFuturesAccountConfigMsg, BinanceFuturesAccountUpdateMsg, BinanceFuturesAggTradeMsg,
        BinanceFuturesBookTickerMsg, BinanceFuturesDepthUpdateMsg, BinanceFuturesExecWsMessage,
        BinanceFuturesKlineMsg, BinanceFuturesListenKeyExpiredMsg, BinanceFuturesMarginCallMsg,
        BinanceFuturesMarkPriceMsg, BinanceFuturesOrderUpdateMsg, BinanceFuturesTradeMsg,
        BinanceFuturesWsErrorMsg, BinanceFuturesWsErrorResponse, BinanceFuturesWsSubscribeRequest,
        BinanceFuturesWsSubscribeResponse, DataHandlerCommand, NautilusDataWsMessage,
        NautilusWsMessage,
    },
    parse::{
        extract_event_type, extract_symbol, parse_agg_trade, parse_book_ticker, parse_depth_update,
        parse_kline, parse_mark_price, parse_trade,
    },
};
use crate::common::{
    consts::BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION,
    enums::{BinanceWsEventType, BinanceWsMethod},
};

/// Handler for Binance Futures WebSocket JSON streams.
pub struct BinanceFuturesDataWsFeedHandler {
    clock: &'static AtomicTime,
    #[allow(dead_code)] // Reserved for shutdown signal handling
    signal: Arc<AtomicBool>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DataHandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    #[allow(dead_code)] // Reserved for async message emission
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    client: Option<WebSocketClient>,
    instruments: AHashMap<Ustr, InstrumentAny>,
    subscriptions_state: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    pending_requests: AHashMap<u64, Vec<String>>,
}

impl Debug for BinanceFuturesDataWsFeedHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceFuturesWsFeedHandler))
            .field("instruments_count", &self.instruments.len())
            .field("pending_requests", &self.pending_requests.len())
            .finish_non_exhaustive()
    }
}

impl BinanceFuturesDataWsFeedHandler {
    /// Creates a new handler instance.
    pub fn new(
        clock: &'static AtomicTime,
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DataHandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        subscriptions_state: SubscriptionState,
        request_id_counter: Arc<AtomicU64>,
    ) -> Self {
        Self {
            clock,
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            client: None,
            instruments: AHashMap::new(),
            subscriptions_state,
            request_id_counter,
            pending_requests: AHashMap::new(),
        }
    }

    /// Returns the next message from the handler.
    ///
    /// Processes both commands and raw WebSocket messages.
    pub async fn next(&mut self) -> Option<NautilusWsMessage> {
        loop {
            if self.signal.load(Ordering::Relaxed) {
                return None;
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await;
                }
                Some(raw) = self.raw_rx.recv() => {
                    if let Some(msg) = self.handle_raw_message(raw).await {
                        return Some(msg);
                    }
                }
                else => {
                    return None;
                }
            }
        }
    }

    async fn handle_command(&mut self, cmd: DataHandlerCommand) {
        match cmd {
            DataHandlerCommand::SetClient(client) => {
                self.client = Some(client);
            }
            DataHandlerCommand::Disconnect => {
                if let Some(client) = &self.client {
                    let () = client.disconnect().await;
                }
                self.client = None;
            }
            DataHandlerCommand::InitializeInstruments(instruments) => {
                for inst in instruments {
                    self.instruments.insert(inst.raw_symbol().inner(), inst);
                }
            }
            DataHandlerCommand::UpdateInstrument(instrument) => {
                self.instruments
                    .insert(instrument.raw_symbol().inner(), instrument);
            }
            DataHandlerCommand::Subscribe { streams } => {
                self.send_subscribe(streams).await;
            }
            DataHandlerCommand::Unsubscribe { streams } => {
                self.send_unsubscribe(streams).await;
            }
        }
    }

    async fn send_subscribe(&mut self, streams: Vec<String>) {
        let Some(client) = &self.client else {
            log::warn!("Cannot subscribe: no client connected");
            return;
        };

        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);

        // Track pending request
        self.pending_requests.insert(request_id, streams.clone());

        // Mark streams as pending subscribe
        for stream in &streams {
            self.subscriptions_state.mark_subscribe(stream);
        }

        let request = BinanceFuturesWsSubscribeRequest {
            method: BinanceWsMethod::Subscribe,
            params: streams,
            id: request_id,
        };

        let json = match serde_json::to_string(&request) {
            Ok(j) => j,
            Err(e) => {
                log::error!("Failed to serialize subscribe request: {e}");
                return;
            }
        };

        if let Err(e) = client
            .send_text(json, Some(BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
            .await
        {
            log::error!("Failed to send subscribe request: {e}");
        }
    }

    async fn send_unsubscribe(&mut self, streams: Vec<String>) {
        let Some(client) = &self.client else {
            log::warn!("Cannot unsubscribe: no client connected");
            return;
        };

        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);

        let request = BinanceFuturesWsSubscribeRequest {
            method: BinanceWsMethod::Unsubscribe,
            params: streams.clone(),
            id: request_id,
        };

        let json = match serde_json::to_string(&request) {
            Ok(j) => j,
            Err(e) => {
                log::error!("Failed to serialize unsubscribe request: {e}");
                return;
            }
        };

        if let Err(e) = client
            .send_text(json, Some(BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
            .await
        {
            log::error!("Failed to send unsubscribe request: {e}");
        }

        // Mark as unsubscribed
        for stream in &streams {
            self.subscriptions_state.confirm_unsubscribe(stream);
        }
    }

    async fn handle_raw_message(&mut self, raw: Vec<u8>) -> Option<NautilusWsMessage> {
        // Check for reconnection signal
        if let Ok(text) = std::str::from_utf8(&raw)
            && text == RECONNECTED
        {
            log::info!("WebSocket reconnected signal received");
            return Some(NautilusWsMessage::Reconnected);
        }

        // Parse JSON
        let json: serde_json::Value = match serde_json::from_slice(&raw) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Failed to parse JSON message: {e}");
                return None;
            }
        };

        // Check for subscription response
        if json.get("result").is_some() || json.get("id").is_some() {
            self.handle_subscription_response(&json);
            return None;
        }

        // Check for error response
        if let Some(code) = json.get("code")
            && let Some(code) = code.as_i64()
        {
            let msg = json
                .get("msg")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            return Some(NautilusWsMessage::Error(BinanceFuturesWsErrorMsg {
                code,
                msg,
            }));
        }

        // Handle stream data
        self.handle_stream_data(&json)
    }

    fn handle_subscription_response(&mut self, json: &serde_json::Value) {
        if let Ok(response) =
            serde_json::from_value::<BinanceFuturesWsSubscribeResponse>(json.clone())
        {
            if let Some(streams) = self.pending_requests.remove(&response.id) {
                if response.result.is_none() {
                    // Success - confirm subscriptions
                    for stream in &streams {
                        self.subscriptions_state.confirm_subscribe(stream);
                    }
                    log::debug!("Subscription confirmed: streams={streams:?}");
                } else {
                    // Failure - mark streams as failed
                    for stream in &streams {
                        self.subscriptions_state.mark_failure(stream);
                    }
                    log::warn!(
                        "Subscription failed: streams={streams:?}, result={:?}",
                        response.result
                    );
                }
            }
        } else if let Ok(error) =
            serde_json::from_value::<BinanceFuturesWsErrorResponse>(json.clone())
        {
            if let Some(id) = error.id
                && let Some(streams) = self.pending_requests.remove(&id)
            {
                for stream in &streams {
                    self.subscriptions_state.mark_failure(stream);
                }
            }
            log::warn!(
                "WebSocket error response: code={}, msg={}",
                error.code,
                error.msg
            );
        }
    }

    fn handle_stream_data(&self, json: &serde_json::Value) -> Option<NautilusWsMessage> {
        let ts_init = self.clock.get_time_ns();
        let event_type = extract_event_type(json)?;

        // Handle user data stream events first (they don't follow market data pattern)
        if let Some(msg) = self.handle_user_data_event(&event_type, json) {
            return Some(NautilusWsMessage::ExecRaw(msg));
        }

        // Skip user data events that weren't parsed (they use raw symbols, not Nautilus format)
        if matches!(
            event_type,
            BinanceWsEventType::AccountUpdate
                | BinanceWsEventType::OrderTradeUpdate
                | BinanceWsEventType::MarginCall
                | BinanceWsEventType::AccountConfigUpdate
                | BinanceWsEventType::ListenKeyExpired
                | BinanceWsEventType::Unknown
        ) {
            return None;
        }

        // Market data events require symbol and instrument lookup
        let symbol = extract_symbol(json)?;
        let Some(instrument) = self.instruments.get(&symbol) else {
            log::warn!(
                "No instrument in cache, dropping message: symbol={symbol}, event_type={event_type:?}"
            );
            return None;
        };

        match event_type {
            BinanceWsEventType::AggTrade => {
                if let Ok(msg) = serde_json::from_value::<BinanceFuturesAggTradeMsg>(json.clone()) {
                    match parse_agg_trade(&msg, instrument, ts_init) {
                        Ok(trade) => {
                            return Some(NautilusWsMessage::Data(NautilusDataWsMessage::Data(
                                vec![Data::Trade(trade)],
                            )));
                        }
                        Err(e) => {
                            log::warn!("Failed to parse aggregate trade: {e}");
                        }
                    }
                }
            }
            BinanceWsEventType::Trade => {
                if let Ok(msg) = serde_json::from_value::<BinanceFuturesTradeMsg>(json.clone()) {
                    match parse_trade(&msg, instrument, ts_init) {
                        Ok(trade) => {
                            return Some(NautilusWsMessage::Data(NautilusDataWsMessage::Data(
                                vec![Data::Trade(trade)],
                            )));
                        }
                        Err(e) => {
                            log::warn!("Failed to parse trade: {e}");
                        }
                    }
                }
            }
            BinanceWsEventType::BookTicker => {
                if let Ok(msg) = serde_json::from_value::<BinanceFuturesBookTickerMsg>(json.clone())
                {
                    match parse_book_ticker(&msg, instrument, ts_init) {
                        Ok(quote) => {
                            return Some(NautilusWsMessage::Data(NautilusDataWsMessage::Data(
                                vec![Data::Quote(quote)],
                            )));
                        }
                        Err(e) => {
                            log::warn!("Failed to parse book ticker: {e}");
                        }
                    }
                }
            }
            BinanceWsEventType::DepthUpdate => {
                if let Ok(msg) =
                    serde_json::from_value::<BinanceFuturesDepthUpdateMsg>(json.clone())
                {
                    match parse_depth_update(&msg, instrument, ts_init) {
                        Ok(deltas) => {
                            return Some(NautilusWsMessage::Data(
                                NautilusDataWsMessage::DepthUpdate {
                                    deltas,
                                    first_update_id: msg.first_update_id,
                                    prev_final_update_id: msg.prev_final_update_id,
                                },
                            ));
                        }
                        Err(e) => {
                            log::warn!("Failed to parse depth update: {e}");
                        }
                    }
                }
            }
            BinanceWsEventType::MarkPriceUpdate => {
                if let Ok(msg) = serde_json::from_value::<BinanceFuturesMarkPriceMsg>(json.clone())
                {
                    match parse_mark_price(&msg, instrument, ts_init) {
                        Ok((mark_update, index_update, _funding_update)) => {
                            // Note: FundingRateUpdate is not a variant of Data enum
                            // Funding rates need custom data handling (like Python adapter)
                            return Some(NautilusWsMessage::Data(NautilusDataWsMessage::Data(
                                vec![
                                    Data::MarkPriceUpdate(mark_update),
                                    Data::IndexPriceUpdate(index_update),
                                ],
                            )));
                        }
                        Err(e) => {
                            log::warn!("Failed to parse mark price: {e}");
                        }
                    }
                }
            }
            BinanceWsEventType::Kline => {
                if let Ok(msg) = serde_json::from_value::<BinanceFuturesKlineMsg>(json.clone()) {
                    match parse_kline(&msg, instrument, ts_init) {
                        Ok(Some(bar)) => {
                            return Some(NautilusWsMessage::Data(NautilusDataWsMessage::Data(
                                vec![Data::Bar(bar)],
                            )));
                        }
                        Ok(None) => {
                            // Kline not closed yet, skip
                        }
                        Err(e) => {
                            log::warn!("Failed to parse kline: {e}");
                        }
                    }
                }
            }
            BinanceWsEventType::ForceOrder
            | BinanceWsEventType::Ticker24Hr
            | BinanceWsEventType::MiniTicker24Hr => {
                // Pass through as raw JSON for now
                return Some(NautilusWsMessage::Data(NautilusDataWsMessage::RawJson(
                    json.clone(),
                )));
            }

            // User data events and Unknown handled before instrument lookup
            BinanceWsEventType::AccountUpdate
            | BinanceWsEventType::OrderTradeUpdate
            | BinanceWsEventType::MarginCall
            | BinanceWsEventType::AccountConfigUpdate
            | BinanceWsEventType::ListenKeyExpired
            | BinanceWsEventType::Unknown => unreachable!(),
        }

        None
    }

    fn handle_user_data_event(
        &self,
        event_type: &BinanceWsEventType,
        json: &serde_json::Value,
    ) -> Option<BinanceFuturesExecWsMessage> {
        match event_type {
            BinanceWsEventType::AccountUpdate => {
                match serde_json::from_value::<BinanceFuturesAccountUpdateMsg>(json.clone()) {
                    Ok(msg) => {
                        log::debug!(
                            "Account update: reason={:?}, balances={}, positions={}",
                            msg.account.reason,
                            msg.account.balances.len(),
                            msg.account.positions.len()
                        );
                        Some(BinanceFuturesExecWsMessage::AccountUpdate(msg))
                    }
                    Err(e) => {
                        log::warn!("Failed to parse account update: {e}");
                        None
                    }
                }
            }
            BinanceWsEventType::OrderTradeUpdate => {
                match serde_json::from_value::<BinanceFuturesOrderUpdateMsg>(json.clone()) {
                    Ok(msg) => {
                        log::debug!(
                            "Order update: symbol={}, order_id={}, exec={:?}, status={:?}",
                            msg.order.symbol,
                            msg.order.order_id,
                            msg.order.execution_type,
                            msg.order.order_status
                        );
                        Some(BinanceFuturesExecWsMessage::OrderUpdate(Box::new(msg)))
                    }
                    Err(e) => {
                        log::warn!("Failed to parse order update: {e}");
                        None
                    }
                }
            }
            BinanceWsEventType::MarginCall => {
                match serde_json::from_value::<BinanceFuturesMarginCallMsg>(json.clone()) {
                    Ok(msg) => {
                        log::warn!(
                            "Margin call: cross_wallet_balance={}, positions_at_risk={}",
                            msg.cross_wallet_balance,
                            msg.positions.len()
                        );
                        Some(BinanceFuturesExecWsMessage::MarginCall(msg))
                    }
                    Err(e) => {
                        log::warn!("Failed to parse margin call: {e}");
                        None
                    }
                }
            }
            BinanceWsEventType::AccountConfigUpdate => {
                match serde_json::from_value::<BinanceFuturesAccountConfigMsg>(json.clone()) {
                    Ok(msg) => {
                        if let Some(ref lc) = msg.leverage_config {
                            log::debug!(
                                "Account config update: symbol={}, leverage={}",
                                lc.symbol,
                                lc.leverage
                            );
                        }
                        Some(BinanceFuturesExecWsMessage::AccountConfigUpdate(msg))
                    }
                    Err(e) => {
                        log::warn!("Failed to parse account config update: {e}");
                        None
                    }
                }
            }
            BinanceWsEventType::ListenKeyExpired => {
                match serde_json::from_value::<BinanceFuturesListenKeyExpiredMsg>(json.clone()) {
                    Ok(msg) => {
                        log::warn!("Listen key expired at {}", msg.event_time);
                        Some(BinanceFuturesExecWsMessage::ListenKeyExpired)
                    }
                    Err(e) => {
                        log::warn!("Failed to parse listen key expired: {e}");
                        None
                    }
                }
            }
            _ => None,
        }
    }
}
