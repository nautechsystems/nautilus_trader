// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::enum_for_python;
use crate::enums::{
    AccountType, AggregationSource, AggressorSide, AssetClass, BarAggregation, BookAction,
    BookType, ContingencyType, CurrencyType, HaltReason, InstrumentClass, InstrumentCloseType,
    LiquiditySide, MarketStatus, OmsType, OptionKind, OrderSide, OrderStatus, OrderType,
    PositionSide, PriceType, TimeInForce, TradingState, TrailingOffsetType, TriggerType,
};
use crate::python::common::EnumIterator;
use pyo3::{exceptions::PyValueError, prelude::*, types::PyType, PyTypeInfo};
use std::str::FromStr;

#[pymethods]
impl AccountType {
    #[classattr]
    #[pyo3(name = "CASH")]
    fn py_cash() -> Self {
        AccountType::Cash
    }
    #[classattr]
    #[pyo3(name = "MARGIN")]
    fn py_margin() -> Self {
        AccountType::Margin
    }
    #[classattr]
    #[pyo3(name = "BETTING")]
    fn py_betting() -> Self {
        AccountType::Betting
    }
}

#[pymethods]
impl AggregationSource {
    #[classattr]
    #[pyo3(name = "EXTERNAL")]
    fn py_external() -> Self {
        AggregationSource::External
    }
    #[classattr]
    #[pyo3(name = "INTERNAL")]
    fn py_internal() -> Self {
        AggregationSource::Internal
    }
}

#[pymethods]
impl AggressorSide {
    #[classattr]
    #[pyo3(name = "NO_AGGRESSOR")]
    fn py_no_aggressor() -> Self {
        AggressorSide::NoAggressor
    }

    #[classattr]
    #[pyo3(name = "BUYER")]
    fn py_buyer() -> Self {
        AggressorSide::Buyer
    }

    #[classattr]
    #[pyo3(name = "SELLER")]
    fn py_seller() -> Self {
        AggressorSide::Seller
    }
}

#[pymethods]
impl AssetClass {
    #[classattr]
    #[pyo3(name = "FX")]
    fn py_fx() -> Self {
        AssetClass::FX
    }
    #[classattr]
    #[pyo3(name = "EQUITY")]
    fn py_equity() -> Self {
        AssetClass::Equity
    }
    #[classattr]
    #[pyo3(name = "COMMODITY")]
    fn py_commodity() -> Self {
        AssetClass::Commodity
    }
    #[classattr]
    #[pyo3(name = "DEBT")]
    fn py_debt() -> Self {
        AssetClass::Debt
    }
    #[classattr]
    #[pyo3(name = "INDEX")]
    fn py_index() -> Self {
        AssetClass::Index
    }
    #[classattr]
    #[pyo3(name = "CRYPTOCURRENCY")]
    fn py_cryptocurrency() -> Self {
        AssetClass::Cryptocurrency
    }
    #[classattr]
    #[pyo3(name = "ALTERNATIVE")]
    fn py_alternative() -> Self {
        AssetClass::Alternative
    }
}

#[pymethods]
impl InstrumentClass {
    #[classattr]
    #[pyo3(name = "SPOT")]
    fn py_spot() -> Self {
        InstrumentClass::Spot
    }
    #[classattr]
    #[pyo3(name = "SWAP")]
    fn py_swap() -> Self {
        InstrumentClass::Swap
    }
    #[classattr]
    #[pyo3(name = "FUTURE")]
    fn py_future() -> Self {
        InstrumentClass::Future
    }
    #[classattr]
    #[pyo3(name = "FORWARD")]
    fn py_forward() -> Self {
        InstrumentClass::Forward
    }
    #[classattr]
    #[pyo3(name = "CFD")]
    fn py_cfd() -> Self {
        InstrumentClass::Cfd
    }
    #[classattr]
    #[pyo3(name = "BOND")]
    fn py_bond() -> Self {
        InstrumentClass::Bond
    }
    #[classattr]
    #[pyo3(name = "OPTION")]
    fn py_option() -> Self {
        InstrumentClass::Option
    }
    #[classattr]
    #[pyo3(name = "WARRANT")]
    fn py_warrant() -> Self {
        InstrumentClass::Warrant
    }
    #[classattr]
    #[pyo3(name = "SPORTS_BETTING")]
    fn py_sports_betting() -> Self {
        InstrumentClass::SportsBetting
    }
}

#[pymethods]
impl BarAggregation {
    #[classattr]
    #[pyo3(name = "TICK")]
    fn py_tick() -> Self {
        BarAggregation::Tick
    }

