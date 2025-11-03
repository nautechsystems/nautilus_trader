// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! WebSocket message handler for Bybit.

use std::{num::NonZero, sync::Arc};

use ahash::AHashMap;
use nautilus_core::{nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{BarSpecification, BarType, Data},
    enums::{AggregationSource, BarAggregation, PriceType},
    identifiers::AccountId,
    instruments::{Instrument, InstrumentAny},
};
use tokio::sync::RwLock;
use ustr::Ustr;

use super::{
    cache,
    messages::{BybitWebSocketError, BybitWebSocketMessage, NautilusWsMessage},
    parse::{
        parse_kline_topic, parse_millis_i64, parse_orderbook_deltas, parse_ticker_linear_funding,
        parse_ws_account_state, parse_ws_fill_report, parse_ws_kline_bar,
        parse_ws_order_status_report, parse_ws_position_status_report, parse_ws_trade_tick,
    },
};
use crate::common::{enums::BybitProductType, parse::make_bybit_symbol};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum HandlerCommand {
    /// Initialize the instruments cache with the given instruments.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(InstrumentAny),
}

/// Type alias for the funding rate cache.
type FundingCache = Arc<RwLock<AHashMap<Ustr, (Option<String>, Option<String>)>>>;

pub(super) struct FeedHandler {
    message_rx: tokio::sync::mpsc::UnboundedReceiver<BybitWebSocketMessage>,
    pub tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    account_id: Option<AccountId>,
    product_type: Option<BybitProductType>,
    quote_cache: Arc<RwLock<cache::QuoteCache>>,
    funding_cache: FundingCache,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    pub(super) fn new(
        message_rx: tokio::sync::mpsc::UnboundedReceiver<BybitWebSocketMessage>,
        tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        account_id: Option<AccountId>,
        product_type: Option<BybitProductType>,
        quote_cache: Arc<RwLock<cache::QuoteCache>>,
        funding_cache: FundingCache,
    ) -> Self {
        Self {
            message_rx,
            tx,
            cmd_rx,
            instruments_cache: AHashMap::new(),
            account_id,
            product_type,
            quote_cache,
            funding_cache,
        }
    }

