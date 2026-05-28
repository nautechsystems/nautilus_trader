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

//! WebSocket transport for the Derive JSON-RPC stream.
//!
//! The module is split into:
//!
//! - [`error`]: [`DeriveWsError`] taxonomy.
//! - [`messages`]: wire payloads and a discriminated inbound frame.
//! - [`parse`]: typed public market data payload parsers.
//! - [`handler`]: inner I/O loop owning the `WebSocketClient`.
//! - [`client`]: outer client orchestrating connect / login / subscribe.

pub mod client;
pub mod context;
pub mod dispatch;
pub mod error;
pub mod handler;
pub mod messages;
pub mod parse;

pub use client::{DeriveWebSocketClient, DeriveWebSocketSubscriptionHandle, DeriveWsCredentials};
pub(crate) use context::WsMessageContext;
pub use dispatch::{ORDER_DEDUP_CAPACITY, OrderIdentity, TRADE_DEDUP_CAPACITY, WsDispatchState};
pub use error::DeriveWsError;
pub use handler::DeriveWsMessage;
pub(crate) use messages::{
    DEFAULT_ORDERBOOK_DEPTH, DEFAULT_ORDERBOOK_GROUP, DEFAULT_TICKER_INTERVAL,
};
pub use messages::{
    DeriveOrderbookData, DeriveOrderbookLevel, DeriveOrderbookMsg, DeriveOrdersSubscriptionData,
    DerivePublicWsData, DeriveTickerData, DeriveTickerMsg, DeriveTradesMsg,
    DeriveTradesSubscriptionData, DeriveWsChannel, DeriveWsFrame, WsLoginParams, WsLoginResult,
    WsRequestParams, WsSubscribeParams, WsSubscribeResult, WsSubscriptionFrame,
    WsSubscriptionPayload, WsUnsubscribeParams, WsUnsubscribeResult, balances_channel, methods,
    orderbook_channel, orders_channel, private_trades_channel, ticker_channel, trades_channel,
};
pub use parse::{
    bar_spec_to_derive_period, parse_candle_record, parse_funding_rate,
    parse_funding_rate_history_record, parse_index_price, parse_mark_price, parse_option_greeks,
    parse_orderbook_deltas, parse_orderbook_msg, parse_public_ws_data, parse_ticker_msg,
    parse_ticker_quote, parse_ticker_quote_from_rest, parse_trade_tick, parse_trades_msg,
};