    #[classattr]
    #[pyo3(name = "TICK_IMBALANCE")]
    fn py_tick_imbalance() -> Self {
        BarAggregation::TickImbalance
    }

    #[classattr]
    #[pyo3(name = "TICK_RUNS")]
    fn py_tick_runs() -> Self {
        BarAggregation::TickRuns
    }

    #[classattr]
    #[pyo3(name = "VOLUME")]
    fn py_volume() -> Self {
        BarAggregation::Volume
    }

    #[classattr]
    #[pyo3(name = "VOLUME_IMBALANCE")]
    fn py_volume_imbalance() -> Self {
        BarAggregation::VolumeImbalance
    }

    #[classattr]
    #[pyo3(name = "VOLUME_RUNS")]
    fn py_volume_runs() -> Self {
        BarAggregation::VolumeRuns
    }

    #[classattr]
    #[pyo3(name = "VALUE")]
    fn py_value() -> Self {
        BarAggregation::Value
    }

    #[classattr]
    #[pyo3(name = "VALUE_IMBALANCE")]
    fn py_value_imbalance() -> Self {
        BarAggregation::ValueImbalance
    }

    #[classattr]
    #[pyo3(name = "VALUE_RUNS")]
    fn py_value_runs() -> Self {
        BarAggregation::ValueRuns
    }

    #[classattr]
    #[pyo3(name = "MILLISECOND")]
    fn py_millisecond() -> Self {
        BarAggregation::Millisecond
    }

    #[classattr]
    #[pyo3(name = "SECOND")]
    fn py_second() -> Self {
        BarAggregation::Second
    }

    #[classattr]
    #[pyo3(name = "MINUTE")]
    fn py_minute() -> Self {
        BarAggregation::Minute
    }

    #[classattr]
    #[pyo3(name = "HOUR")]
    fn py_hour() -> Self {
        BarAggregation::Hour
    }

    #[classattr]
    #[pyo3(name = "DAY")]
    fn py_day() -> Self {
        BarAggregation::Day
    }

    #[classattr]
    #[pyo3(name = "WEEK")]
    fn py_week() -> Self {
        BarAggregation::Week
    }

    #[classattr]
    #[pyo3(name = "MONTH")]
    fn py_month() -> Self {
        BarAggregation::Month
    }
}

#[pymethods]
impl BookAction {
    #[classattr]
    #[pyo3(name = "ADD")]
    fn py_add() -> Self {
        BookAction::Add
    }
    #[classattr]
    #[pyo3(name = "UPDATE")]
    fn py_update() -> Self {
        BookAction::Update
    }
    #[classattr]
    #[pyo3(name = "DELETE")]
    fn py_delete() -> Self {
        BookAction::Delete
    }
    #[classattr]
    #[pyo3(name = "CLEAR")]
    fn py_clear() -> Self {
        BookAction::Clear
    }
}

#[pymethods]
impl ContingencyType {
    #[classattr]
    #[pyo3(name = "NO_CONTINGENCY")]
    fn py_no_contingency() -> Self {
        ContingencyType::NoContingency
    }
    #[classattr]
    #[pyo3(name = "OCO")]
    fn py_oco() -> Self {
        ContingencyType::Oco
    }
    #[classattr]
    #[pyo3(name = "OTO")]
    fn py_oto() -> Self {
        ContingencyType::Oto
    }
    #[classattr]
    #[pyo3(name = "OUO")]
    fn py_ouo() -> Self {
        ContingencyType::Ouo
    }
}

#[pymethods]
impl CurrencyType {
    #[classattr]
    #[pyo3(name = "CRYPTO")]
    fn py_crypto() -> Self {
        CurrencyType::Crypto
    }
    #[classattr]
    #[pyo3(name = "FIAT")]
    fn py_fiat() -> Self {
        CurrencyType::Fiat
    }
    #[classattr]
    #[pyo3(name = "COMMODITY_BACKED")]
    fn py_commodity_backed() -> Self {
        CurrencyType::CommodityBacked
    }
}

#[pymethods]
impl InstrumentCloseType {
    #[classattr]
    #[pyo3(name = "END_OF_SESSION")]
    fn py_end_of_session() -> Self {
        InstrumentCloseType::EndOfSession
    }
    #[classattr]
    #[pyo3(name = "CONTRACT_EXPIRED")]
    fn py_contract_expired() -> Self {
        InstrumentCloseType::ContractExpired
    }
}

