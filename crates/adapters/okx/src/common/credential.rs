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

use aws_lc_rs::hmac;
use base64::prelude::*;
use ustr::Ustr;

/// OKX API credentials for signing requests.
///
/// Uses HMAC SHA256 for request signing as per OKX API specifications.
#[derive(Debug, Clone)]
pub struct Credential {
    pub api_key: Ustr,
    pub api_passphrase: Ustr,
    api_secret: Vec<u8>,
}

impl Credential {
    /// Creates a new [`Credential`] instance.
    #[must_use]
    pub fn new(api_key: String, api_secret: String, api_passphrase: String) -> Self {
        Self {
            api_key: api_key.into(),
            api_passphrase: api_passphrase.into(),
            api_secret: api_secret.into_bytes(),
        }
    }

    /// Signs a request message according to the OKX authentication scheme.
    pub fn sign(&self, timestamp: &str, method: &str, endpoint: &str, body: &str) -> String {
        let message = format!("{timestamp}{method}{endpoint}{body}");
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret);
        let tag = hmac::sign(&key, message.as_bytes());
        BASE64_STANDARD.encode(tag.as_ref())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const API_KEY: &str = "985d5b66-57ce-40fb-b714-afc0b9787083";
    const API_SECRET: &str = "chNOOS4KvNXR_Xq4k4c9qsfoKWvnDecLATCRlcBwyKDYnWgO";
    const API_PASSPHRASE: &str = "1234567890";

    #[rstest]
    fn test_simple_get() {
        let credential = Credential::new(
            API_KEY.to_string(),
            API_SECRET.to_string(),
            API_PASSPHRASE.to_string(),
        );

        let signature = credential.sign(
            "2020-12-08T09:08:57.715Z",
            "GET",
            "/api/v5/account/balance",
            "",
        );

        assert_eq!(signature, "PJ61e1nb2F2Qd7D8SPiaIcx2gjdELc+o0ygzre9z33k=");
    }
}
