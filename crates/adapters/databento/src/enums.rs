//! Enumerations for the Databento integration.

use std::str::FromStr;

use nautilus_model::{enum_strum_serde, enums::FromU8};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{AsRefStr, Display, EnumIter, EnumString, FromRepr};

/// Represents a Databento statistic type.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        rename_all = "SCREAMING_SNAKE_CASE",
        module = "nautilus_trader.core.nautilus_pyo3.databento",
        from_py_object
    )
)]
pub enum DatabentoStatisticType {
    OpeningPrice = 1,
    IndicativeOpeningPrice = 2,
    SettlementPrice = 3,
    TradingSessionLowPrice = 4,
    TradingSessionHighPrice = 5,
    ClearedVolume = 6,
    LowestOffer = 7,
    HighestBid = 8,
    OpenInterest = 9,
    FixingPrice = 10,
    ClosePrice = 11,
    NetChange = 12,
    Vwap = 13,
}

impl FromU8 for DatabentoStatisticType {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::OpeningPrice),
            2 => Some(Self::IndicativeOpeningPrice),
            3 => Some(Self::SettlementPrice),
            4 => Some(Self::TradingSessionLowPrice),
            5 => Some(Self::TradingSessionHighPrice),
            6 => Some(Self::ClearedVolume),
            7 => Some(Self::LowestOffer),
            8 => Some(Self::HighestBid),
            9 => Some(Self::OpenInterest),
            10 => Some(Self::FixingPrice),
            11 => Some(Self::ClosePrice),
            12 => Some(Self::NetChange),
            13 => Some(Self::Vwap),
            _ => None,
        }
    }
}

/// Represents a Databento statistic update action.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        rename_all = "SCREAMING_SNAKE_CASE",
        module = "nautilus_trader.core.nautilus_pyo3.databento",
        from_py_object
    )
)]
pub enum DatabentoStatisticUpdateAction {
    Added = 1,
    Deleted = 2,
}

impl FromU8 for DatabentoStatisticUpdateAction {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Added),
            2 => Some(Self::Deleted),
            _ => None,
        }
    }
}

enum_strum_serde!(DatabentoStatisticType);
enum_strum_serde!(DatabentoStatisticUpdateAction);
