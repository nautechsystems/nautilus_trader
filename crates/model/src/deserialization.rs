// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::borrow::Cow;

use serde::Deserialize;

// A borrowed field lends input that plain Cow deserialization copies
#[derive(Deserialize)]
#[serde(transparent)]
pub(crate) struct StringInput<'a>(#[serde(borrow)] pub(crate) Cow<'a, str>);

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    #[rstest]
    #[case::string(b"\xa3312", 1)]
    #[case::binary(b"\xc4\x03312", 2)]
    fn test_msgpack_borrows_utf8(#[case] bytes: &[u8], #[case] offset: usize) {
        let StringInput(Cow::Borrowed(value)) = rmp_serde::from_slice(bytes).unwrap() else {
            panic!("MessagePack input must remain borrowed");
        };

        assert_eq!((value, value.as_ptr()), ("312", bytes[offset..].as_ptr()));
    }

    #[rstest]
    fn test_json_borrows_unescaped_text() {
        let bytes = b"\"312\"";

        let StringInput(Cow::Borrowed(value)) = serde_json::from_slice(bytes).unwrap() else {
            panic!("unescaped JSON input must remain borrowed");
        };

        assert_eq!((value, value.as_ptr()), ("312", bytes[1..].as_ptr()));
    }

    #[rstest]
    fn test_owned_value_remains_supported() {
        let StringInput(value) = StringInput::deserialize(Value::String("312".into())).unwrap();

        assert!(matches!(value, Cow::Owned(value) if value == "312"));
    }

    #[rstest]
    fn test_json_reader_accepts_escaped_text() {
        let mut deserializer = serde_json::Deserializer::from_reader(Cursor::new(br#""3\u00312""#));
        let StringInput(value) = StringInput::deserialize(&mut deserializer).unwrap();

        assert!(matches!(value, Cow::Owned(value) if value == "312"));
    }

    #[rstest]
    fn test_msgpack_reader_accepts_binary_utf8() {
        let mut deserializer = rmp_serde::Deserializer::new(Cursor::new(b"\xc4\x03312"));
        let StringInput(value) = StringInput::deserialize(&mut deserializer).unwrap();

        assert!(matches!(value, Cow::Owned(value) if value == "312"));
    }

    #[rstest]
    #[case(r#""12.34""#)]
    #[case(r#""12\u002e34""#)]
    #[case(r#""\u00e9""#)]
    #[case(r#""""#)]
    #[case("null")]
    #[case("18446744073709551615")]
    #[case("true")]
    #[case("[]")]
    #[case("{}")]
    #[case("\"")]
    fn test_json_matches_owned_string_results_and_errors(#[case] input: &str) {
        let expected = serde_json::from_str::<String>(input).map_err(|e| e.to_string());
        let actual = serde_json::from_str::<StringInput>(input)
            .map(|value| value.0.into_owned())
            .map_err(|e| e.to_string());

        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case(b"\xa3312")]
    #[case(b"\xc4\x03312")]
    #[case(b"\xc4\x01\xff")]
    #[case(b"\xa1\xff")]
    #[case(b"\x90")]
    #[case(b"\xc0")]
    #[case(b"\x01")]
    #[case(b"\x80")]
    #[case(b"\xc1")]
    #[case(b"\xa3x")]
    fn test_msgpack_matches_owned_string_results_and_errors(#[case] input: &[u8]) {
        let expected = rmp_serde::from_slice::<String>(input).map_err(|e| e.to_string());
        let actual = rmp_serde::from_slice::<StringInput>(input)
            .map(|value| value.0.into_owned())
            .map_err(|e| e.to_string());

        assert_eq!(actual, expected);
    }
}
