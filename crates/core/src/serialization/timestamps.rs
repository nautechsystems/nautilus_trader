// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! In-place timestamp rewriting for payload value trees.
//!
//! Field names determine conversion, including inside general metadata maps.

use anyhow::Context;
use jiff::Timestamp;
use serde_json::Value;

/// Converts `ts_*`, `expire_time`, and `expire_time_ns` fields to ISO 8601 or nanosecond strings.
///
/// Rewrites unsigned integers and decimal strings in nested objects and arrays.
/// ISO output uses UTC with nine fractional digits and a `Z` suffix.
/// Existing nonnumeric strings are preserved.
/// Preserves `exec_algorithm_params` and the `info` maps in `OrderFilled` and `OrderFillVoided`.
///
/// # Errors
///
/// Returns an error if [`Timestamp`] rejects a value; its range includes all `u64` nanoseconds.
pub fn serialize(value: &mut Value, iso8601: bool) -> anyhow::Result<()> {
    match value {
        Value::Object(map) => {
            let string_info = has_string_info(map);
            for (key, value) in map
                .iter_mut()
                .filter(|(key, _)| !is_opaque(key, string_info))
            {
                if is_timestamp(key)
                    && let Some(nanos) = value
                        .as_u64()
                        .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                {
                    let timestamp = if iso8601 {
                        let timestamp = Timestamp::from_nanosecond(i128::from(nanos))?;
                        format!("{timestamp:.9}")
                    } else {
                        nanos.to_string()
                    };

                    *value = Value::String(timestamp);
                } else {
                    serialize(value, iso8601)?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                serialize(value, iso8601)?;
            }
        }
        _ => {}
    }

    Ok(())
}

/// Converts RFC 3339 and nanosecond strings in timestamp fields to nanosecond integers.
///
/// Rewrites nested objects and arrays in place. Timestamp strings require `Z` or a numeric UTC offset.
/// Preserves strings that do not parse as timestamps.
/// Preserves `exec_algorithm_params` and the `info` maps in `OrderFilled` and `OrderFillVoided`.
///
/// # Errors
///
/// Returns an error if a parsed timestamp is outside the unsigned nanosecond range.
pub fn deserialize(value: &mut Value) -> anyhow::Result<()> {
    match value {
        Value::Object(map) => {
            let string_info = has_string_info(map);
            for (key, value) in map
                .iter_mut()
                .filter(|(key, _)| !is_opaque(key, string_info))
            {
                if is_timestamp(key)
                    && let Value::String(text) = value
                {
                    if let Ok(nanos) = text.parse::<u64>() {
                        *value = Value::Number(nanos.into());
                    } else if let Ok(timestamp) = text.parse::<Timestamp>() {
                        let nanos = u64::try_from(timestamp.as_nanosecond())
                            .context("timestamp is outside the unsigned nanosecond range")?;
                        *value = Value::Number(nanos.into());
                    }
                } else {
                    deserialize(value)?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                deserialize(value)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn is_timestamp(key: &str) -> bool {
    key.starts_with("ts_") || matches!(key, "expire_time" | "expire_time_ns")
}

fn has_string_info(map: &serde_json::Map<String, Value>) -> bool {
    matches!(
        map.get("type").and_then(Value::as_str),
        Some("OrderFilled" | "OrderFillVoided")
    )
}

fn is_opaque(key: &str, string_info: bool) -> bool {
    key == "exec_algorithm_params" || (string_info && key == "info")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde::Serialize;

    use super::*;

    #[derive(Serialize)]
    struct Payload {
        ts_event: u64,
        ts_init: u64,
        expire_time_ns: Option<u64>,
        expire_time: u64,
        value: u64,
        nested: Vec<Nested>,
    }

    #[derive(Serialize)]
    struct Nested {
        ts_event: u64,
        text: &'static str,
    }

    #[rstest]
    #[case("1969-12-31T23:59:59.999999999Z")]
    #[case("3000-01-01T00:00:00Z")]
    fn test_timestamp_outside_unsigned_range_is_rejected(#[case] timestamp: &str) {
        let mut value = Value::Object(serde_json::Map::from_iter([(
            "ts_event".into(),
            Value::String(timestamp.into()),
        )]));

        let error = deserialize(&mut value).unwrap_err();

        assert_eq!(
            error.to_string(),
            "timestamp is outside the unsigned nanosecond range"
        );
    }

    #[rstest]
    fn test_execution_parameters_preserve_timestamp_keys(#[values(false, true)] iso8601: bool) {
        let mut value = serde_json::json!({
            "ts_event": 123,
            "nested": [{"exec_algorithm_params": {
                "ts_start": "9007199254740993",
                "ts_end": "1970-01-01T00:00:00.000000987Z"
            }}]
        });
        let original = value.clone();

        serialize(&mut value, iso8601).unwrap();

        assert_eq!(value["nested"], original["nested"]);

        deserialize(&mut value).unwrap();

        assert_eq!(value, original);
    }

    #[rstest]
    #[case(false, "123", "456", "789", "987", "9007199254740993")]
    #[case(
        true,
        "1970-01-01T00:00:00.000000123Z",
        "1970-01-01T00:00:00.000000456Z",
        "1970-01-01T00:00:00.000000789Z",
        "1970-01-01T00:00:00.000000987Z",
        "1970-04-15T05:59:59.254740993Z"
    )]
    fn test_timestamp_fields_round_trip(
        #[case] iso8601: bool,
        #[case] ts_event: &str,
        #[case] ts_init: &str,
        #[case] expire_time_ns: &str,
        #[case] expire_time: &str,
        #[case] nested: &str,
        #[values(false, true)] strings: bool,
    ) {
        let payload = Payload {
            ts_event: 123,
            ts_init: 456,
            expire_time_ns: Some(789),
            expire_time: 987,
            value: 42,
            nested: vec![Nested {
                ts_event: 9_007_199_254_740_993,
                text: "unchanged",
            }],
        };

        let original = serde_json::to_value(payload).unwrap();
        let mut value = original.clone();

        if strings {
            for field in ["ts_event", "ts_init", "expire_time_ns", "expire_time"] {
                value[field] = Value::String(value[field].to_string());
            }

            value["nested"][0]["ts_event"] = Value::String("9007199254740993".into());
        }

        serialize(&mut value, iso8601).unwrap();

        assert_eq!(value["ts_event"], ts_event);
        assert_eq!(value["ts_init"], ts_init);
        assert_eq!(value["expire_time_ns"], expire_time_ns);
        assert_eq!(value["expire_time"], expire_time);
        assert_eq!(value["value"], 42);
        assert_eq!(value["nested"][0]["ts_event"], nested);
        assert_eq!(value["nested"][0]["text"], "unchanged");

        deserialize(&mut value).unwrap();

        assert_eq!(value, original);
    }
}
