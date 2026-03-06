use std::ffi::c_char;

use nautilus_core::ffi::string::cstr_as_str;

use crate::identifiers::order_list_id::OrderListId;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn order_list_id_new(ptr: *const c_char) -> OrderListId {
    let value = unsafe { cstr_as_str(ptr) };
    OrderListId::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn order_list_id_hash(id: &OrderListId) -> u64 {
    id.inner().precomputed_hash()
}
