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

use std::sync::Arc;

use alloy::primitives::{Address, U256};
use async_trait::async_trait;
use nautilus_model::defi::{AmmType, DexType};

use crate::{
    contracts::pancakeswap_v2_router::PancakeSwapV2RouterContract,
    execution::amm::{AmmAdapterCapabilities, AmmProtocolAdapter, AmmTxCall},
    rpc::http::BlockchainHttpRpcClient,
};

#[derive(Debug)]
pub struct PancakeSwapV2Adapter {
    router: PancakeSwapV2RouterContract,
    wallet_address: Address,
}

impl PancakeSwapV2Adapter {
    #[must_use]
    pub fn new(
        client: Arc<BlockchainHttpRpcClient>,
        router_address: Address,
        wallet_address: Address,
    ) -> Self {
        Self {
            router: PancakeSwapV2RouterContract::new(client, router_address),
            wallet_address,
        }
    }

    #[must_use]
    pub const fn router_address(&self) -> Address {
        self.router.router_address()
    }
}

#[async_trait(?Send)]
impl AmmProtocolAdapter for PancakeSwapV2Adapter {
    fn dex_type(&self) -> DexType {
        DexType::PancakeSwapV2
    }

    fn amm_type(&self) -> AmmType {
        AmmType::CPAMM
    }

    fn capabilities(&self) -> AmmAdapterCapabilities {
        AmmAdapterCapabilities {
            supports_quote_exact_in: true,
            supports_quote_exact_out: true,
            supports_single_hop: true,
            supports_multi_hop: false,
            supports_deadline_arg: true,
            supports_recipient_override: false,
            swap_call_returns_amounts: true,
        }
    }

    async fn quote_exact_in(
        &self,
        amount_in: U256,
        path: Vec<Address>,
    ) -> anyhow::Result<Vec<U256>> {
        if path.len() != 2 {
            anyhow::bail!("PancakeSwapV2 MVP only supports single-hop quotes (path len must be 2)");
        }

        self.router
            .quote_exact_in(amount_in, path)
            .await
            .map_err(anyhow::Error::from)
    }

    async fn quote_exact_out(
        &self,
        amount_out: U256,
        path: Vec<Address>,
    ) -> anyhow::Result<Vec<U256>> {
        if path.len() != 2 {
            anyhow::bail!(
                "PancakeSwapV2 MVP only supports single-hop exact-out quotes (path len must be 2)"
            );
        }

        self.router
            .quote_exact_out(amount_out, path)
            .await
            .map_err(anyhow::Error::from)
    }

    fn build_swap_exact_in_tx(
        &self,
        amount_in: U256,
        amount_out_min: U256,
        path: Vec<Address>,
        recipient: Address,
        deadline: U256,
    ) -> anyhow::Result<AmmTxCall> {
        if path.len() != 2 {
            anyhow::bail!("PancakeSwapV2 MVP only supports single-hop swaps (path len must be 2)");
        }
        if deadline.is_zero() {
            anyhow::bail!("PancakeSwapV2 swap deadline must be > 0");
        }
        if recipient != self.wallet_address {
            anyhow::bail!(
                "PancakeSwapV2 adapter does not support recipient override (expected wallet recipient)"
            );
        }

        let data = PancakeSwapV2RouterContract::encode_swap_exact_tokens_for_tokens_call(
            amount_in,
            amount_out_min,
            path,
            recipient,
            deadline,
        )?;

        Ok(AmmTxCall {
            to: self.router.router_address(),
            data,
            value: U256::ZERO,
        })
    }

    fn build_swap_exact_out_tx(
        &self,
        amount_out: U256,
        amount_in_max: U256,
        path: Vec<Address>,
        recipient: Address,
        deadline: U256,
    ) -> anyhow::Result<AmmTxCall> {
        if path.len() != 2 {
            anyhow::bail!("PancakeSwapV2 MVP only supports single-hop swaps (path len must be 2)");
        }
        if deadline.is_zero() {
            anyhow::bail!("PancakeSwapV2 swap deadline must be > 0");
        }
        if recipient != self.wallet_address {
            anyhow::bail!(
                "PancakeSwapV2 adapter does not support recipient override (expected wallet recipient)"
            );
        }

        let data = PancakeSwapV2RouterContract::encode_swap_tokens_for_exact_tokens_call(
            amount_out,
            amount_in_max,
            path,
            recipient,
            deadline,
        )?;

        Ok(AmmTxCall {
            to: self.router.router_address(),
            data,
            value: U256::ZERO,
        })
    }
}
