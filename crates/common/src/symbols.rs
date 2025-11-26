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

//! Common symbol parsing utilities for exchange adapters.

/// Extracts the raw symbol by removing the product type suffix.
///
/// Removes the last hyphen-delimited segment from a symbol string.
/// This is commonly used to convert exchange-specific instrument symbols
/// to their base ticker format.
#[must_use]
pub fn extract_raw_symbol(symbol: &str) -> &str {
    symbol.rsplit_once('-').map_or(symbol, |(prefix, _)| prefix)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("BTC-USD-PERP", "BTC-USD")]
    #[case("ETH-USD-PERP", "ETH-USD")]
    #[case("BTCUSDT-LINEAR", "BTCUSDT")]
    #[case("BTCUSDT-SPOT", "BTCUSDT")]
    #[case("BTCUSD-INVERSE", "BTCUSD")]
    #[case("ETH-OPTION", "ETH")]
    #[case("BTC-USD", "BTC")]
    #[case("SOL-PERP", "SOL")]
    #[case("AVAX", "AVAX")]
    fn test_extract_raw_symbol(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(extract_raw_symbol(input), expected);
    }
}
