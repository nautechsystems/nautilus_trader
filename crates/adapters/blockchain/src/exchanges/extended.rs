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

use std::ops::Deref;

use hypersync_client::simple_types::Log;
use nautilus_model::defi::dex::Dex;

use crate::events::pool_created::PoolCreated;

/// Extended DEX wrapper that adds provider-specific event parsing capabilities to the domain `Dex` model.
#[derive(Debug, Clone)]
pub struct DexExtended {
    /// The core domain Dex object being extended
    pub dex: Dex,
    /// Function to parse pool creation events for this specific DEX
    pub parse_pool_created_event_fn: Option<fn(Log) -> anyhow::Result<PoolCreated>>,
}

impl DexExtended {
    /// Creates a new [`DexExtended`] wrapper around a domain `Dex` object.
    #[must_use]
    pub fn new(dex: Dex) -> Self {
        Self {
            dex,
            parse_pool_created_event_fn: None,
        }
    }

    /// Sets the function used to parse pool creation events for this Dex.
    pub fn set_pool_created_event_parsing(
        &mut self,
        parse_pool_created_event: fn(Log) -> anyhow::Result<PoolCreated>,
    ) {
        self.parse_pool_created_event_fn = Some(parse_pool_created_event);
    }

    /// Parses a pool creation event log using this DEX's specific parsing function.
    pub fn parse_pool_created_event(&self, log: Log) -> anyhow::Result<PoolCreated> {
        if let Some(parse_pool_created_event_fn) = &self.parse_pool_created_event_fn {
            parse_pool_created_event_fn(log)
        } else {
            Err(anyhow::anyhow!(
                "Parsing of pool created event in not defined in this dex: {}",
                self.dex.name
            ))
        }
    }
}

impl Deref for DexExtended {
    type Target = Dex;

    fn deref(&self) -> &Self::Target {
        &self.dex
    }
}
