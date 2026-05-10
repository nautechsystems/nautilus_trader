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
    consts::{
        REST_URL, REST_URL_SANDBOX, WS_URL, WS_URL_SANDBOX, WS_USER_URL, WS_USER_URL_SANDBOX,
    },
    enums::CoinbaseEnvironment,
};

pub fn rest_url(environment: CoinbaseEnvironment) -> &'static str {
    match environment {
        CoinbaseEnvironment::Live => REST_URL,
        CoinbaseEnvironment::Sandbox => REST_URL_SANDBOX,
    }
}

pub fn ws_url(environment: CoinbaseEnvironment) -> &'static str {
    match environment {
        CoinbaseEnvironment::Live => WS_URL,
        CoinbaseEnvironment::Sandbox => WS_URL_SANDBOX,
    }
}

pub fn ws_user_url(environment: CoinbaseEnvironment) -> &'static str {
    match environment {
        CoinbaseEnvironment::Live => WS_USER_URL,
        CoinbaseEnvironment::Sandbox => WS_USER_URL_SANDBOX,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_rest_url_live() {
        assert_eq!(rest_url(CoinbaseEnvironment::Live), REST_URL);
    }

    #[rstest]
    fn test_rest_url_sandbox() {
        assert_eq!(rest_url(CoinbaseEnvironment::Sandbox), REST_URL_SANDBOX);
    }

    #[rstest]
    fn test_ws_url_live() {
        assert_eq!(ws_url(CoinbaseEnvironment::Live), WS_URL);
    }

    #[rstest]
    fn test_ws_url_sandbox() {
        assert_eq!(ws_url(CoinbaseEnvironment::Sandbox), WS_URL_SANDBOX);
    }

    #[rstest]
    fn test_ws_user_url_live() {
        assert_eq!(ws_user_url(CoinbaseEnvironment::Live), WS_USER_URL);
    }

    #[rstest]
    fn test_ws_user_url_sandbox() {
        assert_eq!(
            ws_user_url(CoinbaseEnvironment::Sandbox),
            WS_USER_URL_SANDBOX
        );
    }
}
