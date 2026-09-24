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

//! Exact decimal ingestion, separate from instrument and currency rounding.
//!
//! [`deserialize_json`](crate::serialization::decimal::deserialize_json) preserves quoted and
//! unquoted JSON tokens with the default features.
//! It uses `RawValue`, which Serde's internally tagged and untagged enum buffers do not support.
//!
//! Use [`deserialize`](crate::serialization::decimal::deserialize) for those buffers: strings and
//! 64-bit integers work with all feature sets. Larger integers, fractional tokens, and scientific
//! numeric tokens require
//! `serde_json/arbitrary_precision` from the first JSON parse. Floats are rejected because their
//! original decimal digits are unavailable.
//! Even with arbitrary precision, `Number`'s Serde deserializer delivers some decimals as floats.
//! For buffered `Value` use [`deserialize_json`](crate::serialization::decimal::deserialize_json);
//! for `Number` use `parse(&number.to_string())`.
//! These routes preserve the buffered decimal text when arbitrary precision was enabled at ingest.
//! Serde enum buffering of a `Value` may still deliver floats, which are rejected.
//!
//! The visitor also accepts `serde_json`'s private number-map encoding with arbitrary precision enabled.
//! A literal JSON object using that reserved key is indistinguishable from the numeric encoding.
//! Use the raw-token route before buffering when every JSON object must be rejected.
//! With arbitrary precision, `Value` parsing itself interprets the reserved object as a number.
//!
//! A `Value` or `Number` created through floating-point conversion has already lost its source
//! spelling. No API here recovers those digits. In particular, passing a default-feature `Value`
//! to [`deserialize_json`](crate::serialization::decimal::deserialize_json) reparses its rendered
//! value, not the original JSON token.
//!
//! Required fields reject missing, null, empty, invalid, overflowing, and underflowing values.
//! Optional fields accept null as `None`; add `#[serde(default)]` to accept missing fields.
//! Empty strings remain errors. Representability concerns numeric value, so redundant trailing
//! zeros may be removed. Instrument precision and currency rounding belong to model constructors.
//!
//! [`serialize`](crate::serialization::decimal::serialize) emits exact JSON numbers, including
//! through `serde_json::to_value`. Enable `serde_json/arbitrary_precision` for the full Decimal
//! range. Without it, values whose numeric value would change are errors. String output uses
//! [`serialize_decimal_as_str`](crate::serialization::serialize_decimal_as_str) and
//! [`serialize_optional_decimal_as_str`](crate::serialization::serialize_optional_decimal_as_str).
//! These JSON number contracts do not extend to MessagePack or other Serde formats.
//! Existing permissive parsing and float serializers in the parent module retain their semantics,
//! except that oversized integer inputs return errors instead of panicking.

use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error, MapAccess, Visitor, value::MapAccessDeserializer},
};
use serde_json::{Number, value::RawValue};

use super::{DecimalVisitor, decimal_components};

/// Deserializes an exact decimal from strings, integers, or arbitrary-precision number maps.
///
/// Supports direct JSON and Serde enum buffers. Integers outside the 64-bit range, fractional
/// tokens, and scientific numeric tokens require `serde_json/arbitrary_precision`.
/// Use [`deserialize_json`] for default-feature JSON or buffered
/// `Value`. Some buffered `Number` values use Serde's float route even with arbitrary precision;
/// parse their text with [`parse`] instead.
/// With arbitrary precision, accepts `serde_json`'s private number maps, including literal objects
/// with the same reserved key.
/// Use [`deserialize_json`] before buffering when the original JSON shape must be checked.
///
/// # Errors
///
/// Returns an error if:
/// - The input is a float, null, an empty string, or an invalid decimal.
/// - Representing the value requires rounding.
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
    deserializer.deserialize_any(ExactDecimalVisitor)
}

/// Deserializes an optional exact decimal, accepting null as `None`.
///
/// Add `#[serde(default)]` to accept a missing field. Other values follow [`deserialize`].
///
/// # Errors
///
/// Returns an error for any non-null value rejected by [`deserialize`].
pub fn deserialize_optional<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error> {
    deserializer.deserialize_option(OptionalExactDecimalVisitor)
}

