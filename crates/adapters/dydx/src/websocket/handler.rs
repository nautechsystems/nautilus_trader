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
use nautilus_model::{identifiers::AccountId, instruments::Instrument};
use ustr::Ustr;

use super::{
    DydxWsResult,
    messages::{DydxWsMessage, NautilusWsMessage},
};

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
#[allow(dead_code)] // TODO: Remove once implementation is complete
pub struct FeedHandler {
    /// Account ID for parsing account-specific messages.
    account_id: Option<AccountId>,
    /// Cached instruments for parsing market data.
    instruments: AHashMap<Ustr, nautilus_model::instruments::InstrumentAny>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`].
    #[must_use]
    pub fn new(account_id: Option<AccountId>) -> Self {
        Self {
            account_id,
            instruments: AHashMap::new(),
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
            DydxWsMessage::ChannelData(_data) => {
                // TODO: Implement parsing for different channels
                // - v4_trades -> TradeTick
                // - v4_orderbook -> OrderBookDeltas
                // - v4_candles -> Bar
                // - v4_markets -> Instrument updates
                // - v4_subaccounts -> OrderStatusReport, FillReport, PositionStatusReport, AccountState
                tracing::warn!("Channel data parsing not yet implemented");
                Ok(None)
            }
            DydxWsMessage::ChannelBatchData(_data) => {
                // TODO: Implement batch data parsing
                tracing::warn!("Channel batch data parsing not yet implemented");
                Ok(None)
            }
            DydxWsMessage::Error(err) => Ok(Some(NautilusWsMessage::Error(err))),
            DydxWsMessage::Reconnected => Ok(Some(NautilusWsMessage::Reconnected)),
            DydxWsMessage::Pong => Ok(None),
            DydxWsMessage::Raw(_) => Ok(None),
        }
    }
}
