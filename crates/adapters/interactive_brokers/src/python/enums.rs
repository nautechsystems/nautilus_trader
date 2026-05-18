// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Python bindings for Interactive Brokers adapter enums.

use pyo3::prelude::*;

use crate::{
    common::enums::{
        IbAccountSummaryEvent, IbAccountUpdateEvent, IbAccountUpdateMultiEvent, IbAction,
        IbArticleType, IbAuctionStrategy, IbAuctionType, IbBondIdentifierKind,
        IbBuilderTimeInForce, IbCancelOrderEvent, IbComboLegOpenClose, IbConditionConjunction,
        IbConditionKind, IbExecutionsEvent, IbExerciseAction, IbExerciseOptionsEvent,
        IbFundAssetType, IbFundDistributionPolicyIndicator, IbHistoricalBarSize,
        IbHistoricalBarUpdateEvent, IbHistoricalTickType, IbHistoricalWhatToShow, IbLegAction,
        IbLiquidity, IbMarketDepthEvent, IbOcaType, IbOptionRight, IbOrderOpenClose, IbOrderOrigin,
        IbOrderStatus, IbOrderType, IbOrderUpdateEvent, IbOrdersEvent, IbPlaceOrderEvent,
        IbPositionUpdateEvent, IbPositionUpdateMultiEvent, IbRealtimeBarSize, IbRealtimeWhatToShow,
        IbReferencePriceType, IbRiskAversion, IbRule80A, IbSecurityType, IbShortSaleSlot,
        IbTickEvent, IbTickType, IbTimeInForce, IbTradingHours, IbTriggerMethod,
        IbTwapStrategyType, IbVolatilityType,
    },
    error::{ErrorCategory, InteractiveBrokersErrorKind},
};

