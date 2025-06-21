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

use alloy_primitives::Address;
use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{defi::chain::Blockchain, identifiers::ClientId};

#[derive(Debug, Clone)]
pub struct SubscribeBlocks {
    pub chain: Blockchain,
    pub client_id: Option<ClientId>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribeBlocks {
    /// Creates a new [`SubscribeBlocks`] instance.
    #[must_use]
    pub const fn new(
        chain: Blockchain,
        client_id: Option<ClientId>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            chain,
            client_id,
            command_id,
            ts_init,
            params,
        }
    }
}

/// Represents a subscription command for pool definition updates from a specific AMM pool.
#[derive(Debug, Clone)]
pub struct SubscribePool {
    pub address: Address,
    pub client_id: Option<ClientId>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribePool {
    /// Creates a new [`SubscribePool`] instance.
    #[must_use]
    pub const fn new(
        address: Address,
        client_id: Option<ClientId>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            address,
            client_id,
            command_id,
            ts_init,
            params,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubscribePoolSwaps {
    pub address: Address,
    pub client_id: Option<ClientId>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribePoolSwaps {
    /// Creates a new [`SubscribePoolSwaps`] instance.
    #[must_use]
    pub const fn new(
        address: Address,
        client_id: Option<ClientId>,
        command_id: UUID4,
        ts_init: UnixNanos,

        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            address,
            client_id,
            command_id,
            ts_init,
            params,
        }
    }
}

/// Represents a subscription command for pool liquidity updates from a specific AMM pool.
#[derive(Debug, Clone)]
pub struct SubscribePoolLiquidityUpdates {
    pub address: Address,
    pub client_id: Option<ClientId>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl SubscribePoolLiquidityUpdates {
    /// Creates a new [`SubscribePoolLiquidityUpdates`] instance.
    #[must_use]
    pub const fn new(
        address: Address,
        client_id: Option<ClientId>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            address,
            client_id,
            command_id,
            ts_init,
            params,
        }
    }
}
