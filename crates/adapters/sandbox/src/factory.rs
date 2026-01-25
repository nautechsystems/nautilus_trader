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

//! Factory for creating sandbox execution clients.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, clients::ExecutionClient, clock::Clock};
use nautilus_execution::client::base::ExecutionClientCore;
use nautilus_model::identifiers::ClientId;
use nautilus_system::factories::{ClientConfig, ExecutionClientFactory};

use crate::{config::SandboxExecutionClientConfig, execution::SandboxExecutionClient};

impl ClientConfig for SandboxExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating sandbox execution clients.
#[derive(Debug, Default)]
pub struct SandboxExecutionClientFactory;

impl SandboxExecutionClientFactory {
    /// Creates a new [`SandboxExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ExecutionClientFactory for SandboxExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let sandbox_config = config
            .as_any()
            .downcast_ref::<SandboxExecutionClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for SandboxExecutionClientFactory. Expected SandboxExecutionClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);

        let core = ExecutionClientCore::new(
            sandbox_config.trader_id,
            client_id,
            sandbox_config.venue,
            sandbox_config.oms_type,
            sandbox_config.account_id,
            sandbox_config.account_type,
            sandbox_config.base_currency,
            clock.clone(),
            cache.clone(),
        );

        let client = SandboxExecutionClient::new(core, sandbox_config, clock, cache);
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "SANDBOX"
    }

    fn config_type(&self) -> &'static str {
        "SandboxExecutionClientConfig"
    }
}
