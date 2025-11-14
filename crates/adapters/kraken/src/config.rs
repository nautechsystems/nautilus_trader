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

//! Configuration types for Kraken data and execution clients.

use crate::common::{
    enums::{KrakenEnvironment, KrakenProductType},
    urls::{get_http_base_url, get_ws_private_url, get_ws_public_url},
};

#[derive(Debug, Clone)]
pub struct KrakenDataClientConfig {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub product_type: KrakenProductType,
    pub environment: KrakenEnvironment,
    pub base_url: Option<String>,
    pub ws_public_url: Option<String>,
    pub ws_private_url: Option<String>,
    pub http_proxy: Option<String>,
    pub ws_proxy: Option<String>,
    pub timeout_secs: Option<u64>,
    pub heartbeat_interval_secs: Option<u64>,
}

impl Default for KrakenDataClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            product_type: KrakenProductType::Spot,
            environment: KrakenEnvironment::Mainnet,
            base_url: None,
            ws_public_url: None,
            ws_private_url: None,
            http_proxy: None,
            ws_proxy: None,
            timeout_secs: Some(30),
            heartbeat_interval_secs: Some(30),
        }
    }
}

impl KrakenDataClientConfig {
    pub fn has_api_credentials(&self) -> bool {
        self.api_key.is_some() && self.api_secret.is_some()
    }

    pub fn http_base_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| get_http_base_url(self.product_type, self.environment).to_string())
    }

    pub fn ws_public_url(&self) -> String {
        self.ws_public_url
            .clone()
            .unwrap_or_else(|| get_ws_public_url(self.product_type, self.environment).to_string())
    }

    pub fn ws_private_url(&self) -> String {
        self.ws_private_url
            .clone()
            .unwrap_or_else(|| get_ws_private_url(self.product_type, self.environment).to_string())
    }
}

#[derive(Debug, Clone)]
pub struct KrakenExecClientConfig {
    pub api_key: String,
    pub api_secret: String,
    pub product_type: KrakenProductType,
    pub environment: KrakenEnvironment,
    pub base_url: Option<String>,
    pub ws_url: Option<String>,
    pub http_proxy: Option<String>,
    pub ws_proxy: Option<String>,
    pub timeout_secs: Option<u64>,
    pub heartbeat_interval_secs: Option<u64>,
}

impl KrakenExecClientConfig {
    pub fn http_base_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| get_http_base_url(self.product_type, self.environment).to_string())
    }

    pub fn ws_url(&self) -> String {
        self.ws_url
            .clone()
            .unwrap_or_else(|| get_ws_private_url(self.product_type, self.environment).to_string())
    }
}
