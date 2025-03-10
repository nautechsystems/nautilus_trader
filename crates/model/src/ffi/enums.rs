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

use std::{ffi::c_char, str::FromStr};

use nautilus_core::ffi::string::{cstr_as_str, str_to_cstr};

use crate::enums::{
    AccountType, AggregationSource, AggressorSide, AssetClass, BarAggregation, BookAction,
    BookType, ContingencyType, CurrencyType, InstrumentClass, InstrumentCloseType, LiquiditySide,
    MarketStatus, MarketStatusAction, OmsType, OptionKind, OrderSide, OrderStatus, OrderType,
    PositionSide, PriceType, RecordFlag, TimeInForce, TradingState, TrailingOffsetType,
    TriggerType,
};

#[unsafe(no_mangle)]
pub extern "C" fn account_type_to_cstr(value: AccountType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn account_type_from_cstr(ptr: *const c_char) -> AccountType {
    let value = unsafe { cstr_as_str(ptr) };
    AccountType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `AccountType` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn aggregation_source_to_cstr(value: AggregationSource) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aggregation_source_from_cstr(ptr: *const c_char) -> AggregationSource {
    let value = unsafe { cstr_as_str(ptr) };
    AggregationSource::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `AggregationSource` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn aggressor_side_to_cstr(value: AggressorSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aggressor_side_from_cstr(ptr: *const c_char) -> AggressorSide {
    let value = unsafe { cstr_as_str(ptr) };
    AggressorSide::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `AggressorSide` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn asset_class_to_cstr(value: AssetClass) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn asset_class_from_cstr(ptr: *const c_char) -> AssetClass {
    let value = unsafe { cstr_as_str(ptr) };
    AssetClass::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `AssetClass` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn instrument_class_to_cstr(value: InstrumentClass) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instrument_class_from_cstr(ptr: *const c_char) -> InstrumentClass {
    let value = unsafe { cstr_as_str(ptr) };
    InstrumentClass::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `InstrumentClass` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn bar_aggregation_to_cstr(value: BarAggregation) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bar_aggregation_from_cstr(ptr: *const c_char) -> BarAggregation {
    let value = unsafe { cstr_as_str(ptr) };
    BarAggregation::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `BarAggregation` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn book_action_to_cstr(value: BookAction) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn book_action_from_cstr(ptr: *const c_char) -> BookAction {
    let value = unsafe { cstr_as_str(ptr) };
    BookAction::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `BookAction` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn book_type_to_cstr(value: BookType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn book_type_from_cstr(ptr: *const c_char) -> BookType {
    let value = unsafe { cstr_as_str(ptr) };
    BookType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `BookType` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn contingency_type_to_cstr(value: ContingencyType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn contingency_type_from_cstr(ptr: *const c_char) -> ContingencyType {
    let value = unsafe { cstr_as_str(ptr) };
    ContingencyType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `ContingencyType` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn currency_type_to_cstr(value: CurrencyType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currency_type_from_cstr(ptr: *const c_char) -> CurrencyType {
    let value = unsafe { cstr_as_str(ptr) };
    CurrencyType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `CurrencyType` enum string value, was '{value}'"))
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instrument_close_type_from_cstr(
    ptr: *const c_char,
) -> InstrumentCloseType {
    let value = unsafe { cstr_as_str(ptr) };
    InstrumentCloseType::from_str(value).unwrap_or_else(|_| {
        panic!("invalid `InstrumentCloseType` enum string value, was '{value}'")
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn instrument_close_type_to_cstr(value: InstrumentCloseType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

#[unsafe(no_mangle)]
pub extern "C" fn liquidity_side_to_cstr(value: LiquiditySide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn liquidity_side_from_cstr(ptr: *const c_char) -> LiquiditySide {
    let value = unsafe { cstr_as_str(ptr) };
    LiquiditySide::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `LiquiditySide` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn market_status_to_cstr(value: MarketStatus) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn market_status_from_cstr(ptr: *const c_char) -> MarketStatus {
    let value = unsafe { cstr_as_str(ptr) };
    MarketStatus::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `MarketStatus` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn market_status_action_to_cstr(value: MarketStatusAction) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn market_status_action_from_cstr(ptr: *const c_char) -> MarketStatusAction {
    let value = unsafe { cstr_as_str(ptr) };
    MarketStatusAction::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `MarketStatusAction` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn oms_type_to_cstr(value: OmsType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oms_type_from_cstr(ptr: *const c_char) -> OmsType {
    let value = unsafe { cstr_as_str(ptr) };
    OmsType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `OmsType` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn option_kind_to_cstr(value: OptionKind) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn option_kind_from_cstr(ptr: *const c_char) -> OptionKind {
    let value = unsafe { cstr_as_str(ptr) };
    OptionKind::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `OptionKind` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn order_side_to_cstr(value: OrderSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn order_side_from_cstr(ptr: *const c_char) -> OrderSide {
    let value = unsafe { cstr_as_str(ptr) };
    OrderSide::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `OrderSide` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn order_status_to_cstr(value: OrderStatus) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn order_status_from_cstr(ptr: *const c_char) -> OrderStatus {
    let value = unsafe { cstr_as_str(ptr) };
    OrderStatus::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `OrderStatus` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn order_type_to_cstr(value: OrderType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn order_type_from_cstr(ptr: *const c_char) -> OrderType {
    let value = unsafe { cstr_as_str(ptr) };
    OrderType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `OrderType` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn position_side_to_cstr(value: PositionSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn position_side_from_cstr(ptr: *const c_char) -> PositionSide {
    let value = unsafe { cstr_as_str(ptr) };
    PositionSide::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `PositionSide` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn price_type_to_cstr(value: PriceType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn price_type_from_cstr(ptr: *const c_char) -> PriceType {
    let value = unsafe { cstr_as_str(ptr) };
    PriceType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `PriceType` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn record_flag_to_cstr(value: RecordFlag) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn record_flag_from_cstr(ptr: *const c_char) -> RecordFlag {
    let value = unsafe { cstr_as_str(ptr) };
    RecordFlag::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `RecordFlag` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn time_in_force_to_cstr(value: TimeInForce) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn time_in_force_from_cstr(ptr: *const c_char) -> TimeInForce {
    let value = unsafe { cstr_as_str(ptr) };
    TimeInForce::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `TimeInForce` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn trading_state_to_cstr(value: TradingState) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trading_state_from_cstr(ptr: *const c_char) -> TradingState {
    let value = unsafe { cstr_as_str(ptr) };
    TradingState::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `TradingState` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn trailing_offset_type_to_cstr(value: TrailingOffsetType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trailing_offset_type_from_cstr(ptr: *const c_char) -> TrailingOffsetType {
    let value = unsafe { cstr_as_str(ptr) };
    TrailingOffsetType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `TrailingOffsetType` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn trigger_type_to_cstr(value: TriggerType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trigger_type_from_cstr(ptr: *const c_char) -> TriggerType {
    let value = unsafe { cstr_as_str(ptr) };
    TriggerType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `TriggerType` enum string value, was '{value}'"))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_name() {
        assert_eq!(OrderSide::NoOrderSide.name(), "NO_ORDER_SIDE");
        assert_eq!(OrderSide::Buy.name(), "BUY");
        assert_eq!(OrderSide::Sell.name(), "SELL");
    }

    #[rstest]
    fn test_value() {
        assert_eq!(OrderSide::NoOrderSide.value(), 0);
        assert_eq!(OrderSide::Buy.value(), 1);
        assert_eq!(OrderSide::Sell.value(), 2);
    }
}