#[pymethods]
impl LiquiditySide {
    #[classattr]
    #[pyo3(name = "NO_LIQUIDITY_SIDE")]
    fn py_no_liquidity_side() -> Self {
        LiquiditySide::NoLiquiditySide
    }
    #[classattr]
    #[pyo3(name = "MAKER")]
    fn py_maker() -> Self {
        LiquiditySide::Maker
    }
    #[classattr]
    #[pyo3(name = "TAKER")]
    fn py_taker() -> Self {
        LiquiditySide::Taker
    }
}

#[pymethods]
impl MarketStatus {
    #[classattr]
    #[pyo3(name = "PRE_OPEN")]
    fn py_pre_open() -> Self {
        MarketStatus::PreOpen
    }
    #[classattr]
    #[pyo3(name = "OPEN")]
    fn py_open() -> Self {
        MarketStatus::Open
    }
    #[classattr]
    #[pyo3(name = "PAUSE")]
    fn py_pause() -> Self {
        MarketStatus::Pause
    }
    #[classattr]
    #[pyo3(name = "HALT")]
    fn py_halt() -> Self {
        MarketStatus::Halt
    }
    #[classattr]
    #[pyo3(name = "REOPEN")]
    fn py_reopen() -> Self {
        MarketStatus::Reopen
    }
    #[classattr]
    #[pyo3(name = "PRE_CLOSE")]
    fn py_pre_close() -> Self {
        MarketStatus::PreClose
    }
    #[classattr]
    #[pyo3(name = "CLOSED")]
    fn py_closed() -> Self {
        MarketStatus::Closed
    }
}

#[pymethods]
impl HaltReason {
    #[classattr]
    #[pyo3(name = "NOT_HALTED")]
    fn py_not_halted() -> Self {
        HaltReason::NotHalted
    }
    #[classattr]
    #[pyo3(name = "GENERAL")]
    fn py_general() -> Self {
        HaltReason::General
    }
    #[classattr]
    #[pyo3(name = "VOLATILITY")]
    fn py_volatility() -> Self {
        HaltReason::Volatility
    }
}

#[pymethods]
impl OmsType {
    #[classattr]
    #[pyo3(name = "UNSPECIFIED")]
    fn py_unspecified() -> Self {
        OmsType::Unspecified
    }
    #[classattr]
    #[pyo3(name = "NETTING")]
    fn py_netting() -> Self {
        OmsType::Netting
    }
    #[classattr]
    #[pyo3(name = "HEDGING")]
    fn py_hedging() -> Self {
        OmsType::Hedging
    }
}

#[pymethods]
impl OptionKind {
    #[classattr]
    #[pyo3(name = "CALL")]
    fn py_call() -> Self {
        OptionKind::Call
    }

    #[classattr]
    #[pyo3(name = "PUT")]
    fn py_put() -> Self {
        OptionKind::Put
    }
}

#[pymethods]
impl OrderSide {
    #[classattr]
    #[pyo3(name = "NO_ORDER_SIDE")]
    fn py_no_order_side() -> Self {
        OrderSide::NoOrderSide
    }
    #[classattr]
    #[pyo3(name = "BUY")]
    fn py_buy() -> Self {
        OrderSide::Buy
    }
    #[classattr]
    #[pyo3(name = "SELL")]
    fn py_sell() -> Self {
        OrderSide::Sell
    }
}

#[pymethods]
impl OrderStatus {
    #[classattr]
    #[pyo3(name = "INITIALIZED")]
    fn py_initialized() -> Self {
        OrderStatus::Initialized
    }
    #[classattr]
    #[pyo3(name = "DENIED")]
    fn py_denied() -> Self {
        OrderStatus::Denied
    }
    #[classattr]
    #[pyo3(name = "EMULATED")]
    fn py_emulated() -> Self {
        OrderStatus::Emulated
    }
    #[classattr]
    #[pyo3(name = "RELEASED")]
    fn py_released() -> Self {
        OrderStatus::Released
    }
    #[classattr]
    #[pyo3(name = "SUBMITTED")]
    fn py_submitted() -> Self {
        OrderStatus::Submitted
    }
    #[classattr]
    #[pyo3(name = "ACCEPTED")]
    fn py_accepted() -> Self {
        OrderStatus::Accepted
    }
    #[classattr]
    #[pyo3(name = "REJECTED")]
    fn py_rejected() -> Self {
        OrderStatus::Rejected
    }
    #[classattr]
    #[pyo3(name = "CANCELED")]
    fn py_canceled() -> Self {
        OrderStatus::Canceled
    }
    #[classattr]
    #[pyo3(name = "EXPIRED")]
    fn py_expired() -> Self {
        OrderStatus::Expired
    }
    #[classattr]
    #[pyo3(name = "TRIGGERED")]
    fn py_triggered() -> Self {
        OrderStatus::Triggered
    }
    #[classattr]
    #[pyo3(name = "PENDING_UPDATE")]
    fn py_pending_update() -> Self {
        OrderStatus::PendingUpdate
    }
    #[classattr]
    #[pyo3(name = "PENDING_CANCEL")]
    fn py_pending_cancel() -> Self {
        OrderStatus::PendingCancel
    }
    #[classattr]
    #[pyo3(name = "PARTIALLY_FILLED")]
    fn py_partially_filled() -> Self {
        OrderStatus::PartiallyFilled
    }
    #[classattr]
    #[pyo3(name = "FILLED")]
    fn py_filled() -> Self {
        OrderStatus::Filled
    }
}

