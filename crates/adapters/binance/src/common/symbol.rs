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

//! Binance symbol conversion utilities.

use nautilus_model::identifiers::InstrumentId;
use ustr::Ustr;

use super::{consts::BINANCE_VENUE, enums::BinanceProductType};

/// Converts a Binance symbol to a Nautilus instrument ID.
///
/// For USD-M futures, appends "-PERP" suffix to match Nautilus symbology.
/// For COIN-M futures, keeps the symbol as-is (uses "_PERP" format).
///
/// # Examples
///
/// - ("BTCUSDT", UsdM) → "BTCUSDT-PERP.BINANCE"
/// - ("ETHUSD_PERP", CoinM) → "ETHUSD_PERP.BINANCE"
#[must_use]
pub fn format_instrument_id(symbol: &Ustr, product_type: BinanceProductType) -> InstrumentId {
    let nautilus_symbol = match product_type {
        BinanceProductType::UsdM => {
            // USD-M symbols don't have _PERP suffix from Binance, we add -PERP
            format!("{symbol}-PERP")
        }
        BinanceProductType::CoinM => {
            // COIN-M symbols already have _PERP suffix from Binance
            symbol.to_string()
        }
        _ => symbol.to_string(),
    };
    InstrumentId::new(nautilus_symbol.into(), *BINANCE_VENUE)
}

/// Converts a Nautilus instrument ID to a Binance-compatible symbol.
///
/// This function strips common suffixes like "-PERP" that Nautilus uses for
/// internal symbology but Binance doesn't recognize.
///
/// # Examples
///
/// - "BTCUSDT-PERP" → "BTCUSDT"
/// - "ETHUSD_PERP" → "ETHUSD_PERP" (COIN-M format, kept as-is)
/// - "BTCUSDT" → "BTCUSDT"
#[must_use]
pub fn format_binance_symbol(instrument_id: &InstrumentId) -> String {
    let symbol = instrument_id.symbol.as_str();

    if symbol.ends_with("-PERP") {
        symbol.trim_end_matches("-PERP").to_string()
    } else {
        symbol.to_string()
    }
}

/// Converts a Nautilus instrument ID to a lowercase Binance WebSocket stream symbol.
///
/// This is used for constructing WebSocket stream names which require lowercase symbols.
#[must_use]
pub fn format_binance_stream_symbol(instrument_id: &InstrumentId) -> String {
    format_binance_symbol(instrument_id).to_lowercase()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("BTCUSDT-PERP.BINANCE", "BTCUSDT")]
    #[case("ETHUSDT-PERP.BINANCE", "ETHUSDT")]
    #[case("BTCUSD_PERP.BINANCE", "BTCUSD_PERP")]
    #[case("BTCUSDT.BINANCE", "BTCUSDT")]
    #[case("ETHBTC.BINANCE", "ETHBTC")]
    fn test_format_binance_symbol(#[case] input: &str, #[case] expected: &str) {
        let instrument_id = InstrumentId::from(input);
        assert_eq!(format_binance_symbol(&instrument_id), expected);
    }

    #[rstest]
    #[case("BTCUSDT-PERP.BINANCE", "btcusdt")]
    #[case("ETHUSDT-PERP.BINANCE", "ethusdt")]
    #[case("BTCUSD_PERP.BINANCE", "btcusd_perp")]
    fn test_format_binance_stream_symbol(#[case] input: &str, #[case] expected: &str) {
        let instrument_id = InstrumentId::from(input);
        assert_eq!(format_binance_stream_symbol(&instrument_id), expected);
    }
}