/// Deserializes an exact decimal directly from a quoted or unquoted JSON token.
///
/// Works without arbitrary precision. Use before `Value` buffering to preserve source digits.
/// Serde internally tagged and untagged enum buffers do not support this raw-token route.
///
/// # Errors
///
/// Returns an error if:
/// - The input is null, an empty string, a non-decimal shape, or an invalid decimal.
/// - Representing the value requires rounding.
pub fn deserialize_json<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
    let raw = Box::<RawValue>::deserialize(deserializer)?;
    parse_json(&raw).map_err(D::Error::custom)
}

/// Deserializes an optional exact JSON decimal, accepting null as `None`.
///
/// Add `#[serde(default)]` to accept a missing field. Other values follow [`deserialize_json`].
///
/// # Errors
///
/// Returns an error for any non-null value rejected by [`deserialize_json`].
pub fn deserialize_optional_json<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error> {
    Option::<Box<RawValue>>::deserialize(deserializer)?
        .map(|raw| parse_json(&raw).map_err(D::Error::custom))
        .transpose()
}

/// Serializes a decimal as a JSON number without changing its numeric value.
///
/// Checks the `Number` representation before serialization so `to_value` is also lossless.
/// The full Decimal range requires `serde_json/arbitrary_precision`. Without that feature,
/// scale is not preserved and output may use exponent notation. Check the receiving API's
/// numeric syntax before using this serializer for wire payloads.
///
/// Raw-token serializers can emit the full Decimal range directly to JSON text without arbitrary
/// precision. Replacing them with this serializer can reject previously supported values unless
/// `serde_json/arbitrary_precision` is enabled. Use [`super::serialize_decimal_as_str`] for
/// portable string output.
///
/// # Errors
///
/// Returns an error if:
/// - The enabled JSON representation would round the value.
/// - Serialization fails.
pub fn serialize<S: Serializer>(value: &Decimal, serializer: S) -> Result<S::Ok, S::Error> {
    let number = Number::from_str(&value.to_string()).map_err(serde::ser::Error::custom)?;
    let represented = parse(&number.to_string()).map_err(serde::ser::Error::custom)?;
    if represented != *value {
        return Err(serde::ser::Error::custom(
            "exact decimal number serialization requires serde_json/arbitrary_precision",
        ));
    }

    number.serialize(serializer)
}

/// Serializes an optional decimal as an exact JSON number or null.
///
/// # Errors
///
/// Returns the errors described by [`serialize`], or an error serializing null.
pub fn serialize_optional<S: Serializer>(
    value: &Option<Decimal>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => serialize(value, serializer),
        None => serializer.serialize_none(),
    }
}

