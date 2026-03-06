use std::{
    collections::hash_map::DefaultHasher,
    ffi::{CString, c_char},
    hash::{Hash, Hasher},
};

use nautilus_core::StackStr;

use crate::identifiers::TradeId;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trade_id_new(ptr: *const c_char) -> TradeId {
    TradeId::new(unsafe { StackStr::from_c_ptr(ptr) }.as_str())
}

#[unsafe(no_mangle)]
pub extern "C" fn trade_id_hash(id: &TradeId) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

#[unsafe(no_mangle)]
pub extern "C" fn trade_id_to_cstr(trade_id: &TradeId) -> *const c_char {
    trade_id.as_cstr().as_ptr()
}

impl From<CString> for TradeId {
    fn from(value: CString) -> Self {
        Self::from_bytes(value.as_bytes_with_nul()).unwrap()
    }
}