#[pymethods]
impl OrderType {
    #[classattr]
    #[pyo3(name = "MARKET")]
    fn py_market() -> Self {
        OrderType::Market
    }
    #[classattr]
    #[pyo3(name = "LIMIT")]
    fn py_limit() -> Self {
        OrderType::Limit
    }
    #[classattr]
    #[pyo3(name = "STOP_MARKET")]
    fn py_stop_market() -> Self {
        OrderType::StopMarket
    }
    #[classattr]
    #[pyo3(name = "STOP_LIMIT")]
    fn py_stop_limit() -> Self {
        OrderType::StopLimit
    }
    #[classattr]
    #[pyo3(name = "MARKET_TO_LIMIT")]
    fn py_market_to_limit() -> Self {
        OrderType::MarketToLimit
    }
    #[classattr]
    #[pyo3(name = "MARKET_IF_TOUCHED")]
    fn py_market_if_touched() -> Self {
        OrderType::MarketIfTouched
    }
    #[classattr]
    #[pyo3(name = "LIMIT_IF_TOUCHED")]
    fn py_limit_if_touched() -> Self {
        OrderType::LimitIfTouched
    }
    #[classattr]
    #[pyo3(name = "TRAILING_STOP_MARKET")]
    fn py_trailing_stop_market() -> Self {
        OrderType::TrailingStopMarket
    }
    #[classattr]
    #[pyo3(name = "TRAILING_STOP_LIMIT")]
    fn py_trailing_stop_limit() -> Self {
        OrderType::TrailingStopLimit
    }
}

#[pymethods]
impl PositionSide {
    #[classattr]
    #[pyo3(name = "NO_POSITION_SIDE")]
    fn py_no_position_side() -> Self {
        PositionSide::NoPositionSide
    }
    #[classattr]
    #[pyo3(name = "FLAT")]
    fn py_flat() -> Self {
        PositionSide::Flat
    }
    #[classattr]
    #[pyo3(name = "LONG")]
    fn py_long() -> Self {
        PositionSide::Long
    }
    #[classattr]
    #[pyo3(name = "SHORT")]
    fn py_short() -> Self {
        PositionSide::Short
    }
}

#[pymethods]
impl PriceType {
    #[classattr]
    #[pyo3(name = "BID")]
    fn py_bid() -> Self {
        PriceType::Bid
    }

    #[classattr]
    #[pyo3(name = "ASK")]
    fn py_ask() -> Self {
        PriceType::Ask
    }

    #[classattr]
    #[pyo3(name = "MID")]
    fn py_mid() -> Self {
        PriceType::Mid
    }

    #[classattr]
    #[pyo3(name = "LAST")]
    fn py_last() -> Self {
        PriceType::Last
    }
}

#[pymethods]
impl TimeInForce {
    #[classattr]
    #[pyo3(name = "GTC")]
    fn py_gtc() -> Self {
        TimeInForce::Gtc
    }
    #[classattr]
    #[pyo3(name = "IOC")]
    fn py_ioc() -> Self {
        TimeInForce::Ioc
    }
    #[classattr]
    #[pyo3(name = "FOK")]
    fn py_fok() -> Self {
        TimeInForce::Fok
    }
    #[classattr]
    #[pyo3(name = "GTD")]
    fn py_gtd() -> Self {
        TimeInForce::Gtd
    }
    #[classattr]
    #[pyo3(name = "DAY")]
    fn py_day() -> Self {
        TimeInForce::Day
    }
    #[classattr]
    #[pyo3(name = "AT_THE_OPEN")]
    fn py_at_the_open() -> Self {
        TimeInForce::AtTheOpen
    }
    #[classattr]
    #[pyo3(name = "AT_THE_CLOSE")]
    fn py_at_the_close() -> Self {
        TimeInForce::AtTheClose
    }
}

