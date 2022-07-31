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

pub fn is_valid_string(s: &str) -> bool {
    return !s.is_empty() & !s.as_bytes().iter().any(u8::is_ascii_whitespace);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::correctness::is_valid_string;
    use rstest::*;

    #[test]
    fn test_with_valid_value() {
        let value = String::from("abcd");

        assert!(is_valid_string(&value));
    }

    #[rstest]
    #[case("")]
    #[case(" ")]
    #[case("  ")]
    fn test_with_invalid_values(#[case] value: &str) {
        assert!(!is_valid_string(value));
    }
}
