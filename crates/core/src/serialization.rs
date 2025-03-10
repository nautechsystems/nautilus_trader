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

//! Common serialization traits and functions.

use bytes::Bytes;
use serde::{
    Deserializer,
    de::{Unexpected, Visitor},
};

struct BoolVisitor;
use serde::{Deserialize, Serialize};

/// Represents types which are serializable for JSON and `MsgPack` specifications.
pub trait Serializable: Serialize + for<'de> Deserialize<'de> {
    /// Deserialize an object from JSON encoded bytes.
    ///
    /// # Errors
    ///
    /// Returns serialization errors.
    fn from_json_bytes(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }

    /// Deserialize an object from `MsgPack` encoded bytes.
    ///
    /// # Errors
    ///
    /// Returns serialization errors.
    fn from_msgpack_bytes(data: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        rmp_serde::from_slice(data)
    }

    /// Serialize an object to JSON encoded bytes.
    ///
    /// # Errors
    ///
    /// Returns serialization errors.
    fn as_json_bytes(&self) -> Result<Bytes, serde_json::Error> {
        serde_json::to_vec(self).map(Bytes::from)
    }

    /// Serialize an object to `MsgPack` encoded bytes.
    ///
    /// # Errors
    ///
    /// Returns serialization errors.
    fn as_msgpack_bytes(&self) -> Result<Bytes, rmp_serde::encode::Error> {
        rmp_serde::to_vec_named(self).map(Bytes::from)
    }
}

impl Visitor<'_> for BoolVisitor {
    type Value = u8;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a boolean as u8")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(u8::from(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if value > u64::from(u8::MAX) {
            Err(E::invalid_value(Unexpected::Unsigned(value), &self))
        } else {
            Ok(value as u8)
        }
    }
}

/// Deserialize the boolean value as a `u8`.
///
/// # Errors
///
/// Returns serialization errors.
pub fn from_bool_as_u8<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(BoolVisitor)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::*;
    use serde::Deserialize;

    use super::from_bool_as_u8;

    #[derive(Deserialize)]
    pub struct TestStruct {
        #[serde(deserialize_with = "from_bool_as_u8")]
        pub value: u8,
    }

    #[rstest]
    #[case(r#"{"value": true}"#, 1)]
    #[case(r#"{"value": false}"#, 0)]
    fn test_deserialize_bool_as_u8_with_boolean(#[case] json_str: &str, #[case] expected: u8) {
        let test_struct: TestStruct = serde_json::from_str(json_str).unwrap();
        assert_eq!(test_struct.value, expected);
    }

    #[rstest]
    #[case(r#"{"value": 1}"#, 1)]
    #[case(r#"{"value": 0}"#, 0)]
    fn test_deserialize_bool_as_u8_with_u64(#[case] json_str: &str, #[case] expected: u8) {
        let test_struct: TestStruct = serde_json::from_str(json_str).unwrap();
        assert_eq!(test_struct.value, expected);
    }
}
