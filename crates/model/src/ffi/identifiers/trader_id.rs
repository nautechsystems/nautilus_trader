use std::ffi::c_char;

use nautilus_core::ffi::string::cstr_as_str;

use crate::identifiers::trader_id::TraderId;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trader_id_new(ptr: *const c_char) -> TraderId {
    let value = unsafe { cstr_as_str(ptr) };
    TraderId::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn trader_id_hash(id: &TraderId) -> u64 {
    id.inner().precomputed_hash()
}
