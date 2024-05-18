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

use std::fmt;

use serde::{
    de::{Unexpected, Visitor},
    Deserializer,
};

struct BoolVisitor;

impl<'de> Visitor<'de> for BoolVisitor {
    type Value = u8;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
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

pub fn from_bool_as_u8<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(BoolVisitor)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::from_bool_as_u8;

    #[derive(Deserialize)]
    pub struct TestStruct {
        #[serde(deserialize_with = "from_bool_as_u8")]
        pub value: u8,
    }

    #[test]
    fn test_deserialize_bool_as_u8_with_boolean() {
        let json_true = r#"{"value": true}"#;
        let test_struct: TestStruct = serde_json::from_str(json_true).unwrap();
        assert_eq!(test_struct.value, 1);

        let json_false = r#"{"value": false}"#;
        let test_struct: TestStruct = serde_json::from_str(json_false).unwrap();
        assert_eq!(test_struct.value, 0);
    }

    #[test]
    fn test_deserialize_bool_as_u8_with_u64() {
        let json_true = r#"{"value": 1}"#;
        let test_struct: TestStruct = serde_json::from_str(json_true).unwrap();
        assert_eq!(test_struct.value, 1);

        let json_false = r#"{"value": 0}"#;
        let test_struct: TestStruct = serde_json::from_str(json_false).unwrap();
        assert_eq!(test_struct.value, 0);
    }
}
