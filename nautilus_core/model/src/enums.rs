// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::ffi::c_char;
use std::fmt::Debug;
use std::str::FromStr;

use nautilus_core::string::{cstr_to_string, string_to_cstr};
use strum::{Display, EnumString, FromRepr};

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum AccountType {
    Cash = 1,
    Margin = 2,
    Betting = 3,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum AggregationSource {
    External = 1,
    Internal = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum AggressorSide {
    NoAggressor = 0, // Will be replaced by `Option`
    Buyer = 1,
    Seller = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
pub enum AssetClass {
    FX = 1,
    Equity = 2,
    Commodity = 3,
    Metal = 4,
    Energy = 5,
    Bond = 6,
    Index = 7,
    Cryptocurrency = 8,
    SportsBetting = 9,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum AssetType {
    Spot = 1,
    Swap = 2,
    Future = 3,
    Forward = 4,
    Cfd = 5,
    Option = 6,
    Warrant = 7,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum BarAggregation {
    Tick = 1,
    TickImbalance = 2,
    TickRuns = 3,
    Volume = 4,
    VolumeImbalance = 5,
    VolumeRuns = 6,
    Value = 7,
    ValueImbalance = 8,
    ValueRuns = 9,
    Millisecond = 10,
    Second = 11,
    Minute = 12,
    Hour = 13,
    Day = 14,
    Week = 15,
    Month = 16,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum BookAction {
    Add = 1,
    Update = 2,
    Delete = 3,
    Clear = 4,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
pub enum BookType {
    /// Top-of-book best bid/offer.
    L1_TBBO = 1,
    /// Market by price.
    L2_MBP = 2,
    /// Market by order.
    L3_MBO = 3,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum ContingencyType {
    NoContingency = 0, // Will be replaced by `Option`
    Oco = 1,
    Oto = 2,
    Ouo = 3,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CurrencyType {
    Crypto = 1,
    Fiat = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum DepthType {
    Volume = 1,
    Exposure = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum InstrumentCloseType {
    EndOfSession = 1,
    ContractExpired = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)]
pub enum LiquiditySide {
    NoLiquiditySide = 0, // Will be replaced by `Option`
    Maker = 1,
    Taker = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketStatus {
    Closed = 1,
    PreOpen = 2,
    Open = 3,
    Pause = 4,
    PreClose = 5,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum OmsType {
    Unspecified = 0, // Will be replaced by `Option`
    Netting = 1,
    Hedging = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum OptionKind {
    Call = 1,
    Put = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)]
pub enum OrderSide {
    NoOrderSide = 0, // Will be replaced by `Option`
    Buy = 1,
    Sell = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderStatus {
    Initialized = 1,
    Denied = 2,
    Submitted = 3,
    Accepted = 4,
    Rejected = 5,
    Canceled = 6,
    Expired = 7,
    Triggered = 8,
    PendingUpdate = 9,
    PendingCancel = 10,
    PartiallyFilled = 11,
    Filled = 12,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderType {
    Market = 1,
    Limit = 2,
    StopMarket = 3,
    StopLimit = 4,
    MarketToLimit = 5,
    MarketIfTouched = 6,
    LimitIfTouched = 7,
    TrailingStopMarket = 8,
    TrailingStopLimit = 9,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)]
pub enum PositionSide {
    NoPositionSide = 0, // Will be replaced by `Option`
    Flat = 1,
    Long = 2,
    Short = 3,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PriceType {
    Bid = 1,
    Ask = 2,
    Mid = 3,
    Last = 4,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum TimeInForce {
    Gtc = 1,
    Ioc = 2,
    Fok = 3,
    Gtd = 4,
    Day = 5,
    AtTheOpen = 6,
    AtTheClose = 7,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum TradingState {
    Active = 1,
    Halted = 2,
    Reducing = 3,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum TrailingOffsetType {
    NoTrailingOffset = 0, // Will be replaced by `Option`
    Price = 1,
    BasisPoints = 2,
    Ticks = 3,
    PriceTier = 4,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum TriggerType {
    NoTrigger = 0, // Will be replaced by `Option`
    Default = 1,
    BidAsk = 2,
    LastTrade = 3,
    DoubleLast = 4,
    DoubleBidAsk = 5,
    LastOrBidAsk = 6,
    MidPoint = 7,
    MarkPrice = 8,
    IndexPrice = 9,
}

// TODO(cs): These should be macros

#[no_mangle]
pub extern "C" fn account_type_to_cstr(value: AccountType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn account_type_from_cstr(ptr: *const c_char) -> AccountType {
    let value = cstr_to_string(ptr);
    AccountType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AccountType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn aggregation_source_to_cstr(value: AggregationSource) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn aggregation_source_from_cstr(ptr: *const c_char) -> AggregationSource {
    let value = cstr_to_string(ptr);
    AggregationSource::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AggregationSource` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn aggressor_side_to_cstr(value: AggressorSide) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn aggressor_side_from_cstr(ptr: *const c_char) -> AggressorSide {
    let value = cstr_to_string(ptr);
    AggressorSide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AggressorSide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn asset_class_to_cstr(value: AssetClass) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn asset_class_from_cstr(ptr: *const c_char) -> AssetClass {
    let value = cstr_to_string(ptr);
    AssetClass::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AssetClass` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn asset_type_to_cstr(value: AssetType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn asset_type_from_cstr(ptr: *const c_char) -> AssetType {
    let value = cstr_to_string(ptr);
    AssetType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AssetType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn bar_aggregation_to_cstr(value: BarAggregation) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn bar_aggregation_from_cstr(ptr: *const c_char) -> BarAggregation {
    let value = cstr_to_string(ptr);
    BarAggregation::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `BarAggregation` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn book_action_to_cstr(value: BookAction) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn book_action_from_cstr(ptr: *const c_char) -> BookAction {
    let value = cstr_to_string(ptr);
    BookAction::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `BookAction` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn book_type_to_cstr(value: BookType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn book_type_from_cstr(ptr: *const c_char) -> BookType {
    let value = cstr_to_string(ptr);
    BookType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `BookType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn contingency_type_to_cstr(value: ContingencyType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn contingency_type_from_cstr(ptr: *const c_char) -> ContingencyType {
    let value = cstr_to_string(ptr);
    ContingencyType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `ContingencyType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn currency_type_to_cstr(value: CurrencyType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn currency_type_from_cstr(ptr: *const c_char) -> CurrencyType {
    let value = cstr_to_string(ptr);
    CurrencyType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `CurrencyType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn depth_type_to_cstr(value: DepthType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn instrument_close_type_from_cstr(
    ptr: *const c_char,
) -> InstrumentCloseType {
    let value = cstr_to_string(ptr);
    InstrumentCloseType::from_str(&value).unwrap_or_else(|_| {
        panic!("invalid `InstrumentCloseType` enum string value, was '{value}'")
    })
}

#[no_mangle]
pub extern "C" fn instrument_close_type_to_cstr(value: InstrumentCloseType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn depth_type_from_cstr(ptr: *const c_char) -> DepthType {
    let value = cstr_to_string(ptr);
    DepthType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `DepthType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn liquidity_side_to_cstr(value: LiquiditySide) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn liquidity_side_from_cstr(ptr: *const c_char) -> LiquiditySide {
    let value = cstr_to_string(ptr);
    LiquiditySide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `LiquiditySide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn market_status_to_cstr(value: MarketStatus) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn market_status_from_cstr(ptr: *const c_char) -> MarketStatus {
    let value = cstr_to_string(ptr);
    MarketStatus::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `MarketStatus` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn oms_type_to_cstr(value: OmsType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn oms_type_from_cstr(ptr: *const c_char) -> OmsType {
    let value = cstr_to_string(ptr);
    OmsType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OmsType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn option_kind_to_cstr(value: OptionKind) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn option_kind_from_cstr(ptr: *const c_char) -> OptionKind {
    let value = cstr_to_string(ptr);
    OptionKind::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OptionKind` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn order_side_to_cstr(value: OrderSide) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_side_from_cstr(ptr: *const c_char) -> OrderSide {
    let value = cstr_to_string(ptr);
    OrderSide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OrderSide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn order_status_to_cstr(value: OrderStatus) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_status_from_cstr(ptr: *const c_char) -> OrderStatus {
    let value = cstr_to_string(ptr);
    OrderStatus::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OrderStatus` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn order_type_to_cstr(value: OrderType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_type_from_cstr(ptr: *const c_char) -> OrderType {
    let value = cstr_to_string(ptr);
    OrderType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OrderType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn position_side_to_cstr(value: PositionSide) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn position_side_from_cstr(ptr: *const c_char) -> PositionSide {
    let value = cstr_to_string(ptr);
    PositionSide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `PositionSide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn price_type_to_cstr(value: PriceType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn price_type_from_cstr(ptr: *const c_char) -> PriceType {
    let value = cstr_to_string(ptr);
    PriceType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `PriceType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn time_in_force_to_cstr(value: TimeInForce) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn time_in_force_from_cstr(ptr: *const c_char) -> TimeInForce {
    let value = cstr_to_string(ptr);
    TimeInForce::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TimeInForce` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn trading_state_to_cstr(value: TradingState) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn trading_state_from_cstr(ptr: *const c_char) -> TradingState {
    let value = cstr_to_string(ptr);
    TradingState::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TradingState` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn trailing_offset_type_to_cstr(value: TrailingOffsetType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn trailing_offset_type_from_cstr(ptr: *const c_char) -> TrailingOffsetType {
    let value = cstr_to_string(ptr);
    TrailingOffsetType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TrailingOffsetType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn trigger_type_to_cstr(value: TriggerType) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn trigger_type_from_cstr(ptr: *const c_char) -> TriggerType {
    let value = cstr_to_string(ptr);
    TriggerType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TriggerType` enum string value, was '{value}'"))
}
