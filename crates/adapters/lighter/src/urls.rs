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

//! Helpers for resolving HTTP/WebSocket base URLs.

use crate::common::LighterNetwork;
use crate::common::constants::{
    LIGHTER_MAINNET_HTTP_BASE, LIGHTER_MAINNET_WS_BASE, LIGHTER_TESTNET_HTTP_BASE,
    LIGHTER_TESTNET_WS_BASE,
};

#[must_use]
pub fn get_http_base_url(network: LighterNetwork, override_url: Option<&str>) -> String {
    override_url.map_or_else(
        || match network {
            LighterNetwork::Mainnet => format!("{LIGHTER_MAINNET_HTTP_BASE}/api/v1"),
            LighterNetwork::Testnet => format!("{LIGHTER_TESTNET_HTTP_BASE}/api/v1"),
        },
        str::to_owned,
    )
}

#[must_use]
pub fn get_ws_url(network: LighterNetwork, override_url: Option<&str>) -> String {
    override_url.map_or_else(
        || match network {
            LighterNetwork::Mainnet => LIGHTER_MAINNET_WS_BASE.to_string(),
            LighterNetwork::Testnet => LIGHTER_TESTNET_WS_BASE.to_string(),
        },
        str::to_owned,
    )
}
