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

//! CoinAPI wire classifications shared by REST, WebSocket, and flat-file ingestion.

use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// An OHLCV period from the provider catalog.
///
/// The provider can add periods to its
/// [catalog](https://www.coinapi.io/products/market-data-api/docs/rest-api/ohlcv/ohlcv/periods/get);
/// unrecognized identifiers retain their wire value.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    strum::AsRefStr,
    strum::Display,
    strum::EnumString,
    strum::VariantNames,
)]
#[serde(from = "Ustr", into = "Ustr")]
pub enum OhlcvPeriod {
    #[strum(serialize = "1SEC")]
    Second1,
    #[strum(serialize = "2SEC")]
    Second2,
    #[strum(serialize = "3SEC")]
    Second3,
    #[strum(serialize = "4SEC")]
    Second4,
    #[strum(serialize = "5SEC")]
    Second5,
    #[strum(serialize = "6SEC")]
    Second6,
    #[strum(serialize = "10SEC")]
    Second10,
    #[strum(serialize = "15SEC")]
    Second15,
    #[strum(serialize = "20SEC")]
    Second20,
    #[strum(serialize = "30SEC")]
    Second30,
    #[strum(serialize = "1MIN")]
    Minute1,
    #[strum(serialize = "2MIN")]
    Minute2,
    #[strum(serialize = "3MIN")]
    Minute3,
    #[strum(serialize = "4MIN")]
    Minute4,
    #[strum(serialize = "5MIN")]
    Minute5,
    #[strum(serialize = "6MIN")]
    Minute6,
    #[strum(serialize = "10MIN")]
    Minute10,
    #[strum(serialize = "15MIN")]
    Minute15,
    #[strum(serialize = "20MIN")]
    Minute20,
    #[strum(serialize = "30MIN")]
    Minute30,
    #[strum(serialize = "1HRS")]
    Hour1,
    #[strum(serialize = "2HRS")]
    Hour2,
    #[strum(serialize = "3HRS")]
    Hour3,
    #[strum(serialize = "4HRS")]
    Hour4,
    #[strum(serialize = "6HRS")]
    Hour6,
    #[strum(serialize = "8HRS")]
    Hour8,
    #[strum(serialize = "12HRS")]
    Hour12,
    #[strum(serialize = "1DAY")]
    Day1,
    #[strum(serialize = "2DAY")]
    Day2,
    #[strum(serialize = "3DAY")]
    Day3,
    #[strum(serialize = "5DAY")]
    Day5,
    #[strum(serialize = "7DAY")]
    Day7,
    #[strum(serialize = "10DAY")]
    Day10,
    #[strum(serialize = "1MTH")]
    Month1,
    #[strum(serialize = "2MTH")]
    Month2,
    #[strum(serialize = "3MTH")]
    Month3,
    #[strum(serialize = "4MTH")]
    Month4,
    #[strum(serialize = "6MTH")]
    Month6,
    #[strum(serialize = "1YRS")]
    Year1,
    #[strum(serialize = "2YRS")]
    Year2,
    #[strum(serialize = "3YRS")]
    Year3,
    #[strum(serialize = "4YRS")]
    Year4,
    #[strum(serialize = "5YRS")]
    Year5,
    /// An unrecognized value, retained without changing its spelling.
    #[strum(default, transparent)]
    Other(Ustr),
}

impl From<Ustr> for OhlcvPeriod {
    fn from(value: Ustr) -> Self {
        value.as_str().into()
    }
}

impl From<OhlcvPeriod> for Ustr {
    fn from(value: OhlcvPeriod) -> Self {
        Self::from(value.as_ref())
    }
}

/// The provider-reported or estimated aggressor side of a trade.
///
/// Unrecognized values remain distinct from the documented `UNKNOWN` value.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    strum::AsRefStr,
    strum::Display,
    strum::EnumString,
)]
#[serde(from = "Ustr", into = "Ustr")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum TakerSide {
    Buy,
    Sell,
    BuyEstimated,
    SellEstimated,
    Unknown,
    /// An unrecognized value, retained without changing its spelling.
    #[strum(default, transparent)]
    Other(Ustr),
}

impl From<Ustr> for TakerSide {
    fn from(value: Ustr) -> Self {
        value.as_str().into()
    }
}

impl From<TakerSide> for Ustr {
    fn from(value: TakerSide) -> Self {
        Self::from(value.as_ref())
    }
}

/// The provider classification of a symbol.
///
/// Recognition does not imply support for a native instrument mapping.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    strum::AsRefStr,
    strum::Display,
    strum::EnumString,
)]
#[serde(from = "Ustr", into = "Ustr")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum SymbolType {
    Spot,
    Futures,
    Option,
    Perpetual,
    DeployerPerpetual,
    Index,
    Credit,
    Contract,
    OptionCombo,
    FutureCombo,
    /// An unrecognized value, retained without changing its spelling.
    #[strum(default, transparent)]
    Other(Ustr),
}

impl From<Ustr> for SymbolType {
    fn from(value: Ustr) -> Self {
        value.as_str().into()
    }
}

