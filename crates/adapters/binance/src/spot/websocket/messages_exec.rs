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

//! Binance Spot User Data Stream message types for execution handling.
//!
//! Defines command enums for communication between the execution client and
//! the execution WebSocket feed handler, and output event types for the handler.

use nautilus_model::{
    events::{AccountState, OrderCanceled, OrderFilled, OrderRejected},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
};

use super::types_exec::{BinanceSpotAccountPosition, BinanceSpotExecutionReport};

/// Deserialized User Data Stream event from the UDS WebSocket client.
///
/// These events are produced by the UDS client after parsing JSON push
/// event frames and dispatching by the `"e"` event type field.
#[derive(Debug, Clone)]
pub enum BinanceSpotUserDataEvent {
    /// Execution report (order lifecycle: NEW, TRADE, CANCELED, etc.).
    ExecutionReport(Box<BinanceSpotExecutionReport>),
    /// Account position update (balance changes).
    AccountPosition(BinanceSpotAccountPosition),
    /// WebSocket reconnected — subscriptions restored.
    Reconnected,
}

/// Command from the execution client to the exec feed handler.
///
/// Used to register order context before HTTP submission, so that
/// incoming WebSocket events can be correlated with the correct
/// strategy and instrument context.
#[derive(Debug)]
pub enum SpotExecHandlerCommand {
    /// Register an order for context tracking before HTTP submission.
    RegisterOrder {
        /// Client-assigned order identifier.
        client_order_id: ClientOrderId,
        /// Trader who owns this order.
        trader_id: TraderId,
        /// Strategy that submitted this order.
        strategy_id: StrategyId,
        /// Instrument being traded.
        instrument_id: InstrumentId,
    },
    /// Register a cancel request for context tracking.
    RegisterCancel {
        /// Client order ID of the order being canceled.
        client_order_id: ClientOrderId,
        /// Trader who owns this order.
        trader_id: TraderId,
        /// Strategy that submitted this order.
        strategy_id: StrategyId,
        /// Instrument being traded.
        instrument_id: InstrumentId,
        /// Venue-assigned order ID (if known).
        venue_order_id: Option<VenueOrderId>,
    },
    /// Cache instrument precision for fill event construction.
    CacheInstrument {
        /// Instrument symbol as used by Binance (e.g., `"ETHUSDC"`).
        symbol: String,
        /// Price precision (decimal places for Price type).
        price_precision: u8,
        /// Quantity precision (decimal places for Quantity type).
        qty_precision: u8,
    },
}

/// Normalized execution event from the Binance Spot exec feed handler.
///
/// These events are produced by the handler after correlating raw
/// WebSocket events with order context and constructing proper
/// Nautilus domain types with correct precision.
#[derive(Debug, Clone)]
pub enum NautilusSpotExecWsMessage {
    /// Order filled (partial or full).
    OrderFilled(OrderFilled),
    /// Order canceled (by user, exchange, or expiry).
    OrderCanceled(OrderCanceled),
    /// Order rejected after initial acceptance.
    OrderRejected(OrderRejected),
    /// Account state update (balance changes).
    AccountUpdate(AccountState),
    /// WebSocket reconnected — pending requests drained.
    Reconnected,
}