#[pymethods]
impl IbAction {
    #[classattr]
    const BUY: Self = Self::Buy;
    #[classattr]
    const BOUGHT: Self = Self::Bought;
    #[classattr]
    const SELL: Self = Self::Sell;
    #[classattr]
    const SOLD: Self = Self::Sold;
    #[classattr]
    const SELL_SHORT: Self = Self::SellShort;
    #[classattr]
    const SELL_LONG: Self = Self::SellLong;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
impl IbOrderStatus {
    #[classattr]
    const API_PENDING: Self = Self::ApiPending;
    #[classattr]
    const PENDING_SUBMIT: Self = Self::PendingSubmit;
    #[classattr]
    const PRE_SUBMITTED: Self = Self::PreSubmitted;
    #[classattr]
    const SUBMITTED: Self = Self::Submitted;
    #[classattr]
    const PENDING_CANCEL: Self = Self::PendingCancel;
    #[classattr]
    const API_CANCELLED: Self = Self::ApiCancelled;
    #[classattr]
    const CANCELLED: Self = Self::Cancelled;
    #[classattr]
    const FILLED: Self = Self::Filled;
    #[classattr]
    const INACTIVE: Self = Self::Inactive;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
impl IbOrderType {
    #[classattr]
    const MARKET: Self = Self::Market;
    #[classattr]
    const MARKET_ON_CLOSE: Self = Self::MarketOnClose;
    #[classattr]
    const LIMIT: Self = Self::Limit;
    #[classattr]
    const LIMIT_ON_CLOSE: Self = Self::LimitOnClose;
    #[classattr]
    const STOP: Self = Self::Stop;
    #[classattr]
    const STOP_LIMIT: Self = Self::StopLimit;
    #[classattr]
    const TRAILING_STOP: Self = Self::TrailingStop;
    #[classattr]
    const TRAILING_STOP_LIMIT: Self = Self::TrailingStopLimit;
    #[classattr]
    const MARKET_IF_TOUCHED: Self = Self::MarketIfTouched;
    #[classattr]
    const LIMIT_IF_TOUCHED: Self = Self::LimitIfTouched;
    #[classattr]
    const MARKET_TO_LIMIT: Self = Self::MarketToLimit;
    #[classattr]
    const MARKET_WITH_PROTECTION: Self = Self::MarketWithProtection;
    #[classattr]
    const STOP_WITH_PROTECTION: Self = Self::StopWithProtection;
    #[classattr]
    const MIDPRICE: Self = Self::Midprice;
    #[classattr]
    const PEGGED_TO_MARKET: Self = Self::PeggedToMarket;
    #[classattr]
    const PEGGED_TO_STOCK: Self = Self::PeggedToStock;
    #[classattr]
    const PEGGED_TO_MIDPOINT: Self = Self::PeggedToMidpoint;
    #[classattr]
    const PEGGED_TO_BENCHMARK: Self = Self::PeggedToBenchmark;
    #[classattr]
    const PEG_BEST: Self = Self::PegBest;
    #[classattr]
    const RELATIVE: Self = Self::Relative;
    #[classattr]
    const PASSIVE_RELATIVE: Self = Self::PassiveRelative;
    #[classattr]
    const VOLATILITY: Self = Self::Volatility;
    #[classattr]
    const BOX_TOP: Self = Self::BoxTop;
    #[classattr]
    const RELATIVE_LIMIT_COMBO: Self = Self::RelativeLimitCombo;
    #[classattr]
    const RELATIVE_MARKET_COMBO: Self = Self::RelativeMarketCombo;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
impl IbTimeInForce {
    #[classattr]
    const DAY: Self = Self::Day;
    #[classattr]
    const GOOD_TIL_CANCELED: Self = Self::GoodTilCanceled;
    #[classattr]
    const IMMEDIATE_OR_CANCEL: Self = Self::ImmediateOrCancel;
    #[classattr]
    const GOOD_TIL_DATE: Self = Self::GoodTilDate;
    #[classattr]
    const ON_OPEN: Self = Self::OnOpen;
    #[classattr]
    const FILL_OR_KILL: Self = Self::FillOrKill;
    #[classattr]
    const DAY_TIL_CANCELED: Self = Self::DayTilCanceled;
    #[classattr]
    const AUCTION: Self = Self::Auction;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
impl IbBuilderTimeInForce {
    #[classattr]
    const DAY: Self = Self::Day;
    #[classattr]
    const GOOD_TILL_CANCEL: Self = Self::GoodTillCancel;
    #[classattr]
    const IMMEDIATE_OR_CANCEL: Self = Self::ImmediateOrCancel;
    #[classattr]
    const GOOD_TILL_DATE: Self = Self::GoodTillDate;
    #[classattr]
    const FILL_OR_KILL: Self = Self::FillOrKill;
    #[classattr]
    const GOOD_TILL_CROSSING: Self = Self::GoodTillCrossing;
    #[classattr]
    const DAY_TILL_CANCELED: Self = Self::DayTillCanceled;
    #[classattr]
    const AUCTION: Self = Self::Auction;
    #[classattr]
    const OPENING_AUCTION: Self = Self::OpeningAuction;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
impl IbSecurityType {
    #[classattr]
    const STOCK: Self = Self::Stock;
    #[classattr]
    const OPTION: Self = Self::Option;
    #[classattr]
    const FUTURE: Self = Self::Future;
    #[classattr]
    const CONTINUOUS_FUTURE: Self = Self::ContinuousFuture;
    #[classattr]
    const INDEX: Self = Self::Index;
    #[classattr]
    const FUTURES_OPTION: Self = Self::FuturesOption;
    #[classattr]
    const FOREX_PAIR: Self = Self::ForexPair;
    #[classattr]
    const SPREAD: Self = Self::Spread;
    #[classattr]
    const WARRANT: Self = Self::Warrant;
    #[classattr]
    const BOND: Self = Self::Bond;
    #[classattr]
    const COMMODITY: Self = Self::Commodity;
    #[classattr]
    const NEWS: Self = Self::News;
    #[classattr]
    const MUTUAL_FUND: Self = Self::MutualFund;
    #[classattr]
    const CRYPTO: Self = Self::Crypto;
    #[classattr]
    const CFD: Self = Self::Cfd;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
impl IbOptionRight {
    #[classattr]
    const CALL: Self = Self::Call;
    #[classattr]
    const PUT: Self = Self::Put;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
impl IbHistoricalTickType {
    #[classattr]
    const TRADES: Self = Self::Trades;
    #[classattr]
    const BID_ASK: Self = Self::BidAsk;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
impl IbTradingHours {
    #[classattr]
    const REGULAR: Self = Self::Regular;
    #[classattr]
    const EXTENDED: Self = Self::Extended;