impl From<SymbolType> for Ustr {
    fn from(value: SymbolType) -> Self {
        Self::from(value.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("1SEC", OhlcvPeriod::Second1)]
    #[case("2SEC", OhlcvPeriod::Second2)]
    #[case("3SEC", OhlcvPeriod::Second3)]
    #[case("4SEC", OhlcvPeriod::Second4)]
    #[case("5SEC", OhlcvPeriod::Second5)]
    #[case("6SEC", OhlcvPeriod::Second6)]
    #[case("10SEC", OhlcvPeriod::Second10)]
    #[case("15SEC", OhlcvPeriod::Second15)]
    #[case("20SEC", OhlcvPeriod::Second20)]
    #[case("30SEC", OhlcvPeriod::Second30)]
    #[case("1MIN", OhlcvPeriod::Minute1)]
    #[case("2MIN", OhlcvPeriod::Minute2)]
    #[case("3MIN", OhlcvPeriod::Minute3)]
    #[case("4MIN", OhlcvPeriod::Minute4)]
    #[case("5MIN", OhlcvPeriod::Minute5)]
    #[case("6MIN", OhlcvPeriod::Minute6)]
    #[case("10MIN", OhlcvPeriod::Minute10)]
    #[case("15MIN", OhlcvPeriod::Minute15)]
    #[case("20MIN", OhlcvPeriod::Minute20)]
    #[case("30MIN", OhlcvPeriod::Minute30)]
    #[case("1HRS", OhlcvPeriod::Hour1)]
    #[case("2HRS", OhlcvPeriod::Hour2)]
    #[case("3HRS", OhlcvPeriod::Hour3)]
    #[case("4HRS", OhlcvPeriod::Hour4)]
    #[case("6HRS", OhlcvPeriod::Hour6)]
    #[case("8HRS", OhlcvPeriod::Hour8)]
    #[case("12HRS", OhlcvPeriod::Hour12)]
    #[case("1DAY", OhlcvPeriod::Day1)]
    #[case("2DAY", OhlcvPeriod::Day2)]
    #[case("3DAY", OhlcvPeriod::Day3)]
    #[case("5DAY", OhlcvPeriod::Day5)]
    #[case("7DAY", OhlcvPeriod::Day7)]
    #[case("10DAY", OhlcvPeriod::Day10)]
    #[case("1MTH", OhlcvPeriod::Month1)]
    #[case("2MTH", OhlcvPeriod::Month2)]
    #[case("3MTH", OhlcvPeriod::Month3)]
    #[case("4MTH", OhlcvPeriod::Month4)]
    #[case("6MTH", OhlcvPeriod::Month6)]
    #[case("1YRS", OhlcvPeriod::Year1)]
    #[case("2YRS", OhlcvPeriod::Year2)]
    #[case("3YRS", OhlcvPeriod::Year3)]
    #[case("4YRS", OhlcvPeriod::Year4)]
    #[case("5YRS", OhlcvPeriod::Year5)]
    fn ohlcv_period_preserves_wire_value(#[case] wire: &str, #[case] expected: OhlcvPeriod) {
        let encoded = serde_json::to_string(wire).unwrap();
        let actual: OhlcvPeriod = serde_json::from_str(&encoded).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(actual.as_ref(), wire);
        assert_eq!(actual.to_string(), wire);
        assert_eq!(wire.parse::<OhlcvPeriod>().unwrap(), expected);
        assert_eq!(serde_json::to_string(&actual).unwrap(), encoded);
    }

    #[rstest]
    #[case("BUY", TakerSide::Buy)]
    #[case("SELL", TakerSide::Sell)]
    #[case("BUY_ESTIMATED", TakerSide::BuyEstimated)]
    #[case("SELL_ESTIMATED", TakerSide::SellEstimated)]
    #[case("UNKNOWN", TakerSide::Unknown)]
    fn taker_side_preserves_wire_value(#[case] wire: &str, #[case] expected: TakerSide) {
        let encoded = serde_json::to_string(wire).unwrap();
        let actual: TakerSide = serde_json::from_str(&encoded).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(actual.as_ref(), wire);
        assert_eq!(actual.to_string(), wire);
        assert_eq!(wire.parse::<TakerSide>().unwrap(), expected);
        assert_eq!(serde_json::to_string(&actual).unwrap(), encoded);
    }

    #[rstest]
    #[case("SPOT", SymbolType::Spot)]
    #[case("FUTURES", SymbolType::Futures)]
    #[case("OPTION", SymbolType::Option)]
    #[case("PERPETUAL", SymbolType::Perpetual)]
    #[case("DEPLOYER_PERPETUAL", SymbolType::DeployerPerpetual)]
    #[case("INDEX", SymbolType::Index)]
    #[case("CREDIT", SymbolType::Credit)]
    #[case("CONTRACT", SymbolType::Contract)]
    #[case("OPTION_COMBO", SymbolType::OptionCombo)]
    #[case("FUTURE_COMBO", SymbolType::FutureCombo)]
    fn symbol_type_preserves_wire_value(#[case] wire: &str, #[case] expected: SymbolType) {
        let encoded = serde_json::to_string(wire).unwrap();
        let actual: SymbolType = serde_json::from_str(&encoded).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(actual.as_ref(), wire);
        assert_eq!(actual.to_string(), wire);
        assert_eq!(wire.parse::<SymbolType>().unwrap(), expected);
        assert_eq!(serde_json::to_string(&actual).unwrap(), encoded);
    }

    #[rstest]
    #[case("FUTURE_VALUE")]
    #[case("8SEC")]
    #[case("")]
    #[case("escaped\"value")]
    fn unknown_classifications_keep_their_wire_value(#[case] wire: &str) {
        let encoded = serde_json::to_string(wire).unwrap();
        let period: OhlcvPeriod = serde_json::from_str(&encoded).unwrap();
        let side: TakerSide = serde_json::from_str(&encoded).unwrap();
        let symbol: SymbolType = serde_json::from_str(&encoded).unwrap();

        assert_eq!(period, OhlcvPeriod::Other(wire.into()));
        assert_eq!(side, TakerSide::Other(wire.into()));
        assert_eq!(symbol, SymbolType::Other(wire.into()));
        assert_eq!(serde_json::to_string(&period).unwrap(), encoded);
        assert_eq!(serde_json::to_string(&side).unwrap(), encoded);
        assert_eq!(serde_json::to_string(&symbol).unwrap(), encoded);
    }
}
