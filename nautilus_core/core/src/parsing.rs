// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::ffi::c_char;

use crate::string::cstr_to_string;

/// Return the decimal precision inferred from the given string.
pub fn precision_from_str(s: &str) -> u8 {
    let lower_s = s.to_lowercase();
    // Handle scientific notation
    if lower_s.contains("e-") {
        return lower_s.split("e-").last().unwrap().parse::<u8>().unwrap();
    }
    if !lower_s.contains('.') {
        return 0;
    }
    return lower_s.split('.').last().unwrap().len() as u8;
}

/// Return the decimal precision inferred from the given C string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
/// # Panics
/// - If `ptr` is null.
#[no_mangle]
pub unsafe extern "C" fn precision_from_cstr(ptr: *const c_char) -> u8 {
    precision_from_str(&cstr_to_string(ptr))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use rstest::rstest;

    use super::*;

    #[rstest(
        s,
        expected,
        case("", 0),
        case("0", 0),
        case("1.0", 1),
        case("1.00", 2),
        case("1.23456789", 8),
        case("123456.789101112", 9),
        case("0.000000001", 9),
        case("1e-1", 1),
        case("1e-2", 2),
        case("1e-3", 3),
        case("1e8", 0)
    )]
    fn test_precision_from_str(s: &str, expected: u8) {
        let result = precision_from_str(s);
        assert_eq!(result, expected);
    }

    #[rstest(
        input,
        expected,
        case("1e8", 0),
        case("123", 0),
        case("123.45", 2),
        case("123.456789", 6),
        case("1.23456789e-2", 2),
        case("1.23456789e-12", 12)
    )]
    fn test_precision_from_cstr(input: &str, expected: u8) {
        let c_str = CString::new(input).unwrap();
        assert_eq!(unsafe { precision_from_cstr(c_str.as_ptr()) }, expected);
    }
}
