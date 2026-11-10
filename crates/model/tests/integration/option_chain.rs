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

use nautilus_model::{data::StrikeRange, types::Price};
use rstest::rstest;
use serde_json::{Value, json};

#[rstest]
#[case::fixed(
    StrikeRange::Fixed(vec![Price::from("95.00"), Price::from("110.00")]),
    json!({"Fixed": ["95.00", "110.00"]}),
    vec![Price::from("95.00"), Price::from("110.00")],
)]
#[case::atm_relative(
    StrikeRange::AtmRelative { strikes_above: 2, strikes_below: 1 },
    json!({"AtmRelative": {"strikes_above": 2, "strikes_below": 1}}),
    vec![Price::from("95.00"), Price::from("100.00"), Price::from("105.00"), Price::from("110.00")],
)]
#[case::atm_percent(
    StrikeRange::atm_percent(0.05).unwrap(),
    json!({"AtmPercent": {"pct": 0.05}}),
    vec![Price::from("95.00"), Price::from("100.00"), Price::from("105.00")],
)]
#[case::delta(
    StrikeRange::delta(0.25, 0.05).unwrap(),
    json!({"Delta": {"target": 0.25, "tolerance": 0.05}}),
    vec![Price::from("90.00"), Price::from("95.00"), Price::from("100.00"), Price::from("105.00"), Price::from("110.00"), Price::from("115.00")],
)]
fn test_strike_range_external_contract(
    #[case] range: StrikeRange,
    #[case] expected: Value,
    #[case] expected_strikes: Vec<Price>,
) {
    let matched = match &range {
        StrikeRange::Fixed(strikes) => json!({"Fixed": strikes}),
        StrikeRange::AtmRelative {
            strikes_above,
            strikes_below,
        } => {
            json!({"AtmRelative": {"strikes_above": strikes_above, "strikes_below": strikes_below}})
        }
        StrikeRange::AtmPercent { pct } => json!({"AtmPercent": {"pct": pct}}),
        StrikeRange::Delta { target, tolerance } => {
            json!({"Delta": {"target": target, "tolerance": tolerance}})
        }
        other => panic!("Unsupported strike range: {other:?}"),
    };

    let available = ["90.00", "95.00", "100.00", "105.00", "110.00", "115.00"].map(Price::from);
    let selected = range.resolve(Some(Price::from("100.00")), &available);
    let encoded_json = serde_json::to_value(&range).unwrap();
    let encoded_msgpack = rmp_serde::to_vec_named(&range).unwrap();
    let expected_msgpack = rmp_serde::to_vec_named(&expected).unwrap();
    let restored_json: StrikeRange = serde_json::from_value(expected.clone()).unwrap();
    let restored_msgpack: StrikeRange = rmp_serde::from_slice(&expected_msgpack).unwrap();

    assert_eq!(matched, expected);
    assert_eq!(selected, expected_strikes);
    assert_eq!(encoded_json, expected);
    assert_eq!(encoded_msgpack, expected_msgpack);
    assert_eq!(restored_json, range);
    assert_eq!(restored_msgpack, range);
}

#[rstest]
#[case(f64::NAN, "invalid f64 for 'pct', was NaN")]
#[case(f64::INFINITY, "invalid f64 for 'pct', was inf")]
#[case(f64::NEG_INFINITY, "invalid f64 for 'pct', was -inf")]
#[case(-0.1, "invalid f64 for 'pct' negative, was -0.1")]
#[case(-1e-16, "invalid f64 for 'pct' negative, was -0.0000000000000001")]
fn test_strike_range_rejects_invalid_atm_percent(#[case] pct: f64, #[case] expected: &str) {
    let error = StrikeRange::atm_percent(pct).unwrap_err();
    assert_eq!(error.to_string(), expected);
}

#[rstest]
#[case(f64::NAN)]
#[case(f64::INFINITY)]
#[case(f64::NEG_INFINITY)]
#[case(-0.1)]
#[case(-f64::MIN_POSITIVE)]
#[case(1.0_f64.next_up())]
#[case(1.1)]
#[case(2.0)]
fn test_strike_range_rejects_invalid_delta(#[case] value: f64) {
    let target = StrikeRange::delta(value, 0.05).unwrap_err();

    let tolerance = StrikeRange::delta(0.25, value).unwrap_err();

    assert_eq!(target.to_string(), "target must be finite and in [0, 1]");
    assert_eq!(
        tolerance.to_string(),
        "tolerance must be finite and in [0, 1]"
    );
}

#[rstest]
#[case(0.0)]
#[case(-0.0)]
#[case(f64::MIN_POSITIVE)]
#[case(0.25)]
#[case(1.0)]
fn test_strike_range_accepts_numeric_boundaries(#[case] value: f64) {
    let percent = StrikeRange::atm_percent(value).unwrap();
    let delta = StrikeRange::delta(value, value).unwrap();

    assert_eq!(percent, StrikeRange::AtmPercent { pct: value });
    assert_eq!(
        delta,
        StrikeRange::Delta {
            target: value,
            tolerance: value
        }
    );
}

#[rstest]
fn test_strike_range_accepts_large_finite_atm_percent() {
    let range = StrikeRange::atm_percent(f64::MAX).unwrap();
    assert_eq!(range, StrikeRange::AtmPercent { pct: f64::MAX });
}
