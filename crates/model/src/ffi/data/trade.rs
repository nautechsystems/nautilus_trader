use std::{
    collections::hash_map::DefaultHasher,
    ffi::c_char,
    hash::{Hash, Hasher},
};

use nautilus_core::ffi::string::str_to_cstr;

use crate::{
    data::TradeTick,
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn trade_tick_new(
    instrument_id: InstrumentId,
    price: Price,
    size: Quantity,
    aggressor_side: AggressorSide,
    trade_id: TradeId,
    ts_event: u64,
    ts_init: u64,
) -> TradeTick {
    TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event.into(),
        ts_init.into(),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn trade_tick_eq(lhs: &TradeTick, rhs: &TradeTick) -> u8 {
    u8::from(lhs == rhs)
}

#[unsafe(no_mangle)]
pub extern "C" fn trade_tick_hash(delta: &TradeTick) -> u64 {
    let mut hasher = DefaultHasher::new();
    delta.hash(&mut hasher);
    hasher.finish()
}

/// Returns a [`TradeTick`] as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn trade_tick_to_cstr(trade: &TradeTick) -> *const c_char {
    str_to_cstr(&trade.to_string())
}
