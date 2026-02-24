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

//! HTTP REST model types for the Polymarket CLOB API.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::{
        PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOrderStatus, PolymarketOrderType,
        PolymarketOutcome, PolymarketTradeStatus, SignatureType,
    },
    models::PolymarketMakerOrder,
    parse::{deserialize_decimal_from_str, serialize_decimal_as_str},
};

/// A signed limit order for submission to the CLOB exchange.
///
/// References: <https://docs.polymarket.com/#create-and-place-an-order>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolymarketOrder {
    pub salt: u64,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    pub token_id: Ustr,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub maker_amount: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub taker_amount: Decimal,
    pub expiration: String,
    pub nonce: String,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub fee_rate_bps: Decimal,
    pub side: PolymarketOrderSide,
    pub signature_type: SignatureType,
    pub signature: String,
}

/// An active order returned by REST GET /orders.
///
/// References: <https://docs.polymarket.com/#get-orders>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketOpenOrder {
    pub associate_trades: Option<Vec<String>>,
    pub id: String,
    pub status: PolymarketOrderStatus,
    pub market: Ustr,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub original_size: Decimal,
    pub outcome: PolymarketOutcome,
    pub maker_address: Ustr,
    pub owner: Ustr,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub price: Decimal,
    pub side: PolymarketOrderSide,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub size_matched: Decimal,
    pub asset_id: Ustr,
    pub expiration: Option<String>,
    pub order_type: PolymarketOrderType,
    pub created_at: u64,
}

/// A trade report returned by REST GET /trades.
///
/// References: <https://docs.polymarket.com/#get-trades>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketTradeReport {
    pub id: String,
    pub taker_order_id: String,
    pub market: Ustr,
    pub asset_id: Ustr,
    pub side: PolymarketOrderSide,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub size: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub fee_rate_bps: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub price: Decimal,
    pub status: PolymarketTradeStatus,
    pub match_time: String,
    pub last_update: String,
    pub outcome: PolymarketOutcome,
    pub bucket_index: u64,
    pub owner: Ustr,
    pub maker_address: Ustr,
    pub transaction_hash: String,
    pub maker_orders: Vec<PolymarketMakerOrder>,
    pub trader_side: PolymarketLiquiditySide,
}
