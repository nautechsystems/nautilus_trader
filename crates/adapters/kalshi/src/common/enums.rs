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

//! Kalshi-specific enums mapped to NautilusTrader core types.

use serde::{Deserialize, Serialize};

/// Status of a Kalshi market (contract).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KalshiMarketStatus {
    Initialized,
    Inactive,
    Active,
    Closed,
    Determined,
    Disputed,
    Amended,
    Finalized,
}

/// Which side the taker (aggressor) was on in a trade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KalshiTakerSide {
    Yes,
    No,
}

/// Market type (binary or scalar).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KalshiMarketType {
    Binary,
    Scalar,
}

/// Candlestick interval in minutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CandlestickInterval {
    Minutes1 = 1,
    Hours1 = 60,
    Days1 = 1440,
}

impl CandlestickInterval {
    /// Returns the interval length in minutes.
    #[must_use]
    pub fn as_minutes(self) -> u32 {
        self as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_status_deserialize() {
        let s: KalshiMarketStatus = serde_json::from_str(r#""active""#).unwrap();
        assert_eq!(s, KalshiMarketStatus::Active);
    }

    #[test]
    fn test_market_status_all_variants_deserialize() {
        let cases = [
            (r#""initialized""#, KalshiMarketStatus::Initialized),
            (r#""inactive""#, KalshiMarketStatus::Inactive),
            (r#""active""#, KalshiMarketStatus::Active),
            (r#""closed""#, KalshiMarketStatus::Closed),
            (r#""determined""#, KalshiMarketStatus::Determined),
            (r#""disputed""#, KalshiMarketStatus::Disputed),
            (r#""amended""#, KalshiMarketStatus::Amended),
            (r#""finalized""#, KalshiMarketStatus::Finalized),
        ];
        for (json, expected) in cases {
            let got: KalshiMarketStatus = serde_json::from_str(json).unwrap();
            assert_eq!(got, expected, "failed for {json}");
        }
    }

    #[test]
    fn test_taker_side_deserialize() {
        let yes: KalshiTakerSide = serde_json::from_str(r#""yes""#).unwrap();
        assert_eq!(yes, KalshiTakerSide::Yes);
        let no: KalshiTakerSide = serde_json::from_str(r#""no""#).unwrap();
        assert_eq!(no, KalshiTakerSide::No);
    }

    #[test]
    fn test_market_type_deserialize() {
        let binary: KalshiMarketType = serde_json::from_str(r#""binary""#).unwrap();
        assert_eq!(binary, KalshiMarketType::Binary);
        let scalar: KalshiMarketType = serde_json::from_str(r#""scalar""#).unwrap();
        assert_eq!(scalar, KalshiMarketType::Scalar);
    }

    #[test]
    fn test_candlestick_interval_as_minutes() {
        assert_eq!(CandlestickInterval::Minutes1.as_minutes(), 1);
        assert_eq!(CandlestickInterval::Hours1.as_minutes(), 60);
        assert_eq!(CandlestickInterval::Days1.as_minutes(), 1440);
    }
}
