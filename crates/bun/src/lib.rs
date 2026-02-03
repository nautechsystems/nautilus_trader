// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Bun/TypeScript FFI aggregator for NautilusTrader.
//!
//! Compiles to a single `cdylib` shared library for use with Bun's `dlopen`.
//! All `#[no_mangle] extern "C"` symbols from dependent crates are exported
//! through the linker.
//!
//! This crate also provides heap-allocating wrapper functions (prefixed `bun_`)
//! for Rust FFI functions that return structs by value. Bun's FFI can only handle
//! pointer-sized returns, so any struct > 8 bytes must be heap-allocated and
//! returned as a pointer, with a corresponding `_drop` function for cleanup.

#![allow(unsafe_code)]
#![allow(unused_imports)]

use std::ffi::{c_char, CStr};

// Re-export FFI modules so the linker includes all symbols
pub use nautilus_common::ffi as common_ffi;
pub use nautilus_core::ffi as core_ffi;
pub use nautilus_model::ffi as model_ffi;

use nautilus_core::UUID4;
use nautilus_core::ffi::string::{cstr_as_str, str_to_cstr};
use nautilus_model::identifiers::{InstrumentId, Symbol, TradeId, Venue};
use nautilus_model::types::{Currency, Money, Price, Quantity};

// -- Symbol / Venue to_cstr helpers ------------------------------------------
// Symbol and Venue are Ustr (8 bytes) so they return fine by value from FFI.
// But we need a way to get the full string value as a C string.

