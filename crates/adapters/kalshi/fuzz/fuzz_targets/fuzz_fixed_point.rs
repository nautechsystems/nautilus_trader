#![no_main]

use nautilus_kalshi::http::models::{DecimalNumber, FixedPoint};
use nautilus_live::fuzz::fuzz_target;
use rust_decimal::Decimal;
use serde_json::json;

#[path = "../common/mod.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let Some(data) = common::capped(data) else {
        return;
    };

    if data.is_empty() {
        return;
    }

    probe_fixed(&candidate_text(data));
    if let Ok(text) = std::str::from_utf8(data) {
        probe_fixed(text);
        probe_number(text);
    }
});

fn candidate_text(data: &[u8]) -> String {
    let mut text = String::new();
    if data[0] & 1 == 1 {
        text.push('-');
    }

    let mut dotted = false;

    for byte in data.iter().take(80) {
        match byte % 11 {
            10 if !dotted && !text.is_empty() && !text.ends_with('-') => {
                text.push('.');
                dotted = true;
            }
            digit => text.push(char::from(b'0' + digit)),
        }
    }

    text
}

fn probe_fixed(text: &str) {
    let encoded = json!(text).to_string();
    let first = serde_json::from_str::<FixedPoint>(&encoded);
    let second = serde_json::from_str::<FixedPoint>(&encoded);
    assert_eq!(
        first.is_ok(),
        second.is_ok(),
        "fixed-point rejection changed"
    );

    let Ok(value) = first else {
        return;
    };

    assert_eq!(value, second.unwrap());
    let expected = Decimal::from_str_exact(text).expect("accepted fixed-point text is not exact");
    assert_eq!(value.0, expected, "fixed-point decode changed the input");
    let text_scale = text
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    assert_eq!(
        usize::try_from(value.0.scale()).unwrap(),
        text_scale,
        "fixed-point scale does not match the text"
    );
    assert!(value.0.scale() <= 28, "fixed-point scale exceeds 28");
    let canonical = value.0.to_string();
    assert!(canonical.len() <= 64, "fixed-point encoding is unbounded");
    let again: FixedPoint = serde_json::from_str(&json!(canonical).to_string()).unwrap();
    assert_eq!(value, again, "fixed-point canonical round trip diverged");
    assert_eq!(
        value.0.scale(),
        again.0.scale(),
        "fixed-point canonical round trip changed the scale"
    );
    let reparsed = Decimal::from_str_exact(&canonical).unwrap();
    assert_eq!(
        value.0, reparsed,
        "fixed-point text diverged from its decimal"
    );
}

#[derive(serde::Deserialize)]
struct NumberProbe {
    value: DecimalNumber,
}

fn probe_number(text: &str) {
    let trimmed = text.trim();

    let Some(first) = trimmed.as_bytes().first() else {
        return;
    };

    if *first != b'{' && *first != b'-' && !first.is_ascii_digit() {
        return;
    }

    let encoded = if trimmed.starts_with('{') {
        trimmed.to_owned()
    } else {
        format!(r#"{{"value":{trimmed}}}"#)
    };

    let first = serde_json::from_str::<NumberProbe>(&encoded);
    let second = serde_json::from_str::<NumberProbe>(&encoded);
    assert_eq!(
        first.is_ok(),
        second.is_ok(),
        "decimal number rejection changed"
    );

    let Ok(probe) = first else {
        return;
    };

    assert!(
        probe.value.0.scale() <= 28,
        "decimal number scale exceeds 28"
    );
    let wire = serde_json::to_string(&probe.value).unwrap();
    let again: DecimalNumber = serde_json::from_str(&wire).unwrap();
    assert_eq!(
        probe.value, again,
        "decimal number canonical round trip diverged"
    );
    assert!(wire.len() <= 64, "decimal number encoding is unbounded");
}
