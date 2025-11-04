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
// - parse_instrument_any: Convert dYdX perpetual market to InstrumentAny
// - parse_trade_tick: Convert dYdX trade to TradeTick
// - parse_candlestick: Convert dYdX candle to Bar
// - parse_account_state: Convert dYdX subaccount to AccountState
// - parse_order_status_report: Convert dYdX order to OrderStatusReport
// - parse_position_status_report: Convert dYdX position to PositionStatusReport
// - parse_fill_report: Convert dYdX fill to FillReport
