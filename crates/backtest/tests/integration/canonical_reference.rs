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

use std::sync::LazyLock;

use rstest::rstest;
use serde_json::Value;

use super::{
    CanonicalInput, CanonicalReference, CanonicalScenario, NUMERICAL_ABSOLUTE_TOLERANCE,
    decode_numerical_value, escape_pointer_token, extract_numerical_map, load_fixture,
    numerical_values_match,
};

#[rstest]
fn test_reported_windows_skewness_roundoff_is_accepted() {
    let mut expected = CanonicalScenario::BarEmaCross.expected().clone();
    let path = "/statistics/returns/Returns Skewness";
    expected
        .numerical_values
        .insert(path.to_string(), "bfecf97f1111079c".to_string());
    let mut actual = expected.clone();
    actual
        .numerical_values
        .insert(path.to_string(), "bfecf97f1111079e".to_string());

    actual.verify(&expected).unwrap();
}

#[rstest]
#[case("/statistics/general/Long Ratio")]
#[case("/statistics/pnls/USDT/Avg Winner")]
#[case("/statistics/returns/Returns Skewness")]
#[case("/statistics/returns_series/0/value")]
#[case("/positions/0/realized_return")]
#[case("/position_snapshots/0/realized_return")]
fn test_numerical_roundoff_preserves_exact_fingerprint(#[case] path: &str) {
    let value = bar_ema_result();
    let expected = CanonicalReference::from_value(value).unwrap();
    let mut changed = value.clone();
    let bits = u64::from_str_radix(changed.pointer(path).unwrap().as_str().unwrap(), 16).unwrap();
    *changed.pointer_mut(path).unwrap() = Value::String(format!("{:016x}", bits + 2));
    let actual = CanonicalReference::from_value(&changed).unwrap();

    assert_eq!(actual.fingerprint, expected.fingerprint);
    assert_ne!(
        actual.numerical_values[path],
        expected.numerical_values[path]
    );
    actual.verify(&expected).unwrap();
}

#[rstest]
#[case("/statistics/general/Long Ratio")]
#[case("/statistics/pnls/USDT/Avg Winner")]
#[case("/statistics/returns/Returns Skewness")]
#[case("/statistics/returns_series/0/value")]
#[case("/positions/0/realized_return")]
#[case("/position_snapshots/0/realized_return")]
fn test_material_numerical_change_is_rejected(#[case] path: &str) {
    let mut value = bar_ema_result().clone();
    let expected = CanonicalReference::from_value(&value).unwrap();
    let original = decode_numerical_value(value.pointer(path).unwrap().as_str().unwrap()).unwrap();
    let changed = original + 0.001;
    *value.pointer_mut(path).unwrap() = Value::String(format!("{:016x}", changed.to_bits()));
    let actual = CanonicalReference::from_value(&value).unwrap();

    let error = actual.verify(&expected).unwrap_err();

    assert_eq!(actual.fingerprint, expected.fingerprint);
    assert!(error.to_string().contains(path));
    assert!(error.to_string().contains("differs, expected"));
}

#[rstest]
#[case("/fills/0/event/Filled/last_px", "41150.01")]
#[case("/fills/0/event/Filled/last_qty", "0.011")]
#[case("/fills/0/event/Filled/commission", "0.16460001 USDT")]
#[case("/accounts/0/Margin/base/balances/USDT/total", "999920.60515201 USDT")]
#[case("/positions/0/quantity", "0.001")]
#[case("/orders/0/Market/core/quantity", "0.011")]
#[case("/statistics/returns_series/0/timestamp_ns", "1640908800000000001")]
#[case("/fills/0/event/Filled/trade_id", "trade-changed")]
fn test_exact_state_change_is_rejected(#[case] path: &str, #[case] replacement: &str) {
    let mut value = bar_ema_result().clone();
    let expected = CanonicalReference::from_value(&value).unwrap();
    let original = value.pointer(path).unwrap().as_str().unwrap();
    assert_ne!(original, replacement);
    *value.pointer_mut(path).unwrap() = Value::String(replacement.to_string());
    let actual = CanonicalReference::from_value(&value).unwrap();

    let error = actual.verify(&expected).unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("exact state fingerprint mismatch")
    );
}

#[rstest]
fn test_order_event_sequence_is_exact() {
    let mut value = bar_ema_result().clone();
    let expected = CanonicalReference::from_value(&value).unwrap();
    value["orders"][0]["Market"]["core"]["events"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    let actual = CanonicalReference::from_value(&value).unwrap();

    let error = actual.verify(&expected).unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("exact state fingerprint mismatch")
    );
}

#[rstest]
#[case(false)]
#[case(true)]
fn test_statistic_shape_change_is_rejected(#[case] extra: bool) {
    let mut value = bar_ema_result().clone();
    let expected = CanonicalReference::from_value(&value).unwrap();
    let general = value["statistics"]["general"].as_object_mut().unwrap();
    if extra {
        general.insert(
            "Extra statistic".to_string(),
            Value::String("0000000000000000".to_string()),
        );
    } else {
        general.shift_remove("Long Ratio");
    }

    let actual = CanonicalReference::from_value(&value).unwrap();

    let error = actual.verify(&expected).unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("exact state fingerprint mismatch")
    );
}

