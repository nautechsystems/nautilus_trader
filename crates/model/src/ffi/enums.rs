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

use std::{ffi::c_char, str::FromStr};

use nautilus_core::ffi::{
    abort_on_panic,
    string::{cstr_as_str, str_to_cstr},
};
use strum::{AsRefStr, Display, EnumString};

use crate::enums::{
    AccountType, AggregationSource, AggressorSide, AssetClass, BarAggregation, BookAction,
    BookType, ContingencyType, CurrencyType, InstrumentClass, InstrumentCloseType, LiquiditySide,
    MarketStatus, MarketStatusAction, OmsType, OptionKind, OrderSide, OrderStatus, OrderType,
    OtoTriggerMode, PositionAdjustmentType, PositionSide, PriceType, RecordFlag, TimeInForce,
    TradingState, TrailingOffsetType, TriggerType,
};

/// The stable zero-inclusive contingency-type representation required by the existing C ABI.
///
/// Use [`Option<ContingencyType>`] for ordinary Rust optionality.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Display, Hash, PartialEq, Eq, AsRefStr, EnumString)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum ContingencyTypeOptional {
    /// Compatibility value for no specified contingency type.
    ///
    /// This value may be removed in a future version.
    #[default]
    NoContingency = 0,
    /// One-Cancels-the-Other.
    Oco = 1,
    /// One-Triggers-the-Other.
    Oto = 2,
    /// One-Updates-the-Other.
    Ouo = 3,
}

impl ContingencyTypeOptional {
    #[must_use]
    pub const fn as_option(self) -> Option<ContingencyType> {
        match self {
            Self::NoContingency => None,
            Self::Oco => Some(ContingencyType::Oco),
            Self::Oto => Some(ContingencyType::Oto),
            Self::Ouo => Some(ContingencyType::Ouo),
        }
    }
}

impl From<Option<ContingencyType>> for ContingencyTypeOptional {
    fn from(value: Option<ContingencyType>) -> Self {
        match value {
            None => Self::NoContingency,
            Some(ContingencyType::Oco) => Self::Oco,
            Some(ContingencyType::Oto) => Self::Oto,
            Some(ContingencyType::Ouo) => Self::Ouo,
        }
    }
}

/// The stable zero-inclusive order-side representation required by the existing C ABI.
///
/// Use [`Option<OrderSide>`] for ordinary Rust optionality.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Display, Hash, PartialEq, Eq, AsRefStr, EnumString)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderSideOptional {
    /// Compatibility value for no specified order side.
    ///
    /// This value may be removed in a future version.
    #[default]
    NoOrderSide = 0,
    /// The order is a BUY.
    Buy = 1,
    /// The order is a SELL.
    Sell = 2,
}

impl OrderSideOptional {
    #[must_use]
    pub const fn as_option(self) -> Option<OrderSide> {
        match self {
            Self::NoOrderSide => None,
            Self::Buy => Some(OrderSide::Buy),
            Self::Sell => Some(OrderSide::Sell),
        }
    }
}

impl From<Option<OrderSide>> for OrderSideOptional {
    fn from(value: Option<OrderSide>) -> Self {
        match value {
            None => Self::NoOrderSide,
            Some(OrderSide::Buy) => Self::Buy,
            Some(OrderSide::Sell) => Self::Sell,
        }
    }
}

/// The stable zero-inclusive position-side representation required by the existing C ABI.
///
/// Use [`Option<PositionSide>`] for ordinary Rust optionality.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Display, Hash, PartialEq, Eq, AsRefStr, EnumString)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PositionSideOptional {
    /// Compatibility value for no specified position side.
    ///
    /// This value may be removed in a future version.
    #[default]
    NoPositionSide = 0,
    /// A neutral/flat position.
    Flat = 1,
    /// A long position.
    Long = 2,
    /// A short position.
    Short = 3,
}

impl PositionSideOptional {
    #[must_use]
    pub const fn as_option(self) -> Option<PositionSide> {
        match self {
            Self::NoPositionSide => None,
            Self::Flat => Some(PositionSide::Flat),
            Self::Long => Some(PositionSide::Long),
            Self::Short => Some(PositionSide::Short),
        }
    }
}

