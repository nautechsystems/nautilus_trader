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

//! String manipulation functionality.

/// Masks an API key by showing only the first and last 4 characters.
///
/// For keys 8 characters or shorter, returns asterisks only.
///
/// # Examples
///
/// ```
/// use nautilus_core::string::mask_api_key;
///
/// assert_eq!(mask_api_key("abcdefghijklmnop"), "abcd...mnop");
/// assert_eq!(mask_api_key("short"), "*****");
/// ```
#[must_use]
pub fn mask_api_key(key: &str) -> String {
    let len = key.len();
    if len <= 8 {
        "*".repeat(len)
    } else {
        format!("{}...{}", &key[..4], &key[len - 4..])
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("", "")]
    #[case("a", "*")]
    #[case("abc", "***")]
    #[case("abcdefgh", "********")]
    #[case("abcdefghi", "abcd...fghi")]
    #[case("abcdefghijklmnop", "abcd...mnop")]
    #[case("VeryLongAPIKey123456789", "Very...6789")]
    fn test_mask_api_key(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(mask_api_key(input), expected);
    }
}
