// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    ffi::{c_char, CStr},
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use anyhow::Result;
use nautilus_core::correctness::check_valid_string;
use pyo3::prelude::*;
use ustr::Ustr;

pub const SYNTHETIC_VENUE: &str = "SYNTH";

#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[pyclass]
pub struct Venue {
    pub value: Ustr,
}

impl Venue {
    pub fn new(s: &str) -> Result<Self> {
        check_valid_string(s, "`Venue` value")?;

        Ok(Self {
            value: Ustr::from(s),
        })
    }

    #[must_use]
    pub fn synthetic() -> Self {
        // Safety: using synethtic venue constant
        Self::new(SYNTHETIC_VENUE).unwrap()
    }

    pub fn is_synthetic(&self) -> bool {
        self.value.as_str() == SYNTHETIC_VENUE
    }
}

impl Default for Venue {
    fn default() -> Self {
        Self {
            value: Ustr::from("SIM"),
        }
    }
}

impl Debug for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for Venue {
    fn from(input: &str) -> Self {
        Self::new(input).unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn venue_new(ptr: *const c_char) -> Venue {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    Venue::from(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn venue_hash(id: &Venue) -> u64 {
    id.value.precomputed_hash()
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn venue_is_synthetic(venue: &Venue) -> u8 {
    u8::from(venue.is_synthetic())
}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use rstest::fixture;

    use crate::identifiers::venue::Venue;

    #[fixture]
    pub fn binance() -> Venue {
        Venue::from("BINANCE")
    }
    #[fixture]
    pub fn simulation() -> Venue {
        Venue::from("SIM")
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{stubs::*, Venue};

    #[rstest]
    fn test_string_reprs(binance: Venue) {
        assert_eq!(binance.to_string(), "BINANCE");
        assert_eq!(format!("{binance}"), "BINANCE");
    }
}
