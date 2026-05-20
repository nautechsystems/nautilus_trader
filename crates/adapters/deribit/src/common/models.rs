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

//! Models shared between Deribit HTTP and WebSocket layers.

use nautilus_core::serialization::{deserialize_decimal, deserialize_optional_decimal};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A single leg entry of a Deribit combo trade.
///
/// Appears in the `legs` array of a combo's public trade message
/// (`/public/get_last_trades_by_currency` with `kind=option_combo|future_combo`,
/// and the `trades.{combo}.{interval}` subscription channel). Each leg is also
/// published independently on the leg instrument's own `trades.{instrument}.*`
/// stream, carrying `combo_id` and `combo_trade_id` to link back to the parent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitTradeLeg {
    /// Trade timestamp in milliseconds.
    pub timestamp: i64,
    /// Trade price.
    #[serde(deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    /// Trade amount.
    #[serde(deserialize_with = "deserialize_decimal")]
    pub amount: Decimal,
    /// Trade direction: `buy` or `sell`.
    pub direction: String,
    /// Underlying index price at trade time (may be empty on older historical trades,
    /// matching the optionality on the parent [`crate::http::models::DeribitPublicTrade`]).
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub index_price: Option<Decimal>,
    /// Leg instrument name (e.g., `BTC-PERPETUAL`).
    pub instrument_name: String,
    /// Per-instrument trade sequence number.
    pub trade_seq: i64,
    /// Mark price at trade time (may be empty on older historical trades,
    /// matching the optionality on the parent [`crate::http::models::DeribitPublicTrade`]).
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub mark_price: Option<Decimal>,
    /// Tick direction: 0 = Plus, 1 = Zero-Plus, 2 = Minus, 3 = Zero-Minus.
    pub tick_direction: i32,
    /// The combo instrument that produced this leg.
    pub combo_id: String,
    /// Trade size in contract units (may be absent on historical combo trades,
    /// matching the optionality on the parent [`crate::http::models::DeribitPublicTrade`]).
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub contracts: Option<Decimal>,
    /// Unique (per currency) trade identifier for the leg.
    pub trade_id: String,
    /// Trade identifier of the parent combo trade.
    pub combo_trade_id: String,
    /// Implied volatility (option legs only).
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub iv: Option<Decimal>,
}
