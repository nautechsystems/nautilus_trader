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

//! Factory functions for creating BitMEX clients and components.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::CacheView,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{AccountId, ClientId, TraderId},
};

use crate::{
    common::consts::{BITMEX, BITMEX_VENUE},
    config::{BitmexDataClientConfig, BitmexExecutionClientConfig},
    data::BitmexDataClient,
    execution::BitmexExecutionClient,
};

impl ClientConfig for BitmexDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for BitmexExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating BitMEX data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.bitmex", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.bitmex")
)]
pub struct BitmexDataClientFactory;

impl BitmexDataClientFactory {
    /// Creates a new [`BitmexDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BitmexDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for BitmexDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let bitmex_config = config
            .as_any()
            .downcast_ref::<BitmexDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BitmexDataClientFactory. Expected BitmexDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = BitmexDataClient::new(client_id, bitmex_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        BITMEX
    }

    fn config_type(&self) -> &'static str {
        "BitmexDataClientConfig"
    }
}

/// Factory for creating BitMEX execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.bitmex", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.bitmex")
)]
pub struct BitmexExecutionClientFactory;

impl BitmexExecutionClientFactory {
    /// Creates a new [`BitmexExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BitmexExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for BitmexExecutionClientFactory {
    fn create(
        &self,
        trader_id: TraderId,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let mut bitmex_config = config
            .as_any()
            .downcast_ref::<BitmexExecutionClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BitmexExecutionClientFactory. Expected BitmexExecutionClientConfig, was {config:?}",
                )
            })?
            .clone();

        let account_id = bitmex_config
            .account_id
            .unwrap_or_else(|| AccountId::from("BITMEX-001"));
        bitmex_config.account_id = Some(account_id);

        let core = ExecutionClientCore::new(
            trader_id,
            ClientId::from(name),
            *BITMEX_VENUE,
            OmsType::Netting,
            account_id,
            AccountType::Margin,
            None, // base_currency
            cache,
        );

        let client = BitmexExecutionClient::new(core, bitmex_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        BITMEX
    }

    fn config_type(&self) -> &'static str {
        "BitmexExecutionClientConfig"
    }
}
