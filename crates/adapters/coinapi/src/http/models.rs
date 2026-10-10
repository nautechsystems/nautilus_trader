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

//! Raw CoinAPI responses, decoded directly from JSON bytes to preserve decimal tokens.

use jiff::Timestamp;
use nautilus_core::serialization::decimal;
use rust_decimal::Decimal;
use serde::Deserialize;
use ustr::Ustr;

/// Exchange identity and metadata.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Exchange {
    pub exchange_id: Ustr,
    pub name: String,
    pub website: Option<String>,
    pub data_start: Option<String>,
    pub data_end: Option<String>,
    pub data_symbols_count: Option<u64>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub volume_1hrs_usd: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub volume_1day_usd: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub volume_1mth_usd: Option<Decimal>,
}

/// Provider symbol identity and data resolution, without trading-increment inference.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Symbol {
    pub symbol_id: Ustr,
    pub exchange_id: Ustr,
    pub symbol_type: Ustr,
    pub asset_id_base: Option<Ustr>,
    pub asset_id_quote: Option<Ustr>,
    pub asset_id_unit: Option<Ustr>,
    pub symbol_id_exchange: Option<Ustr>,
    pub asset_id_base_exchange: Option<Ustr>,
    pub asset_id_quote_exchange: Option<Ustr>,
    pub future_contract_unit_asset: Option<Ustr>,
    pub data_start: Option<String>,
    pub data_end: Option<String>,
    pub future_delivery_time: Option<Timestamp>,
    pub data_quote_start: Option<Timestamp>,
    pub data_quote_end: Option<Timestamp>,
    pub data_trade_start: Option<Timestamp>,
    pub data_trade_end: Option<Timestamp>,
    pub data_orderbook_start: Option<Timestamp>,
    pub data_orderbook_end: Option<Timestamp>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub future_contract_unit: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub price_precision: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub size_precision: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub price: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub volume_1hrs: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub volume_1hrs_usd: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub volume_1day: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub volume_1day_usd: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub volume_1mth: Option<Decimal>,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub volume_1mth_usd: Option<Decimal>,
    pub symbol_id_int: Option<u64>,
    pub raw_kvp: Option<SymbolRawKvp>,
}

/// Venue metadata that CoinAPI forwards as string key-value pairs, limited to product
/// classification keys.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolRawKvp {
    pub contract_type: Option<Ustr>,
    pub underlying_type: Option<Ustr>,
}

/// A current provider order-book snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Book {
    pub symbol_id: Ustr,
    pub time_exchange: Timestamp,
    pub time_coinapi: Timestamp,
    pub asks: Vec<BookLevel>,
    pub bids: Vec<BookLevel>,
}

/// A book entry, retaining an order identifier when supplied.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BookLevel {
    pub id: Option<String>,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub size: Decimal,
}

/// A historical best bid and ask update.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Quote {
    pub symbol_id: Option<Ustr>,
    pub time_exchange: Timestamp,
    pub time_coinapi: Timestamp,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub ask_price: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub ask_size: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub bid_price: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub bid_size: Decimal,
}

/// An executed trade in provider wire terms.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Trade {
    pub symbol_id: Option<Ustr>,
    pub time_exchange: Timestamp,
    pub time_coinapi: Timestamp,
    pub uuid: String,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub size: Decimal,
    pub taker_side: Ustr,
    pub id_trade: Option<String>,
}

/// A provider OHLCV period.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ohlcv {
    pub time_period_start: Timestamp,
    pub time_period_end: Timestamp,
    pub time_open: Timestamp,
    pub time_close: Timestamp,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price_open: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price_high: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price_low: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price_close: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub volume_traded: Decimal,
    pub trades_count: u64,
}
