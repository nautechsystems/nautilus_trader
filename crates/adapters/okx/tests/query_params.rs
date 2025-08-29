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

use nautilus_okx::http::query::GetCandlesticksParamsBuilder;
use rstest::rstest;

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[rstest]
fn test_optional_parameters_are_omitted_when_none() {
    let mut builder = GetCandlesticksParamsBuilder::default();
    builder.inst_id("BTC-USDT-SWAP");
    builder.bar("1m");

    let params = builder.build().unwrap();
    let qs = serde_urlencoded::to_string(&params).unwrap();
    assert_eq!(
        qs, "instId=BTC-USDT-SWAP&bar=1m",
        "unexpected optional parameters were serialized: {qs}"
    );
}

#[rstest]
fn test_no_literal_none_strings_leak_into_query_string() {
    let mut builder = GetCandlesticksParamsBuilder::default();
    builder.inst_id("BTC-USDT-SWAP");
    builder.bar("1m");

    let params = builder.build().unwrap();
    let qs = serde_urlencoded::to_string(&params).unwrap();
    assert!(
        !qs.contains("None"),
        "found literal \"None\" in query string: {qs}"
    );
    assert!(
        !qs.contains("after=") && !qs.contains("before=") && !qs.contains("limit="),
        "empty optional parameters must be omitted entirely: {qs}"
    );
}

#[rstest]
fn test_cursor_nanoseconds_rejected() {
    // 2025-07-01T00:00:00Z in *nanoseconds* on purpose.
    let after_nanos = 1_725_307_200_000_000_000i64;

    let mut builder = GetCandlesticksParamsBuilder::default();
    builder.inst_id("BTC-USDT-SWAP");
    builder.bar("1m");
    builder.after_ms(after_nanos);

    // This should fail because nanoseconds > 13 digits
    let result = builder.build();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("nanoseconds"));
}

#[rstest]
fn test_both_cursors_rejected() {
    let mut builder = GetCandlesticksParamsBuilder::default();
    builder.inst_id("BTC-USDT-SWAP");
    builder.bar("1m");
    builder.after_ms(1725307200000);
    builder.before_ms(1725393600000);

    // Both cursors should be rejected
    let result = builder.build();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("both"));
}

#[rstest]
fn test_limit_exceeds_maximum_rejected() {
    let mut builder = GetCandlesticksParamsBuilder::default();
    builder.inst_id("BTC-USDT-SWAP");
    builder.bar("1m");
    builder.limit(301u32); // Exceeds maximum limit

    // Limit should be rejected
    let result = builder.build();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("300"));
}

#[rstest]
#[case(1725307200000, "after=1725307200000")] // 13 digits = milliseconds
#[case(1725307200, "after=1725307200")] // 10 digits = seconds
#[case(1725307, "after=1725307")] // 7 digits = also valid
fn test_valid_millisecond_cursor_passes(#[case] timestamp: i64, #[case] expected: &str) {
    let mut builder = GetCandlesticksParamsBuilder::default();
    builder.inst_id("BTC-USDT-SWAP");
    builder.bar("1m");
    builder.after_ms(timestamp);

    let params = builder.build().unwrap();
    let qs = serde_urlencoded::to_string(&params).unwrap();
    assert!(qs.contains(expected));
}

#[rstest]
#[case(1, "limit=1")]
#[case(50, "limit=50")]
#[case(100, "limit=100")]
#[case(300, "limit=300")] // Maximum allowed limit
fn test_valid_limit_passes(#[case] limit: u32, #[case] expected: &str) {
    let mut builder = GetCandlesticksParamsBuilder::default();
    builder.inst_id("BTC-USDT-SWAP");
    builder.bar("1m");
    builder.limit(limit);

    let params = builder.build().unwrap();
    let qs = serde_urlencoded::to_string(&params).unwrap();
    assert!(qs.contains(expected));
}
