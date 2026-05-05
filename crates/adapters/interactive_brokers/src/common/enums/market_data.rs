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

use std::{fmt::Display, str::FromStr};

/// Interactive Brokers historical tick request types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbHistoricalTickType {
    /// Historical trade ticks.
    Trades,
    /// Historical bid/ask ticks.
    BidAsk,
}

impl IbHistoricalTickType {
    /// Returns the IB wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trades => "TRADES",
            Self::BidAsk => "BID_ASK",
        }
    }
}

impl FromStr for IbHistoricalTickType {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_uppercase().as_str() {
            "TRADES" => Ok(Self::Trades),
            "BID_ASK" => Ok(Self::BidAsk),
            _ => anyhow::bail!("Unknown IB historical tick type: {value}"),
        }
    }
}

impl Display for IbHistoricalTickType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers trading hours selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbTradingHours {
    /// Regular trading hours only.
    Regular,
    /// Include extended trading hours.
    Extended,
}

impl IbTradingHours {
    /// Returns whether IB should use regular trading hours only.
    #[must_use]
    pub const fn use_rth(self) -> bool {
        matches!(self, Self::Regular)
    }

    /// Converts to the rust-ibapi trading hours enum.
    #[must_use]
    pub const fn ibapi_trading_hours(self) -> ibapi::market_data::TradingHours {
        match self {
            Self::Regular => ibapi::market_data::TradingHours::Regular,
            Self::Extended => ibapi::market_data::TradingHours::Extended,
        }
    }
}

impl From<bool> for IbTradingHours {
    fn from(use_rth: bool) -> Self {
        if use_rth {
            Self::Regular
        } else {
            Self::Extended
        }
    }
}

/// Interactive Brokers historical bar-size values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbHistoricalBarSize {
    Sec,
    Sec5,
    Sec10,
    Sec15,
    Sec30,
    Min,
    Min2,
    Min3,
    Min5,
    Min10,
    Min15,
    Min20,
    Min30,
    Hour,
    Hour2,
    Hour3,
    Hour4,
    Hour8,
    Day,
    Week,
    Month,
}

impl IbHistoricalBarSize {
    /// Converts to the rust-ibapi historical bar-size enum.
    #[must_use]
    pub const fn ibapi_bar_size(self) -> ibapi::market_data::historical::BarSize {
        match self {
            Self::Sec => ibapi::market_data::historical::BarSize::Sec,
            Self::Sec5 => ibapi::market_data::historical::BarSize::Sec5,
            Self::Sec10 => ibapi::market_data::historical::BarSize::Sec10,
            Self::Sec15 => ibapi::market_data::historical::BarSize::Sec15,
            Self::Sec30 => ibapi::market_data::historical::BarSize::Sec30,
            Self::Min => ibapi::market_data::historical::BarSize::Min,
            Self::Min2 => ibapi::market_data::historical::BarSize::Min2,
            Self::Min3 => ibapi::market_data::historical::BarSize::Min3,
            Self::Min5 => ibapi::market_data::historical::BarSize::Min5,
            Self::Min10 => ibapi::market_data::historical::BarSize::Min10,
            Self::Min15 => ibapi::market_data::historical::BarSize::Min15,
            Self::Min20 => ibapi::market_data::historical::BarSize::Min20,
            Self::Min30 => ibapi::market_data::historical::BarSize::Min30,
            Self::Hour => ibapi::market_data::historical::BarSize::Hour,
            Self::Hour2 => ibapi::market_data::historical::BarSize::Hour2,
            Self::Hour3 => ibapi::market_data::historical::BarSize::Hour3,
            Self::Hour4 => ibapi::market_data::historical::BarSize::Hour4,
            Self::Hour8 => ibapi::market_data::historical::BarSize::Hour8,
            Self::Day => ibapi::market_data::historical::BarSize::Day,
            Self::Week => ibapi::market_data::historical::BarSize::Week,
            Self::Month => ibapi::market_data::historical::BarSize::Month,
        }
    }
}

impl Display for IbHistoricalBarSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.ibapi_bar_size())
    }
}

/// Interactive Brokers historical data selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbHistoricalWhatToShow {
    Trades,
    Midpoint,
    Bid,
    Ask,
    BidAsk,
    HistoricalVolatility,
    OptionImpliedVolatility,
    FeeRate,
    Schedule,
    AdjustedLast,
}

