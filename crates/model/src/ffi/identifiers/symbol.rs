use std::ffi::c_char;

use nautilus_core::ffi::string::{cstr_as_str, str_to_cstr};

use crate::identifiers::Symbol;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn symbol_new(ptr: *const c_char) -> Symbol {
    let value = unsafe { cstr_as_str(ptr) };
    Symbol::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn symbol_hash(id: &Symbol) -> u64 {
    id.inner().precomputed_hash()
}

#[unsafe(no_mangle)]
pub extern "C" fn symbol_is_composite(id: &Symbol) -> u8 {
    u8::from(id.is_composite())
}

#[unsafe(no_mangle)]
pub extern "C" fn symbol_root(id: &Symbol) -> *const c_char {
    str_to_cstr(id.root())
}

#[unsafe(no_mangle)]
pub extern "C" fn symbol_topic(id: &Symbol) -> *const c_char {
    str_to_cstr(&id.topic())
}
