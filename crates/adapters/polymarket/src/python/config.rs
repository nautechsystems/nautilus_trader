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

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::pymethods;

use crate::{
    common::enums::SignatureType,
    config::{PolymarketDataClientConfig, PolymarketExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PolymarketDataClientConfig {
    /// Configuration for the Polymarket data client.
    #[new]
    #[pyo3(signature = (base_url_http=None, base_url_ws=None, base_url_gamma=None, base_url_data_api=None, http_timeout_secs=None, ws_timeout_secs=None, ws_max_subscriptions=None, update_instruments_interval_mins=None, subscribe_new_markets=None, auto_load_missing_instruments=None, auto_load_debounce_ms=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        base_url_gamma: Option<String>,
        base_url_data_api: Option<String>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        ws_max_subscriptions: Option<usize>,
        update_instruments_interval_mins: Option<u64>,
        subscribe_new_markets: Option<bool>,
        auto_load_missing_instruments: Option<bool>,
        auto_load_debounce_ms: Option<u64>,
    ) -> Self {
        let default = Self::default();
        Self {
            base_url_http,
            base_url_ws,
            base_url_gamma,
            base_url_data_api,
            http_timeout_secs: http_timeout_secs.unwrap_or(default.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(default.ws_timeout_secs),
            ws_max_subscriptions: ws_max_subscriptions.unwrap_or(default.ws_max_subscriptions),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(default.update_instruments_interval_mins),
            subscribe_new_markets: subscribe_new_markets.unwrap_or(false),
            auto_load_missing_instruments: auto_load_missing_instruments
                .unwrap_or(default.auto_load_missing_instruments),
            auto_load_debounce_ms: auto_load_debounce_ms.unwrap_or(default.auto_load_debounce_ms),
            filters: Vec::new(),
            new_market_filter: None,
            transport_backend: default.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PolymarketExecClientConfig {
    /// Configuration for the Polymarket execution client.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (trader_id=None, account_id=None, private_key=None, api_key=None, api_secret=None, passphrase=None, funder=None, signature_type=None, base_url_http=None, base_url_ws=None, base_url_data_api=None, http_timeout_secs=None, max_retries=None, retry_delay_initial_ms=None, retry_delay_max_ms=None, ack_timeout_secs=None))]
    fn py_new(
        trader_id: Option<String>,
        account_id: Option<String>,
        private_key: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        passphrase: Option<String>,
        funder: Option<String>,
        signature_type: Option<SignatureType>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        base_url_data_api: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        ack_timeout_secs: Option<u64>,
    ) -> Self {
        let default = Self::default();
        Self {
            trader_id: trader_id.map_or(default.trader_id, |s| TraderId::from(s.as_str())),
            account_id: account_id.map_or(default.account_id, |s| AccountId::from(s.as_str())),
            private_key,
            api_key,
            api_secret,
            passphrase,
            funder,
            signature_type: signature_type.unwrap_or(default.signature_type),
            base_url_http,
            base_url_ws,
            base_url_data_api,
            http_timeout_secs: http_timeout_secs.unwrap_or(default.http_timeout_secs),
            max_retries: max_retries.unwrap_or(default.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(default.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(default.retry_delay_max_ms),
            ack_timeout_secs: ack_timeout_secs.unwrap_or(default.ack_timeout_secs),
            transport_backend: default.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}