impl IbHistoricalWhatToShow {
    /// Returns the IB wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trades => "TRADES",
            Self::Midpoint => "MIDPOINT",
            Self::Bid => "BID",
            Self::Ask => "ASK",
            Self::BidAsk => "BID_ASK",
            Self::HistoricalVolatility => "HISTORICAL_VOLATILITY",
            Self::OptionImpliedVolatility => "OPTION_IMPLIED_VOLATILITY",
            Self::FeeRate => "FEE_RATE",
            Self::Schedule => "SCHEDULE",
            Self::AdjustedLast => "ADJUSTED_LAST",
        }
    }

    /// Converts to the rust-ibapi historical data selector.
    #[must_use]
    pub const fn ibapi_what_to_show(self) -> ibapi::market_data::historical::WhatToShow {
        match self {
            Self::Trades => ibapi::market_data::historical::WhatToShow::Trades,
            Self::Midpoint => ibapi::market_data::historical::WhatToShow::MidPoint,
            Self::Bid => ibapi::market_data::historical::WhatToShow::Bid,
            Self::Ask => ibapi::market_data::historical::WhatToShow::Ask,
            Self::BidAsk => ibapi::market_data::historical::WhatToShow::BidAsk,
            Self::HistoricalVolatility => {
                ibapi::market_data::historical::WhatToShow::HistoricalVolatility
            }
            Self::OptionImpliedVolatility => {
                ibapi::market_data::historical::WhatToShow::OptionImpliedVolatility
            }
            Self::FeeRate => ibapi::market_data::historical::WhatToShow::FeeRate,
            Self::Schedule => ibapi::market_data::historical::WhatToShow::Schedule,
            Self::AdjustedLast => ibapi::market_data::historical::WhatToShow::AdjustedLast,
        }
    }
}

impl Display for IbHistoricalWhatToShow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers realtime bar-size values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbRealtimeBarSize {
    Sec5,
}

impl IbRealtimeBarSize {
    /// Converts to the rust-ibapi realtime bar-size enum.
    #[must_use]
    pub const fn ibapi_bar_size(self) -> ibapi::market_data::realtime::BarSize {
        match self {
            Self::Sec5 => ibapi::market_data::realtime::BarSize::Sec5,
        }
    }
}

impl Display for IbRealtimeBarSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sec5 => f.write_str("5 secs"),
        }
    }
}

/// Interactive Brokers realtime bar selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbRealtimeWhatToShow {
    Trades,
    Midpoint,
    Bid,
    Ask,
}

impl IbRealtimeWhatToShow {
    /// Returns the IB wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trades => "TRADES",
            Self::Midpoint => "MIDPOINT",
            Self::Bid => "BID",
            Self::Ask => "ASK",
        }
    }

    /// Converts to the rust-ibapi realtime data selector.
    #[must_use]
    pub const fn ibapi_what_to_show(self) -> ibapi::market_data::realtime::WhatToShow {
        match self {
            Self::Trades => ibapi::market_data::realtime::WhatToShow::Trades,
            Self::Midpoint => ibapi::market_data::realtime::WhatToShow::MidPoint,
            Self::Bid => ibapi::market_data::realtime::WhatToShow::Bid,
            Self::Ask => ibapi::market_data::realtime::WhatToShow::Ask,
        }
    }
}

impl Display for IbRealtimeWhatToShow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers market data tick types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbTickType {
    Unknown,
    BidSize,
    Bid,
    Ask,
    AskSize,
    Last,
    LastSize,
    High,
    Low,
    Volume,
    Close,
    BidOption,
    AskOption,
    LastOption,
    ModelOption,
    Open,
    Low13Week,
    High13Week,
    Low26Week,
    High26Week,
    Low52Week,
    High52Week,
    AvgVolume,
    OpenInterest,
    OptionHistoricalVol,
    OptionImpliedVol,
    OptionBidExch,
    OptionAskExch,
    OptionCallOpenInterest,
    OptionPutOpenInterest,
    OptionCallVolume,
    OptionPutVolume,
    IndexFuturePremium,
    BidExch,
    AskExch,
    AuctionVolume,
    AuctionPrice,
    AuctionImbalance,
    MarkPrice,
    BidEfpComputation,
    AskEfpComputation,
    LastEfpComputation,
    OpenEfpComputation,
    HighEfpComputation,
    LowEfpComputation,
    CloseEfpComputation,
    LastTimestamp,
    Shortable,
    FundamentalRatios,
    RtVolume,
    Halted,
    BidYield,
    AskYield,
    LastYield,
    CustOptionComputation,
    TradeCount,
    TradeRate,
    VolumeRate,
    LastRthTrade,
    RtHistoricalVol,
    IbDividends,
    BondFactorMultiplier,
    RegulatoryImbalance,
    NewsTick,
    ShortTermVolume3Min,
    ShortTermVolume5Min,
    ShortTermVolume10Min,
    DelayedBid,
    DelayedAsk,
    DelayedLast,
    DelayedBidSize,
    DelayedAskSize,
    DelayedLastSize,
    DelayedHigh,
    DelayedLow,
    DelayedVolume,
    DelayedClose,
    DelayedOpen,
    RtTrdVolume,
    CreditmanMarkPrice,
    CreditmanSlowMarkPrice,
    DelayedBidOption,
    DelayedAskOption,
    DelayedLastOption,
    DelayedModelOption,
    LastExch,
    LastRegTime,
    FuturesOpenInterest,
    AvgOptVolume,
    DelayedLastTimestamp,
    ShortableShares,
    DelayedHalted,
    Reuters2MutualFunds,
    EtfNavClose,
    EtfNavPriorClose,
    EtfNavBid,
    EtfNavAsk,
    EtfNavLast,
    EtfFrozenNavLast,
    EtfNavHigh,
    EtfNavLow,
    SocialMarketAnalytics,
    EstimatedIpoMidpoint,
    FinalIpoLast,
    DelayedYieldBid,
    DelayedYieldAsk,
}

