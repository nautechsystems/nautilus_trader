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

//! A `UUID4` universally unique identifier (UUID) version 4 based on a 128-bit
//! label (RFC 4122).

use std::{
    ffi::CStr,
    fmt::{Debug, Display, Formatter},
    hash::Hash,
    io::{Cursor, Write},
    str::FromStr,
};

use rand::RngCore;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// The maximum length of ASCII characters for a `UUID4` string value (includes null terminator).
pub(crate) const UUID4_LEN: usize = 37;

/// Represents a pseudo-random UUID (universally unique identifier)
/// version 4 based on a 128-bit label as specified in RFC 4122.
#[repr(C)]
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.core")
)]
pub struct UUID4 {
    /// The UUID v4 value as a fixed-length C string byte array (includes null terminator).
    pub(crate) value: [u8; 37], // cbindgen issue using the constant in the array
}

impl UUID4 {
    /// Creates a new [`UUID4`] instance.
    ///
    /// Generates a new UUID version 4, which is based on random or pseudo-random numbers.
    /// The UUID is stored as a fixed-length C string byte array.
    #[must_use]
    pub fn new() -> Self {
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 16];
        rng.fill_bytes(&mut bytes);

        bytes[6] = (bytes[6] & 0x0F) | 0x40; // Set the version to 4
        bytes[8] = (bytes[8] & 0x3F) | 0x80; // Set the variant to RFC 4122

        let mut value = [0u8; UUID4_LEN];
        let mut cursor = Cursor::new(&mut value[..36]);

        write!(
            cursor,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u16::from_be_bytes([bytes[4], bytes[5]]),
            u16::from_be_bytes([bytes[6], bytes[7]]),
            u16::from_be_bytes([bytes[8], bytes[9]]),
            u64::from_be_bytes([
                bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], 0, 0
            ]) >> 16
        )
        .expect("Failed to write UUID string to buffer");

        value[36] = 0; // Add the null terminator

        Self { value }
    }

    /// Converts the `UUID4` to a C string reference.
    #[must_use]
    pub fn to_cstr(&self) -> &CStr {
        // SAFETY: We always store valid C strings
        CStr::from_bytes_with_nul(&self.value)
            .expect("Expected UUID byte representation to be a valid `CString`")
    }
}

impl FromStr for UUID4 {
    type Err = uuid::Error;

    /// Attempts to create a UUID4 from a string representation.
    ///
    /// The string should be a valid UUID in the standard format (e.g., "6ba7b810-9dad-11d1-80b4-00c04fd430c8").
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::try_parse(s)?;
        let mut value = [0; UUID4_LEN];
        let uuid_str = uuid.to_string();
        value[..uuid_str.len()].copy_from_slice(uuid_str.as_bytes());
        value[uuid_str.len()] = 0; // Add null terminator

        Ok(Self { value })
    }
}

impl From<&str> for UUID4 {
    /// Creates a UUID4 from a string slice.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If the `value` string is not a valid UUID.
    fn from(value: &str) -> Self {
        value.parse().expect("`value` should be a valid UUID")
    }
}

impl From<String> for UUID4 {
    /// Creates a UUID4 from a string.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If the `value` string is not a valid UUID.
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl Default for UUID4 {
    /// Creates a new default [`UUID4`] instance.
    ///
    /// The default UUID4 is simply a newly generated UUID version 4.
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}('{}')", stringify!(UUID4), self)
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
        self.to_string().serialize(serializer)
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
    fn test_new() {
        let uuid = UUID4::new();
        let uuid_string = uuid.to_string();
        let uuid_parsed = Uuid::parse_str(&uuid_string).unwrap();
        assert_eq!(uuid_parsed.get_version().unwrap(), uuid::Version::Random);
        assert_eq!(uuid_parsed.to_string().len(), 36);
    }

    #[rstest]
    fn test_invalid_uuid() {
        let invalid_uuid = "invalid-uuid-string";
        assert!(UUID4::from_str(invalid_uuid).is_err());
    }

    #[rstest]
    fn test_default() {
        let uuid: UUID4 = UUID4::default();
        let uuid_string = uuid.to_string();
        let uuid_parsed = Uuid::parse_str(&uuid_string).unwrap();
        assert_eq!(uuid_parsed.get_version().unwrap(), uuid::Version::Random);
    }

    #[rstest]
    fn test_from_str() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let uuid = UUID4::from(uuid_string);
        let result_string = uuid.to_string();
        let result_parsed = Uuid::parse_str(&result_string).unwrap();
        let expected_parsed = Uuid::parse_str(uuid_string).unwrap();
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
    fn test_debug() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let uuid = UUID4::from(uuid_string);
        assert_eq!(format!("{uuid:?}"), format!("UUID4('{uuid_string}')"));
    }

    #[rstest]
    fn test_display() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let uuid = UUID4::from(uuid_string);
        assert_eq!(format!("{uuid}"), uuid_string);
    }

    #[rstest]
    fn test_serialize_json() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let uuid = UUID4::from(uuid_string);

        let serialized = serde_json::to_string(&uuid).unwrap();
        let expected_json = format!("\"{uuid_string}\"");
        assert_eq!(serialized, expected_json);
    }

    #[rstest]
    fn test_deserialize_json() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let serialized = format!("\"{uuid_string}\"");

        let deserialized: UUID4 = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.to_string(), uuid_string);
    }

    #[rstest]
    fn test_serialize_deserialize_round_trip() {
        let uuid = UUID4::new();

        let serialized = serde_json::to_string(&uuid).unwrap();
        let deserialized: UUID4 = serde_json::from_str(&serialized).unwrap();

        assert_eq!(uuid, deserialized);
    }
}