/// Parses a decimal or scientific string without rounding its numeric value.
///
/// Accepts signs, decimal points, and digit separators supported by `rust_decimal`.
/// Scientific input is normalized. Plain input drops redundant trailing zeros only when needed
/// to fit Decimal's mantissa or scale.
/// Scientific exponents must fit `i64`.
///
/// # Errors
///
/// Returns an error if:
/// - The input is not a valid decimal or scientific string.
/// - The value exceeds Decimal's exact mantissa or scale bounds.
pub fn parse(value: &str) -> Result<Decimal, rust_decimal::Error> {
    if !value.contains(['e', 'E'])
        && let Ok(value) = Decimal::from_str_exact(value)
    {
        return Ok(value);
    }

    // Normalize the significand before construction: from_scientific parses its base with
    // the rounding FromStr implementation, even when the final value is representable.
    let (base, exponent) = match value.split_once(['e', 'E']) {
        Some((base, exponent)) => (
            base,
            exponent
                .parse::<i64>()
                .map_err(|_| rust_decimal::Error::from("invalid decimal exponent"))?,
        ),
        None => (value, 0),
    };

    let negative = base.starts_with('-');
    let base = base.strip_prefix(['-', '+']).unwrap_or(base);
    if base.starts_with('_') || base.starts_with("._") {
        return Err(rust_decimal::Error::from("invalid decimal significand"));
    }

    let base = base.replace('_', "");
    let (whole, fraction) = decimal_components(&base)
        .ok_or_else(|| rust_decimal::Error::from("invalid decimal significand"))?;
    let mut digits = format!("{whole}{fraction}");
    let mut scale = i64::try_from(fraction.len())
        .ok()
        .and_then(|scale| scale.checked_sub(exponent))
        .ok_or_else(|| rust_decimal::Error::from("decimal scale out of range"))?;
    digits = digits.trim_start_matches('0').to_string();
    if digits.is_empty() {
        return Ok(Decimal::ZERO);
    }

    while scale > 0 && digits.ends_with('0') {
        digits.pop();
        scale -= 1;
    }

    if scale < 0 {
        let zeros = scale
            .checked_neg()
            .and_then(|scale| usize::try_from(scale).ok())
            .filter(|zeros| digits.len().saturating_add(*zeros) <= 29)
            .ok_or(rust_decimal::Error::ExceedsMaximumPossibleValue)?;
        digits.extend(std::iter::repeat_n('0', zeros));
        scale = 0;
    }

    let scale = u32::try_from(scale).map_err(|_| rust_decimal::Error::Underflow)?;
    if scale > Decimal::MAX_SCALE {
        return Err(rust_decimal::Error::Underflow);
    }

    let mantissa = digits
        .parse::<i128>()
        .map_err(|_| rust_decimal::Error::ExceedsMaximumPossibleValue)?;
    Decimal::try_from_i128_with_scale(if negative { -mantissa } else { mantissa }, scale)
}

fn parse_json(raw: &RawValue) -> Result<Decimal, String> {
    if raw.get().starts_with('"') {
        let value: String = serde_json::from_str(raw.get()).map_err(|e| e.to_string())?;
        parse(&value).map_err(|e| e.to_string())
    } else {
        parse(raw.get()).map_err(|e| e.to_string())
    }
}

struct ExactDecimalVisitor;

impl<'de> Visitor<'de> for ExactDecimalVisitor {
    type Value = Decimal;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter
            .write_str("an exact decimal string or integer, or an arbitrary-precision JSON number")
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
        parse(value).map_err(E::custom)
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
        DecimalVisitor.visit_i64(value)
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
        DecimalVisitor.visit_u64(value)
    }

    fn visit_i128<E: Error>(self, value: i128) -> Result<Self::Value, E> {
        DecimalVisitor.visit_i128(value)
    }

    fn visit_u128<E: Error>(self, value: u128) -> Result<Self::Value, E> {
        DecimalVisitor.visit_u128(value)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        let number = Number::deserialize(MapAccessDeserializer::new(map))?;
        parse(&number.to_string()).map_err(A::Error::custom)
    }
}

struct OptionalExactDecimalVisitor;