impl IbTickType {
    /// Returns the IB integer code.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::BidSize => 0,
            Self::Bid => 1,
            Self::Ask => 2,
            Self::AskSize => 3,
            Self::Last => 4,
            Self::LastSize => 5,
            Self::High => 6,
            Self::Low => 7,
            Self::Volume => 8,
            Self::Close => 9,
            Self::BidOption => 10,
            Self::AskOption => 11,
            Self::LastOption => 12,
            Self::ModelOption => 13,
            Self::Open => 14,
            Self::Low13Week => 15,
            Self::High13Week => 16,
            Self::Low26Week => 17,
            Self::High26Week => 18,
            Self::Low52Week => 19,
            Self::High52Week => 20,
            Self::AvgVolume => 21,
            Self::OpenInterest => 22,
            Self::OptionHistoricalVol => 23,
            Self::OptionImpliedVol => 24,
            Self::OptionBidExch => 25,
            Self::OptionAskExch => 26,
            Self::OptionCallOpenInterest => 27,
            Self::OptionPutOpenInterest => 28,
            Self::OptionCallVolume => 29,
            Self::OptionPutVolume => 30,
            Self::IndexFuturePremium => 31,
            Self::BidExch => 32,
            Self::AskExch => 33,
            Self::AuctionVolume => 34,
            Self::AuctionPrice => 35,
            Self::AuctionImbalance => 36,
            Self::MarkPrice => 37,
            Self::BidEfpComputation => 38,
            Self::AskEfpComputation => 39,
            Self::LastEfpComputation => 40,
            Self::OpenEfpComputation => 41,
            Self::HighEfpComputation => 42,
            Self::LowEfpComputation => 43,
            Self::CloseEfpComputation => 44,
            Self::LastTimestamp => 45,
            Self::Shortable => 46,
            Self::FundamentalRatios => 47,
            Self::RtVolume => 48,
            Self::Halted => 49,
            Self::BidYield => 50,
            Self::AskYield => 51,
            Self::LastYield => 52,
            Self::CustOptionComputation => 53,
            Self::TradeCount => 54,
            Self::TradeRate => 55,
            Self::VolumeRate => 56,
            Self::LastRthTrade => 57,
            Self::RtHistoricalVol => 58,
            Self::IbDividends => 59,
            Self::BondFactorMultiplier => 60,
            Self::RegulatoryImbalance => 61,
            Self::NewsTick => 62,
            Self::ShortTermVolume3Min => 63,
            Self::ShortTermVolume5Min => 64,
            Self::ShortTermVolume10Min => 65,
            Self::DelayedBid => 66,
            Self::DelayedAsk => 67,
            Self::DelayedLast => 68,
            Self::DelayedBidSize => 69,
            Self::DelayedAskSize => 70,
            Self::DelayedLastSize => 71,
            Self::DelayedHigh => 72,
            Self::DelayedLow => 73,
            Self::DelayedVolume => 74,
            Self::DelayedClose => 75,
            Self::DelayedOpen => 76,
            Self::RtTrdVolume => 77,
            Self::CreditmanMarkPrice => 78,
            Self::CreditmanSlowMarkPrice => 79,
            Self::DelayedBidOption => 80,
            Self::DelayedAskOption => 81,
            Self::DelayedLastOption => 82,
            Self::DelayedModelOption => 83,
            Self::LastExch => 84,
            Self::LastRegTime => 85,
            Self::FuturesOpenInterest => 86,
            Self::AvgOptVolume => 87,
            Self::DelayedLastTimestamp => 88,
            Self::ShortableShares => 89,
            Self::DelayedHalted => 90,
            Self::Reuters2MutualFunds => 91,
            Self::EtfNavClose => 92,
            Self::EtfNavPriorClose => 93,
            Self::EtfNavBid => 94,
            Self::EtfNavAsk => 95,
            Self::EtfNavLast => 96,
            Self::EtfFrozenNavLast => 97,
            Self::EtfNavHigh => 98,
            Self::EtfNavLow => 99,
            Self::SocialMarketAnalytics => 100,
            Self::EstimatedIpoMidpoint => 101,
            Self::FinalIpoLast => 102,
            Self::DelayedYieldBid => 103,
            Self::DelayedYieldAsk => 104,
        }
    }

    /// Converts to the rust-ibapi tick type enum.
    #[must_use]
    pub fn ibapi_tick_type(self) -> ibapi::contracts::tick_types::TickType {
        ibapi::contracts::tick_types::TickType::from(self.as_i32())
    }
}