    #[pyo3(name = "use_rth")]
    fn py_use_rth(&self) -> bool {
        self.use_rth()
    }
}

#[pymethods]
impl IbHistoricalBarSize {
    #[classattr]
    const SEC: Self = Self::Sec;
    #[classattr]
    const SEC5: Self = Self::Sec5;
    #[classattr]
    const SEC10: Self = Self::Sec10;
    #[classattr]
    const SEC15: Self = Self::Sec15;
    #[classattr]
    const SEC30: Self = Self::Sec30;
    #[classattr]
    const MIN: Self = Self::Min;
    #[classattr]
    const MIN2: Self = Self::Min2;
    #[classattr]
    const MIN3: Self = Self::Min3;
    #[classattr]
    const MIN5: Self = Self::Min5;
    #[classattr]
    const MIN10: Self = Self::Min10;
    #[classattr]
    const MIN15: Self = Self::Min15;
    #[classattr]
    const MIN20: Self = Self::Min20;
    #[classattr]
    const MIN30: Self = Self::Min30;
    #[classattr]
    const HOUR: Self = Self::Hour;
    #[classattr]
    const HOUR2: Self = Self::Hour2;
    #[classattr]
    const HOUR3: Self = Self::Hour3;
    #[classattr]
    const HOUR4: Self = Self::Hour4;
    #[classattr]
    const HOUR8: Self = Self::Hour8;
    #[classattr]
    const DAY: Self = Self::Day;
    #[classattr]
    const WEEK: Self = Self::Week;
    #[classattr]
    const MONTH: Self = Self::Month;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
impl IbHistoricalWhatToShow {
    #[classattr]
    const TRADES: Self = Self::Trades;
    #[classattr]
    const MIDPOINT: Self = Self::Midpoint;
    #[classattr]
    const BID: Self = Self::Bid;
    #[classattr]
    const ASK: Self = Self::Ask;
    #[classattr]
    const BID_ASK: Self = Self::BidAsk;
    #[classattr]
    const HISTORICAL_VOLATILITY: Self = Self::HistoricalVolatility;
    #[classattr]
    const OPTION_IMPLIED_VOLATILITY: Self = Self::OptionImpliedVolatility;
    #[classattr]
    const FEE_RATE: Self = Self::FeeRate;
    #[classattr]
    const SCHEDULE: Self = Self::Schedule;
    #[classattr]
    const ADJUSTED_LAST: Self = Self::AdjustedLast;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
impl IbRealtimeBarSize {
    #[classattr]
    const SEC5: Self = Self::Sec5;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
impl IbRealtimeWhatToShow {
    #[classattr]
    const TRADES: Self = Self::Trades;
    #[classattr]
    const MIDPOINT: Self = Self::Midpoint;
    #[classattr]
    const BID: Self = Self::Bid;
    #[classattr]
    const ASK: Self = Self::Ask;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
impl IbConditionKind {
    #[classattr]
    const PRICE: Self = Self::Price;
    #[classattr]
    const TIME: Self = Self::Time;
    #[classattr]
    const MARGIN: Self = Self::Margin;
    #[classattr]
    const EXECUTION: Self = Self::Execution;
    #[classattr]
    const VOLUME: Self = Self::Volume;
    #[classattr]
    const PERCENT_CHANGE: Self = Self::PercentChange;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
impl IbConditionConjunction {
    #[classattr]
    const AND: Self = Self::And;
    #[classattr]
    const OR: Self = Self::Or;

    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }

