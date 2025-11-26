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

//! Factory functions for creating OKX clients and components.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_data::client::DataClient;
use nautilus_model::identifiers::ClientId;
use nautilus_system::factories::{ClientConfig, DataClientFactory};

use crate::{config::OKXDataClientConfig, data::OKXDataClient};

impl ClientConfig for OKXDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating OKX data clients.
#[derive(Debug)]
pub struct OKXDataClientFactory;

impl OKXDataClientFactory {
    /// Creates a new [`OKXDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for OKXDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for OKXDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let okx_config = config
            .as_any()
            .downcast_ref::<OKXDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for OKXDataClientFactory. Expected OKXDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = OKXDataClient::new(client_id, okx_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "OKX"
    }

    fn config_type(&self) -> &'static str {
        "OKXDataClientConfig"
    }
}

// TODO: Implement OKXExecutionClientFactory when needed for execution testing