impl<'de> Visitor<'de> for OptionalExactDecimalVisitor {
    type Value = Option<Decimal>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("null or an exact decimal")
    }

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserialize(deserializer).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, str::FromStr};

    use proptest::prelude::*;
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use serde::{
        Deserialize, Serialize,
        de::value::{Error, F64Deserializer, I128Deserializer, U128Deserializer},
    };
    use serde_json::{Number, Value, value::RawValue};

    use crate::serialization::{self, decimal};

    #[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
    struct Exact(#[serde(with = "decimal")] Decimal);

    #[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
    struct Raw(
        #[serde(
            deserialize_with = "decimal::deserialize_json",
            serialize_with = "decimal::serialize"
        )]
        Decimal,
    );

    #[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
    struct Optional {
        #[serde(
            default,
            deserialize_with = "decimal::deserialize_optional",
            serialize_with = "decimal::serialize_optional"
        )]
        value: Option<Decimal>,
    }

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    struct OptionalRaw {
        #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
        value: Option<Decimal>,
    }

    #[derive(Debug, Deserialize)]
    struct Required {
        #[serde(deserialize_with = "decimal::deserialize")]
        value: Decimal,
    }

    #[derive(Debug, Deserialize)]
    struct RequiredRaw {
        #[serde(deserialize_with = "decimal::deserialize_json")]
        value: Decimal,
    }

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    #[serde(tag = "kind")]
    enum Tagged {
        Amount {
            #[serde(deserialize_with = "decimal::deserialize")]
            value: Decimal,
        },
    }

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    #[serde(untagged)]
    enum Untagged {
        Amount {
            #[serde(deserialize_with = "decimal::deserialize")]
            value: Decimal,
        },
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "kind")]
    enum TaggedRaw {
        Amount {
            #[serde(rename = "value", deserialize_with = "decimal::deserialize_json")]
            _value: Decimal,
        },
    }

    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum UntaggedRaw {
        Amount {
            #[serde(rename = "value", deserialize_with = "decimal::deserialize_json")]
            _value: Decimal,
        },
    }

    fn cases() -> [(&'static str, Decimal, bool); 22] {
        [
            ("123456789.123456789", dec!(123456789.123456789), false),
            ("0.12345678901234568", dec!(0.12345678901234568), true),
            ("9007199254740993", dec!(9007199254740993), true),
            ("18446744073709551617", dec!(18446744073709551617), false),
            (
                "79228162514264337593543950335",
                dec!(79228162514264337593543950335),
                false,
            ),
            (
                "-79228162514264337593543950335",
                dec!(-79228162514264337593543950335),
                false,
            ),
            ("1e-28", dec!(0.0000000000000000000000000001), false),
            ("10e-29", dec!(0.0000000000000000000000000001), false),
            ("1e28", dec!(10000000000000000000000000000), true),
            ("0.1e29", dec!(10000000000000000000000000000), true),
            (
                "792281625142643375935439503350e-1",
                dec!(79228162514264337593543950335),
                false,
            ),
            (
                "7.9228162514264337593543950335e28",
                dec!(79228162514264337593543950335),
                false,
            ),
            (
                "0.00000000000000000000000000010",
                dec!(0.0000000000000000000000000001),
                false,
            ),
            (
                "79228162514264337593543950335.0",
                dec!(79228162514264337593543950335),
                false,
            ),
            (
                "1.234567890123456789012345678900E+1",
                dec!(12.345678901234567890123456789),
                false,
            ),
            ("-0.0", dec!(0), true),
            ("0e1000000", dec!(0), true),
            ("-3.75E+2", dec!(-375), true),
            ("-375", dec!(-375), true),
            (
                "0.00000000000000000000000000001e1",
                dec!(0.0000000000000000000000000001),
                false,
            ),
            ("-12345e-4", dec!(-1.2345), true),
            (
                "1.234567890123456789012345678e-1",
                dec!(0.1234567890123456789012345678),
                false,
            ),
        ]
    }

    fn arbitrary_precision() -> bool {
        Number::from_str(&Decimal::MAX.to_string())
            .unwrap()
            .to_string()
            == Decimal::MAX.to_string()
    }

    fn object(token: &str, tagged: bool) -> String {
        let mut fields = BTreeMap::new();
        fields.insert("value", RawValue::from_string(token.to_string()).unwrap());

        if tagged {
            fields.insert(
                "kind",
                RawValue::from_string(serde_json::to_string("Amount").unwrap()).unwrap(),
            );
        }

        serde_json::to_string(&fields).unwrap()
    }

    #[rstest]
    fn exact_quoted_and_numeric_tokens() {
        for (token, expected, _) in cases() {
            let quoted = serde_json::to_string(&token).unwrap();
            assert_eq!(decimal::parse(token).unwrap(), expected, "{token}");
            assert_eq!(serde_json::from_str::<Raw>(token).unwrap(), Raw(expected));
            assert_eq!(
                serde_json::from_slice::<Raw>(token.as_bytes()).unwrap(),
                Raw(expected)
            );
            assert_eq!(
                serde_json::from_reader::<_, Raw>(token.as_bytes()).unwrap(),
                Raw(expected)
            );
            assert_eq!(serde_json::from_str::<Raw>(&quoted).unwrap(), Raw(expected));
            assert_eq!(
                serde_json::from_str::<Exact>(&quoted).unwrap(),
                Exact(expected)
            );
            let number: Number = serde_json::from_str(token).unwrap();
            let direct = serde_json::from_str::<Exact>(token);
            let buffered = serde_json::from_value::<Exact>(Value::Number(number.clone()));
            let from_number = decimal::deserialize(number.clone());
            if arbitrary_precision() || number.is_i64() || number.is_u64() {
                assert_eq!(direct.unwrap(), Exact(expected));
                assert_eq!(
                    serde_json::from_value::<Raw>(Value::Number(number.clone())).unwrap(),
                    Raw(expected)
                );
                assert_eq!(decimal::parse(&number.to_string()).unwrap(), expected);

                match (buffered, from_number) {
                    (Ok(buffered), Ok(from_number)) => {
                        assert_eq!(buffered, Exact(expected));
                        assert_eq!(from_number, expected);
                    }
                    (Err(buffered), Err(from_number)) => {
                        assert!(buffered.to_string().contains("floating point"));
                        assert!(from_number.to_string().contains("floating point"));
                    }
                    results => panic!("buffered routes disagree: {results:?}"),
                }
            } else {
                assert!(direct.is_err(), "{token}");
                assert!(buffered.is_err(), "{token}");
                assert!(from_number.is_err(), "{token}");
            }
        }
    }

    #[rstest]
    fn invalid_values_and_shapes() {
        for token in [
            "79228162514264337593543950336",
            "-79228162514264337593543950336",
            "1e29",
            "1e-29",
            "-1e-29",
            "0.00000000000000000000000000001",
            "1.00000000000000000000000000001",
            "79228162514264337593543950335.1",
            "1e9223372036854775807",
            "1e-9223372036854775808",
            "1e9223372036854775808",
            "1.2.3e1",
            "",
            "NaN",
            "Infinity",
            "1e",
            "1e1e1",
            "--1",
            "0.12345678901234567890123456789e0",
            "0.12345678901234567890123456789",
            "--1e0",
            "_1",
            "._1",
            "_1e1",
            "._1e1",
        ] {
            assert!(decimal::parse(token).is_err(), "{token}");
            let quoted = serde_json::to_string(&token).unwrap();
            assert!(serde_json::from_str::<Raw>(&quoted).is_err(), "{token}");
            assert!(serde_json::from_str::<Exact>(&quoted).is_err(), "{token}");
            assert!(serde_json::from_str::<Raw>(token).is_err(), "{token}");
            assert!(serde_json::from_str::<Exact>(token).is_err(), "{token}");
            let json = object(&quoted, false);
            assert!(serde_json::from_str::<Optional>(&json).is_err(), "{token}");
            assert!(
                serde_json::from_str::<OptionalRaw>(&json).is_err(),
                "{token}"
            );

            if let Ok(value) = serde_json::from_str::<Value>(token) {
                assert!(
                    serde_json::from_value::<Exact>(value.clone()).is_err(),
                    "{token}"
                );

                if arbitrary_precision() {
                    assert!(serde_json::from_value::<Raw>(value).is_err(), "{token}");
                }
            }
        }

        for token in ["null", "true", "false", "[]", "{}", "{\"amount\":1}"] {
            assert!(serde_json::from_str::<Raw>(token).is_err(), "{token}");
            assert!(serde_json::from_str::<Exact>(token).is_err(), "{token}");
        }
    }

    #[rstest]
    #[case("+1_234.50E-2", dec!(12.345))]
    #[case("1.2_5", dec!(1.25))]
    #[case(".5e1", dec!(5))]
    fn quoted_decimal_syntax(#[case] token: &str, #[case] expected: Decimal) {
        let json = serde_json::to_string(token).unwrap();
        assert_eq!(decimal::parse(token).unwrap(), expected);
        assert_eq!(serde_json::from_str::<Raw>(&json).unwrap(), Raw(expected));
        assert_eq!(
            serde_json::from_str::<Exact>(&json).unwrap(),
            Exact(expected)
        );
    }

    #[rstest]
    fn quoted_json_escapes() {
        assert_eq!(
            serde_json::from_str::<Raw>(r#""\u0031.25""#).unwrap(),
            Raw(dec!(1.25))
        );
        assert!(serde_json::from_str::<Raw>(r#""\uD800""#).is_err());
    }

    #[rstest]
    fn private_number_map_requires_raw_shape_validation() {
        let private_number = r#"{"$serde_json::private::Number":"123456789.123456789"}"#;
        assert!(serde_json::from_str::<Raw>(private_number).is_err());
        let direct = serde_json::from_str::<Exact>(private_number);
        let value: Value = serde_json::from_str(private_number).unwrap();

        if arbitrary_precision() {
            assert_eq!(direct.unwrap(), Exact(dec!(123456789.123456789)));
            assert!(value.is_number());
            assert_eq!(
                serde_json::from_value::<Raw>(value).unwrap(),
                Raw(dec!(123456789.123456789))
            );
        } else {
            assert!(direct.is_err());
            assert!(value.is_object());
            assert!(serde_json::from_value::<Raw>(value).is_err());
        }

        for token in [
            "{\"$serde_json::private::Number\":\"not-a-number\"}",
            "{\"$serde_json::private::Number\":\"1e-29\"}",
            "{\"$serde_json::private::Number\":\"79228162514264337593543950336\"}",
        ] {
            assert!(serde_json::from_str::<Raw>(token).is_err());
            assert!(serde_json::from_str::<Exact>(token).is_err());
        }
    }

    #[rstest]
    fn presence_semantics() {
        for json in ["{}", r#"{"value":null}"#] {
            assert!(serde_json::from_str::<Required>(json).is_err());
            assert!(serde_json::from_str::<RequiredRaw>(json).is_err());
            assert_eq!(
                serde_json::from_str::<Optional>(json).unwrap(),
                Optional { value: None }
            );
            assert_eq!(
                serde_json::from_str::<OptionalRaw>(json).unwrap(),
                OptionalRaw { value: None }
            );
        }

        let empty = r#"{"value":""}"#;
        assert!(serde_json::from_str::<Required>(empty).is_err());
        assert!(serde_json::from_str::<RequiredRaw>(empty).is_err());
        assert!(serde_json::from_str::<Optional>(empty).is_err());
        assert!(serde_json::from_str::<OptionalRaw>(empty).is_err());

        for (token, expected, _) in cases() {
            let json = object(&serde_json::to_string(&token).unwrap(), false);
            assert_eq!(
                serde_json::from_str::<Required>(&json).unwrap().value,
                expected
            );
            assert_eq!(
                serde_json::from_str::<RequiredRaw>(&json).unwrap().value,
                expected
            );
            assert_eq!(
                serde_json::from_str::<Optional>(&json).unwrap().value,
                Some(expected)
            );
            assert_eq!(
                serde_json::from_str::<OptionalRaw>(&json).unwrap().value,
                Some(expected)
            );
            let numeric = object(token, false);
            assert_eq!(
                serde_json::from_str::<OptionalRaw>(&numeric).unwrap().value,
                Some(expected)
            );
            let optional = serde_json::from_str::<Optional>(&numeric);
            let number = Number::from_str(token).unwrap();
            if arbitrary_precision() || number.is_i64() || number.is_u64() {
                assert_eq!(optional.unwrap().value, Some(expected));
            } else {
                assert!(optional.is_err());
            }
        }
    }

    #[rstest]
    fn serde_enum_buffering() {
        for (token, expected, _) in cases() {
            let quoted = serde_json::to_string(&token).unwrap();
            let tagged = object(&quoted, true);
            let untagged = object(&quoted, false);
            assert_eq!(
                serde_json::from_str::<Tagged>(&tagged).unwrap(),
                Tagged::Amount { value: expected }
            );
            assert_eq!(
                serde_json::from_str::<Untagged>(&untagged).unwrap(),
                Untagged::Amount { value: expected }
            );
            assert!(serde_json::from_str::<TaggedRaw>(&tagged).is_err());
            assert!(serde_json::from_str::<UntaggedRaw>(&untagged).is_err());
            let tagged = serde_json::from_str::<Tagged>(&object(token, true));
            let untagged = serde_json::from_str::<Untagged>(&object(token, false));
            let number = Number::from_str(token).unwrap();
            if arbitrary_precision() || number.is_i64() || number.is_u64() {
                assert_eq!(tagged.unwrap(), Tagged::Amount { value: expected });
                assert_eq!(untagged.unwrap(), Untagged::Amount { value: expected });
            } else {
                assert!(tagged.is_err());
                assert!(untagged.is_err());
            }
        }
    }

    #[rstest]
    #[case(Decimal::MAX.mantissa(), Some(Decimal::MAX))]
    #[case(Decimal::MIN.mantissa(), Some(Decimal::MIN))]
    #[case(Decimal::MAX.mantissa() + 1, None)]
    #[case(Decimal::MIN.mantissa() - 1, None)]
    #[case(i128::MAX, None)]
    #[case(i128::MIN, None)]
    fn signed_integer_bounds(#[case] value: i128, #[case] expected: Option<Decimal>) {
        assert_eq!(
            decimal::deserialize(I128Deserializer::<Error>::new(value)).ok(),
            expected
        );
        assert_eq!(
            serialization::deserialize_decimal(I128Deserializer::<Error>::new(value)).ok(),
            expected
        );
        assert_eq!(
            serialization::deserialize_optional_decimal(I128Deserializer::<Error>::new(value)).ok(),
            expected.map(Some)
        );
    }

    #[rstest]
    #[case(9_007_199_254_740_993, Some(dec!(9007199254740993)))]
    #[case(18_446_744_073_709_551_617, Some(dec!(18446744073709551617)))]
    #[case(Decimal::MAX.mantissa().unsigned_abs(), Some(Decimal::MAX))]
    #[case(Decimal::MAX.mantissa().unsigned_abs() + 1, None)]
    #[case(u128::MAX, None)]
    fn unsigned_integer_bounds(#[case] value: u128, #[case] expected: Option<Decimal>) {
        assert_eq!(
            decimal::deserialize(U128Deserializer::<Error>::new(value)).ok(),
            expected
        );
        assert_eq!(
            serialization::deserialize_decimal(U128Deserializer::<Error>::new(value)).ok(),
            expected
        );
        assert_eq!(
            serialization::deserialize_optional_decimal(U128Deserializer::<Error>::new(value)).ok(),
            expected.map(Some)
        );
    }

    #[rstest]
    #[case(0.5)]
    #[case(f64::NAN)]
    #[case(f64::INFINITY)]
    fn buffered_floats_are_rejected(#[case] value: f64) {
        assert!(decimal::deserialize(F64Deserializer::<Error>::new(value)).is_err());
    }

    #[rstest]
    fn exact_numeric_serialization() {
        for (token, expected, default_numeric) in cases() {
            let text = serde_json::to_string(&Exact(expected));
            let value = serde_json::to_value(Exact(expected));

            let optional = serde_json::to_value(Optional {
                value: Some(expected),
            });

            let supported = arbitrary_precision() || default_numeric;
            assert_eq!(text.is_ok(), supported, "{token}: {text:?}");
            assert_eq!(value.is_ok(), supported);
            assert_eq!(optional.is_ok(), supported);

            if supported {
                let value = value.unwrap();
                assert!(value.is_number());
                assert_eq!(
                    serde_json::from_str::<Raw>(&text.unwrap()).unwrap(),
                    Raw(expected)
                );
                assert_eq!(
                    serde_json::from_value::<Raw>(value.clone()).unwrap(),
                    Raw(expected)
                );
                assert_eq!(decimal::parse(&value.to_string()).unwrap(), expected);
                assert_eq!(optional.unwrap()["value"], value);
            }
        }

        assert_eq!(
            serde_json::to_value(Optional { value: None }).unwrap(),
            serde_json::json!({"value": null})
        );
    }

    #[rstest]
    fn buffered_precision_boundary() {
        let token = "0.12345678901234568";
        let number = Number::from_str(token).unwrap();
        let value = Value::Number(number.clone());
        assert!(
            decimal::deserialize(number.clone())
                .unwrap_err()
                .to_string()
                .contains("floating point")
        );
        assert!(
            serde_json::from_value::<Exact>(value.clone())
                .unwrap_err()
                .to_string()
                .contains("floating point")
        );
        assert_eq!(
            decimal::parse(&number.to_string()).unwrap(),
            dec!(0.12345678901234568)
        );
        assert_eq!(
            serde_json::from_value::<Raw>(value).unwrap(),
            Raw(dec!(0.12345678901234568))
        );

        let original = "123456789.123456789";
        let value: Value = serde_json::from_str(original).unwrap();
        let buffered = serde_json::from_value::<Raw>(value).unwrap();

        let expected = if arbitrary_precision() {
            dec!(123456789.123456789)
        } else {
            dec!(123456789.12345679)
        };

        assert_eq!(buffered, Raw(expected));
        assert_eq!(
            serde_json::from_str::<Raw>(original).unwrap(),
            Raw(dec!(123456789.123456789))
        );
    }

    #[rstest]
    fn permissive_buffered_overflow_returns_error() {
        for token in [
            "79228162514264337593543950336",
            "-79228162514264337593543950336",
        ] {
            let mut direct = serde_json::Deserializer::from_str(token);
            assert!(serialization::deserialize_decimal(&mut direct).is_err());
            let value: Value = serde_json::from_str(token).unwrap();
            assert!(serialization::deserialize_decimal(value.clone()).is_err());
            assert!(serialization::deserialize_optional_decimal(value).is_err());
        }
    }

    #[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
    struct StringDecimal(
        #[serde(
            deserialize_with = "decimal::deserialize",
            serialize_with = "serialization::serialize_decimal_as_str"
        )]
        Decimal,
    );

    proptest! {
        #[rstest]
        fn prop_checked_integer_visitors(
            signed in prop_oneof![Decimal::MIN.mantissa()..=Decimal::MAX.mantissa(), any::<i128>()],
            unsigned in prop_oneof![0..=Decimal::MAX.mantissa().unsigned_abs(), any::<u128>()],
        ) {
            let expected = Decimal::from_str_exact(&signed.to_string()).ok();
            prop_assert_eq!(decimal::deserialize(I128Deserializer::<Error>::new(signed)).ok(), expected);
            prop_assert_eq!(serialization::deserialize_decimal(I128Deserializer::<Error>::new(signed)).ok(), expected);
            let expected = Decimal::from_str_exact(&unsigned.to_string()).ok();
            prop_assert_eq!(decimal::deserialize(U128Deserializer::<Error>::new(unsigned)).ok(), expected);
            prop_assert_eq!(serialization::deserialize_decimal(U128Deserializer::<Error>::new(unsigned)).ok(), expected);
        }

        #[rstest]
        fn prop_roundtrips_and_exponent_rescaling(
            mantissa in Decimal::MIN.mantissa()..=Decimal::MAX.mantissa(),
            scale in 0u32..=Decimal::MAX_SCALE,
            zeros in 0usize..=35,
        ) {
            let expected = Decimal::try_from_i128_with_scale(mantissa, scale).unwrap();
            let scientific = format!("{mantissa}{}e-{}", "0".repeat(zeros), scale as usize + zeros);
            prop_assert_eq!(decimal::parse(&scientific).unwrap(), expected);
            prop_assert_eq!(serde_json::from_str::<Raw>(&scientific).unwrap(), Raw(expected));
            let mut plain = expected.to_string();
            if !plain.contains('.') { plain.push('.'); }
            plain.push_str(&"0".repeat(zeros));
            prop_assert_eq!(decimal::parse(&plain).unwrap(), expected);
            let string = serde_json::to_value(StringDecimal(expected)).unwrap();
            prop_assert_eq!(serde_json::from_value::<StringDecimal>(string).unwrap(), StringDecimal(expected));
            if let Ok(number) = serde_json::to_value(Exact(expected)) {
                prop_assert_eq!(decimal::parse(&number.to_string()).unwrap(), expected);
                prop_assert_eq!(serde_json::from_value::<Raw>(number).unwrap(), Raw(expected));
            } else {
                prop_assert!(!arbitrary_precision());
            }
        }

        #[rstest]
        fn prop_parse_never_panics(value in "\\PC{0,128}") {
            let _ = decimal::parse(&value);
        }
    }
}