impl From<Option<PositionSide>> for PositionSideOptional {
    fn from(value: Option<PositionSide>) -> Self {
        match value {
            None => Self::NoPositionSide,
            Some(PositionSide::Flat) => Self::Flat,
            Some(PositionSide::Long) => Self::Long,
            Some(PositionSide::Short) => Self::Short,
        }
    }
}

/// The stable zero-inclusive trailing-offset-type representation required by the existing C ABI.
///
/// Use [`Option<TrailingOffsetType>`] for ordinary Rust optionality.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Display, Hash, PartialEq, Eq, AsRefStr, EnumString)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum TrailingOffsetTypeOptional {
    /// Compatibility value for no specified trailing offset type.
    ///
    /// This value may be removed in a future version.
    #[default]
    NoTrailingOffset = 0,
    /// The trailing offset is based on a market price.
    Price = 1,
    /// The trailing offset is based on basis points.
    BasisPoints = 2,
    /// The trailing offset is based on ticks.
    Ticks = 3,
    /// The trailing offset is based on a venue-defined price tier.
    PriceTier = 4,
}

impl TrailingOffsetTypeOptional {
    #[must_use]
    pub const fn as_option(self) -> Option<TrailingOffsetType> {
        match self {
            Self::NoTrailingOffset => None,
            Self::Price => Some(TrailingOffsetType::Price),
            Self::BasisPoints => Some(TrailingOffsetType::BasisPoints),
            Self::Ticks => Some(TrailingOffsetType::Ticks),
            Self::PriceTier => Some(TrailingOffsetType::PriceTier),
        }
    }
}

impl From<Option<TrailingOffsetType>> for TrailingOffsetTypeOptional {
    fn from(value: Option<TrailingOffsetType>) -> Self {
        match value {
            None => Self::NoTrailingOffset,
            Some(TrailingOffsetType::Price) => Self::Price,
            Some(TrailingOffsetType::BasisPoints) => Self::BasisPoints,
            Some(TrailingOffsetType::Ticks) => Self::Ticks,
            Some(TrailingOffsetType::PriceTier) => Self::PriceTier,
        }
    }
}

/// The stable zero-inclusive trigger-type representation required by the existing C ABI.
///
/// Use [`Option<TriggerType>`] for ordinary Rust optionality.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Display, Hash, PartialEq, Eq, AsRefStr, EnumString)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum TriggerTypeOptional {
    /// Compatibility value for no specified trigger type.
    ///
    /// This value may be removed in a future version.
    #[default]
    NoTrigger = 0,
    /// The venue default trigger type.
    Default = 1,
    /// The last traded price.
    LastPrice = 2,
    /// The mark price.
    MarkPrice = 3,
    /// The index price.
    IndexPrice = 4,
    /// The bid or ask price.
    BidAsk = 5,
    /// Two consecutive last prices.
    DoubleLast = 6,
    /// Two consecutive bid or ask prices.
    DoubleBidAsk = 7,
    /// The last price or bid or ask price.
    LastOrBidAsk = 8,
    /// The midpoint price.
    MidPoint = 9,
}

impl TriggerTypeOptional {
    #[must_use]
    pub const fn as_option(self) -> Option<TriggerType> {
        match self {
            Self::NoTrigger => None,
            Self::Default => Some(TriggerType::Default),
            Self::LastPrice => Some(TriggerType::LastPrice),
            Self::MarkPrice => Some(TriggerType::MarkPrice),
            Self::IndexPrice => Some(TriggerType::IndexPrice),
            Self::BidAsk => Some(TriggerType::BidAsk),
            Self::DoubleLast => Some(TriggerType::DoubleLast),
            Self::DoubleBidAsk => Some(TriggerType::DoubleBidAsk),
            Self::LastOrBidAsk => Some(TriggerType::LastOrBidAsk),
            Self::MidPoint => Some(TriggerType::MidPoint),
        }
    }
}

