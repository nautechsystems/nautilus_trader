use std::ffi::c_char;

use nautilus_core::{MUTEX_POISONED, ffi::string::cstr_as_str};

use crate::{identifiers::Venue, venues::VENUE_MAP};

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn venue_new(ptr: *const c_char) -> Venue {
    let value = unsafe { cstr_as_str(ptr) };
    Venue::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn venue_hash(id: &Venue) -> u64 {
    id.inner().precomputed_hash()
}

#[unsafe(no_mangle)]
pub extern "C" fn venue_is_synthetic(venue: &Venue) -> u8 {
    u8::from(venue.is_synthetic())
}

/// Checks if a venue code exists in the internal map.
///
/// # Safety
///
/// Assumes `code_ptr` is a valid NUL-terminated UTF-8 C string pointer.
///
/// # Panics
///
/// Panics if the internal mutex `VENUE_MAP` is poisoned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn venue_code_exists(code_ptr: *const c_char) -> u8 {
    let code = unsafe { cstr_as_str(code_ptr) };
    u8::from(VENUE_MAP.lock().expect(MUTEX_POISONED).contains_key(code))
}

/// Converts a UTF-8 C string pointer to a `Venue`.
///
/// # Safety
///
/// Assumes `code_ptr` is a valid NUL-terminated UTF-8 C string pointer.
///
/// # Panics
///
/// Panics if the code is not found or invalid (unwrap on `from_code`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn venue_from_cstr_code(code_ptr: *const c_char) -> Venue {
    let code = unsafe { cstr_as_str(code_ptr) };
    Venue::from_code(code).unwrap()
}
