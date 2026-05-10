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

//! Factory functions for creating Bullet clients and components.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
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
    common::consts::BULLET_VENUE,
    config::{BulletDataClientConfig, BulletExecClientConfig},
    data::BulletDataClient,
    execution::BulletExecutionClient,
};

impl ClientConfig for BulletDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for BulletExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Bullet data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.bullet",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bullet")
)]
pub struct BulletDataClientFactory;

impl BulletDataClientFactory {
    /// Creates a new [`BulletDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BulletDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for BulletDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let bullet_config = config
            .as_any()
            .downcast_ref::<BulletDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BulletDataClientFactory. Expected BulletDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = BulletDataClient::new(client_id, bullet_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "BULLET"
    }

    fn config_type(&self) -> &'static str {
        "BulletDataClientConfig"
    }
}

/// Configuration for creating Bullet execution clients via factory.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.bullet",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bullet")
)]
pub struct BulletExecFactoryConfig {
    /// The trader ID for the execution client.
    pub trader_id: TraderId,
    /// The account ID for the execution client.
    pub account_id: AccountId,
    /// The underlying execution client configuration.
    pub config: BulletExecClientConfig,
}

impl ClientConfig for BulletExecFactoryConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Bullet execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.bullet",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bullet")
)]
pub struct BulletExecutionClientFactory;

impl BulletExecutionClientFactory {
    /// Creates a new [`BulletExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BulletExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for BulletExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let factory_config = config
            .as_any()
            .downcast_ref::<BulletExecFactoryConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BulletExecutionClientFactory. Expected BulletExecFactoryConfig, was {config:?}",
                )
            })?
            .clone();

        // Bullet perpetual futures use netting
        let oms_type = OmsType::Netting;
        let account_type = AccountType::Margin;

        let core = ExecutionClientCore::new(
            factory_config.trader_id,
            ClientId::from(name),
            *BULLET_VENUE,
            oms_type,
            factory_config.account_id,
            account_type,
            None,
            cache,
        );

        let client = BulletExecutionClient::new(core, factory_config.config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "BULLET"
    }

    fn config_type(&self) -> &'static str {
        "BulletExecFactoryConfig"
    }
}
