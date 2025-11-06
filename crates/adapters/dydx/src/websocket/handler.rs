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

//! Message handler for dYdX WebSocket streams.
//!
//! This module processes incoming WebSocket messages and converts them into
//! Nautilus domain objects.

use ahash::AHashMap;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, Data, OrderBookDelta, OrderBookDeltas, TradeTick,
        bar::get_bar_interval_ns,
    },
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::{AccountId, InstrumentId, TradeId},
    instruments::Instrument,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use std::str::FromStr;
use ustr::Ustr;

use super::{
    DydxWsError, DydxWsResult,
    enums::DydxWsChannel,
    messages::{DydxWsChannelBatchDataMsg, DydxWsChannelDataMsg, DydxWsMessage, NautilusWsMessage},
    types::{
        DydxCandle, DydxMarketsContents, DydxOrderbookContents, DydxOrderbookSnapshotContents,
        DydxTradeContents,
    },
};
use crate::common::enums::DydxOrderSide;

/// Commands sent to the feed handler.
#[derive(Debug, Clone)]
pub enum HandlerCommand {
    /// Update a single instrument in the cache.
    UpdateInstrument(Box<nautilus_model::instruments::InstrumentAny>),
    /// Initialize instruments in bulk.
    InitializeInstruments(Vec<nautilus_model::instruments::InstrumentAny>),
}

