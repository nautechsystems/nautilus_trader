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

pub fn valid_string(s: &str, desc: &str) {
    if s.is_empty() {
        panic!("invalid {desc} string, was empty");
    } else if s.as_bytes().iter().all(u8::is_ascii_whitespace) {
        panic!("invalid {desc} string, was '{s}'");
    }
}

pub fn string_contains(s: &str, pat: &str, desc: &str) {
    if !s.contains(pat) {
        panic!("invalid {desc} string which did not contain '{pat}', was '{s}'");
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
    #[case(" a ")]
    fn test_valid_string_with_valid_value(#[case] s: &str) {
        correctness::valid_string(s, "value");
    }

    #[rstest]
    #[case("")]
    #[case(" ")]
    #[case("  ")]
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
}
