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

//! Helpers for owning, redacting, and masking secrets in strings.

use std::fmt::Debug;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Placeholder used in `Debug` impls to redact secret fields.
pub const REDACTED: &str = "<redacted>";

/// An owned string that zeroizes its allocation on drop and redacts its debug output.
///
/// Serialization intentionally emits the underlying value for wire-format compatibility. Do not
/// serialize this type into logs or other outputs where secrets must remain redacted.
///
/// Equality uses ordinary string comparison and is not constant-time. Do not use it to verify an
/// attacker-controlled secret.
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    /// Exposes the secret value as a string slice.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }

    /// Consumes this value and returns the secret string.
    #[must_use]
    pub fn into_inner(mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(REDACTED)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Replaces a present value with [`REDACTED`] while preserving absence.
///
/// This is intended for optional secret fields in `Debug` implementations.
#[must_use]
pub const fn redact_option<T: ?Sized>(value: Option<&T>) -> Option<&'static str> {
    match value {
        Some(_) => Some(REDACTED),
        None => None,
    }
}

/// Zeroizes every owned string contained in a JSON value.
pub fn zeroize_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(value) => value.zeroize(),
        serde_json::Value::Array(values) => values.iter_mut().for_each(zeroize_json_value),
        serde_json::Value::Object(values) => values.values_mut().for_each(zeroize_json_value),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

/// Masks an API key by showing only the first and last 4 characters.
///
/// For keys 8 characters or shorter, returns asterisks only.
///
/// # Examples
///
/// ```
/// use nautilus_core::string::secret::mask_api_key;
///
/// assert_eq!(mask_api_key("abcdefghijklmnop"), "abcd...mnop");
/// assert_eq!(mask_api_key("short"), "*****");
/// ```
#[must_use]
pub fn mask_api_key(key: &str) -> String {
    // Work with Unicode scalars to avoid panicking on multibyte characters.
    let chars: Vec<char> = key.chars().collect();
    let len = chars.len();

    if len <= 8 {
        return "*".repeat(len);
    }

    let first: String = chars[..4].iter().collect();
    let last: String = chars[len - 4..].iter().collect();

    format!("{first}...{last}")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use zeroize::ZeroizeOnDrop;

    use super::*;

    fn assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}

    #[rstest]
    fn test_secret_string_redacts_debug() {
        let value = SecretString::from("session-secret");

        assert_eq!(value.expose_secret(), "session-secret");
        assert_eq!(format!("{value:?}"), REDACTED);
        assert_zeroize_on_drop::<SecretString>();
    }

    #[rstest]
    fn test_secret_string_serde_is_transparent() {
        let value = SecretString::from("session-secret");

        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: SecretString = serde_json::from_str(&serialized).unwrap();

        assert_eq!(serialized, r#""session-secret""#);
        assert_eq!(deserialized.expose_secret(), "session-secret");
    }

    #[rstest]
    fn test_zeroize_json_value_clears_nested_strings() {
        let mut value = serde_json::json!({
            "secret": "top-level",
            "nested": ["array-value", {"secret": "nested-value"}],
            "number": 42,
        });

        zeroize_json_value(&mut value);

        assert_eq!(value["secret"], "");
        assert_eq!(value["nested"][0], "");
        assert_eq!(value["nested"][1]["secret"], "");
        assert_eq!(value["number"], 42);
    }

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

    #[rstest]
    fn test_redact_option_present() {
        assert_eq!(redact_option(Some("secret")), Some(REDACTED));
    }

    #[rstest]
    fn test_redact_option_absent() {
        assert_eq!(redact_option(None::<&str>), None);
    }
}
