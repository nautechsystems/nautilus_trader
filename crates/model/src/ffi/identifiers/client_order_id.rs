use std::ffi::c_char;

use nautilus_core::ffi::string::cstr_as_str;

use crate::identifiers::ClientOrderId;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn client_order_id_new(ptr: *const c_char) -> ClientOrderId {
    let value = unsafe { cstr_as_str(ptr) };
    ClientOrderId::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn client_order_id_hash(id: &ClientOrderId) -> u64 {
    id.inner().precomputed_hash()
}