    #[pyo3(name = "is_conjunction")]
    fn py_is_conjunction(&self) -> bool {
        self.is_conjunction()
    }
}

#[pymethods]
impl IbComboLegOpenClose {
    #[classattr]
    const SAME: Self = Self::Same;
    #[classattr]
    const OPEN: Self = Self::Open;
    #[classattr]
    const CLOSE: Self = Self::Close;
    #[classattr]
    const UNKNOWN: Self = Self::Unknown;

    #[pyo3(name = "as_i32")]
    fn py_as_i32(&self) -> i32 {
        self.as_i32()
    }
}

#[pymethods]
impl IbTriggerMethod {
    #[classattr]
    const DEFAULT: Self = Self::Default;
    #[classattr]
    const DOUBLE_BID_ASK: Self = Self::DoubleBidAsk;
    #[classattr]
    const LAST: Self = Self::Last;
    #[classattr]
    const DOUBLE_LAST: Self = Self::DoubleLast;
    #[classattr]
    const BID_ASK: Self = Self::BidAsk;
    #[classattr]
    const LAST_OR_BID_ASK: Self = Self::LastOrBidAsk;
    #[classattr]
    const MIDPOINT: Self = Self::Midpoint;

    #[pyo3(name = "as_i32")]
    fn py_as_i32(&self) -> i32 {
        self.as_i32()
    }
}

#[pymethods]
impl IbOcaType {
    #[classattr]
    const NONE: Self = Self::None;
    #[classattr]
    const CANCEL_WITH_BLOCK: Self = Self::CancelWithBlock;
    #[classattr]
    const REDUCE_WITH_BLOCK: Self = Self::ReduceWithBlock;
    #[classattr]
    const REDUCE_WITHOUT_BLOCK: Self = Self::ReduceWithoutBlock;

    #[pyo3(name = "as_i32")]
    fn py_as_i32(&self) -> i32 {
        self.as_i32()
    }
}

#[pymethods]
impl IbLiquidity {
    #[classattr]
    const NONE: Self = Self::None;
    #[classattr]
    const ADDED_LIQUIDITY: Self = Self::AddedLiquidity;
    #[classattr]
    const REMOVED_LIQUIDITY: Self = Self::RemovedLiquidity;
    #[classattr]
    const LIQUIDITY_ROUTED_OUT: Self = Self::LiquidityRoutedOut;

