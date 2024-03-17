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
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// The maximum length of ASCII characters for a `UUID4` string value (includes null terminator).
const UUID4_LEN: usize = 37;

/// Represents a pseudo-random UUID (universally unique identifier)
/// version 4 based on a 128-bit label as specified in RFC 4122.
#[repr(C)]
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.core")
)]
pub struct UUID4 {
    /// The UUID v4 value as a fixed-length C string byte array (includes null terminator).
    pub(crate) value: [u8; 37], // cbindgen issue using the constant in the array
}

impl UUID4 {
    #[must_use]
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        let c_string = CString::new(uuid.to_string()).expect("`CString` conversion failed");
        let bytes = c_string.as_bytes_with_nul();
        let mut value = [0; UUID4_LEN];
        value[..bytes.len()].copy_from_slice(bytes);

        Self { value }
    }

    #[must_use]
    pub fn to_cstr(&self) -> &CStr {
        // SAFETY: We always store valid C strings
        CStr::from_bytes_with_nul(&self.value).unwrap()
    }
}

impl FromStr for UUID4 {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(s).map_err(|_| "Invalid UUID string")?;
        let c_string = CString::new(uuid.to_string()).expect("`CString` conversion failed");
        let bytes = c_string.as_bytes_with_nul();
        let mut value = [0; UUID4_LEN];
        value[..bytes.len()].copy_from_slice(bytes);

        Ok(Self { value })
    }
}

impl From<&str> for UUID4 {
    fn from(input: &str) -> Self {
        input.parse().unwrap_or_else(|err| panic!("{}", err))
    }
}

impl Default for UUID4 {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_cstr().to_string_lossy())
    }
}

impl Serialize for UUID4 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for UUID4 {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let uuid4_str: &str = Deserialize::deserialize(_deserializer)?;
        let uuid4: Self = uuid4_str.into();
        Ok(uuid4)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::*;
    use uuid;

    use super::*;

    #[rstest]
    fn test_uuid4_new() {
        let uuid = UUID4::new();
        let uuid_string = uuid.to_string();
        let uuid_parsed = Uuid::parse_str(&uuid_string).expect("Uuid::parse_str failed");
        assert_eq!(uuid_parsed.get_version().unwrap(), uuid::Version::Random);
        assert_eq!(uuid_parsed.to_string().len(), 36);
    }

    #[rstest]
    fn test_uuid4_default() {
        let uuid: UUID4 = UUID4::default();
        let uuid_string = uuid.to_string();
        let uuid_parsed = Uuid::parse_str(&uuid_string).expect("Uuid::parse_str failed");
        assert_eq!(uuid_parsed.get_version().unwrap(), uuid::Version::Random);
    }

    #[rstest]
    fn test_uuid4_from_str() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let uuid = UUID4::from(uuid_string);
        let result_string = uuid.to_string();
        let result_parsed = Uuid::parse_str(&result_string).expect("Uuid::parse_str failed");
        let expected_parsed = Uuid::parse_str(uuid_string).expect("Uuid::parse_str failed");
        assert_eq!(result_parsed, expected_parsed);
    }

    #[rstest]
    fn test_equality() {
        let uuid1 = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");
        let uuid2 = UUID4::from("46922ecb-4324-4e40-a56c-841e0d774cef");
        assert_eq!(uuid1, uuid1);
        assert_ne!(uuid1, uuid2);
    }

    #[rstest]
    fn test_uuid4_display() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let uuid = UUID4::from(uuid_string);
        let result_string = format!("{uuid}");
        assert_eq!(result_string, uuid_string);
    }
}
