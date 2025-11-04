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

//! Parsing utilities for converting dYdX v4 Indexer API responses into Nautilus domain models.
//!
//! This module contains functions that transform raw JSON data structures
//! from the dYdX Indexer API into strongly-typed Nautilus data types such as
//! instruments, trades, bars, account states, etc.

// TODO: Implement parsing functions for:
// - parse_instrument_any: Convert DydxPerpetualMarket to InstrumentAny
// - parse_trade_tick: Convert DydxTrade to TradeTick
// - parse_candlestick: Convert DydxCandle to Bar
// - parse_account_state: Convert DydxSubaccount to AccountState
// - parse_order_status_report: Convert DydxOrder to OrderStatusReport
// - parse_position_status_report: Convert DydxPerpetualPosition to PositionStatusReport
// - parse_fill_report: Convert DydxFill to FillReport
