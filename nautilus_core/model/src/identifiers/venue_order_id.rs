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
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use nautilus_core::correctness::check_valid_string;
use ustr::Ustr;

/// Represents a valid venue order ID (assigned by a trading venue).
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct VenueOrderId {
    /// The venue assigned order ID value.
    pub value: Ustr,
}

impl VenueOrderId {
    pub fn new(value: &str) -> anyhow::Result<Self> {
        check_valid_string(value, stringify!(value))?;

        Ok(Self {
            value: Ustr::from(value),
        })
    }
}

impl Default for VenueOrderId {
    fn default() -> Self {
        Self {
            value: Ustr::from("001"),
        }
    }
}

impl Debug for VenueOrderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for VenueOrderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for VenueOrderId {
    fn from(input: &str) -> Self {
        Self::new(input).unwrap()
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
        assert_eq!(venue_order_id.to_string(), "001");
        assert_eq!(format!("{venue_order_id}"), "001");
    }
}
