// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

const FAILED: &str = "condition check failed:";

pub fn valid_string(s: &str, desc: &str) {
    if s.is_empty() {
        panic!("{FAILED} invalid string for {desc}, was empty");
    } else if s.as_bytes().iter().all(u8::is_ascii_whitespace) {
        panic!("{FAILED} invalid string for {desc}, was all whitespace");
    } else if !s.is_ascii() {
        panic!("{FAILED} invalid string for {desc} contained a Non-ASCII char, was '{s}'");
    }
}

pub fn string_contains(s: &str, pat: &str, desc: &str) {
    if !s.contains(pat) {
        panic!("{FAILED} invalid string for {desc} did not contain '{pat}', was '{s}'");
    }
}

pub fn u8_in_range_inclusive(value: u8, l: u8, r: u8, desc: &str) {
    if !(value.ge(&l) && value.le(&r)) {
        panic!("{FAILED} invalid u8 for {desc} not in range [{l}, {r}], was {value}");
    }
}

pub fn u64_in_range_inclusive(value: u64, l: u64, r: u64, desc: &str) {
    if !(value.ge(&l) && value.le(&r)) {
        panic!("{FAILED} invalid u64 for {desc} not in range [{l}, {r}], was {value}");
    }
}

pub fn i64_in_range_inclusive(value: i64, l: i64, r: i64, desc: &str) {
    if !(value.ge(&l) && value.le(&r)) {
        panic!("{FAILED} invalid i64 for {desc} not in range [{l}, {r}], was {value}");
    }
}

pub fn f64_in_range_inclusive(value: f64, l: f64, r: f64, desc: &str) {
    if !(value.ge(&l) && value.le(&r)) {
        panic!("{FAILED} invalid f64 for {desc} not in range [{l}, {r}], was {value}");
    }
}

pub fn f64_non_negative(value: f64, desc: &str) {
    if value < 0.0 {
        panic!("{FAILED} invalid f64 for {desc} negative, was {value}");
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::correctness;
    use rstest::*;

    #[rstest]
    #[case(" a")]
    #[case("a ")]
    #[case("a a")]
    #[case(" a ")]
    #[case("abc")]
    fn test_valid_string_with_valid_value(#[case] s: &str) {
        correctness::valid_string(s, "value");
    }

    #[rstest]
    #[case("")] // <-- empty
    #[case(" ")] // <-- all whitespace
    #[case("  ")] // <-- all whitespace
    #[case("🦀")] // <-- Non-ASCII char
    #[should_panic]
    fn test_valid_string_with_invalid_values(#[case] s: &str) {
        correctness::valid_string(s, "value");
    }

    #[rstest]
    #[case("a", "a")]
    fn test_string_contains_when_it_does_contain(#[case] s: &str, #[case] pat: &str) {
        correctness::string_contains(s, pat, "value");
    }

    #[rstest]
    #[case("a", "b")]
    #[should_panic]
    fn test_string_contains_with_invalid_values(#[case] s: &str, #[case] pat: &str) {
        correctness::string_contains(s, pat, "value");
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_u8_in_range_inclusive_when_valid_values(
        #[case] value: u8,
        #[case] l: u8,
        #[case] r: u8,
        #[case] desc: &str,
    ) {
        correctness::u8_in_range_inclusive(value, l, r, desc);
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    #[should_panic]
    fn test_u8_in_range_inclusive_when_invalid_values(
        #[case] value: u8,
        #[case] l: u8,
        #[case] r: u8,
        #[case] desc: &str,
    ) {
        correctness::u8_in_range_inclusive(value, l, r, desc);
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_u64_in_range_inclusive_when_valid_values(
        #[case] value: u64,
        #[case] l: u64,
        #[case] r: u64,
        #[case] desc: &str,
    ) {
        correctness::u64_in_range_inclusive(value, l, r, desc);
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    #[should_panic]
    fn test_u64_in_range_inclusive_when_invalid_values(
        #[case] value: u64,
        #[case] l: u64,
        #[case] r: u64,
        #[case] desc: &str,
    ) {
        correctness::u64_in_range_inclusive(value, l, r, desc);
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_i64_in_range_inclusive_when_valid_values(
        #[case] value: i64,
        #[case] l: i64,
        #[case] r: i64,
        #[case] desc: &str,
    ) {
        correctness::i64_in_range_inclusive(value, l, r, desc);
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    #[should_panic]
    fn test_i64_in_range_inclusive_when_invalid_values(
        #[case] value: i64,
        #[case] l: i64,
        #[case] r: i64,
        #[case] desc: &str,
    ) {
        correctness::i64_in_range_inclusive(value, l, r, desc);
    }

    #[rstest]
    #[case(0.0, "value")]
    #[case(1.0, "value")]
    fn test_f64_non_negative_when_valid_values(#[case] value: f64, #[case] desc: &str) {
        correctness::f64_non_negative(value, desc);
    }

    #[rstest]
    #[case(-0.1, "value")]
    #[should_panic]
    fn test_f64_non_negative_when_invalid_values(#[case] value: f64, #[case] desc: &str) {
        correctness::f64_non_negative(value, desc);
    }
}
