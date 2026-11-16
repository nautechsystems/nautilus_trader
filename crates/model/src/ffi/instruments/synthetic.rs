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

use std::ffi::c_char;

use nautilus_core::{
    UnixNanos,
    ffi::{
        abort_on_panic,
        cvec::CVec,
        parsing::{bytes_to_string_vec, string_vec_to_bytes},
        string::{cstr_as_str, str_to_cstr},
    },
};

use crate::{
    identifiers::{InstrumentId, Symbol},
    instruments::synthetic::SyntheticInstrument,
    types::{ERROR_PRICE, Price},
};

/// Creates a new [`SyntheticInstrument`] from the given components and formula.
///
/// # Panics
///
/// Panics if:
/// - The components string contains invalid UTF-8 or JSON, or is not an array of instrument IDs.
/// - An instrument construction check fails, including formula validation.
///
/// # Safety
///
/// This function assumes:
/// - `components_ptr` is a valid C string pointer.
/// - `formula_ptr` is a valid C string pointer.
///
/// Returns an owning pointer to the heap-allocated `SyntheticInstrument` which the
/// caller must eventually pass to [`synthetic_instrument_drop`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn synthetic_instrument_new(
    symbol: Symbol,
    price_precision: u8,
    components_ptr: *const c_char,
    formula_ptr: *const c_char,
    ts_event: u64,
    ts_init: u64,
) -> *mut SyntheticInstrument {
    // TODO: There is absolutely no error handling here yet
    let components = unsafe { bytes_to_string_vec(components_ptr) }
        .expect("Invalid synthetic instrument components JSON")
        .into_iter()
        .map(InstrumentId::from)
        .collect::<Vec<InstrumentId>>();
    let formula = unsafe { cstr_as_str(formula_ptr) };
    let synth = SyntheticInstrument::builder()
        .symbol(symbol)
        .price_precision(price_precision)
        .components(components)
        .formula(formula)
        .ts_event(ts_event.into())
        .ts_init(ts_init.into())
        .build()
        .unwrap();

    Box::into_raw(Box::new(synth))
}

/// # Safety
///
/// `synth` must be a live owning pointer returned by [`synthetic_instrument_new`],
/// and must not be used after this call.
///
/// # Panics
///
/// Panics if `synth` is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn synthetic_instrument_drop(synth: *mut SyntheticInstrument) {
    abort_on_panic(|| {
        assert!(!synth.is_null(), "`synth` was NULL");
        // SAFETY: Caller guarantees `synth` was allocated by `synthetic_instrument_new`
        drop(unsafe { Box::from_raw(synth) }); // Memory freed here
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn synthetic_instrument_id(synth: &SyntheticInstrument) -> InstrumentId {
    synth.id
}

#[unsafe(no_mangle)]
pub extern "C" fn synthetic_instrument_price_precision(synth: &SyntheticInstrument) -> u8 {
    synth.price_precision
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn synthetic_instrument_price_increment(synth: &SyntheticInstrument) -> Price {
    synth.price_increment
}

#[unsafe(no_mangle)]
pub extern "C" fn synthetic_instrument_formula_to_cstr(
    synth: &SyntheticInstrument,
) -> *const c_char {
    str_to_cstr(&synth.formula)
}

#[unsafe(no_mangle)]
pub extern "C" fn synthetic_instrument_components_to_cstr(
    synth: &SyntheticInstrument,
) -> *const c_char {
    let components_vec = synth
        .components
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<String>>();

    string_vec_to_bytes(&components_vec)
}

#[unsafe(no_mangle)]
pub extern "C" fn synthetic_instrument_components_count(synth: &SyntheticInstrument) -> usize {
    synth.components.len()
}

#[unsafe(no_mangle)]
pub extern "C" fn synthetic_instrument_ts_event(synth: &SyntheticInstrument) -> UnixNanos {
    synth.ts_event
}

#[unsafe(no_mangle)]
pub extern "C" fn synthetic_instrument_ts_init(synth: &SyntheticInstrument) -> UnixNanos {
    synth.ts_init
}

/// Returns 1 if the formula is valid for the components, otherwise 0.
///
/// Null pointers and invalid components JSON return 0.
///
/// # Panics
///
/// Panics if:
/// - The formula string contains invalid UTF-8.
/// - A decoded component is not a valid instrument ID.
///
/// # Safety
///
/// Non-null pointers must reference valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn synthetic_instrument_is_valid_formula(
    formula_ptr: *const c_char,
    components_ptr: *const c_char,
) -> u8 {
    if formula_ptr.is_null() || components_ptr.is_null() {
        return 0;
    }

    let Ok(components) = (unsafe { bytes_to_string_vec(components_ptr) }) else {
        return 0;
    };

    let components = components
        .into_iter()
        .map(InstrumentId::from)
        .collect::<Vec<InstrumentId>>();

    let formula = unsafe { cstr_as_str(formula_ptr) };

    u8::from(SyntheticInstrument::is_valid_formula_for_components(
        formula,
        &components,
    ))
}

/// # Safety
///
/// Assumes `formula_ptr` is a valid C string pointer.
///
/// # Panics
///
/// Panics if changing the formula fails (i.e., `unwrap()` in `change_formula`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn synthetic_instrument_change_formula(
    synth: &mut SyntheticInstrument,
    formula_ptr: *const c_char,
) {
    let formula = unsafe { cstr_as_str(formula_ptr) };
    synth.change_formula(formula).unwrap();
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
/// # Safety
///
/// `inputs_ptr` must describe initialized `f64` values that remain valid and immutable for the
/// duration of this call.
pub unsafe extern "C" fn synthetic_instrument_calculate(
    synth: &mut SyntheticInstrument,
    inputs_ptr: &CVec,
) -> Price {
    let inputs = unsafe { inputs_ptr.as_slice::<f64>() };

    match synth.calculate(inputs) {
        Ok(price) => price,
        Err(_) => ERROR_PRICE,
    }
}

#[cfg(test)]
mod cvec_tests {
    use std::ffi::CString;

    use nautilus_core::ffi::string::cstr_drop;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_synthetic_formula_accepts_valid_components_json() {
        let synth = SyntheticInstrument::default();
        let formula = CString::new(synth.formula.as_str()).unwrap();
        let components = synth
            .components
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let components_ptr = string_vec_to_bytes(&components);

        let valid =
            unsafe { synthetic_instrument_is_valid_formula(formula.as_ptr(), components_ptr) };
        unsafe { cstr_drop(components_ptr) };

        assert_eq!(valid, 1);
    }

    #[rstest]
    #[case::empty(Vec::new())]
    #[case::invalid_utf8(vec![0xff])]
    #[case::invalid_utf8_string(vec![b'[', b'"', 0xff, b'"', b']'])]
    fn test_synthetic_formula_rejects_invalid_components_json(#[case] bytes: Vec<u8>) {
        let components = CString::new(bytes).unwrap();

        let valid = unsafe {
            synthetic_instrument_is_valid_formula(c"BTCUSDT.BINANCE".as_ptr(), components.as_ptr())
        };

        assert_eq!(valid, 0);
    }

    #[rstest]
    fn test_synthetic_calculate_borrows_inputs() {
        let mut synth = SyntheticInstrument::default();
        let mut inputs = vec![100.0, 200.0];
        let cvec = CVec {
            ptr: inputs.as_mut_ptr().cast(),
            len: inputs.len(),
            cap: inputs.capacity(),
        };

        let price = unsafe { synthetic_instrument_calculate(&mut synth, &cvec) };

        assert_eq!(price, Price::from("150.0"));
        assert_eq!(inputs, [100.0, 200.0]);
    }
}