#[rstest]
fn test_numerical_reference_keys_must_match() {
    let expected = CanonicalScenario::BarEmaCross.expected();
    let mut actual = expected.clone();
    let value = actual
        .numerical_values
        .remove("/statistics/general/Long Ratio")
        .unwrap();
    actual
        .numerical_values
        .insert("/statistics/general/Other Ratio".to_string(), value);

    let error = actual.verify(expected).unwrap_err();

    assert_eq!(
        actual.numerical_values.len(),
        expected.numerical_values.len()
    );
    assert_eq!(
        error.to_string(),
        "canonical numerical field '/statistics/general/Long Ratio' is missing"
    );
}

#[rstest]
fn test_null_does_not_replace_a_numerical_value() {
    let mut value = bar_ema_result().clone();
    let expected = CanonicalReference::from_value(&value).unwrap();
    value["positions"][0]["realized_return"] = Value::Null;
    let actual = CanonicalReference::from_value(&value).unwrap();

    let error = actual.verify(&expected).unwrap_err();

    assert_eq!(actual.fingerprint, expected.fingerprint);
    assert!(
        error
            .to_string()
            .starts_with("canonical numerical field counts differ")
    );
}

#[rstest]
fn test_reference_precision_must_match() {
    let expected = CanonicalScenario::BarEmaCross.expected();
    let mut actual = expected.clone();
    actual.high_precision = !actual.high_precision;

    let error = actual.verify(expected).unwrap_err();

    assert_eq!(
        error.to_string(),
        "canonical reference precision does not match the active precision mode"
    );
}

#[rstest]
#[case(0.0, NUMERICAL_ABSOLUTE_TOLERANCE, true)]
#[case(0.0, f64::from_bits(NUMERICAL_ABSOLUTE_TOLERANCE.to_bits() + 1), false)]
#[case(1_000_000.0, 1_000_000.000_000_1, true)]
#[case(1_000_000.0, 1_000_000.000_01, false)]
#[case(-0.0, 0.0, true)]
#[case(f64::NAN, f64::NAN, true)]
#[case(f64::NAN, 0.0, false)]
#[case(f64::NAN, f64::INFINITY, false)]
#[case(f64::INFINITY, f64::INFINITY, true)]
#[case(f64::NEG_INFINITY, f64::NEG_INFINITY, true)]
#[case(f64::INFINITY, f64::NEG_INFINITY, false)]
#[case(f64::INFINITY, f64::MAX, false)]
fn test_numerical_comparison_boundaries(
    #[case] expected: f64,
    #[case] actual: f64,
    #[case] matches: bool,
) {
    assert_eq!(numerical_values_match(expected, actual), matches);
    assert_eq!(numerical_values_match(actual, expected), matches);
}

#[rstest]
#[case("")]
#[case("0")]
#[case("NAN")]
#[case("BFECF97F1111079C")]
#[case("gfecf97f1111079c")]
#[case("7ff0000000000000")]
#[case("7ff8000000000000")]
fn test_malformed_numerical_encoding_is_rejected(#[case] encoded: &str) {
    assert!(decode_numerical_value(encoded).is_err());
}

#[rstest]
#[case("0000000000000000", 0.0_f64)]
#[case("8000000000000000", -0.0_f64)]
#[case("3ff8000000000000", 1.5_f64)]
#[case("c002000000000000", -2.25_f64)]
#[case("nan", f64::NAN)]
#[case("+inf", f64::INFINITY)]
#[case("-inf", f64::NEG_INFINITY)]
fn test_numerical_encoding_values(#[case] encoded: &str, #[case] expected: f64) {
    let actual = decode_numerical_value(encoded).unwrap();

    if expected.is_nan() {
        assert!(actual.is_nan());
    } else {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
}

#[rstest]
fn test_numerical_pointer_tokens_are_escaped() {
    let mut value = serde_json::Map::new();
    value.insert(
        "gain~/loss".to_string(),
        Value::String("3ff0000000000000".to_string()),
    );
    let mut value = Value::Object(value);
    let mut numerical_values = Default::default();

    extract_numerical_map(&mut value, "/statistics/returns", &mut numerical_values).unwrap();

    assert_eq!(escape_pointer_token("gain~/loss"), "gain~0~1loss");
    assert_eq!(
        numerical_values["/statistics/returns/gain~0~1loss"],
        "3ff0000000000000"
    );
    assert_eq!(value["gain~/loss"], Value::Null);
}

fn bar_ema_result() -> &'static Value {
    static RESULT: LazyLock<Value> = LazyLock::new(|| {
        let fixture = load_fixture(CanonicalInput::Legacy).unwrap();
        let result = CanonicalScenario::BarEmaCross.run(&fixture).unwrap();
        result.as_value().clone()
    });
    &RESULT
}
