use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::UnixNanos;

use crate::{
    data::{BookOrder, OrderBookDelta},
    enums::BookAction,
    identifiers::InstrumentId,
};

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_delta_new(
    instrument_id: InstrumentId,
    action: BookAction,
    order: BookOrder,
    flags: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_delta_eq(lhs: &OrderBookDelta, rhs: &OrderBookDelta) -> u8 {
    u8::from(lhs == rhs)
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_delta_hash(delta: &OrderBookDelta) -> u64 {
    let mut hasher = DefaultHasher::new();
    delta.hash(&mut hasher);
    hasher.finish()
}
