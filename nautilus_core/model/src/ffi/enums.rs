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

use std::{ffi::c_char, str::FromStr};

use nautilus_core::ffi::string::{cstr_to_str, str_to_cstr};

use crate::enums::{
    AccountType, AggregationSource, AggressorSide, AssetClass, BarAggregation, BookAction,
    BookType, ContingencyType, CurrencyType, HaltReason, InstrumentClass, InstrumentCloseType,
    LiquiditySide, MarketStatus, OmsType, OptionKind, OrderSide, OrderStatus, OrderType,
    PositionSide, PriceType, TimeInForce, TradingState, TrailingOffsetType, TriggerType,
};

#[no_mangle]
pub extern "C" fn account_type_to_cstr(value: AccountType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn account_type_from_cstr(ptr: *const c_char) -> AccountType {
    let value = cstr_to_str(ptr);
    AccountType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `AccountType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn aggregation_source_to_cstr(value: AggregationSource) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn aggregation_source_from_cstr(ptr: *const c_char) -> AggregationSource {
    let value = cstr_to_str(ptr);
    AggregationSource::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `AggregationSource` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn aggressor_side_to_cstr(value: AggressorSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn aggressor_side_from_cstr(ptr: *const c_char) -> AggressorSide {
    let value = cstr_to_str(ptr);
    AggressorSide::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `AggressorSide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn asset_class_to_cstr(value: AssetClass) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn asset_class_from_cstr(ptr: *const c_char) -> AssetClass {
    let value = cstr_to_str(ptr);
    AssetClass::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `AssetClass` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn instrument_class_to_cstr(value: InstrumentClass) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn instrument_class_from_cstr(ptr: *const c_char) -> InstrumentClass {
    let value = cstr_to_str(ptr);
    InstrumentClass::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `InstrumentClass` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn bar_aggregation_to_cstr(value: BarAggregation) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn bar_aggregation_from_cstr(ptr: *const c_char) -> BarAggregation {
    let value = cstr_to_str(ptr);
    BarAggregation::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `BarAggregation` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn book_action_to_cstr(value: BookAction) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn book_action_from_cstr(ptr: *const c_char) -> BookAction {
    let value = cstr_to_str(ptr);
    BookAction::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `BookAction` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn book_type_to_cstr(value: BookType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn book_type_from_cstr(ptr: *const c_char) -> BookType {
    let value = cstr_to_str(ptr);
    BookType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `BookType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn contingency_type_to_cstr(value: ContingencyType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn contingency_type_from_cstr(ptr: *const c_char) -> ContingencyType {
    let value = cstr_to_str(ptr);
    ContingencyType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `ContingencyType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn currency_type_to_cstr(value: CurrencyType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn currency_type_from_cstr(ptr: *const c_char) -> CurrencyType {
    let value = cstr_to_str(ptr);
    CurrencyType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `CurrencyType` enum string value, was '{value}'"))
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn instrument_close_type_from_cstr(
    ptr: *const c_char,
) -> InstrumentCloseType {
    let value = cstr_to_str(ptr);
    InstrumentCloseType::from_str(value).unwrap_or_else(|_| {
        panic!("invalid `InstrumentCloseType` enum string value, was '{value}'")
    })
}

#[no_mangle]
pub extern "C" fn instrument_close_type_to_cstr(value: InstrumentCloseType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

#[no_mangle]
pub extern "C" fn liquidity_side_to_cstr(value: LiquiditySide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn liquidity_side_from_cstr(ptr: *const c_char) -> LiquiditySide {
    let value = cstr_to_str(ptr);
    LiquiditySide::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `LiquiditySide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn market_status_to_cstr(value: MarketStatus) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn market_status_from_cstr(ptr: *const c_char) -> MarketStatus {
    let value = cstr_to_str(ptr);
    MarketStatus::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `MarketStatus` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn halt_reason_to_cstr(value: HaltReason) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn halt_reason_from_cstr(ptr: *const c_char) -> HaltReason {
    let value = cstr_to_str(ptr);
    HaltReason::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `HaltReason` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn oms_type_to_cstr(value: OmsType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn oms_type_from_cstr(ptr: *const c_char) -> OmsType {
    let value = cstr_to_str(ptr);
    OmsType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `OmsType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn option_kind_to_cstr(value: OptionKind) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn option_kind_from_cstr(ptr: *const c_char) -> OptionKind {
    let value = cstr_to_str(ptr);
    OptionKind::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `OptionKind` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn order_side_to_cstr(value: OrderSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_side_from_cstr(ptr: *const c_char) -> OrderSide {
    let value = cstr_to_str(ptr);
    OrderSide::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `OrderSide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn order_status_to_cstr(value: OrderStatus) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_status_from_cstr(ptr: *const c_char) -> OrderStatus {
    let value = cstr_to_str(ptr);
    OrderStatus::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `OrderStatus` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn order_type_to_cstr(value: OrderType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_type_from_cstr(ptr: *const c_char) -> OrderType {
    let value = cstr_to_str(ptr);
    OrderType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `OrderType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn position_side_to_cstr(value: PositionSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn position_side_from_cstr(ptr: *const c_char) -> PositionSide {
    let value = cstr_to_str(ptr);
    PositionSide::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `PositionSide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn price_type_to_cstr(value: PriceType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn price_type_from_cstr(ptr: *const c_char) -> PriceType {
    let value = cstr_to_str(ptr);
    PriceType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `PriceType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn time_in_force_to_cstr(value: TimeInForce) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn time_in_force_from_cstr(ptr: *const c_char) -> TimeInForce {
    let value = cstr_to_str(ptr);
    TimeInForce::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `TimeInForce` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn trading_state_to_cstr(value: TradingState) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn trading_state_from_cstr(ptr: *const c_char) -> TradingState {
    let value = cstr_to_str(ptr);
    TradingState::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `TradingState` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn trailing_offset_type_to_cstr(value: TrailingOffsetType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn trailing_offset_type_from_cstr(ptr: *const c_char) -> TrailingOffsetType {
    let value = cstr_to_str(ptr);
    TrailingOffsetType::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `TrailingOffsetType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn trigger_type_to_cstr(value: TriggerType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn trigger_type_from_cstr(ptr: *const c_char) -> TriggerType {
    let value = cstr_to_str(ptr);
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
