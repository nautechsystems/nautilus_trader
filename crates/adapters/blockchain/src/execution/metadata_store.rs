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

use std::{collections::HashMap, sync::Arc};

use alloy::primitives::Address;
use nautilus_model::defi::{Pool, PoolIdentifier, SharedPool, Token};

/// Storage and lookup for token metadata used during execution flows.
pub trait TokenMetadataStore {
    /// Returns token metadata for the provided token contract address.
    fn get_token(&self, address: &Address) -> Option<&Token>;

    /// Stores token metadata.
    fn insert_token(&mut self, token: Token);
}

/// Storage and lookup for pool metadata used during execution flows.
pub trait PoolMetadataStore {
    /// Returns pool metadata for the provided pool identifier.
    fn get_pool(&self, pool_identifier: &PoolIdentifier) -> Option<&SharedPool>;

    /// Stores pool metadata.
    fn insert_pool(&mut self, pool: Pool);
}

/// Composite trait for execution metadata storage backends.
pub trait MetadataStore: TokenMetadataStore + PoolMetadataStore + std::fmt::Debug {}

impl<T> MetadataStore for T where T: TokenMetadataStore + PoolMetadataStore + std::fmt::Debug {}

/// In-memory metadata backend used by the execution client by default.
#[derive(Debug, Default)]
pub struct InMemoryMetadataStore {
    tokens: HashMap<Address, Token>,
    pools: HashMap<PoolIdentifier, SharedPool>,
}

impl InMemoryMetadataStore {
    /// Creates a new in-memory metadata store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
            pools: HashMap::new(),
        }
    }
}

impl TokenMetadataStore for InMemoryMetadataStore {
    fn get_token(&self, address: &Address) -> Option<&Token> {
        self.tokens.get(address)
    }

    fn insert_token(&mut self, token: Token) {
        self.tokens.insert(token.address, token);
    }
}

impl PoolMetadataStore for InMemoryMetadataStore {
    fn get_pool(&self, pool_identifier: &PoolIdentifier) -> Option<&SharedPool> {
        self.pools.get(pool_identifier)
    }

    fn insert_pool(&mut self, pool: Pool) {
        self.pools.insert(pool.pool_identifier, Arc::new(pool));
    }
}
