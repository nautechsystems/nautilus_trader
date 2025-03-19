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

use base64::prelude::*;
use hex;
use ring::{
    hmac,
    rand::SystemRandom,
    signature::{Ed25519KeyPair, RSA_PKCS1_SHA256, RsaKeyPair, Signature},
};

#[must_use]
pub fn hmac_signature(secret: &str, data: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let signature = hmac::sign(&key, data.as_bytes());
    hex::encode(signature.as_ref())
}

pub fn rsa_signature(private_key_pem: &str, data: &str) -> anyhow::Result<String> {
    if data.is_empty() {
        anyhow::bail!("Query string cannot be empty");
    }

    let pem = pem::parse(private_key_pem)?;
    let private_key =
        RsaKeyPair::from_pkcs8(pem.contents()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let mut signature = vec![0; private_key.public().modulus_len()];
    let rng = SystemRandom::new();

    private_key
        .sign(&RSA_PKCS1_SHA256, &rng, data.as_bytes(), &mut signature)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;

    Ok(BASE64_STANDARD.encode(&signature))
}

pub fn ed25519_signature(private_key: &[u8], data: &str) -> anyhow::Result<String> {
    let key_pair =
        Ed25519KeyPair::from_seed_unchecked(private_key).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let signature: Signature = key_pair.sign(data.as_bytes());
    Ok(hex::encode(signature.as_ref()))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        "mysecretkey",
        "data-to-sign",
        "19ed21a8b2a6b847d7d7aea059ab3134cd58f13c860cfbe89338c718685fe077"
    )]
    #[case(
        "anothersecretkey",
        "somedata",
        "fb44dab41435775b44a96aa008af58cbf1fa1cea32f4605562c586b98f7326c5"
    )]
    #[case(
        "",
        "data-without-secret",
        "740c92f9c332fbb22d80aa6a3c9c10197a3e9dc61ca7e3c298c21597e4672133"
    )]
    #[case(
        "mysecretkey",
        "",
        "bb4e89236de3b03c17e36d48ca059fa277b88165cb14813a49f082ed8974b9f4"
    )]
    #[case(
        "",
        "",
        "b613679a0814d9ec772f95d778c35fc5ff1697c493715653c6c712144292c5ad"
    )]
    fn test_hmac_signature(
        #[case] secret: &str,
        #[case] data: &str,
        #[case] expected_signature: &str,
    ) {
        let result = hmac_signature(secret, data);
        assert_eq!(
            result, expected_signature,
            "Expected signature did not match"
        );
    }

    #[rstest]
    #[case(
        r"-----BEGIN TEST KEY-----
MIIBVwIBADANBgkqhkiG9w0BAQEFAASCATswggE3AgEAAkEAu/...
-----END PRIVATE KEY-----",
        ""
    )]
    fn test_rsa_signature_empty_query(#[case] private_key_pem: &str, #[case] query_string: &str) {
        let result = rsa_signature(private_key_pem, query_string);
        assert!(
            result.is_err(),
            "Expected an error with empty query string, but got Ok"
        );
    }

    #[rstest]
    #[case(
        r"-----BEGIN INVALID KEY-----
INVALID_KEY_DATA
-----END INVALID KEY-----",
        "This is a test query"
    )]
    fn test_rsa_signature_invalid_key(#[case] private_key_pem: &str, #[case] query_string: &str) {
        let result = rsa_signature(private_key_pem, query_string);
        assert!(
            result.is_err(),
            "Expected an error due to invalid key, but got Ok"
        );
    }

    const fn valid_ed25519_private_key() -> [u8; 32] {
        [
            0x0c, 0x74, 0x18, 0x92, 0x6b, 0x5d, 0xe9, 0x8f, 0xe2, 0xb6, 0x47, 0x8a, 0x51, 0xf9,
            0x97, 0x31, 0x9a, 0xcd, 0x2d, 0xbc, 0xf9, 0x94, 0xea, 0x8f, 0xc3, 0x1b, 0x65, 0x24,
            0x1f, 0x91, 0xd8, 0x6f,
        ]
    }

    #[rstest]
    #[case(valid_ed25519_private_key(), "This is a test query")]
    #[case(valid_ed25519_private_key(), "")]
    fn test_ed25519_signature(#[case] private_key_bytes: [u8; 32], #[case] query_string: &str) {
        let result = ed25519_signature(&private_key_bytes, query_string);
        assert!(
            result.is_ok(),
            "Expected valid signature but got an error: {result:?}"
        );
        assert!(!result.unwrap().is_empty(), "Signature should not be empty");
    }
}