    pub(super) async fn next(&mut self) -> Option<()> {
        let clock = get_atomic_clock_realtime();

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                self.instruments_cache.insert(inst.symbol().inner(), inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.symbol().inner(), inst);
                        }
                    }
                    continue;
                }

                Some(msg) = self.message_rx.recv() => {
                    let ts_init = clock.get_time_ns();
                    let nautilus_messages = Self::parse_to_nautilus_messages(
                        msg,
                        &self.instruments_cache,
                        self.account_id,
                        self.product_type,
                        &self.quote_cache,
                        &self.funding_cache,
                        ts_init,
                    )
                    .await;

                    for nautilus_msg in nautilus_messages {
                        if self.tx.send(nautilus_msg).is_err() {
                            tracing::debug!("Receiver dropped, stopping handler");
                            return None;
                        }
                    }
                }

                else => {
                    tracing::debug!("Handler shutting down: stream ended or command channel closed");
                    return None;
                }
            }
        }
    }

    async fn parse_to_nautilus_messages(
        msg: BybitWebSocketMessage,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        account_id: Option<AccountId>,
        product_type: Option<BybitProductType>,
        quote_cache: &Arc<RwLock<cache::QuoteCache>>,
        funding_cache: &FundingCache,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let mut result = Vec::new();

        match msg {
            BybitWebSocketMessage::Orderbook(msg) => {
                let raw_symbol = msg.data.s;
                let symbol =
                    product_type.map_or(raw_symbol, |pt| make_bybit_symbol(raw_symbol, pt));

                if let Some(instrument) = instruments.get(&symbol) {
                    match parse_orderbook_deltas(&msg, instrument, ts_init) {
                        Ok(deltas) => result.push(NautilusWsMessage::Deltas(deltas)),
                        Err(e) => tracing::error!("Error parsing orderbook deltas: {e}"),
                    }
                } else {
                    tracing::warn!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in Orderbook message");
                }
            }
            BybitWebSocketMessage::Trade(msg) => {
                let mut data_vec = Vec::new();
                for trade in &msg.data {
                    let raw_symbol = trade.s;
                    let symbol =
                        product_type.map_or(raw_symbol, |pt| make_bybit_symbol(raw_symbol, pt));

                    if let Some(instrument) = instruments.get(&symbol) {
                        match parse_ws_trade_tick(trade, instrument, ts_init) {
                            Ok(tick) => data_vec.push(Data::Trade(tick)),
                            Err(e) => tracing::error!("Error parsing trade tick: {e}"),
                        }
                    } else {
                        tracing::warn!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in Trade message");
                    }
                }

                if !data_vec.is_empty() {
                    result.push(NautilusWsMessage::Data(data_vec));
                }
            }
            BybitWebSocketMessage::Kline(msg) => {
                let (interval_str, raw_symbol) = match parse_kline_topic(&msg.topic) {
                    Ok(parts) => parts,
                    Err(e) => {
                        tracing::warn!("Failed to parse kline topic: {e}");
                        return result;
                    }
                };

                let symbol = product_type
                    .map_or_else(|| raw_symbol.into(), |pt| make_bybit_symbol(raw_symbol, pt));

                if let Some(instrument) = instruments.get(&symbol) {
                    let (step, aggregation) = match interval_str.parse::<usize>() {
                        Ok(minutes) if minutes > 0 => (minutes, BarAggregation::Minute),
                        _ => {
                            tracing::warn!("Unsupported kline interval: {}", interval_str);
                            return result;
                        }
                    };

                    if let Some(non_zero_step) = NonZero::new(step) {
                        let bar_spec = BarSpecification {
                            step: non_zero_step,
                            aggregation,
                            price_type: PriceType::Last,
                        };
                        let bar_type =
                            BarType::new(instrument.id(), bar_spec, AggregationSource::External);

                        let mut data_vec = Vec::new();
                        for kline in &msg.data {
                            // Only process confirmed bars (not partial/building bars)
                            if !kline.confirm {
                                continue;
                            }
                            match parse_ws_kline_bar(kline, instrument, bar_type, false, ts_init) {
                                Ok(bar) => data_vec.push(Data::Bar(bar)),
                                Err(e) => tracing::error!("Error parsing kline to bar: {e}"),
                            }
                        }
                        if !data_vec.is_empty() {
                            result.push(NautilusWsMessage::Data(data_vec));
                        }
                    } else {
                        tracing::error!("Invalid step value: {}", step);
                    }
                } else {
                    tracing::warn!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in Kline message");
                }
            }
            BybitWebSocketMessage::TickerLinear(msg) => {
                let raw_symbol = msg.data.symbol;
                let symbol =
                    product_type.map_or(raw_symbol, |pt| make_bybit_symbol(raw_symbol, pt));

                if let Some(instrument) = instruments.get(&symbol) {
                    let instrument_id = instrument.id();
                    let ts_event = parse_millis_i64(msg.ts, "ticker.ts").unwrap_or(ts_init);

                    match quote_cache.write().await.process_linear_ticker(
                        &msg.data,
                        instrument_id,
                        instrument,
                        ts_event,
                        ts_init,
                    ) {
                        Ok(quote) => result.push(NautilusWsMessage::Data(vec![Data::Quote(quote)])),
                        Err(e) => {
                            let raw_data = serde_json::to_string(&msg.data)
                                .unwrap_or_else(|_| "<failed to serialize>".to_string());
                            tracing::debug!(
                                "Skipping partial ticker update: {e}, raw_data: {raw_data}"
                            );
                        }
                    }

                    // Extract funding rate if available
                    if msg.data.funding_rate.is_some() && msg.data.next_funding_time.is_some() {
                        let cache_key = (
                            msg.data.funding_rate.clone(),
                            msg.data.next_funding_time.clone(),
                        );

                        let should_publish = {
                            let cache = funding_cache.read().await;
                            cache.get(&symbol) != Some(&cache_key)
                        };

                        if should_publish {
                            match parse_ticker_linear_funding(
                                &msg.data,
                                instrument_id,
                                ts_event,
                                ts_init,
                            ) {
                                Ok(funding) => {
                                    funding_cache.write().await.insert(symbol, cache_key);
                                    result.push(NautilusWsMessage::FundingRates(vec![funding]));
                                }
                                Err(e) => {
                                    tracing::debug!("Skipping funding rate update: {e}");
                                }
                            }
                        }
                    }
                } else {
                    tracing::warn!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in TickerLinear message");
                }
            }
            BybitWebSocketMessage::TickerOption(msg) => {
                let raw_symbol = &msg.data.symbol;
                let symbol = product_type.map_or_else(
                    || raw_symbol.as_str().into(),
                    |pt| make_bybit_symbol(raw_symbol, pt),
                );

                if let Some(instrument) = instruments.get(&symbol) {
                    let instrument_id = instrument.id();
                    let ts_event = parse_millis_i64(msg.ts, "ticker.ts").unwrap_or(ts_init);

                    match quote_cache.write().await.process_option_ticker(
                        &msg.data,
                        instrument_id,
                        instrument,
                        ts_event,
                        ts_init,
                    ) {
                        Ok(quote) => result.push(NautilusWsMessage::Data(vec![Data::Quote(quote)])),
                        Err(e) => {
                            let raw_data = serde_json::to_string(&msg.data)
                                .unwrap_or_else(|_| "<failed to serialize>".to_string());
                            tracing::debug!(
                                "Skipping partial ticker update: {e}, raw_data: {raw_data}"
                            );
                        }
                    }
                } else {
                    tracing::warn!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in TickerOption message");
                }
            }
            BybitWebSocketMessage::AccountOrder(msg) => {
                if let Some(account_id) = account_id {
                    let mut reports = Vec::new();
                    for order in &msg.data {
                        let raw_symbol = order.symbol;
                        let symbol = make_bybit_symbol(raw_symbol, order.category);

                        if let Some(instrument) = instruments.get(&symbol) {
                            match parse_ws_order_status_report(
                                order, instrument, account_id, ts_init,
                            ) {
                                Ok(report) => reports.push(report),
                                Err(e) => tracing::error!("Error parsing order status report: {e}"),
                            }
                        } else {
                            tracing::warn!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in AccountOrder message");
                        }
                    }
                    if !reports.is_empty() {
                        result.push(NautilusWsMessage::OrderStatusReports(reports));
                    }
                }
            }
            BybitWebSocketMessage::AccountExecution(msg) => {
                if let Some(account_id) = account_id {
                    let mut reports = Vec::new();
                    for execution in &msg.data {
                        let raw_symbol = execution.symbol;
                        let symbol = make_bybit_symbol(raw_symbol, execution.category);

                        if let Some(instrument) = instruments.get(&symbol) {
                            match parse_ws_fill_report(execution, account_id, instrument, ts_init) {
                                Ok(report) => reports.push(report),
                                Err(e) => tracing::error!("Error parsing fill report: {e}"),
                            }
                        } else {
                            tracing::warn!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in AccountExecution message");
                        }
                    }
                    if !reports.is_empty() {
                        result.push(NautilusWsMessage::FillReports(reports));
                    }
                }
            }
            BybitWebSocketMessage::AccountPosition(msg) => {
                if let Some(account_id) = account_id {
                    for position in &msg.data {
                        let raw_symbol = position.symbol;
                        let symbol = make_bybit_symbol(raw_symbol, position.category);

                        if let Some(instrument) = instruments.get(&symbol) {
                            match parse_ws_position_status_report(
                                position, account_id, instrument, ts_init,
                            ) {
                                Ok(report) => {
                                    result.push(NautilusWsMessage::PositionStatusReport(report));
                                }
                                Err(e) => {
                                    tracing::error!("Error parsing position status report: {e}");
                                }
                            }
                        } else {
                            tracing::warn!(raw_symbol = %raw_symbol, full_symbol = %symbol, "No instrument found for symbol in AccountPosition message");
                        }
                    }
                }
            }
            BybitWebSocketMessage::AccountWallet(msg) => {
                if let Some(account_id) = account_id {
                    for wallet in &msg.data {
                        let ts_event = UnixNanos::from(msg.creation_time as u64 * 1_000_000);

                        match parse_ws_account_state(wallet, account_id, ts_event, ts_init) {
                            Ok(state) => result.push(NautilusWsMessage::AccountState(state)),
                            Err(e) => tracing::error!("Error parsing account state: {e}"),
                        }
                    }
                }
            }
            BybitWebSocketMessage::OrderResponse(resp) => {
                if resp.ret_code == 0 {
                    tracing::debug!(op = %resp.op, ret_msg = %resp.ret_msg, "Order operation successful");
                } else {
                    let operation_type = if resp.op.contains("create") {
                        "order submission"
                    } else if resp.op.contains("cancel") {
                        "order cancellation"
                    } else if resp.op.contains("amend") {
                        "order modification"
                    } else {
                        "order operation"
                    };

                    tracing::warn!(
                        op = %resp.op,
                        ret_code = resp.ret_code,
                        ret_msg = %resp.ret_msg,
                        "Order operation failed: {} rejected", operation_type
                    );

                    let error_msg = format!(
                        "Bybit {} failed: {} (code: {})",
                        operation_type, resp.ret_msg, resp.ret_code
                    );
                    let error = BybitWebSocketError::new(resp.ret_code, error_msg);
                    result.push(NautilusWsMessage::Error(error));
                }
            }
            BybitWebSocketMessage::Error(err) => {
                result.push(NautilusWsMessage::Error(err));
            }
            BybitWebSocketMessage::Reconnected => {
                result.push(NautilusWsMessage::Reconnected);
            }
            _ => {} // Ignore other message types (pong, auth, subscription confirmations, etc.)
        }

        result
    }
}