/// Processes incoming WebSocket messages and converts them to Nautilus domain objects.
#[derive(Debug)]
pub struct FeedHandler {
    /// Account ID for parsing account-specific messages.
    #[allow(dead_code)] // TODO: Will be used for subaccount message parsing
    account_id: Option<AccountId>,
    /// Cached instruments for parsing market data.
    instruments: AHashMap<Ustr, nautilus_model::instruments::InstrumentAny>,
    /// Cached bar types by topic (e.g., "BTC-USD/1MIN").
    bar_types: AHashMap<String, BarType>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`].
    #[must_use]
    pub fn new(account_id: Option<AccountId>) -> Self {
        Self {
            account_id,
            instruments: AHashMap::new(),
            bar_types: AHashMap::new(),
        }
    }

    /// Handles a command to update the internal state.
    pub fn handle_command(&mut self, command: HandlerCommand) {
        match command {
            HandlerCommand::UpdateInstrument(instrument) => {
                let symbol = instrument.id().symbol.inner();
                self.instruments.insert(symbol, *instrument);
            }
            HandlerCommand::InitializeInstruments(instruments) => {
                for instrument in instruments {
                    let symbol = instrument.id().symbol.inner();
                    self.instruments.insert(symbol, instrument);
                }
            }
        }
    }

    /// Registers a bar type for a specific topic (e.g., "BTC-USD/1MIN").
    pub fn register_bar_type(&mut self, topic: String, bar_type: BarType) {
        self.bar_types.insert(topic, bar_type);
    }

    /// Unregisters a bar type for a specific topic.
    pub fn unregister_bar_type(&mut self, topic: &str) {
        self.bar_types.remove(topic);
    }

    /// Processes a WebSocket message and converts it to Nautilus domain objects.
    ///
    /// # Errors
    ///
    /// Returns an error if message parsing fails.
    #[allow(clippy::result_large_err)]
    pub fn handle_message(&self, msg: DydxWsMessage) -> DydxWsResult<Option<NautilusWsMessage>> {
        match msg {
            DydxWsMessage::Connected(_) => {
                tracing::info!("dYdX WebSocket connected");
                Ok(None)
            }
            DydxWsMessage::Subscribed(sub) => {
                tracing::debug!("Subscribed to {} (id: {:?})", sub.channel, sub.id);
                Ok(None)
            }
            DydxWsMessage::Unsubscribed(unsub) => {
                tracing::debug!("Unsubscribed from {} (id: {:?})", unsub.channel, unsub.id);
                Ok(None)
            }
            DydxWsMessage::ChannelData(data) => self.handle_channel_data(data),
            DydxWsMessage::ChannelBatchData(data) => self.handle_channel_batch_data(data),
            DydxWsMessage::Error(err) => Ok(Some(NautilusWsMessage::Error(err))),
            DydxWsMessage::Reconnected => Ok(Some(NautilusWsMessage::Reconnected)),
            DydxWsMessage::Pong => Ok(None),
            DydxWsMessage::Raw(_) => Ok(None),
        }
    }

    fn handle_channel_data(
        &self,
        data: DydxWsChannelDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        match data.channel {
            DydxWsChannel::Trades => self.parse_trades(&data),
            DydxWsChannel::Orderbook => self.parse_orderbook(&data, false),
            DydxWsChannel::Candles => self.parse_candles(&data),
            DydxWsChannel::Markets => self.parse_markets(&data),
            DydxWsChannel::Subaccounts => {
                // TODO: Parse subaccount updates (orders, fills, positions)
                tracing::debug!("Subaccount channel_data not yet implemented");
                Ok(None)
            }
            DydxWsChannel::BlockHeight => {
                tracing::debug!("Block height update received");
                Ok(None)
            }
        }
    }

    fn handle_channel_batch_data(
        &self,
        data: DydxWsChannelBatchDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        match data.channel {
            DydxWsChannel::Orderbook => self.parse_orderbook_batch(&data),
            _ => {
                tracing::warn!("Unexpected batch data for channel: {:?}", data.channel);
                Ok(None)
            }
        }
    }

    fn parse_trades(&self, data: &DydxWsChannelDataMsg) -> DydxWsResult<Option<NautilusWsMessage>> {
        let symbol = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for trades channel".into()))?;

        let instrument_id = self.parse_instrument_id(symbol)?;
        let instrument = self.get_instrument(&instrument_id)?;

        let contents: DydxTradeContents = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse trade contents: {e}")))?;

        let mut ticks = Vec::new();
        let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

        for trade in contents.trades {
            let aggressor_side = match trade.side {
                DydxOrderSide::Buy => AggressorSide::Buyer,
                DydxOrderSide::Sell => AggressorSide::Seller,
            };

            let price = Decimal::from_str(&trade.price)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse trade price: {e}")))?;

            let size = Decimal::from_str(&trade.size)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse trade size: {e}")))?;

            let tick = TradeTick::new(
                instrument_id,
                Price::new(price.try_into().unwrap(), instrument.price_precision()),
                Quantity::new(size.try_into().unwrap(), instrument.size_precision()),
                aggressor_side,
                TradeId::new(&trade.id),
                UnixNanos::from(trade.created_at.timestamp_nanos_opt().unwrap_or(0) as u64),
                ts_init,
            );
            ticks.push(Data::Trade(tick));
        }

        if ticks.is_empty() {
            Ok(None)
        } else {
            Ok(Some(NautilusWsMessage::Data(ticks)))
        }
    }

    fn parse_orderbook(
        &self,
        data: &DydxWsChannelDataMsg,
        is_snapshot: bool,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        let symbol = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for orderbook channel".into()))?;

        let instrument_id = self.parse_instrument_id(symbol)?;
        let instrument = self.get_instrument(&instrument_id)?;

        let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

        if is_snapshot {
            let contents: DydxOrderbookSnapshotContents =
                serde_json::from_value(data.contents.clone()).map_err(|e| {
                    DydxWsError::Parse(format!("Failed to parse orderbook snapshot: {e}"))
                })?;

            let deltas = self.parse_orderbook_snapshot(
                &instrument_id,
                &contents,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )?;

            Ok(Some(NautilusWsMessage::Deltas(Box::new(deltas))))
        } else {
            let contents: DydxOrderbookContents = serde_json::from_value(data.contents.clone())
                .map_err(|e| {
                    DydxWsError::Parse(format!("Failed to parse orderbook contents: {e}"))
                })?;

            let deltas = self.parse_orderbook_deltas(
                &instrument_id,
                &contents,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )?;

            Ok(Some(NautilusWsMessage::Deltas(Box::new(deltas))))
        }
    }

    fn parse_orderbook_batch(
        &self,
        data: &DydxWsChannelBatchDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        let symbol = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for orderbook batch channel".into()))?;

        let instrument_id = self.parse_instrument_id(symbol)?;
        let instrument = self.get_instrument(&instrument_id)?;

        let contents: Vec<DydxOrderbookContents> = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse orderbook batch: {e}")))?;

        let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
        let mut all_deltas = Vec::new();

        let num_messages = contents.len();
        for (idx, content) in contents.iter().enumerate() {
            let is_last_message = idx == num_messages - 1;
            let deltas = self.parse_orderbook_deltas_with_flag(
                &instrument_id,
                content,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
                is_last_message,
            )?;
            all_deltas.extend(deltas);
        }

        let deltas = OrderBookDeltas::new(instrument_id, all_deltas);
        Ok(Some(NautilusWsMessage::Deltas(Box::new(deltas))))
    }

    fn parse_orderbook_snapshot(
        &self,
        instrument_id: &InstrumentId,
        contents: &DydxOrderbookSnapshotContents,
        price_precision: u8,
        size_precision: u8,
        ts_init: UnixNanos,
    ) -> DydxWsResult<OrderBookDeltas> {
        let mut deltas = Vec::new();

        // Add clear delta first
        deltas.push(OrderBookDelta::clear(*instrument_id, 0, ts_init, ts_init));

        let bids = contents.bids.as_deref().unwrap_or(&[]);
        let asks = contents.asks.as_deref().unwrap_or(&[]);

        let bids_len = bids.len();
        let asks_len = asks.len();

        for (idx, bid) in bids.iter().enumerate() {
            let is_last = idx == bids_len - 1 && asks_len == 0;
            let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

            let price = Decimal::from_str(&bid.price)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid price: {e}")))?;

            let size = Decimal::from_str(&bid.size)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid size: {e}")))?;

            let order = BookOrder::new(
                OrderSide::Buy,
                Price::new(price.try_into().unwrap(), price_precision),
                Quantity::new(size.try_into().unwrap(), size_precision),
                0,
            );

            deltas.push(OrderBookDelta::new(
                *instrument_id,
                BookAction::Add,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        for (idx, ask) in asks.iter().enumerate() {
            let is_last = idx == asks_len - 1;
            let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

            let price = Decimal::from_str(&ask.price)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask price: {e}")))?;

            let size = Decimal::from_str(&ask.size)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask size: {e}")))?;

            let order = BookOrder::new(
                OrderSide::Sell,
                Price::new(price.try_into().unwrap(), price_precision),
                Quantity::new(size.try_into().unwrap(), size_precision),
                0,
            );

            deltas.push(OrderBookDelta::new(
                *instrument_id,
                BookAction::Add,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        Ok(OrderBookDeltas::new(*instrument_id, deltas))
    }

    fn parse_orderbook_deltas(
        &self,
        instrument_id: &InstrumentId,
        contents: &DydxOrderbookContents,
        price_precision: u8,
        size_precision: u8,
        ts_init: UnixNanos,
    ) -> DydxWsResult<OrderBookDeltas> {
        let deltas = self.parse_orderbook_deltas_with_flag(
            instrument_id,
            contents,
            price_precision,
            size_precision,
            ts_init,
            true, // Mark as last message by default
        )?;
        Ok(OrderBookDeltas::new(*instrument_id, deltas))
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_orderbook_deltas_with_flag(
        &self,
        instrument_id: &InstrumentId,
        contents: &DydxOrderbookContents,
        price_precision: u8,
        size_precision: u8,
        ts_init: UnixNanos,
        is_last_message: bool,
    ) -> DydxWsResult<Vec<OrderBookDelta>> {
        let mut deltas = Vec::new();

        let bids = contents.bids.as_deref().unwrap_or(&[]);
        let asks = contents.asks.as_deref().unwrap_or(&[]);

        let bids_len = bids.len();
        let asks_len = asks.len();

        for (idx, (price_str, size_str)) in bids.iter().enumerate() {
            let is_last = is_last_message && idx == bids_len - 1 && asks_len == 0;
            let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

            let price = Decimal::from_str(price_str)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid price: {e}")))?;

            let size = Decimal::from_str(size_str)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse bid size: {e}")))?;

            let qty = Quantity::new(size.try_into().unwrap(), size_precision);
            let action = if qty.is_zero() {
                BookAction::Delete
            } else {
                BookAction::Update
            };

            let order = BookOrder::new(
                OrderSide::Buy,
                Price::new(price.try_into().unwrap(), price_precision),
                qty,
                0,
            );

            deltas.push(OrderBookDelta::new(
                *instrument_id,
                action,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        for (idx, (price_str, size_str)) in asks.iter().enumerate() {
            let is_last = is_last_message && idx == asks_len - 1;
            let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

            let price = Decimal::from_str(price_str)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask price: {e}")))?;

            let size = Decimal::from_str(size_str)
                .map_err(|e| DydxWsError::Parse(format!("Failed to parse ask size: {e}")))?;

            let qty = Quantity::new(size.try_into().unwrap(), size_precision);
            let action = if qty.is_zero() {
                BookAction::Delete
            } else {
                BookAction::Update
            };

            let order = BookOrder::new(
                OrderSide::Sell,
                Price::new(price.try_into().unwrap(), price_precision),
                qty,
                0,
            );

            deltas.push(OrderBookDelta::new(
                *instrument_id,
                action,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        Ok(deltas)
    }

    fn parse_candles(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        let topic = data
            .id
            .as_ref()
            .ok_or_else(|| DydxWsError::Parse("Missing id for candles channel".into()))?;

        let bar_type = self.bar_types.get(topic).ok_or_else(|| {
            DydxWsError::Parse(format!("No bar type registered for topic: {topic}"))
        })?;

        let candle: DydxCandle = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse candle contents: {e}")))?;

        let instrument_id = self.parse_instrument_id(&candle.ticker)?;
        let instrument = self.get_instrument(&instrument_id)?;

        let open = Decimal::from_str(&candle.open)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse open: {e}")))?;
        let high = Decimal::from_str(&candle.high)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse high: {e}")))?;
        let low = Decimal::from_str(&candle.low)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse low: {e}")))?;
        let close = Decimal::from_str(&candle.close)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse close: {e}")))?;
        let volume = Decimal::from_str(&candle.base_token_volume)
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse volume: {e}")))?;

        let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

        // Calculate ts_event: startedAt + interval
        let started_at_nanos = candle.started_at.timestamp_nanos_opt().unwrap_or(0) as u64;
        let interval_nanos = get_bar_interval_ns(bar_type);
        let ts_event = UnixNanos::from(started_at_nanos) + interval_nanos;

        let bar = Bar::new(
            *bar_type,
            Price::new(open.try_into().unwrap(), instrument.price_precision()),
            Price::new(high.try_into().unwrap(), instrument.price_precision()),
            Price::new(low.try_into().unwrap(), instrument.price_precision()),
            Price::new(close.try_into().unwrap(), instrument.price_precision()),
            Quantity::new(volume.try_into().unwrap(), instrument.size_precision()),
            ts_event,
            ts_init,
        );

        Ok(Some(NautilusWsMessage::Data(vec![Data::Bar(bar)])))
    }

    fn parse_markets(
        &self,
        data: &DydxWsChannelDataMsg,
    ) -> DydxWsResult<Option<NautilusWsMessage>> {
        let contents: DydxMarketsContents = serde_json::from_value(data.contents.clone())
            .map_err(|e| DydxWsError::Parse(format!("Failed to parse markets contents: {e}")))?;

        // Markets channel is primarily for oracle price updates
        // Python implementation publishes custom DYDXOraclePrice data type
        // For now, we just log the update
        if let Some(oracle_prices) = contents.oracle_prices {
            tracing::debug!(
                "Received oracle price updates for {} markets",
                oracle_prices.len()
            );
            // TODO: Implement custom oracle price data type if needed
        }

        Ok(None)
    }

    fn parse_instrument_id(&self, symbol: &str) -> DydxWsResult<InstrumentId> {
        // dYdX WS uses raw symbols (e.g., "BTC-USD")
        // Need to append "-PERP" to match Nautilus instrument IDs
        let symbol_with_perp = format!("{symbol}-PERP");
        Ok(crate::common::parse::parse_instrument_id(&symbol_with_perp))
    }

    fn get_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> DydxWsResult<&nautilus_model::instruments::InstrumentAny> {
        self.instruments
            .get(&instrument_id.symbol.inner())
            .ok_or_else(|| DydxWsError::Parse(format!("No instrument cached for {instrument_id}")))
    }
}
