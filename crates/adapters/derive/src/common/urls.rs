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

use super::{
    consts::{REST_URL_MAINNET, REST_URL_TESTNET, WS_URL_MAINNET, WS_URL_TESTNET},
    enums::DeriveEnvironment,
};

#[must_use]
pub fn rest_url(environment: DeriveEnvironment) -> &'static str {
    match environment {
        DeriveEnvironment::Mainnet => REST_URL_MAINNET,
        DeriveEnvironment::Testnet => REST_URL_TESTNET,
    }
}

#[must_use]
pub fn ws_url(environment: DeriveEnvironment) -> &'static str {
    match environment {
        DeriveEnvironment::Mainnet => WS_URL_MAINNET,
        DeriveEnvironment::Testnet => WS_URL_TESTNET,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_rest_url_routes_by_environment() {
        assert_eq!(rest_url(DeriveEnvironment::Mainnet), REST_URL_MAINNET);
        assert_eq!(rest_url(DeriveEnvironment::Testnet), REST_URL_TESTNET);
    }

    #[rstest]
    fn test_ws_url_routes_by_environment() {
        assert_eq!(ws_url(DeriveEnvironment::Mainnet), WS_URL_MAINNET);
        assert_eq!(ws_url(DeriveEnvironment::Testnet), WS_URL_TESTNET);
    }
}
