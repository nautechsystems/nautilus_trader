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
    fn to_json_bytes(&self) -> Result<Bytes, serde_json::Error> {
        serde_json::to_vec(self).map(Bytes::from)
    }

    /// Serialize an object to `MsgPack` encoded bytes.
    ///
    /// # Errors
    ///
    /// Returns serialization errors.
    fn to_msgpack_bytes(&self) -> Result<Bytes, rmp_serde::encode::Error> {
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

    #[allow(clippy::cast_possible_truncation)]
    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        // Only 0 or 1 are considered valid representations when provided as an
        // integer. We deliberately reject values outside this range to avoid
        // silently truncating larger integers into impl-defined boolean
        // semantics.
        if value > 1 {
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
    use serde::{Deserialize, Serialize};

    use super::{Serializable, from_bool_as_u8};

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

    #[rstest]
    fn test_deserialize_bool_as_u8_with_invalid_integer() {
        // Any integer other than 0/1 is invalid and should error
        let json = r#"{"value": 2}"#;
        let result: Result<TestStruct, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct SerializableTestStruct {
        id: u32,
        name: String,
        value: f64,
    }

    impl Serializable for SerializableTestStruct {}

    #[rstest]
    fn test_serializable_json_roundtrip() {
        let original = SerializableTestStruct {
            id: 42,
            name: "test".to_string(),
            value: std::f64::consts::PI,
        };

        let json_bytes = original.to_json_bytes().unwrap();
        let deserialized = SerializableTestStruct::from_json_bytes(&json_bytes).unwrap();

        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_serializable_msgpack_roundtrip() {
        let original = SerializableTestStruct {
            id: 123,
            name: "msgpack_test".to_string(),
            value: std::f64::consts::E,
        };

        let msgpack_bytes = original.to_msgpack_bytes().unwrap();
        let deserialized = SerializableTestStruct::from_msgpack_bytes(&msgpack_bytes).unwrap();

        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_serializable_json_invalid_data() {
        let invalid_json = b"invalid json data";
        let result = SerializableTestStruct::from_json_bytes(invalid_json);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_serializable_msgpack_invalid_data() {
        let invalid_msgpack = b"invalid msgpack data";
        let result = SerializableTestStruct::from_msgpack_bytes(invalid_msgpack);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_serializable_json_empty_values() {
        let test_struct = SerializableTestStruct {
            id: 0,
            name: String::new(),
            value: 0.0,
        };

        let json_bytes = test_struct.to_json_bytes().unwrap();
        let deserialized = SerializableTestStruct::from_json_bytes(&json_bytes).unwrap();

        assert_eq!(test_struct, deserialized);
    }

    #[rstest]
    fn test_serializable_msgpack_empty_values() {
        let test_struct = SerializableTestStruct {
            id: 0,
            name: String::new(),
            value: 0.0,
        };

        let msgpack_bytes = test_struct.to_msgpack_bytes().unwrap();
        let deserialized = SerializableTestStruct::from_msgpack_bytes(&msgpack_bytes).unwrap();

        assert_eq!(test_struct, deserialized);
    }
}
