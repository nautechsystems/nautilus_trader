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

//! URL helpers for BitMEX services.

use super::{
    consts::{BITMEX_HTTP_TESTNET_URL, BITMEX_HTTP_URL, BITMEX_WS_TESTNET_URL, BITMEX_WS_URL},
    enums::BitmexEnvironment,
};

/// Gets the BitMEX HTTP base URL.
pub fn get_http_base_url(environment: BitmexEnvironment) -> String {
    match environment {
        BitmexEnvironment::Testnet => BITMEX_HTTP_TESTNET_URL.to_string(),
        BitmexEnvironment::Mainnet => BITMEX_HTTP_URL.to_string(),
    }
}

/// Gets the BitMEX WebSocket URL.
pub fn get_ws_url(environment: BitmexEnvironment) -> String {
    match environment {
        BitmexEnvironment::Testnet => BITMEX_WS_TESTNET_URL.to_string(),
        BitmexEnvironment::Mainnet => BITMEX_WS_URL.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::enums::BitmexEnvironment;

    #[rstest]
    fn test_http_urls() {
        assert_eq!(
            get_http_base_url(BitmexEnvironment::Mainnet),
            "https://www.bitmex.com/api/v1"
        );
        assert_eq!(
            get_http_base_url(BitmexEnvironment::Testnet),
            "https://testnet.bitmex.com/api/v1"
        );
    }

    #[rstest]
    fn test_ws_urls() {
        assert_eq!(
            get_ws_url(BitmexEnvironment::Mainnet),
            "wss://ws.bitmex.com/realtime"
        );
        assert_eq!(
            get_ws_url(BitmexEnvironment::Testnet),
            "wss://ws.testnet.bitmex.com/realtime"
        );
    }
}