#[pymethods]
impl TrailingOffsetType {
    #[classattr]
    #[pyo3(name = "NO_TRAILING_OFFSET")]
    fn py_no_trailing_offset() -> Self {
        TrailingOffsetType::NoTrailingOffset
    }
    #[classattr]
    #[pyo3(name = "PRICE")]
    fn py_price() -> Self {
        TrailingOffsetType::Price
    }
    #[classattr]
    #[pyo3(name = "BASIS_POINTS")]
    fn py_basis_points() -> Self {
        TrailingOffsetType::BasisPoints
    }
    #[classattr]
    #[pyo3(name = "TICKS")]
    fn py_ticks() -> Self {
        TrailingOffsetType::Ticks
    }
    #[classattr]
    #[pyo3(name = "PRICE_TIER")]
    fn py_price_tier() -> Self {
        TrailingOffsetType::PriceTier
    }
}

#[pymethods]
impl TriggerType {
    #[classattr]
    #[pyo3(name = "NO_TRIGGER")]
    fn py_no_trigger() -> Self {
        TriggerType::NoTrigger
    }
    #[classattr]
    #[pyo3(name = "DEFAULT")]
    fn py_default() -> Self {
        TriggerType::Default
    }
    #[classattr]
    #[pyo3(name = "BID_ASK")]
    fn py_bid_ask() -> Self {
        TriggerType::BidAsk
    }
    #[classattr]
    #[pyo3(name = "LAST_TRADE")]
    fn py_last_trade() -> Self {
        TriggerType::LastTrade
    }
    #[classattr]
    #[pyo3(name = "DOUBLE_LAST")]
    fn py_double_last() -> Self {
        TriggerType::DoubleLast
    }
    #[classattr]
    #[pyo3(name = "DOUBLE_BID_ASK")]
    fn py_double_bid_ask() -> Self {
        TriggerType::DoubleBidAsk
    }
    #[classattr]
    #[pyo3(name = "LAST_OR_BID_ASK")]
    fn py_last_or_bid_ask() -> Self {
        TriggerType::LastOrBidAsk
    }
    #[classattr]
    #[pyo3(name = "MID_POINT")]
    fn py_mid_point() -> Self {
        TriggerType::MidPoint
    }
    #[classattr]
    #[pyo3(name = "MARK_PRICE")]
    fn py_mark_price() -> Self {
        TriggerType::MarkPrice
    }
    #[classattr]
    #[pyo3(name = "INDEX_PRICE")]
    fn py_index_price() -> Self {
        TriggerType::IndexPrice
    }
}

#[pymethods]
impl BookType {
    #[classattr]
    #[pyo3(name = "L1_MBP")]
    fn py_l1_mbp() -> Self {
        BookType::L1_MBP
    }
    #[classattr]
    #[pyo3(name = "L2_MBP")]
    fn py_l2_mbp() -> Self {
        BookType::L2_MBP
    }
    #[classattr]
    #[pyo3(name = "L3_MBO")]
    fn py_l3_mbo() -> Self {
        BookType::L3_MBO
    }
}

#[pymethods]
impl TradingState {
    #[classattr]
    #[pyo3(name = "ACTIVE")]
    fn py_active() -> Self {
        TradingState::Active
    }
    #[classattr]
    #[pyo3(name = "HALTED")]
    fn py_halted() -> Self {
        TradingState::Halted
    }
    #[classattr]
    #[pyo3(name = "REDUCING")]
    fn py_reducing() -> Self {
        TradingState::Reducing
    }
}

enum_for_python!(AggregationSource);
enum_for_python!(AggressorSide);
enum_for_python!(AssetClass);
enum_for_python!(BarAggregation);
enum_for_python!(BookAction);
enum_for_python!(BookType);
enum_for_python!(ContingencyType);
enum_for_python!(CurrencyType);
enum_for_python!(InstrumentCloseType);
enum_for_python!(LiquiditySide);
enum_for_python!(MarketStatus);
enum_for_python!(OmsType);
enum_for_python!(OptionKind);
enum_for_python!(OrderSide);
enum_for_python!(OrderStatus);
enum_for_python!(OrderType);
enum_for_python!(PositionSide);
enum_for_python!(PriceType);
enum_for_python!(TimeInForce);
enum_for_python!(TradingState);
enum_for_python!(TrailingOffsetType);
enum_for_python!(TriggerType);
