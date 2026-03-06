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

use alloy::primitives::{Address, U256};
use async_trait::async_trait;
use nautilus_model::defi::{AmmType, DexType, TransactionReceipt};
use thiserror::Error;

pub mod pancakeswap_v2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmmAdapterCapabilities {
    pub supports_quote_exact_in: bool,
    pub supports_quote_exact_out: bool,
    pub supports_single_hop: bool,
    pub supports_multi_hop: bool,
    pub supports_deadline_arg: bool,
    pub supports_recipient_override: bool,
    pub swap_call_returns_amounts: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmmTxCall {
    pub to: Address,
    pub data: Vec<u8>,
    pub value: U256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmmFill {
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: U256,
    pub amount_out: U256,
    pub tx_hash: String,
    pub log_index: u64,
}

#[async_trait(?Send)]
pub trait AmmProtocolAdapter: std::fmt::Debug {
    fn dex_type(&self) -> DexType;

    fn amm_type(&self) -> AmmType;

    fn capabilities(&self) -> AmmAdapterCapabilities;

    async fn quote_exact_in(
        &self,
        amount_in: U256,
        path: Vec<Address>,
    ) -> anyhow::Result<Vec<U256>>;

    async fn quote_exact_out(
        &self,
        amount_out: U256,
        path: Vec<Address>,
    ) -> anyhow::Result<Vec<U256>>;

    fn build_swap_exact_in_tx(
        &self,
        amount_in: U256,
        amount_out_min: U256,
        path: Vec<Address>,
        recipient: Address,
        deadline: U256,
    ) -> anyhow::Result<AmmTxCall>;

    fn build_swap_exact_out_tx(
        &self,
        amount_out: U256,
        amount_in_max: U256,
        path: Vec<Address>,
        recipient: Address,
        deadline: U256,
    ) -> anyhow::Result<AmmTxCall>;

    fn decode_fills_from_receipt(
        &self,
        receipt: &TransactionReceipt,
        expected_pool_address: Address,
        expected_path: Vec<Address>,
    ) -> anyhow::Result<Vec<AmmFill>>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AmmRegistryError {
    #[error("AMM adapter already registered for dex_type={0:?}")]
    DuplicateRegistration(DexType),
    #[error("No AMM adapter registered for dex_type={0:?}")]
    AdapterNotFound(DexType),
}

#[derive(Debug, Default)]
pub struct AmmAdapterRegistry {
    adapters: HashMap<DexType, Arc<dyn AmmProtocolAdapter>>,
}

impl AmmAdapterRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        adapter: Arc<dyn AmmProtocolAdapter>,
    ) -> Result<(), AmmRegistryError> {
        let dex_type = adapter.dex_type();
        if self.adapters.contains_key(&dex_type) {
            return Err(AmmRegistryError::DuplicateRegistration(dex_type));
        }

        self.adapters.insert(dex_type, adapter);
        Ok(())
    }

    pub fn get(&self, dex_type: DexType) -> Result<Arc<dyn AmmProtocolAdapter>, AmmRegistryError> {
        self.adapters
            .get(&dex_type)
            .cloned()
            .ok_or(AmmRegistryError::AdapterNotFound(dex_type))
    }
}
