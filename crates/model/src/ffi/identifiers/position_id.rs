use std::ffi::c_char;

use nautilus_core::ffi::string::cstr_as_str;

use crate::identifiers::position_id::PositionId;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn position_id_new(ptr: *const c_char) -> PositionId {
    let value = unsafe { cstr_as_str(ptr) };
    PositionId::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn position_id_hash(id: &PositionId) -> u64 {
    id.inner().precomputed_hash()
}
