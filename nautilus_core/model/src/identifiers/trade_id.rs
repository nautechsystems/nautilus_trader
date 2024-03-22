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
    ffi::{CStr, CString},
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use nautilus_core::correctness::{check_in_range_inclusive_usize, check_valid_string};
use serde::{Deserialize, Deserializer, Serialize};

/// The maximum length of ASCII characters for a `TradeId` string value (including null terminator).
const TRADE_ID_LEN: usize = 37;

/// Represents a valid trade match ID (assigned by a trading venue).
///
/// Maximum length is 36 characters.
///
/// The unique ID assigned to the trade entity once it is received or matched by
/// the exchange or central counterparty.
///
/// Can correspond to the `TradeID <1003> field` of the FIX protocol.
#[repr(C)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct TradeId {
    /// The trade match ID value as a fixed-length C string byte array (includes null terminator).
    pub(crate) value: [u8; 37], // cbindgen issue using the constant in the array
}

impl TradeId {
    pub fn new(value: &str) -> anyhow::Result<Self> {
        let cstr = CString::new(value).expect("`CString` conversion failed");
        Self::from_cstr(cstr)
    }

    pub fn from_cstr(cstr: CString) -> anyhow::Result<Self> {
        let bytes = cstr.as_bytes_with_nul();
        check_valid_string(cstr.to_str()?, stringify!(cstr))?;
        check_in_range_inclusive_usize(bytes.len(), 2, TRADE_ID_LEN, stringify!(cstr))?;

        let mut value = [0; TRADE_ID_LEN];
        value[..bytes.len()].copy_from_slice(bytes);

        Ok(Self { value })
    }

    #[must_use]
    pub fn to_cstr(&self) -> &CStr {
        // SAFETY: Unwrap safe as we always store valid C strings
        // We use until nul because the values array may be padded with nul bytes
        CStr::from_bytes_until_nul(&self.value).unwrap()
    }
}

impl Default for TradeId {
    fn default() -> Self {
        Self::from("1")
    }
}

impl Display for TradeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_cstr().to_str().unwrap())
    }
}

impl From<&str> for TradeId {
    fn from(input: &str) -> Self {
        Self::new(input).unwrap()
    }
}

impl Serialize for TradeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TradeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value_str = String::deserialize(deserializer)?;
        Self::new(&value_str).map_err(|err| serde::de::Error::custom(err.to_string()))
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::identifiers::{stubs::*, trade_id::TradeId};

    #[rstest]
    fn test_string_reprs(trade_id: TradeId) {
        assert_eq!(trade_id.to_string(), "1234567890");
        assert_eq!(format!("{trade_id}"), "1234567890");
    }
}
