// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use serde::{Deserialize, Serialize};

use crate::tardis::enums::{Exchange, InstrumentType, OptionType};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
/// The response format for a Tardis HTTP API request.
pub enum Response<T> {
    /// The error response.
    Error {
        /// The error code.
        code: u64,
        /// The error message.
        message: String,
    },
    /// The success response.
    Success(T),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
/// The changes info returned by the exchanges API.
pub struct InstrumentChanges {
    /// Date in ISO format.
    pub until: String,
    /// The minimum price increment (tick size).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub price_increment: Option<f64>,
    /// The minimum size increment.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub amount_increment: Option<f64>,
    /// The instrument contract multiplier (only for derivatives).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub contract_multiplier: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
/// The metadata of a particular instrument.
/// See <https://docs.tardis.dev/api/instruments-metadata-api>.
pub struct InstrumentInfo {
    /// The instrument symbol.
    pub id: String,
    /// The instrument exchange.
    pub exchange: Exchange,
    /// The instrument base currency (normalized, e.g., BTC for `BitMEX`, not XBT).
    pub base_currency: String,
    /// The instrument quote currency (normalized, e.g., BTC for `BitMEX`, not XBT).
    pub quote_currency: String,
    /// The instrument type e.g., spot, perpetual, future, option.
    #[serde(rename = "type")]
    pub instrument_type: InstrumentType,
    /// If the instrument is actively listed.
    pub active: bool,
    /// The available from date in ISO format.
    pub available_since: String,
    /// The available to date in ISO format.
    pub available_to: Option<String>,
    /// The contract expiry date in ISO format (applicable to futures and options).
    pub expiry: Option<String>,
    /// The instrument price increment.
    pub price_increment: f64,
    /// The instrument size increment.
    pub amount_increment: f64,
    /// The minimum tradeable size for the instrument.
    pub min_trade_amount: f64,
    /// The instrument maker fee: consider it as illustrative only, as it depends in practice on account traded volume levels, different categories, VIP levels, owning exchange currency etc.
    pub maker_fee: f64,
    /// The instrument taker fee: consider it as illustrative only, as it depends in practice on account traded volume levels, different categories, VIP levels, owning exchange currency etc.
    pub taker_fee: f64,
    /// If the instrument is inverse (only for derivatives such as futures and perpetual swaps).
    pub inverse: Option<bool>,
    /// The instrument contract multiplier (only for derivatives).
    pub contract_multiplier: Option<f64>,
    /// If the instrument is quanto (only for quanto instruments).
    pub quanto: Option<bool>,
    /// The instrument settlement currency (only for Quanto instruments where settlement currency is different both base and quote currency).
    pub settlement_currency: Option<String>,
    /// The instrument strike price (only for options).
    pub strike_price: Option<f64>,
    /// The option type (only for options).
    pub option_type: Option<OptionType>,
    /// The changes for the instrument (best-effort basis from Tardis).
    pub changes: Option<Vec<InstrumentChanges>>,
}
