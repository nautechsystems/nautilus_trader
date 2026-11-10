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

use base64::prelude::*;

const PKCS8_TAG: &str = "PRIVATE KEY";

pub(super) fn rsa_pkcs1() -> String {
    encode(
        include_bytes!("../../test_data/auth_rsa_pkcs1.der"),
        "RSA PRIVATE KEY",
    )
}

pub(super) fn rsa_pkcs8() -> String {
    encode(
        include_bytes!("../../test_data/auth_rsa_pkcs8.der"),
        PKCS8_TAG,
    )
}

pub(super) fn ed25519_pkcs8() -> String {
    encode(
        include_bytes!("../../test_data/auth_ed25519_pkcs8.der"),
        PKCS8_TAG,
    )
}

pub(super) fn encrypted() -> String {
    encode(
        include_bytes!("../../test_data/auth_encrypted.der"),
        "ENCRYPTED PRIVATE KEY",
    )
}

pub(super) fn unsupported_ec() -> String {
    encode(
        include_bytes!("../../test_data/auth_unsupported_ec.der"),
        PKCS8_TAG,
    )
}

fn encode(der: &[u8], tag: &str) -> String {
    let body = BASE64_STANDARD.encode(der);
    format!("-----BEGIN {tag}-----\n{body}\n-----END {tag}-----\n")
}