#[unsafe(no_mangle)]
pub extern "C" fn bun_symbol_to_cstr(symbol: &Symbol) -> *const c_char {
    str_to_cstr(&symbol.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_venue_to_cstr(venue: &Venue) -> *const c_char {
    str_to_cstr(&venue.to_string())
}

// Tokio runtime for async operations
static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

/// Initialize the Nautilus runtime. Must be called before any other FFI function.
/// Returns 1 on success.
#[unsafe(no_mangle)]
pub extern "C" fn nautilus_init() -> u8 {
    let _ = RUNTIME.get_or_init(|| {
        tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime")
    });
    1
}

/// Shutdown the Nautilus runtime.
#[unsafe(no_mangle)]
pub extern "C" fn nautilus_shutdown() {}

// =============================================================================
// Heap-allocating wrapper functions for Bun FFI compatibility.
//
// These wrap the existing by-value FFI functions, boxing the result on the heap
// and returning a raw pointer. Each has a corresponding `_drop` function.
// =============================================================================

// -- UUID4 (37 bytes) ---------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn bun_uuid4_new() -> *mut UUID4 {
    Box::into_raw(Box::new(UUID4::new()))
}

/// # Safety
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_uuid4_from_cstr(ptr: *const c_char) -> *mut UUID4 {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    let cstr = unsafe { CStr::from_ptr(ptr) };
    let value = cstr.to_str().expect("Failed to convert C string to UTF-8");
    Box::into_raw(Box::new(UUID4::from(value)))
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_uuid4_to_cstr(uuid: &UUID4) -> *const c_char {
    uuid.to_cstr().as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_uuid4_eq(lhs: &UUID4, rhs: &UUID4) -> u8 {
    u8::from(lhs == rhs)
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_uuid4_hash(uuid: &UUID4) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    uuid.hash(&mut h);
    h.finish()
}

/// # Safety
/// Assumes `ptr` was allocated by a `bun_uuid4_*` function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_uuid4_drop(ptr: *mut UUID4) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

// -- InstrumentId (16 bytes) --------------------------------------------------

/// # Safety
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_instrument_id_from_cstr(ptr: *const c_char) -> *mut InstrumentId {
    let value = unsafe { cstr_as_str(ptr) };
    Box::into_raw(Box::new(InstrumentId::from(value)))
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_instrument_id_to_cstr(id: &InstrumentId) -> *const c_char {
    str_to_cstr(&id.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_instrument_id_hash(id: &InstrumentId) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    h.finish()
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_instrument_id_is_synthetic(id: &InstrumentId) -> u8 {
    u8::from(id.is_synthetic())
}

/// # Safety
/// Assumes `ptr` was allocated by a `bun_instrument_id_*` function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_instrument_id_drop(ptr: *mut InstrumentId) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

// -- TradeId (38 bytes) -------------------------------------------------------

/// # Safety
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_trade_id_new(ptr: *const c_char) -> *mut TradeId {
    let value = unsafe { cstr_as_str(ptr) };
    Box::into_raw(Box::new(TradeId::from(value)))
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_trade_id_to_cstr(id: &TradeId) -> *const c_char {
    str_to_cstr(&id.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_trade_id_hash(id: &TradeId) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    h.finish()
}

/// # Safety
/// Assumes `ptr` was allocated by a `bun_trade_id_*` function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_trade_id_drop(ptr: *mut TradeId) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

// -- Price (16 bytes) ---------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn bun_price_new(value: f64, precision: u8) -> *mut Price {
    Box::into_raw(Box::new(Price::new(value, precision)))
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_price_as_f64(price: &Price) -> f64 {
    price.as_f64()
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_price_precision(price: &Price) -> u8 {
    price.precision
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_price_to_cstr(price: &Price) -> *const c_char {
    str_to_cstr(&price.to_string())
}

/// # Safety
/// Assumes `ptr` was allocated by a `bun_price_*` function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_price_drop(ptr: *mut Price) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

// -- Quantity (16 bytes) ------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn bun_quantity_new(value: f64, precision: u8) -> *mut Quantity {
    Box::into_raw(Box::new(Quantity::new(value, precision)))
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_quantity_as_f64(qty: &Quantity) -> f64 {
    qty.as_f64()
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_quantity_precision(qty: &Quantity) -> u8 {
    qty.precision
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_quantity_to_cstr(qty: &Quantity) -> *const c_char {
    str_to_cstr(&qty.to_string())
}

/// # Safety
/// Assumes `ptr` was allocated by a `bun_quantity_*` function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_quantity_drop(ptr: *mut Quantity) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

// -- Currency (32 bytes) ------------------------------------------------------

/// # Safety
/// Assumes `code_ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_currency_from_cstr(code_ptr: *const c_char) -> *mut Currency {
    use std::str::FromStr;
    let code = unsafe { cstr_as_str(code_ptr) };
    Box::into_raw(Box::new(Currency::from_str(code).unwrap()))
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_currency_code_to_cstr(currency: &Currency) -> *const c_char {
    str_to_cstr(&currency.code)
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_currency_name_to_cstr(currency: &Currency) -> *const c_char {
    str_to_cstr(&currency.name)
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_currency_precision(currency: &Currency) -> u8 {
    currency.precision
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_currency_hash(currency: &Currency) -> u64 {
    currency.code.precomputed_hash()
}

/// # Safety
/// Assumes `code_ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_currency_exists(code_ptr: *const c_char) -> u8 {
    use nautilus_model::currencies::CURRENCY_MAP;
    let code = unsafe { cstr_as_str(code_ptr) };
    u8::from(CURRENCY_MAP.lock().unwrap().contains_key(code))
}

/// # Safety
/// Assumes `ptr` was allocated by a `bun_currency_*` function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_currency_drop(ptr: *mut Currency) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

// -- Money (40 bytes) ---------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn bun_money_new(amount: f64, currency: &Currency) -> *mut Money {
    Box::into_raw(Box::new(Money::new(amount, *currency)))
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_money_as_f64(money: &Money) -> f64 {
    money.as_f64()
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_money_to_cstr(money: &Money) -> *const c_char {
    str_to_cstr(&money.to_string())
}

/// # Safety
/// Assumes `ptr` was allocated by a `bun_money_*` function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_money_drop(ptr: *mut Money) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

// -- TestClock_API / LiveClock_API (8 bytes each — Box<Clock>) ----------------
// The upstream FFI returns these by value and takes `&T` references.
// We heap-allocate them so Bun always has a stable pointer.

use nautilus_common::ffi::clock::{TestClock_API, LiveClock_API};

#[unsafe(no_mangle)]
pub extern "C" fn bun_test_clock_new() -> *mut TestClock_API {
    let api = common_ffi::clock::test_clock_new();
    Box::into_raw(Box::new(api))
}

/// # Safety
/// Assumes `ptr` was allocated by `bun_test_clock_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_test_clock_drop(ptr: *mut TestClock_API) {
    if !ptr.is_null() {
        // Drop the outer Box, which drops TestClock_API, which drops the inner Box<TestClock>
        drop(unsafe { Box::from_raw(ptr) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bun_live_clock_new() -> *mut LiveClock_API {
    let api = common_ffi::clock::live_clock_new();
    Box::into_raw(Box::new(api))
}

/// # Safety
/// Assumes `ptr` was allocated by `bun_live_clock_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bun_live_clock_drop(ptr: *mut LiveClock_API) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

// =============================================================================
// Force linker to include all FFI symbols from dependent crates
// =============================================================================

#[unsafe(no_mangle)]
pub extern "C" fn _nautilus_force_link() {
    // Core FFI symbols
    let _ = core_ffi::uuid::uuid4_new as *const ();
    let _ = core_ffi::datetime::secs_to_nanos as *const ();
    let _ = core_ffi::datetime::nanos_to_secs as *const ();
    let _ = core_ffi::datetime::millis_to_nanos as *const ();
    let _ = core_ffi::datetime::micros_to_nanos as *const ();
    let _ = core_ffi::datetime::nanos_to_millis as *const ();
    let _ = core_ffi::datetime::nanos_to_micros as *const ();
    let _ = core_ffi::datetime::secs_to_millis as *const ();
    let _ = core_ffi::datetime::unix_nanos_to_iso8601_cstr as *const ();
    let _ = core_ffi::datetime::unix_nanos_to_iso8601_millis_cstr as *const ();
    let _ = core_ffi::string::cstr_drop as *const ();

    // Model FFI symbols - enums
    let _ = model_ffi::enums::account_type_to_cstr as *const ();
    let _ = model_ffi::enums::order_side_to_cstr as *const ();
    let _ = model_ffi::enums::order_status_to_cstr as *const ();
    let _ = model_ffi::enums::order_type_to_cstr as *const ();
    let _ = model_ffi::enums::position_side_to_cstr as *const ();

    // Model FFI symbols - identifiers
    let _ = model_ffi::identifiers::symbol::symbol_new as *const ();
    let _ = model_ffi::identifiers::symbol::symbol_hash as *const ();
    let _ = model_ffi::identifiers::symbol::symbol_is_composite as *const ();
    let _ = model_ffi::identifiers::symbol::symbol_root as *const ();
    let _ = model_ffi::identifiers::symbol::symbol_topic as *const ();
    let _ = model_ffi::identifiers::venue::venue_new as *const ();
    let _ = model_ffi::identifiers::venue::venue_hash as *const ();
    let _ = model_ffi::identifiers::venue::venue_is_synthetic as *const ();
    let _ = model_ffi::identifiers::instrument_id::instrument_id_new as *const ();
    let _ = model_ffi::identifiers::instrument_id::instrument_id_to_cstr as *const ();
    let _ = model_ffi::identifiers::trader_id::trader_id_new as *const ();
    let _ = model_ffi::identifiers::account_id::account_id_new as *const ();
    let _ = model_ffi::identifiers::client_id::client_id_new as *const ();
    let _ = model_ffi::identifiers::client_order_id::client_order_id_new as *const ();
    let _ = model_ffi::identifiers::component_id::component_id_new as *const ();
    let _ = model_ffi::identifiers::exec_algorithm_id::exec_algorithm_id_new as *const ();
    let _ = model_ffi::identifiers::order_list_id::order_list_id_new as *const ();
    let _ = model_ffi::identifiers::position_id::position_id_new as *const ();
    let _ = model_ffi::identifiers::strategy_id::strategy_id_new as *const ();
    let _ = model_ffi::identifiers::trade_id::trade_id_new as *const ();
    let _ = model_ffi::identifiers::venue_order_id::venue_order_id_new as *const ();

    // Model FFI symbols - types
    let _ = model_ffi::types::price::price_new as *const ();
    let _ = model_ffi::types::quantity::quantity_new as *const ();
    let _ = model_ffi::types::money::money_new as *const ();
    let _ = model_ffi::types::currency::currency_from_cstr as *const ();

    // Common FFI symbols - TestClock
    let _ = common_ffi::clock::test_clock_new as *const ();
    let _ = common_ffi::clock::test_clock_drop as *const ();
    let _ = common_ffi::clock::test_clock_set_time as *const ();
    let _ = common_ffi::clock::test_clock_timestamp as *const ();
    let _ = common_ffi::clock::test_clock_timestamp_ms as *const ();
    let _ = common_ffi::clock::test_clock_timestamp_us as *const ();
    let _ = common_ffi::clock::test_clock_timestamp_ns as *const ();
    let _ = common_ffi::clock::test_clock_timer_names as *const ();
    let _ = common_ffi::clock::test_clock_timer_count as *const ();
    let _ = common_ffi::clock::test_clock_next_time as *const ();
    let _ = common_ffi::clock::test_clock_cancel_timer as *const ();
    let _ = common_ffi::clock::test_clock_cancel_timers as *const ();

    // Common FFI symbols - LiveClock
    let _ = common_ffi::clock::live_clock_new as *const ();
    let _ = common_ffi::clock::live_clock_drop as *const ();
    let _ = common_ffi::clock::live_clock_timestamp as *const ();
    let _ = common_ffi::clock::live_clock_timestamp_ms as *const ();
    let _ = common_ffi::clock::live_clock_timestamp_us as *const ();
    let _ = common_ffi::clock::live_clock_timestamp_ns as *const ();
    let _ = common_ffi::clock::live_clock_timer_names as *const ();
    let _ = common_ffi::clock::live_clock_timer_count as *const ();
    let _ = common_ffi::clock::live_clock_next_time as *const ();
    let _ = common_ffi::clock::live_clock_cancel_timer as *const ();
    let _ = common_ffi::clock::live_clock_cancel_timers as *const ();
}
