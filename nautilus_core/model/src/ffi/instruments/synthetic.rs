// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    ffi::c_char,
    ops::{Deref, DerefMut},
};

use nautilus_core::{
    ffi::{
        cvec::CVec,
        parsing::{bytes_to_string_vec, string_vec_to_bytes},
        string::{cstr_to_str, str_to_cstr},
    },
    time::UnixNanos,
};

use crate::{
    identifiers::{instrument_id::InstrumentId, symbol::Symbol},
    instruments::synthetic::SyntheticInstrument,
    types::price::{Price, ERROR_PRICE},
};

/// Provides a C compatible Foreign Function Interface (FFI) for an underlying
/// [`SyntheticInstrument`].
///
/// This struct wraps `SyntheticInstrument` in a way that makes it compatible with C function
/// calls, enabling interaction with `SyntheticInstrument` in a C environment.
///
/// It implements the `Deref` trait, allowing instances of `SyntheticInstrument_API` to be
/// dereferenced to `SyntheticInstrument`, providing access to `SyntheticInstruments`'s methods without
/// having to manually access the underlying instance.
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct SyntheticInstrument_API(Box<SyntheticInstrument>);

impl Deref for SyntheticInstrument_API {
    type Target = SyntheticInstrument;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SyntheticInstrument_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// # Safety
///
/// - Assumes `components_ptr` is a valid C string pointer of a JSON format list of strings.
/// - Assumes `formula_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn synthetic_instrument_new(
    symbol: Symbol,
    price_precision: u8,
    components_ptr: *const c_char,
    formula_ptr: *const c_char,
    ts_event: u64,
    ts_init: u64,
) -> SyntheticInstrument_API {
    // TODO: There is absolutely no error handling here yet
    let components = bytes_to_string_vec(components_ptr)
        .into_iter()
        .map(|s| InstrumentId::from(s.as_str()))
        .collect::<Vec<InstrumentId>>();
    let formula = cstr_to_str(formula_ptr).to_string();
    let synth = SyntheticInstrument::new(
        symbol,
        price_precision,
        components,
        formula,
        ts_event,
        ts_init,
    );

    SyntheticInstrument_API(Box::new(synth.unwrap()))
}

#[no_mangle]
pub extern "C" fn synthetic_instrument_drop(synth: SyntheticInstrument_API) {
    drop(synth); // Memory freed here
}

#[no_mangle]
pub extern "C" fn synthetic_instrument_id(synth: &SyntheticInstrument_API) -> InstrumentId {
    synth.id
}

#[no_mangle]
pub extern "C" fn synthetic_instrument_price_precision(synth: &SyntheticInstrument_API) -> u8 {
    synth.price_precision
}

#[no_mangle]
pub extern "C" fn synthetic_instrument_price_increment(synth: &SyntheticInstrument_API) -> Price {
    synth.price_increment
}

#[no_mangle]
pub extern "C" fn synthetic_instrument_formula_to_cstr(
    synth: &SyntheticInstrument_API,
) -> *const c_char {
    str_to_cstr(&synth.formula)
}

#[no_mangle]
pub extern "C" fn synthetic_instrument_components_to_cstr(
    synth: &SyntheticInstrument_API,
) -> *const c_char {
    let components_vec = synth
        .components
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<String>>();

    string_vec_to_bytes(components_vec)
}

#[no_mangle]
pub extern "C" fn synthetic_instrument_components_count(synth: &SyntheticInstrument_API) -> usize {
    synth.components.len()
}

#[no_mangle]
pub extern "C" fn synthetic_instrument_ts_event(synth: &SyntheticInstrument_API) -> UnixNanos {
    synth.ts_event
}

#[no_mangle]
pub extern "C" fn synthetic_instrument_ts_init(synth: &SyntheticInstrument_API) -> UnixNanos {
    synth.ts_init
}

/// # Safety
///
/// - Assumes `formula_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn synthetic_instrument_is_valid_formula(
    synth: &SyntheticInstrument_API,
    formula_ptr: *const c_char,
) -> u8 {
    if formula_ptr.is_null() {
        return u8::from(false);
    }
    let formula = cstr_to_str(formula_ptr);
    u8::from(synth.is_valid_formula(formula))
}

/// # Safety
///
/// - Assumes `formula_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn synthetic_instrument_change_formula(
    synth: &mut SyntheticInstrument_API,
    formula_ptr: *const c_char,
) {
    let formula = cstr_to_str(formula_ptr);
    synth.change_formula(formula.to_string()).unwrap();
}

#[no_mangle]
pub extern "C" fn synthetic_instrument_calculate(
    synth: &mut SyntheticInstrument_API,
    inputs_ptr: &CVec,
) -> Price {
    let CVec { ptr, len, .. } = inputs_ptr;
    let inputs: &[f64] = unsafe { std::slice::from_raw_parts((*ptr).cast::<f64>(), *len) };

    match synth.calculate(inputs) {
        Ok(price) => price,
        Err(_) => ERROR_PRICE,
    }
}
