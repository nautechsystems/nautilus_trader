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
use ustr::Ustr;

/// BitMEX API credentials for signing requests.
///
/// Uses HMAC SHA256 for request signing as per BitMEX API specifications.
#[derive(Debug, Clone)]
pub struct Credential {
    pub api_key: Ustr,
    api_secret: Vec<u8>,
}

impl Credential {
    /// Creates a new [`Credential`] instance.
    #[must_use]
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            api_key: api_key.into(),
            api_secret: api_secret.into_bytes(),
        }
    }

    /// Signs a request message according to the BitMEX authentication scheme.
    pub fn sign(&self, verb: &str, endpoint: &str, expires: i64, data: &str) -> String {
        let sign_message = format!("{verb}{endpoint}{expires}{data}");
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret);
        let signature = hmac::sign(&key, sign_message.as_bytes());
        hex::encode(signature.as_ref())
    }
}

/// Tests use examples from https://www.bitmex.com/app/apiKeysUsage.
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const API_KEY: &str = "LAqUlngMIQkIUjXMUreyu3qn";
    const API_SECRET: &str = "chNOOS4KvNXR_Xq4k4c9qsfoKWvnDecLATCRlcBwyKDYnWgO";

    #[rstest]
    fn test_simple_get() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string());

        let signature = credential.sign("GET", "/api/v1/instrument", 1518064236, "");

        assert_eq!(
            signature,
            "c7682d435d0cfe87c16098df34ef2eb5a549d4c5a3c2b1f0f77b8af73423bf00"
        );
    }

    #[rstest]
    fn test_get_with_query() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string());

        let signature = credential.sign(
            "GET",
            "/api/v1/instrument?filter=%7B%22symbol%22%3A+%22XBTM15%22%7D",
            1518064237,
            "",
        );

        assert_eq!(
            signature,
            "e2f422547eecb5b3cb29ade2127e21b858b235b386bfa45e1c1756eb3383919f"
        );
    }

    #[rstest]
    fn test_post_with_data() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string());

        let data = r#"{"symbol":"XBTM15","price":219.0,"clOrdID":"mm_bitmex_1a/oemUeQ4CAJZgP3fjHsA","orderQty":98}"#;

        let signature = credential.sign("POST", "/api/v1/order", 1518064238, data);

        assert_eq!(
            signature,
            "1749cd2ccae4aa49048ae09f0b95110cee706e0944e6a14ad0b3a8cb45bd336b"
        );
    }
}
