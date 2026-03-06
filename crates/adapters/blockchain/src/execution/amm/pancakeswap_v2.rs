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

use std::{collections::HashSet, sync::Arc};

use alloy::primitives::{Address, U256};
use async_trait::async_trait;
use nautilus_model::defi::{AmmType, DexType, TransactionReceipt};

use crate::{
    contracts::{
        pancakeswap_v2_router::PancakeSwapV2RouterContract,
        uniswap_v2_pair::{decode_swap_log, map_swap_to_fill, topic0_is_swap},
    },
    execution::amm::{AmmAdapterCapabilities, AmmFill, AmmProtocolAdapter, AmmTxCall},
    rpc::http::BlockchainHttpRpcClient,
};

#[derive(Debug)]
pub struct PancakeSwapV2Adapter {
    router: PancakeSwapV2RouterContract,
    wallet_address: Address,
    unsupported_token_addresses: HashSet<Address>,
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
            unsupported_token_addresses: HashSet::new(),
        }
    }

    #[must_use]
    pub const fn router_address(&self) -> Address {
        self.router.router_address()
    }

    #[must_use]
    pub fn with_unsupported_tokens(
        mut self,
        token_addresses: impl IntoIterator<Item = Address>,
    ) -> Self {
        self.unsupported_token_addresses = token_addresses.into_iter().collect();
        self
    }

    fn ensure_supported_tokens(&self, token_in: Address, token_out: Address) -> anyhow::Result<()> {
        if self.unsupported_token_addresses.contains(&token_in)
            || self.unsupported_token_addresses.contains(&token_out)
        {
            anyhow::bail!(
                "token is marked as taxed/rebasing and is not supported in PancakeSwapV2 MVP decode path"
            );
        }
        Ok(())
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

    fn decode_fills_from_receipt(
        &self,
        receipt: &TransactionReceipt,
        expected_pool_address: Address,
        expected_path: Vec<Address>,
    ) -> anyhow::Result<Vec<AmmFill>> {
        if receipt.status != 1 {
            anyhow::bail!(
                "cannot decode fills from unsuccessful receipt (status={})",
                receipt.status
            );
        }
        if expected_path.len() != 2 {
            anyhow::bail!(
                "PancakeSwapV2 MVP only supports single-hop receipt decode (expected path len 2)"
            );
        }
        if receipt.logs.iter().any(|log| log.removed.unwrap_or(false)) {
            anyhow::bail!("receipt contains removed logs; refusing decode due to reorg artifact");
        }

        let expected_token_in = expected_path[0];
        let expected_token_out = expected_path[1];
        let mut fills = Vec::new();

        for log in &receipt.logs {
            if log.address != expected_pool_address {
                continue;
            }

            let Some(topic0) = log.topics.first() else {
                continue;
            };
            if !topic0_is_swap(topic0.as_str()) {
                continue;
            }

            let decoded = decode_swap_log(log)?;
            if decoded.to != self.wallet_address {
                anyhow::bail!(
                    "swap recipient mismatch in receipt decode expected={} actual={}",
                    self.wallet_address,
                    decoded.to
                );
            }

            // Pair events report amounts in token0/token1 order; try both orientations and
            // require that one maps back to the requested path.
            let fill = match map_swap_to_fill(&decoded, expected_token_in, expected_token_out) {
                Ok(fill)
                    if fill.token_in == expected_token_in
                        && fill.token_out == expected_token_out =>
                {
                    fill
                }
                _ => {
                    let swapped =
                        map_swap_to_fill(&decoded, expected_token_out, expected_token_in)?;
                    if swapped.token_in != expected_token_in
                        || swapped.token_out != expected_token_out
                    {
                        anyhow::bail!(
                                "decoded swap direction does not match expected path token_in={} token_out={}",
                                swapped.token_in,
                                swapped.token_out
                            );
                    }
                    swapped
                }
            };
            self.ensure_supported_tokens(fill.token_in, fill.token_out)?;

            let log_index = log.log_index.ok_or_else(|| {
                anyhow::anyhow!("swap log for expected pool is missing log_index")
            })?;
            fills.push(AmmFill {
                token_in: fill.token_in,
                token_out: fill.token_out,
                amount_in: fill.amount_in,
                amount_out: fill.amount_out,
                tx_hash: receipt.transaction_hash.clone(),
                log_index,
            });
        }

        if fills.is_empty() {
            anyhow::bail!(
                "no swap log found for expected pool {}",
                expected_pool_address
            );
        }
        if fills.len() != 1 {
            anyhow::bail!(
                "expected exactly one swap log for pool {}, found {}",
                expected_pool_address,
                fills.len()
            );
        }

        fills.sort_by_key(|fill| fill.log_index);
        Ok(fills)
    }
}
