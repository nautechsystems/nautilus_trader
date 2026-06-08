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

//! Base URL resolution for Lighter REST and WebSocket endpoints.

use super::{
    consts::{LIGHTER_MAINNET_CHAIN_ID, LIGHTER_TESTNET_CHAIN_ID},
    enums::LighterEnvironment,
};

const LIGHTER_MAINNET_HTTP_URL: &str = "https://mainnet.zklighter.elliot.ai";
const LIGHTER_MAINNET_WS_URL: &str = "wss://mainnet.zklighter.elliot.ai/stream";

const LIGHTER_TESTNET_HTTP_URL: &str = "https://testnet.zklighter.elliot.ai";
const LIGHTER_TESTNET_WS_URL: &str = "wss://testnet.zklighter.elliot.ai/stream";

/// Returns the REST base URL for the given environment.
#[must_use]
pub const fn lighter_http_base_url(environment: LighterEnvironment) -> &'static str {
    match environment {
        LighterEnvironment::Mainnet => LIGHTER_MAINNET_HTTP_URL,
        LighterEnvironment::Testnet => LIGHTER_TESTNET_HTTP_URL,
    }
}

/// Returns the WebSocket URL for the given environment.
#[must_use]
pub const fn lighter_ws_url(environment: LighterEnvironment) -> &'static str {
    match environment {
        LighterEnvironment::Mainnet => LIGHTER_MAINNET_WS_URL,
        LighterEnvironment::Testnet => LIGHTER_TESTNET_WS_URL,
    }
}

/// Returns the L2 chain id for the given environment.
///
/// The chain id is the first element of every signed transaction's hash
/// preimage and must match the value the sequencer expects, otherwise
/// signatures verify against a different message and are rejected.
#[must_use]
pub const fn lighter_chain_id(environment: LighterEnvironment) -> u32 {
    match environment {
        LighterEnvironment::Mainnet => LIGHTER_MAINNET_CHAIN_ID,
        LighterEnvironment::Testnet => LIGHTER_TESTNET_CHAIN_ID,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_http_base_url() {
        assert_eq!(
            lighter_http_base_url(LighterEnvironment::Mainnet),
            LIGHTER_MAINNET_HTTP_URL,
        );
        assert_eq!(
            lighter_http_base_url(LighterEnvironment::Testnet),
            LIGHTER_TESTNET_HTTP_URL,
        );
    }

    #[rstest]
    fn test_ws_url() {
        assert_eq!(
            lighter_ws_url(LighterEnvironment::Mainnet),
            LIGHTER_MAINNET_WS_URL,
        );
        assert_eq!(
            lighter_ws_url(LighterEnvironment::Testnet),
            LIGHTER_TESTNET_WS_URL,
        );
    }

    #[rstest]
    fn test_chain_id() {
        assert_eq!(lighter_chain_id(LighterEnvironment::Mainnet), 304);
        assert_eq!(lighter_chain_id(LighterEnvironment::Testnet), 300);
    }
}