impl From<i32> for IbTickType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::BidSize,
            1 => Self::Bid,
            2 => Self::Ask,
            3 => Self::AskSize,
            4 => Self::Last,
            5 => Self::LastSize,
            6 => Self::High,
            7 => Self::Low,
            8 => Self::Volume,
            9 => Self::Close,
            10 => Self::BidOption,
            11 => Self::AskOption,
            12 => Self::LastOption,
            13 => Self::ModelOption,
            14 => Self::Open,
            15 => Self::Low13Week,
            16 => Self::High13Week,
            17 => Self::Low26Week,
            18 => Self::High26Week,
            19 => Self::Low52Week,
            20 => Self::High52Week,
            21 => Self::AvgVolume,
            22 => Self::OpenInterest,
            23 => Self::OptionHistoricalVol,
            24 => Self::OptionImpliedVol,
            25 => Self::OptionBidExch,
            26 => Self::OptionAskExch,
            27 => Self::OptionCallOpenInterest,
            28 => Self::OptionPutOpenInterest,
            29 => Self::OptionCallVolume,
            30 => Self::OptionPutVolume,
            31 => Self::IndexFuturePremium,
            32 => Self::BidExch,
            33 => Self::AskExch,
            34 => Self::AuctionVolume,
            35 => Self::AuctionPrice,
            36 => Self::AuctionImbalance,
            37 => Self::MarkPrice,
            38 => Self::BidEfpComputation,
            39 => Self::AskEfpComputation,
            40 => Self::LastEfpComputation,
            41 => Self::OpenEfpComputation,
            42 => Self::HighEfpComputation,
            43 => Self::LowEfpComputation,
            44 => Self::CloseEfpComputation,
            45 => Self::LastTimestamp,
            46 => Self::Shortable,
            47 => Self::FundamentalRatios,
            48 => Self::RtVolume,
            49 => Self::Halted,
            50 => Self::BidYield,
            51 => Self::AskYield,
            52 => Self::LastYield,
            53 => Self::CustOptionComputation,
            54 => Self::TradeCount,
            55 => Self::TradeRate,
            56 => Self::VolumeRate,
            57 => Self::LastRthTrade,
            58 => Self::RtHistoricalVol,
            59 => Self::IbDividends,
            60 => Self::BondFactorMultiplier,
            61 => Self::RegulatoryImbalance,
            62 => Self::NewsTick,
            63 => Self::ShortTermVolume3Min,
            64 => Self::ShortTermVolume5Min,
            65 => Self::ShortTermVolume10Min,
            66 => Self::DelayedBid,
            67 => Self::DelayedAsk,
            68 => Self::DelayedLast,
            69 => Self::DelayedBidSize,
            70 => Self::DelayedAskSize,
            71 => Self::DelayedLastSize,
            72 => Self::DelayedHigh,
            73 => Self::DelayedLow,
            74 => Self::DelayedVolume,
            75 => Self::DelayedClose,
            76 => Self::DelayedOpen,
            77 => Self::RtTrdVolume,
            78 => Self::CreditmanMarkPrice,
            79 => Self::CreditmanSlowMarkPrice,
            80 => Self::DelayedBidOption,
            81 => Self::DelayedAskOption,
            82 => Self::DelayedLastOption,
            83 => Self::DelayedModelOption,
            84 => Self::LastExch,
            85 => Self::LastRegTime,
            86 => Self::FuturesOpenInterest,
            87 => Self::AvgOptVolume,
            88 => Self::DelayedLastTimestamp,
            89 => Self::ShortableShares,
            90 => Self::DelayedHalted,
            91 => Self::Reuters2MutualFunds,
            92 => Self::EtfNavClose,
            93 => Self::EtfNavPriorClose,
            94 => Self::EtfNavBid,
            95 => Self::EtfNavAsk,
            96 => Self::EtfNavLast,
            97 => Self::EtfFrozenNavLast,
            98 => Self::EtfNavHigh,
            99 => Self::EtfNavLow,
            100 => Self::SocialMarketAnalytics,
            101 => Self::EstimatedIpoMidpoint,
            102 => Self::FinalIpoLast,
            103 => Self::DelayedYieldBid,
            104 => Self::DelayedYieldAsk,
            _ => Self::Unknown,
        }
    }
}

impl Display for IbTickType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_i32())
    }
}
