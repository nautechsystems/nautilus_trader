use std::{ffi::c_char, str::FromStr};

use nautilus_core::ffi::string::{cstr_as_str, str_to_cstr};

use crate::enums::{ComponentState, ComponentTrigger, LogColor, LogLevel};

#[unsafe(no_mangle)]
pub extern "C" fn component_state_to_cstr(value: ComponentState) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the input C string does not match a valid enum variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn component_state_from_cstr(ptr: *const c_char) -> ComponentState {
    let value = unsafe { cstr_as_str(ptr) };
    ComponentState::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `ComponentState` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn component_trigger_to_cstr(value: ComponentTrigger) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the input C string does not match a valid enum variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn component_trigger_from_cstr(ptr: *const c_char) -> ComponentTrigger {
    let value = unsafe { cstr_as_str(ptr) };
    ComponentTrigger::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `ComponentTrigger` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn log_level_to_cstr(value: LogLevel) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the input C string does not match a valid enum variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn log_level_from_cstr(ptr: *const c_char) -> LogLevel {
    let value = unsafe { cstr_as_str(ptr) };
    LogLevel::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `LogLevel` enum string value, was '{value}'"))
}

#[unsafe(no_mangle)]
pub extern "C" fn log_color_to_cstr(value: LogColor) -> *const c_char {
    str_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if the input C string does not match a valid enum variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn log_color_from_cstr(ptr: *const c_char) -> LogColor {
    let value = unsafe { cstr_as_str(ptr) };
    LogColor::from_str(value)
        .unwrap_or_else(|_| panic!("invalid `LogColor` enum string value, was '{value}'"))
}
