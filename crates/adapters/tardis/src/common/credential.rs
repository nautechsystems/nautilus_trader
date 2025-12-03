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

//! Tardis API credential storage.

use std::fmt::{Debug, Formatter};

use zeroize::ZeroizeOnDrop;

/// API credentials required for Tardis API requests.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    api_key: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance from the API key.
    #[must_use]
    pub fn new(api_key: impl Into<String>) -> Self {
        let api_key_bytes = api_key.into().into_bytes();

        Self {
            api_key: api_key_bytes.into_boxed_slice(),
        }
    }

    /// Returns the API key associated with this credential.
    ///
    /// # Panics
    ///
    /// This method should never panic as the API key is always valid UTF-8,
    /// having been created from a String.
    #[must_use]
    pub fn api_key(&self) -> &str {
        // SAFETY: The API key is always valid UTF-8 since it was created from a String
        std::str::from_utf8(&self.api_key).unwrap()
    }

    /// Returns a masked version of the API key for logging purposes.
    ///
    /// Shows first 4 and last 4 characters with ellipsis in between.
    /// For keys shorter than 8 characters, shows asterisks only.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        nautilus_core::string::mask_api_key(self.api_key())
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
    fn test_api_key_masked_short() {
        let credential = Credential::new("short");
        assert_eq!(credential.api_key_masked(), "*****");
    }

    #[rstest]
    fn test_api_key_masked_long() {
        let credential = Credential::new("abcdefghijklmnop");
        assert_eq!(credential.api_key_masked(), "abcd...mnop");
    }

    #[rstest]
    fn test_debug_redaction() {
        let credential = Credential::new("test_api_key");
        let debug_str = format!("{credential:?}");
        assert!(debug_str.contains("<redacted>"));
        assert!(!debug_str.contains("test_api_key"));
    }
}
