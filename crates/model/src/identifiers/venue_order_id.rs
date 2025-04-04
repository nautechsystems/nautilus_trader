// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Represents a valid venue order ID (assigned by a trading venue).

use std::{
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use nautilus_core::correctness::{FAILED, check_valid_string};
use ustr::Ustr;

/// Represents a valid venue order ID (assigned by a trading venue).
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct VenueOrderId(Ustr);

impl VenueOrderId {
    /// Creates a new [`VenueOrderId`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// This function returns an error:
    /// - If `value` is not a valid string.
    pub fn new_checked<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
        let value = value.as_ref();
        check_valid_string(value, stringify!(value))?;
        Ok(Self(Ustr::from(value)))
    }

    /// Creates a new [`VenueOrderId`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `value` is not a valid string.
    pub fn new<T: AsRef<str>>(value: T) -> Self {
        Self::new_checked(value).expect(FAILED)
    }

    /// Sets the inner identifier value.
    #[allow(dead_code)]
    pub(crate) fn set_inner(&mut self, value: &str) {
        self.0 = Ustr::from(value);
    }

    /// Returns the inner identifier value.
    #[must_use]
    pub fn inner(&self) -> Ustr {
        self.0
    }

    /// Returns the inner identifier value as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for VenueOrderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Display for VenueOrderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::identifiers::{stubs::*, venue_order_id::VenueOrderId};

    #[rstest]
    fn test_string_reprs(venue_order_id: VenueOrderId) {
        assert_eq!(venue_order_id.as_str(), "001");
        assert_eq!(format!("{venue_order_id}"), "001");
    }
}