    #[pyo3(name = "as_i32")]
    fn py_as_i32(&self) -> i32 {
        self.as_i32()
    }
}

macro_rules! py_i32_enum {
    ($ty:ty, $($name:ident => $variant:ident),+ $(,)?) => {
        #[pymethods]
        impl $ty {
            $(
                #[classattr]
                const $name: Self = Self::$variant;
            )+

            #[pyo3(name = "as_i32")]
            fn py_as_i32(&self) -> i32 {
                self.as_i32()
            }
        }
    };
}

macro_rules! py_str_enum {
    ($ty:ty, $($name:ident => $variant:ident),+ $(,)?) => {
        #[pymethods]
        impl $ty {
            $(
                #[classattr]
                const $name: Self = Self::$variant;
            )+

            #[pyo3(name = "as_str")]
            fn py_as_str(&self) -> String {
                self.to_string()
            }
        }
    };
}

macro_rules! py_marker_enum {
    ($ty:ty, $($name:ident => $variant:ident),+ $(,)?) => {
        #[pymethods]
        impl $ty {
            $(
                #[classattr]
                const $name: Self = Self::$variant;
            )+

            #[pyo3(name = "as_str")]
            fn py_as_str(&self) -> String {
                format!("{self:?}")
            }
        }
    };
}

py_i32_enum!(
    IbTickType,
    UNKNOWN => Unknown,
    BID_SIZE => BidSize,
    BID => Bid,
    ASK => Ask,
    ASK_SIZE => AskSize,
    LAST => Last,
    LAST_SIZE => LastSize,
    HIGH => High,
    LOW => Low,
    VOLUME => Volume,
    CLOSE => Close,
    BID_OPTION => BidOption,
    ASK_OPTION => AskOption,
    LAST_OPTION => LastOption,
    MODEL_OPTION => ModelOption,
    OPEN => Open,
    LOW_13_WEEK => Low13Week,
    HIGH_13_WEEK => High13Week,
    LOW_26_WEEK => Low26Week,
    HIGH_26_WEEK => High26Week,
    LOW_52_WEEK => Low52Week,
    HIGH_52_WEEK => High52Week,
    AVG_VOLUME => AvgVolume,
    OPEN_INTEREST => OpenInterest,
    OPTION_HISTORICAL_VOL => OptionHistoricalVol,
    OPTION_IMPLIED_VOL => OptionImpliedVol,
    OPTION_BID_EXCH => OptionBidExch,
    OPTION_ASK_EXCH => OptionAskExch,
    OPTION_CALL_OPEN_INTEREST => OptionCallOpenInterest,
    OPTION_PUT_OPEN_INTEREST => OptionPutOpenInterest,
    OPTION_CALL_VOLUME => OptionCallVolume,
    OPTION_PUT_VOLUME => OptionPutVolume,
    INDEX_FUTURE_PREMIUM => IndexFuturePremium,
    BID_EXCH => BidExch,
    ASK_EXCH => AskExch,
    AUCTION_VOLUME => AuctionVolume,
    AUCTION_PRICE => AuctionPrice,
    AUCTION_IMBALANCE => AuctionImbalance,
    MARK_PRICE => MarkPrice,
    BID_EFP_COMPUTATION => BidEfpComputation,
    ASK_EFP_COMPUTATION => AskEfpComputation,
    LAST_EFP_COMPUTATION => LastEfpComputation,
    OPEN_EFP_COMPUTATION => OpenEfpComputation,
    HIGH_EFP_COMPUTATION => HighEfpComputation,
    LOW_EFP_COMPUTATION => LowEfpComputation,
    CLOSE_EFP_COMPUTATION => CloseEfpComputation,
    LAST_TIMESTAMP => LastTimestamp,
    SHORTABLE => Shortable,
    FUNDAMENTAL_RATIOS => FundamentalRatios,
    RT_VOLUME => RtVolume,
    HALTED => Halted,
    BID_YIELD => BidYield,
    ASK_YIELD => AskYield,
    LAST_YIELD => LastYield,
    CUST_OPTION_COMPUTATION => CustOptionComputation,
    TRADE_COUNT => TradeCount,
    TRADE_RATE => TradeRate,
    VOLUME_RATE => VolumeRate,
    LAST_RTH_TRADE => LastRthTrade,
    RT_HISTORICAL_VOL => RtHistoricalVol,
    IB_DIVIDENDS => IbDividends,
    BOND_FACTOR_MULTIPLIER => BondFactorMultiplier,
    REGULATORY_IMBALANCE => RegulatoryImbalance,
    NEWS_TICK => NewsTick,
    SHORT_TERM_VOLUME_3_MIN => ShortTermVolume3Min,
    SHORT_TERM_VOLUME_5_MIN => ShortTermVolume5Min,
    SHORT_TERM_VOLUME_10_MIN => ShortTermVolume10Min,
    DELAYED_BID => DelayedBid,
    DELAYED_ASK => DelayedAsk,
    DELAYED_LAST => DelayedLast,
    DELAYED_BID_SIZE => DelayedBidSize,
    DELAYED_ASK_SIZE => DelayedAskSize,
    DELAYED_LAST_SIZE => DelayedLastSize,
    DELAYED_HIGH => DelayedHigh,
    DELAYED_LOW => DelayedLow,
    DELAYED_VOLUME => DelayedVolume,
    DELAYED_CLOSE => DelayedClose,
    DELAYED_OPEN => DelayedOpen,
    RT_TRD_VOLUME => RtTrdVolume,
    CREDITMAN_MARK_PRICE => CreditmanMarkPrice,
    CREDITMAN_SLOW_MARK_PRICE => CreditmanSlowMarkPrice,
    DELAYED_BID_OPTION => DelayedBidOption,
    DELAYED_ASK_OPTION => DelayedAskOption,
    DELAYED_LAST_OPTION => DelayedLastOption,
    DELAYED_MODEL_OPTION => DelayedModelOption,
    LAST_EXCH => LastExch,
    LAST_REG_TIME => LastRegTime,
    FUTURES_OPEN_INTEREST => FuturesOpenInterest,
    AVG_OPT_VOLUME => AvgOptVolume,
    DELAYED_LAST_TIMESTAMP => DelayedLastTimestamp,
    SHORTABLE_SHARES => ShortableShares,
    DELAYED_HALTED => DelayedHalted,
    REUTERS2_MUTUAL_FUNDS => Reuters2MutualFunds,
    ETF_NAV_CLOSE => EtfNavClose,
    ETF_NAV_PRIOR_CLOSE => EtfNavPriorClose,
    ETF_NAV_BID => EtfNavBid,
    ETF_NAV_ASK => EtfNavAsk,
    ETF_NAV_LAST => EtfNavLast,
    ETF_FROZEN_NAV_LAST => EtfFrozenNavLast,
    ETF_NAV_HIGH => EtfNavHigh,
    ETF_NAV_LOW => EtfNavLow,
    SOCIAL_MARKET_ANALYTICS => SocialMarketAnalytics,
    ESTIMATED_IPO_MIDPOINT => EstimatedIpoMidpoint,
    FINAL_IPO_LAST => FinalIpoLast,
    DELAYED_YIELD_BID => DelayedYieldBid,
    DELAYED_YIELD_ASK => DelayedYieldAsk,
);

py_i32_enum!(IbOrderOrigin, CUSTOMER => Customer, FIRM => Firm);
py_i32_enum!(IbShortSaleSlot, NONE => None, BROKER => Broker, THIRD_PARTY => ThirdParty);
py_i32_enum!(IbVolatilityType, DAILY => Daily, ANNUAL => Annual);
py_i32_enum!(
    IbReferencePriceType,
    AVERAGE_OF_NBBO => AverageOfNbbo,
    NBBO => Nbbo,
);
py_i32_enum!(
    IbAuctionStrategy,
    MATCH => Match,
    IMPROVEMENT => Improvement,
    TRANSPARENT => Transparent,
);
py_i32_enum!(IbExerciseAction, EXERCISE => Exercise, LAPSE => Lapse);
py_i32_enum!(IbArticleType, TEXT => Text, BINARY => Binary);
py_i32_enum!(
    IbAuctionType,
    OPENING => Opening,
    CLOSING => Closing,
    VOLATILITY => Volatility,
);

py_str_enum!(
    IbRule80A,
    INDIVIDUAL => Individual,
    AGENCY => Agency,
    AGENT_OTHER_MEMBER => AgentOtherMember,
    INDIVIDUAL_PTIA => IndividualPtia,
    AGENCY_PTIA => AgencyPtia,
    AGENT_OTHER_MEMBER_PTIA => AgentOtherMemberPtia,
    INDIVIDUAL_PT => IndividualPt,
    AGENCY_PT => AgencyPt,
    AGENT_OTHER_MEMBER_PT => AgentOtherMemberPt,
);
py_str_enum!(IbOrderOpenClose, OPEN => Open, CLOSE => Close);
py_str_enum!(
    IbTwapStrategyType,
    MARKETABLE => Marketable,
    MATCHING_MIDPOINT => MatchingMidpoint,
    MATCHING_SAME_SIDE => MatchingSameSide,
    MATCHING_LAST => MatchingLast,
);
py_str_enum!(
    IbRiskAversion,
    GET_DONE => GetDone,
    AGGRESSIVE => Aggressive,
    NEUTRAL => Neutral,
    PASSIVE => Passive,
);
py_str_enum!(IbLegAction, BUY => Buy, SELL => Sell);
py_str_enum!(
    IbFundDistributionPolicyIndicator,
    NONE => None,
    ACCUMULATION_FUND => AccumulationFund,
    INCOME_FUND => IncomeFund,
);
py_str_enum!(
    IbFundAssetType,
    NONE => None,
    OTHERS => Others,
    MONEY_MARKET => MoneyMarket,
    FIXED_INCOME => FixedIncome,
    MULTI_ASSET => MultiAsset,
    EQUITY => Equity,
    SECTOR => Sector,
    GUARANTEED => Guaranteed,
    ALTERNATIVE => Alternative,
);
py_str_enum!(IbBondIdentifierKind, CUSIP => Cusip, ISIN => Isin);

py_marker_enum!(
    IbPlaceOrderEvent,
    ORDER_STATUS => OrderStatus,
    OPEN_ORDER => OpenOrder,
    EXECUTION_DATA => ExecutionData,
    COMMISSION_REPORT => CommissionReport,
    MESSAGE => Message,
);
py_marker_enum!(
    IbOrderUpdateEvent,
    ORDER_STATUS => OrderStatus,
    OPEN_ORDER => OpenOrder,
    EXECUTION_DATA => ExecutionData,
    COMMISSION_REPORT => CommissionReport,
    MESSAGE => Message,
);
py_marker_enum!(IbCancelOrderEvent, ORDER_STATUS => OrderStatus, NOTICE => Notice);
py_marker_enum!(
    IbOrdersEvent,
    ORDER_DATA => OrderData,
    ORDER_STATUS => OrderStatus,
    NOTICE => Notice,
);
py_marker_enum!(
    IbExecutionsEvent,
    EXECUTION_DATA => ExecutionData,
    COMMISSION_REPORT => CommissionReport,
    NOTICE => Notice,
);
py_marker_enum!(
    IbExerciseOptionsEvent,
    OPEN_ORDER => OpenOrder,
    ORDER_STATUS => OrderStatus,
    NOTICE => Notice,
);
py_marker_enum!(
    IbHistoricalBarUpdateEvent,
    HISTORICAL => Historical,
    UPDATE => Update,
    END => End,
);
py_marker_enum!(
    IbMarketDepthEvent,
    MARKET_DEPTH => MarketDepth,
    MARKET_DEPTH_L2 => MarketDepthL2,
    NOTICE => Notice,
);
py_marker_enum!(
    IbTickEvent,
    PRICE => Price,
    SIZE => Size,
    STRING => String,
    EFP => Efp,
    GENERIC => Generic,
    OPTION_COMPUTATION => OptionComputation,
    SNAPSHOT_END => SnapshotEnd,
    NOTICE => Notice,
    REQUEST_PARAMETERS => RequestParameters,
    PRICE_SIZE => PriceSize,
);
py_marker_enum!(IbAccountSummaryEvent, SUMMARY => Summary, END => End);
py_marker_enum!(
    IbPositionUpdateEvent,
    POSITION => Position,
    POSITION_END => PositionEnd,
);
py_marker_enum!(
    IbPositionUpdateMultiEvent,
    POSITION => Position,
    POSITION_END => PositionEnd,
);
py_marker_enum!(
    IbAccountUpdateEvent,
    ACCOUNT_VALUE => AccountValue,
    PORTFOLIO_VALUE => PortfolioValue,
    UPDATE_TIME => UpdateTime,
    END => End,
);
py_marker_enum!(
    IbAccountUpdateMultiEvent,
    ACCOUNT_MULTI_VALUE => AccountMultiValue,
    END => End,
);
py_marker_enum!(
    ErrorCategory,
    CLIENT_ERROR => ClientError,
    CONNECTIVITY_ERROR => ConnectivityError,
    SUBSCRIPTION_ERROR => SubscriptionError,
    ORDER_ERROR => OrderError,
    MARKET_DATA_ERROR => MarketDataError,
    UNKNOWN => Unknown,
);
py_marker_enum!(
    InteractiveBrokersErrorKind,
    CONNECTION => Connection,
    AUTHENTICATION => Authentication,
    CONFIGURATION => Configuration,
    REQUEST => Request,
    PARSE => Parse,
    INSTRUMENT => Instrument,
    ORDER => Order,
    MARKET_DATA => MarketData,
    IB_API => IbApi,
    INTERNAL => Internal,
);
