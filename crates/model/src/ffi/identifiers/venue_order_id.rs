use std::ffi::c_char;

use nautilus_core::ffi::string::cstr_as_str;

use crate::identifiers::venue_order_id::VenueOrderId;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn venue_order_id_new(ptr: *const c_char) -> VenueOrderId {
    let value = unsafe { cstr_as_str(ptr) };
    VenueOrderId::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn venue_order_id_hash(id: &VenueOrderId) -> u64 {
    id.inner().precomputed_hash()
}