impl From<Option<TriggerType>> for TriggerTypeOptional {
    fn from(value: Option<TriggerType>) -> Self {
        match value {
            None => Self::NoTrigger,
            Some(TriggerType::Default) => Self::Default,
            Some(TriggerType::LastPrice) => Self::LastPrice,
            Some(TriggerType::MarkPrice) => Self::MarkPrice,
            Some(TriggerType::IndexPrice) => Self::IndexPrice,
            Some(TriggerType::BidAsk) => Self::BidAsk,
            Some(TriggerType::DoubleLast) => Self::DoubleLast,
            Some(TriggerType::DoubleBidAsk) => Self::DoubleBidAsk,
            Some(TriggerType::LastOrBidAsk) => Self::LastOrBidAsk,
            Some(TriggerType::MidPoint) => Self::MidPoint,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn account_type_to_cstr(value: AccountType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `AccountType` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn account_type_from_cstr(ptr: *const c_char) -> AccountType {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        AccountType::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `AccountType` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn aggregation_source_to_cstr(value: AggregationSource) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `AggregationSource` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aggregation_source_from_cstr(ptr: *const c_char) -> AggregationSource {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        AggregationSource::from_str(value).unwrap_or_else(|_| {
            panic!("invalid `AggregationSource` enum string value, was '{value}'")
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn aggressor_side_to_cstr(value: AggressorSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `AggressorSide` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aggressor_side_from_cstr(ptr: *const c_char) -> AggressorSide {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        AggressorSide::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `AggressorSide` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn asset_class_to_cstr(value: AssetClass) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `AssetClass` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn asset_class_from_cstr(ptr: *const c_char) -> AssetClass {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        AssetClass::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `AssetClass` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn instrument_class_to_cstr(value: InstrumentClass) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `InstrumentClass` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instrument_class_from_cstr(ptr: *const c_char) -> InstrumentClass {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        InstrumentClass::from_str(value).unwrap_or_else(|_| {
            panic!("invalid `InstrumentClass` enum string value, was '{value}'")
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn bar_aggregation_to_cstr(value: BarAggregation) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `BarAggregation` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bar_aggregation_from_cstr(ptr: *const c_char) -> BarAggregation {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        BarAggregation::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `BarAggregation` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn book_action_to_cstr(value: BookAction) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `BookAction` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn book_action_from_cstr(ptr: *const c_char) -> BookAction {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        BookAction::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `BookAction` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn book_type_to_cstr(value: BookType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `BookType` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn book_type_from_cstr(ptr: *const c_char) -> BookType {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        BookType::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `BookType` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn contingency_type_to_cstr(value: ContingencyTypeOptional) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `ContingencyTypeOptional` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn contingency_type_from_cstr(ptr: *const c_char) -> ContingencyTypeOptional {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        ContingencyTypeOptional::from_str(value).unwrap_or_else(|_| {
            panic!("invalid `ContingencyTypeOptional` enum string value, was '{value}'")
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn currency_type_to_cstr(value: CurrencyType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `CurrencyType` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currency_type_from_cstr(ptr: *const c_char) -> CurrencyType {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        CurrencyType::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `CurrencyType` enum string value, was '{value}'"))
    })
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `InstrumentCloseType` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instrument_close_type_from_cstr(
    ptr: *const c_char,
) -> InstrumentCloseType {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        InstrumentCloseType::from_str(value).unwrap_or_else(|_| {
            panic!("invalid `InstrumentCloseType` enum string value, was '{value}'")
        })
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

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `LiquiditySide` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn liquidity_side_from_cstr(ptr: *const c_char) -> LiquiditySide {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        LiquiditySide::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `LiquiditySide` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn market_status_to_cstr(value: MarketStatus) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `MarketStatus` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn market_status_from_cstr(ptr: *const c_char) -> MarketStatus {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        MarketStatus::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `MarketStatus` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn market_status_action_to_cstr(value: MarketStatusAction) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `MarketStatusAction` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn market_status_action_from_cstr(ptr: *const c_char) -> MarketStatusAction {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        MarketStatusAction::from_str(value).unwrap_or_else(|_| {
            panic!("invalid `MarketStatusAction` enum string value, was '{value}'")
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn oms_type_to_cstr(value: OmsType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `OmsType` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oms_type_from_cstr(ptr: *const c_char) -> OmsType {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        OmsType::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `OmsType` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn option_kind_to_cstr(value: OptionKind) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `OptionKind` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn option_kind_from_cstr(ptr: *const c_char) -> OptionKind {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        OptionKind::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `OptionKind` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn oto_trigger_mode_to_cstr(value: OtoTriggerMode) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `OtoTriggerMode` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oto_trigger_mode_from_cstr(ptr: *const c_char) -> OtoTriggerMode {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        OtoTriggerMode::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `OtoTriggerMode` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn order_side_to_cstr(value: OrderSideOptional) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `OrderSideOptional` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn order_side_from_cstr(ptr: *const c_char) -> OrderSideOptional {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        OrderSideOptional::from_str(value).unwrap_or_else(|_| {
            panic!("invalid `OrderSideOptional` enum string value, was '{value}'")
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn order_status_to_cstr(value: OrderStatus) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `OrderStatus` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn order_status_from_cstr(ptr: *const c_char) -> OrderStatus {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        OrderStatus::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `OrderStatus` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn order_type_to_cstr(value: OrderType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `OrderType` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn order_type_from_cstr(ptr: *const c_char) -> OrderType {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        OrderType::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `OrderType` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn position_side_to_cstr(value: PositionSideOptional) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `PositionSideOptional` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn position_side_from_cstr(ptr: *const c_char) -> PositionSideOptional {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        PositionSideOptional::from_str(value).unwrap_or_else(|_| {
            panic!("invalid `PositionSideOptional` enum string value, was '{value}'")
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn position_adjustment_type_to_cstr(value: PositionAdjustmentType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `PositionAdjustmentType` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn position_adjustment_type_from_cstr(
    ptr: *const c_char,
) -> PositionAdjustmentType {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        PositionAdjustmentType::from_str(value).unwrap_or_else(|_| {
            panic!("invalid `PositionAdjustmentType` enum string value, was '{value}'")
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn price_type_to_cstr(value: PriceType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `PriceType` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn price_type_from_cstr(ptr: *const c_char) -> PriceType {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        PriceType::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `PriceType` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn record_flag_to_cstr(value: RecordFlag) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `RecordFlag` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn record_flag_from_cstr(ptr: *const c_char) -> RecordFlag {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        RecordFlag::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `RecordFlag` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn time_in_force_to_cstr(value: TimeInForce) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `TimeInForce` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn time_in_force_from_cstr(ptr: *const c_char) -> TimeInForce {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        TimeInForce::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `TimeInForce` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn trading_state_to_cstr(value: TradingState) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `TradingState` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trading_state_from_cstr(ptr: *const c_char) -> TradingState {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        TradingState::from_str(value)
            .unwrap_or_else(|_| panic!("invalid `TradingState` enum string value, was '{value}'"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn trailing_offset_type_to_cstr(value: TrailingOffsetTypeOptional) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `TrailingOffsetTypeOptional` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trailing_offset_type_from_cstr(
    ptr: *const c_char,
) -> TrailingOffsetTypeOptional {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        TrailingOffsetTypeOptional::from_str(value).unwrap_or_else(|_| {
            panic!("invalid `TrailingOffsetTypeOptional` enum string value, was '{value}'")
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn trigger_type_to_cstr(value: TriggerTypeOptional) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a C string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the C string does not correspond to a valid `TriggerTypeOptional` variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trigger_type_from_cstr(ptr: *const c_char) -> TriggerTypeOptional {
    abort_on_panic(|| {
        let value = unsafe { cstr_as_str(ptr) };
        TriggerTypeOptional::from_str(value).unwrap_or_else(|_| {
            panic!("invalid `TriggerTypeOptional` enum string value, was '{value}'")
        })
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::enums::OrderSide;

    #[rstest]
    fn test_name() {
        assert_eq!(OrderSideOptional::NoOrderSide.as_ref(), "NO_ORDER_SIDE");
        assert_eq!(OrderSide::Buy.as_ref(), "BUY");
        assert_eq!(OrderSide::Sell.as_ref(), "SELL");
    }

    #[rstest]
    fn test_value() {
        assert_eq!(OrderSideOptional::NoOrderSide as u8, 0);
        assert_eq!(OrderSide::Buy as u8, 1);
        assert_eq!(OrderSide::Sell as u8, 2);
    }

    #[rstest]
    #[case(None, ContingencyTypeOptional::NoContingency, 0)]
    #[case(Some(ContingencyType::Oco), ContingencyTypeOptional::Oco, 1)]
    #[case(Some(ContingencyType::Oto), ContingencyTypeOptional::Oto, 2)]
    #[case(Some(ContingencyType::Ouo), ContingencyTypeOptional::Ouo, 3)]
    fn test_contingency_type_optional_preserves_abi_values(
        #[case] value: Option<ContingencyType>,
        #[case] ffi_value: ContingencyTypeOptional,
        #[case] discriminant: u8,
    ) {
        assert_eq!(ContingencyTypeOptional::from(value), ffi_value);
        assert_eq!(ffi_value.as_option(), value);
        assert_eq!(ffi_value as u8, discriminant);
    }

    #[rstest]
    #[case(None, TrailingOffsetTypeOptional::NoTrailingOffset, 0)]
    #[case(Some(TrailingOffsetType::Price), TrailingOffsetTypeOptional::Price, 1)]
    #[case(
        Some(TrailingOffsetType::BasisPoints),
        TrailingOffsetTypeOptional::BasisPoints,
        2
    )]
    #[case(Some(TrailingOffsetType::Ticks), TrailingOffsetTypeOptional::Ticks, 3)]
    #[case(
        Some(TrailingOffsetType::PriceTier),
        TrailingOffsetTypeOptional::PriceTier,
        4
    )]
    fn test_trailing_offset_type_optional_preserves_abi_values(
        #[case] value: Option<TrailingOffsetType>,
        #[case] ffi_value: TrailingOffsetTypeOptional,
        #[case] discriminant: u8,
    ) {
        assert_eq!(TrailingOffsetTypeOptional::from(value), ffi_value);
        assert_eq!(ffi_value.as_option(), value);
        assert_eq!(ffi_value as u8, discriminant);
    }

    #[rstest]
    #[case(None, TriggerTypeOptional::NoTrigger, 0)]
    #[case(Some(TriggerType::Default), TriggerTypeOptional::Default, 1)]
    #[case(Some(TriggerType::LastPrice), TriggerTypeOptional::LastPrice, 2)]
    #[case(Some(TriggerType::MarkPrice), TriggerTypeOptional::MarkPrice, 3)]
    #[case(Some(TriggerType::IndexPrice), TriggerTypeOptional::IndexPrice, 4)]
    #[case(Some(TriggerType::BidAsk), TriggerTypeOptional::BidAsk, 5)]
    #[case(Some(TriggerType::DoubleLast), TriggerTypeOptional::DoubleLast, 6)]
    #[case(Some(TriggerType::DoubleBidAsk), TriggerTypeOptional::DoubleBidAsk, 7)]
    #[case(Some(TriggerType::LastOrBidAsk), TriggerTypeOptional::LastOrBidAsk, 8)]
    #[case(Some(TriggerType::MidPoint), TriggerTypeOptional::MidPoint, 9)]
    fn test_trigger_type_optional_preserves_abi_values(
        #[case] value: Option<TriggerType>,
        #[case] ffi_value: TriggerTypeOptional,
        #[case] discriminant: u8,
    ) {
        assert_eq!(TriggerTypeOptional::from(value), ffi_value);
        assert_eq!(ffi_value.as_option(), value);
        assert_eq!(ffi_value as u8, discriminant);
    }
}
