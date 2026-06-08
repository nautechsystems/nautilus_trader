#![no_main]

use std::str::FromStr;

use libfuzzer_sys::fuzz_target;
use nautilus_derive::common::parse::{
    deserialize_derive_decimal, deserialize_optional_derive_decimal,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Number, Value, json};

#[derive(Debug, Deserialize)]
struct DecimalProbe {
    #[serde(deserialize_with = "deserialize_derive_decimal")]
    value: Decimal,
}

#[derive(Debug, Deserialize)]
struct OptionalDecimalProbe {
    #[serde(deserialize_with = "deserialize_optional_derive_decimal")]
    value: Option<Decimal>,
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    if let Some(value) = probe_value(data) {
        decode_value(value);
    }

    if let Ok(text) = std::str::from_utf8(&data[1..])
        && let Ok(value) = serde_json::from_str::<Value>(text)
    {
        decode_value(value);
    }
});

fn probe_value(data: &[u8]) -> Option<Value> {
    match data[0] % 6 {
        0 => std::str::from_utf8(&data[1..]).ok().map(|s| json!(s)),
        1 => read_u64(data, 1).map(|n| json!(n)),
        2 => read_i64(data, 1).map(|n| json!(n)),
        3 => read_u64(data, 1)
            .map(f64::from_bits)
            .filter(|n| n.is_finite())
            .and_then(Number::from_f64)
            .map(Value::Number),
        4 => Some(Value::Null),
        _ => Some(json!("")),
    }
}

fn decode_value(value: Value) {
    let object = json!({ "value": value });

    if let Ok(probe) = serde_json::from_value::<DecimalProbe>(object.clone()) {
        assert!(probe.value.scale() <= 28, "decimal scale exceeds 28");
        let reparsed = Decimal::from_str(&probe.value.to_string()).expect("decimal must reparse");
        assert_eq!(probe.value, reparsed, "decimal string round trip diverged");
    }

    if let Ok(probe) = serde_json::from_value::<OptionalDecimalProbe>(object)
        && let Some(value) = probe.value
    {
        assert!(value.scale() <= 28, "optional decimal scale exceeds 28");
    }
}

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    let bytes = data.get(offset..offset + 8)?;
    let mut buf = [0u8; 8];
    buf.copy_from_slice(bytes);
    Some(u64::from_le_bytes(buf))
}

fn read_i64(data: &[u8], offset: usize) -> Option<i64> {
    read_u64(data, offset).map(|n| n as i64)
}
