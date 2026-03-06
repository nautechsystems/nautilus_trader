use std::ffi::c_char;

use nautilus_core::ffi::string::cstr_as_str;

use crate::identifiers::exec_algorithm_id::ExecAlgorithmId;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn exec_algorithm_id_new(ptr: *const c_char) -> ExecAlgorithmId {
    let value = unsafe { cstr_as_str(ptr) };
    ExecAlgorithmId::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn exec_algorithm_id_hash(id: &ExecAlgorithmId) -> u64 {
    id.inner().precomputed_hash()
}
