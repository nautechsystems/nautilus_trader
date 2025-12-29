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

//! Binance Spot HTTP response models.
//!
//! These models represent Binance venue-specific response types decoded from SBE.

/// Price/quantity level in an order book.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BinancePriceLevel {
    /// Price mantissa (multiply by 10^exponent to get actual price).
    pub price_mantissa: i64,
    /// Quantity mantissa (multiply by 10^exponent to get actual quantity).
    pub qty_mantissa: i64,
}

impl BinancePriceLevel {
    /// Converts the price mantissa to f64 using the given exponent.
    #[must_use]
    pub fn price_f64(&self, exponent: i8) -> f64 {
        self.price_mantissa as f64 * 10_f64.powi(exponent as i32)
    }

    /// Converts the quantity mantissa to f64 using the given exponent.
    #[must_use]
    pub fn qty_f64(&self, exponent: i8) -> f64 {
        self.qty_mantissa as f64 * 10_f64.powi(exponent as i32)
    }
}

/// Binance order book depth response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceDepth {
    /// Last update ID for this depth snapshot.
    pub last_update_id: i64,
    /// Price exponent for all price levels.
    pub price_exponent: i8,
    /// Quantity exponent for all quantity values.
    pub qty_exponent: i8,
    /// Bid price levels (best bid first).
    pub bids: Vec<BinancePriceLevel>,
    /// Ask price levels (best ask first).
    pub asks: Vec<BinancePriceLevel>,
}

/// A single trade from Binance.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceTrade {
    /// Trade ID.
    pub id: i64,
    /// Price mantissa.
    pub price_mantissa: i64,
    /// Quantity mantissa.
    pub qty_mantissa: i64,
    /// Quote quantity mantissa (price * qty).
    pub quote_qty_mantissa: i64,
    /// Trade timestamp in milliseconds.
    pub time: i64,
    /// Whether the buyer is the maker.
    pub is_buyer_maker: bool,
    /// Whether this trade is the best price match.
    pub is_best_match: bool,
}

impl BinanceTrade {
    /// Converts the price mantissa to f64 using the given exponent.
    #[must_use]
    pub fn price_f64(&self, exponent: i8) -> f64 {
        self.price_mantissa as f64 * 10_f64.powi(exponent as i32)
    }

    /// Converts the quantity mantissa to f64 using the given exponent.
    #[must_use]
    pub fn qty_f64(&self, exponent: i8) -> f64 {
        self.qty_mantissa as f64 * 10_f64.powi(exponent as i32)
    }
}

/// Binance trades response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceTrades {
    /// Price exponent for all trades.
    pub price_exponent: i8,
    /// Quantity exponent for all trades.
    pub qty_exponent: i8,
    /// List of trades.
    pub trades: Vec<BinanceTrade>,
}
