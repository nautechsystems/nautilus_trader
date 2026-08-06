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

use std::{collections::HashSet, str::FromStr, sync::Arc, time::Duration};

use alloy::{
    primitives::{Address, B256, Bytes, U256},
    signers::local::PrivateKeySigner,
    sol_types::SolCall,
};
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::UnixNanos;
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::AccountAny,
    defi::{
        DexType, Pool, PoolIdentifier, SharedChain, Token,
        validation::validate_address,
        wallet::{TokenBalance, WalletBalance},
    },
    enums::OmsType,
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money},
};

use crate::{
    cache::BlockchainCache,
    config::BlockchainExecutionClientConfig,
    contracts::{
        erc20::{ERC20, Erc20Contract},
        weth::WETH9,
    },
    execution::{
        preflight::{
            BlockchainPreflightReport, ContractCodeCheck, PoolPreflightCheck, TokenPreflightCheck,
        },
        transaction::{
            TransactionPurpose, TransactionStatus, build_eip1559_transaction, compute_max_fee,
            derive_fees, derive_gas_limit, sign_eip1559_transaction,
        },
    },
    rpc::{
        error::BroadcastError,
        http::{BlockchainHttpRpcClient, EXECUTION_RPC_TIMEOUT_SECS},
        types::RpcTransactionReceipt,
    },
};

/// Maximum number of receipt polls while awaiting transaction inclusion.
const RECEIPT_MAX_POLLS: u32 = 60;
/// Interval between receipt polls while awaiting transaction inclusion.
const RECEIPT_POLL_INTERVAL: Duration = Duration::from_secs(1);

// A broadcast transaction awaiting inclusion, occupying the single in-flight slot.
#[derive(Debug, Clone, Copy)]
struct InFlightTransaction {
    nonce: u64,
    tx_hash: B256,
    purpose: TransactionPurpose,
}

#[derive(Debug, Clone, Copy)]
struct IncludedTransaction {
    tx_hash: B256,
    block_number: u64,
}

/// Execution client for blockchain interactions including balance tracking and order execution.
#[derive(Debug)]
pub struct BlockchainExecutionClient {
    /// Core execution client providing base functionality.
    core: ExecutionClientCore,
    /// Cache for storing token metadata and other blockchain data.
    cache: BlockchainCache,
    /// The client configuration.
    config: BlockchainExecutionClientConfig,
    /// The blockchain network configuration.
    chain: SharedChain,
    /// The wallet address used for transactions and balance queries.
    wallet_address: Address,
    /// Transaction signer loaded from the configured environment variable at connect.
    signer: Option<PrivateKeySigner>,
    /// Validated allowlist of SwapRouter addresses.
    router_addresses: Vec<Address>,
    /// Validated wrapped native token address for wrap operations.
    weth_address: Address,
    /// The transaction currently awaiting inclusion, occupying the single in-flight slot.
    in_flight: Option<InFlightTransaction>,
    /// Tracks native currency and ERC-20 token balances.
    wallet_balance: WalletBalance,
    /// Contract interface for ERC-20 token interactions.
    erc20_contract: Erc20Contract,
    /// HTTP RPC client for blockchain queries.
    http_rpc_client: Arc<BlockchainHttpRpcClient>,
}

impl BlockchainExecutionClient {
    /// Creates a new [`BlockchainExecutionClient`] instance for the specified configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the wallet address, any token address, any router address, or the
    /// WETH address in the config is invalid, or if the router allowlist is empty.
    pub fn new(
        core_client: ExecutionClientCore,
        config: BlockchainExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        let chain = Arc::new(config.chain.clone());
        let cache = BlockchainCache::new(chain.clone());
        let http_rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            config.http_rpc_url.clone(),
            config.rpc_requests_per_second,
            None,
        ));
        let wallet_address = validate_address(config.wallet_address.as_str())?;
        let erc20_contract = Erc20Contract::new_with_timeout(
            http_rpc_client.clone(),
            Some(EXECUTION_RPC_TIMEOUT_SECS),
            true,
        );

        let router_addresses = config
            .router_addresses
            .iter()
            .map(|address| validate_address(address.as_str()))
            .collect::<anyhow::Result<Vec<_>>>()?;
        if router_addresses.is_empty() {
            anyhow::bail!("`router_addresses` must contain at least one router address");
        }
        let weth_address = validate_address(config.weth_address.as_str())?;

        // Initialize token universe, so we can fetch them from the blockchain later.
        let mut token_universe = HashSet::new();

        if let Some(specified_tokens) = &config.tokens {
            for token in specified_tokens {
                let token_address = validate_address(token.as_str())?;
                token_universe.insert(token_address);
            }
        }
        let wallet_balance = WalletBalance::new(token_universe);

        Ok(Self {
            core: core_client,
            wallet_balance,
            chain,
            cache,
            config,
            signer: None,
            router_addresses,
            weth_address,
            in_flight: None,
            erc20_contract,
            http_rpc_client,
            wallet_address,
        })
    }

    /// Fetches the native currency balance (e.g., ETH) for the wallet from the blockchain.
    async fn fetch_native_currency_balance(&self) -> anyhow::Result<Money> {
        let balance_u256 = self
            .http_rpc_client
            .get_balance_with_timeout(&self.wallet_address, None, Some(EXECUTION_RPC_TIMEOUT_SECS))
            .await?;

        let native_currency = self.chain.native_currency();

        // Convert from wei (18 decimals on-chain) to Money
        let balance = Money::from_wei(balance_u256, native_currency);

        Ok(balance)
    }

    /// Fetches the balance of a specific ERC-20 token for the wallet.
    async fn fetch_token_balance(
        &mut self,
        token_address: &Address,
    ) -> anyhow::Result<TokenBalance> {
        // Get the cached token or fetch it from the blockchain and cache it.
        let token = if let Some(token) = self.cache.get_token(token_address) {
            token.to_owned()
        } else {
            let token_info = self.erc20_contract.fetch_token_info(token_address).await?;
            let token = Token::new(
                self.chain.clone(),
                *token_address,
                token_info.name,
                token_info.symbol,
                token_info.decimals,
            );
            self.cache.add_token(token.clone()).await?;
            token
        };

        let amount = self
            .erc20_contract
            .balance_of(token_address, &self.wallet_address)
            .await?;
        let token_balance = TokenBalance::new(amount, token);

        // TODO: Use price oracle here and cache, to get the latest price then convert to USD
        // then use token_balance.set_amount_usd(amount_usd) to set the amount_usd value.

        Ok(token_balance)
    }

    /// Refreshes all wallet balances including native currency and tracked ERC-20 tokens.
    async fn refresh_wallet_balances(&mut self) -> anyhow::Result<()> {
        let native_currency_balance = self.fetch_native_currency_balance().await?;
        log::debug!(
            "Initializing wallet balance with native currency balance: {} {}",
            native_currency_balance.as_decimal(),
            native_currency_balance.currency
        );
        self.wallet_balance
            .set_native_currency_balance(native_currency_balance);

        // Fetch token balances from the blockchain.
        if self.wallet_balance.is_token_universe_initialized() {
            let tokens: Vec<Address> = self
                .wallet_balance
                .token_universe
                .clone()
                .into_iter()
                .collect();

            for token in tokens {
                if let Ok(token_balance) = self.fetch_token_balance(&token).await {
                    log::debug!("Adding token balance to the wallet: {token_balance}");
                    self.wallet_balance.add_token_balance(token_balance);
                }
            }
        } else {
            // TODO sync from transfer events for tokens that wallet interacted with.
        }

        Ok(())
    }

    /// Runs a read-only execution preflight for the pool selected by `instrument_id`.
    ///
    /// Verifies the RPC chain ID against configuration, deployed bytecode at the router, pool,
    /// and token addresses, wallet native and token balances, exact router allowance of the
    /// pool's base (input) token, and current fee conditions. Changes no state.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument ID cannot be resolved to a cached Uniswap V3 pool
    /// on the client's chain, or if any RPC call fails. On-chain readiness failures are
    /// reported in the returned [`BlockchainPreflightReport`], not as errors.
    pub async fn preflight(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<BlockchainPreflightReport> {
        let pool = self.resolve_pool(instrument_id)?;
        let base_token = pool.get_base_token().clone();
        let quote_token = pool.get_quote_token().clone();

        let actual_chain_id = self.http_rpc_client.chain_id().await?;

        let pool_code = self.http_rpc_client.get_code(&pool.address).await?;
        let pool_check = PoolPreflightCheck {
            instrument_id: pool.instrument_id,
            address: pool.address,
            has_deployed_code: !pool_code.is_empty(),
            fee: pool.fee,
            base_token: base_token.address,
            quote_token: quote_token.address,
        };

        let mut routers = Vec::with_capacity(self.router_addresses.len());
        for router in &self.router_addresses {
            let code = self.http_rpc_client.get_code(router).await?;
            routers.push(ContractCodeCheck {
                address: *router,
                has_deployed_code: !code.is_empty(),
            });
        }

        let mut tokens = Vec::with_capacity(2);

        for token in [&base_token, &quote_token] {
            let code = self.http_rpc_client.get_code(&token.address).await?;
            let wallet_balance = self
                .erc20_contract
                .balance_of(&token.address, &self.wallet_address)
                .await?;

            let mut router_allowances = Vec::new();
            if token.address == base_token.address {
                router_allowances.reserve(self.router_addresses.len());
                for router in &self.router_addresses {
                    let amount = self
                        .erc20_contract
                        .allowance(&token.address, &self.wallet_address, router)
                        .await?;
                    router_allowances.push((*router, amount));
                }
            }

            tokens.push(TokenPreflightCheck {
                address: token.address,
                symbol: token.symbol.clone(),
                has_deployed_code: !code.is_empty(),
                wallet_balance,
                router_allowances,
            });
        }

        let native_balance_wei = self
            .http_rpc_client
            .get_balance_with_timeout(&self.wallet_address, None, Some(EXECUTION_RPC_TIMEOUT_SECS))
            .await?;

        let latest_block = self.http_rpc_client.latest_block().await?;
        let base_fee_per_gas_wei = latest_block.base_fee_per_gas.ok_or_else(|| {
            anyhow::anyhow!("Latest block {} has no base fee", latest_block.number)
        })?;
        let max_priority_fee_per_gas_wei = self.http_rpc_client.max_priority_fee_per_gas().await?;
        let derived_max_fee_per_gas_wei = compute_max_fee(
            base_fee_per_gas_wei,
            max_priority_fee_per_gas_wei,
            self.config.base_fee_buffer_bps,
        )?;

        Ok(BlockchainPreflightReport::new(
            u64::from(self.chain.chain_id),
            actual_chain_id,
            pool_check,
            routers,
            tokens,
            native_balance_wei,
            base_fee_per_gas_wei,
            max_priority_fee_per_gas_wei,
            derived_max_fee_per_gas_wei,
            u128::from(self.config.max_fee_per_gas_wei),
        ))
    }

    /// Wraps native currency into the wrapped native token via a WETH `deposit()`
    /// transaction carrying `amount_wei` of native value.
    ///
    /// This is an explicit operator operation; it never runs inside `submit_order`.
    ///
    /// # Errors
    ///
    /// Returns an error if the amount is zero, the WETH target is not a deployed ERC-20 contract,
    /// the client is not connected, another transaction is in flight, no durable store is
    /// configured, the WETH balance does not increase by `amount_wei`, or any RPC, policy, signing,
    /// persistence, or broadcast step fails. A persistence failure after signing leaves the
    /// in-flight slot occupied because database commit acknowledgement is ambiguous.
    pub async fn wrap(&mut self, amount_wei: U256) -> anyhow::Result<B256> {
        if amount_wei.is_zero() {
            anyhow::bail!("Wrap amount must be positive");
        }

        self.ensure_transaction_ready(TransactionPurpose::Wrap)?;
        self.ensure_contract_deployed(&self.weth_address, "WETH")
            .await?;
        let _balance_before_broadcast = self
            .erc20_contract
            .balance_of(&self.weth_address, &self.wallet_address)
            .await?;

        let calldata = WETH9::depositCall {}.abi_encode();
        let IncludedTransaction {
            tx_hash,
            block_number,
        } = self
            .execute_transaction(
                self.weth_address,
                amount_wei,
                Bytes::from(calldata),
                TransactionPurpose::Wrap,
            )
            .await?;
        let previous_block = block_number.checked_sub(1).ok_or_else(|| {
            anyhow::anyhow!("Included wrap transaction {tx_hash} has invalid block number 0")
        })?;
        let balance_before = self
            .erc20_contract
            .balance_of_at(&self.weth_address, &self.wallet_address, previous_block)
            .await
            .with_context(|| {
                format!(
                    "failed to read WETH balance before included transaction {tx_hash} at block {previous_block}"
                )
            })?;
        let balance_after = self
            .erc20_contract
            .balance_of_at(&self.weth_address, &self.wallet_address, block_number)
            .await
            .with_context(|| {
                format!(
                    "failed to read WETH balance after included transaction {tx_hash} at block {block_number}"
                )
            })?;
        let expected_balance = balance_before.checked_add(amount_wei).ok_or_else(|| {
            anyhow::anyhow!(
                "WETH balance overflow for included transaction {tx_hash} at block {block_number}: wrap amount {amount_wei} from balance {balance_before}"
            )
        })?;

        if balance_after != expected_balance {
            anyhow::bail!(
                "WETH balance after transaction {tx_hash} did not increase by {amount_wei}: expected {expected_balance}, was {balance_after}"
            );
        }

        Ok(tx_hash)
    }

    /// Approves an allowlisted SwapRouter to spend `amount` of `token` via an ERC-20
    /// `approve` transaction. When `unlimited_approval` is configured the transaction requests
    /// `U256::MAX`, while the resulting allowance must still cover `amount`.
    ///
    /// This is an explicit operator operation; it never runs inside `submit_order`.
    ///
    /// # Errors
    ///
    /// Returns an error if the router is not allowlisted, the token is not a deployed contract,
    /// approval simulation returns false or malformed data, the client is not connected, another
    /// transaction is in flight, no durable store is configured, the resulting allowance is below
    /// the requested amount, or any RPC, policy, signing, persistence, or broadcast step fails. A
    /// persistence failure after signing leaves the in-flight slot occupied because database
    /// commit acknowledgement is ambiguous.
    pub async fn approve(
        &mut self,
        token: Address,
        amount: U256,
        router: Address,
    ) -> anyhow::Result<B256> {
        if !self.router_addresses.contains(&router) {
            anyhow::bail!("Router {router} is not in the configured `router_addresses` allowlist");
        }

        self.ensure_transaction_ready(TransactionPurpose::Approve)?;
        self.ensure_contract_deployed(&token, "ERC-20 token")
            .await?;

        let approval_amount = if self.config.unlimited_approval {
            U256::MAX
        } else {
            amount
        };

        if !self
            .erc20_contract
            .simulate_approve(&token, &self.wallet_address, &router, approval_amount)
            .await?
        {
            anyhow::bail!("ERC-20 approve returned false for token {token}");
        }
        let calldata = ERC20::approveCall {
            spender: router,
            amount: approval_amount,
        }
        .abi_encode();

        let IncludedTransaction {
            tx_hash,
            block_number,
        } = self
            .execute_transaction(
                token,
                U256::ZERO,
                Bytes::from(calldata),
                TransactionPurpose::Approve,
            )
            .await?;
        let allowance = self
            .erc20_contract
            .allowance_at(&token, &self.wallet_address, &router, block_number)
            .await
            .with_context(|| {
                format!(
                    "failed to read router allowance after included transaction {tx_hash} at block {block_number}"
                )
            })?;

        if allowance < amount {
            anyhow::bail!(
                "Router allowance after transaction {tx_hash} is below the requested amount {amount}: was {allowance}"
            );
        }

        Ok(tx_hash)
    }

    async fn ensure_contract_deployed(
        &self,
        address: &Address,
        description: &str,
    ) -> anyhow::Result<()> {
        let code = self.http_rpc_client.get_code(address).await?;
        if code.is_empty() {
            anyhow::bail!("No deployed bytecode at configured {description} address {address}");
        }
        Ok(())
    }

    /// Resolves the pool selected by `instrument_id` from the shared engine cache.
    fn resolve_pool(&self, instrument_id: &InstrumentId) -> anyhow::Result<Pool> {
        let (blockchain, dex_type) = instrument_id.venue.parse_dex()?;
        if blockchain != self.chain.name {
            anyhow::bail!(
                "Pool venue chain {blockchain} does not match the client chain {}",
                self.chain.name
            );
        }

        if dex_type != DexType::UniswapV3 {
            anyhow::bail!("Unsupported DEX type {dex_type}; only UniswapV3 is supported");
        }

        let pool_identifier = PoolIdentifier::new_checked(instrument_id.symbol.as_str())?;
        if !pool_identifier.is_address() {
            anyhow::bail!(
                "Pool identifier {pool_identifier} is a pool ID; only address identifiers are supported"
            );
        }

        let pool = self
            .core
            .cache()
            .pool(instrument_id)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Unknown pool {instrument_id}; not found in the shared engine cache"
                )
            })?;

        if pool.token0.get_token_priority() == pool.token1.get_token_priority() {
            anyhow::bail!(
                "Pool {instrument_id} tokens share a token priority; base and quote orientation is ambiguous"
            );
        }

        Ok(pool)
    }

    /// Builds, signs, persists, and broadcasts an EIP-1559 transaction, then awaits inclusion.
    ///
    /// Order of operations: chain ID verification, nonce and fee and gas policy checks, local
    /// signing, persist-before-broadcast, broadcast, inclusion observation. The persisted
    /// record guarantees a signed transaction is never forgotten.
    async fn execute_transaction(
        &mut self,
        to: Address,
        value: U256,
        input: Bytes,
        purpose: TransactionPurpose,
    ) -> anyhow::Result<IncludedTransaction> {
        self.ensure_transaction_ready(purpose)?;

        let expected_chain_id = u64::from(self.chain.chain_id);
        let actual_chain_id = self.http_rpc_client.chain_id().await?;
        if actual_chain_id != expected_chain_id {
            anyhow::bail!(
                "Chain ID mismatch: expected {expected_chain_id}, node reported {actual_chain_id}"
            );
        }

        let nonce = self
            .http_rpc_client
            .get_transaction_count_pending(&self.wallet_address)
            .await?;
        let latest_block = self.http_rpc_client.latest_block().await?;
        let base_fee_per_gas_wei = latest_block.base_fee_per_gas.ok_or_else(|| {
            anyhow::anyhow!("Latest block {} has no base fee", latest_block.number)
        })?;
        let priority_fee_per_gas_wei = self.http_rpc_client.max_priority_fee_per_gas().await?;
        let (max_fee_per_gas, max_priority_fee_per_gas) = derive_fees(
            base_fee_per_gas_wei,
            priority_fee_per_gas_wei,
            self.config.base_fee_buffer_bps,
            u128::from(self.config.max_fee_per_gas_wei),
        )?;
        let gas_estimate = self
            .http_rpc_client
            .estimate_gas(&self.wallet_address, &to, value, &input)
            .await?;
        let gas_limit = derive_gas_limit(
            gas_estimate,
            self.config.gas_buffer_bps,
            self.config.gas_limit,
        )?;

        let tx = build_eip1559_transaction(
            expected_chain_id,
            nonce,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            to,
            value,
            input,
        );
        let (tx_hash, raw_tx) = {
            let signer = self.signer.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Signer not initialized; connect the client first")
            })?;
            sign_eip1559_transaction(tx, signer).await?
        };

        let tx_hash_string = tx_hash.to_string();
        // Claim the slot before the cancellable write: PostgreSQL may commit before this future
        // resumes, so cancellation cannot safely restore the pre-transaction state.
        self.in_flight = Some(InFlightTransaction {
            nonce,
            tx_hash,
            purpose,
        });

        self.cache
            .add_execution_transaction(
                self.chain.chain_id,
                nonce,
                &tx_hash_string,
                purpose.as_str(),
                TransactionStatus::Pending.as_str(),
            )
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to persist transaction {tx_hash}: {e}; the in-flight slot stays occupied"
                )
            })?;

        match self
            .http_rpc_client
            .send_raw_transaction(&raw_tx, &tx_hash)
            .await
        {
            Ok(broadcast_hash) => {
                if broadcast_hash != tx_hash {
                    log::warn!(
                        "Broadcast returned hash {broadcast_hash} differing from signed hash {tx_hash}"
                    );
                }
            }
            Err(BroadcastError::TimeoutAfterSend) => {
                anyhow::bail!(
                    "Broadcast of transaction {tx_hash} timed out after send; the persisted record reconciles instead of rebroadcasting"
                );
            }
            Err(BroadcastError::Failed(message)) => {
                // Transport or response failure: acceptance is ambiguous (the failure may also
                // predate dispatch; see BroadcastError::Failed), so occupy the slot and
                // reconcile rather than risk a rebroadcast
                anyhow::bail!(
                    "Broadcast of transaction {tx_hash} failed ambiguously ({message}); the persisted record reconciles instead of rebroadcasting"
                );
            }
            Err(error @ BroadcastError::Rejected { .. }) => {
                self.cache
                    .update_execution_transaction_status(
                        self.chain.chain_id,
                        &tx_hash_string,
                        TransactionStatus::Rejected.as_str(),
                    )
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to persist rejection of transaction {tx_hash}: {e}; the in-flight slot stays occupied"
                        )
                    })?;
                self.in_flight = None;
                anyhow::bail!(error);
            }
        }

        let receipt = self
            .poll_for_receipt(&tx_hash, RECEIPT_MAX_POLLS, RECEIPT_POLL_INTERVAL)
            .await?;

        match receipt {
            Some(receipt) => {
                let status = if receipt.status {
                    TransactionStatus::Included
                } else {
                    TransactionStatus::Reverted
                };
                self.cache
                    .update_execution_transaction_status(
                        self.chain.chain_id,
                        &tx_hash_string,
                        status.as_str(),
                    )
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to update persisted status of transaction {tx_hash}: {e}; the in-flight slot stays occupied and the persisted record reconciles"
                        )
                    })?;
                self.in_flight = None;

                if !receipt.status {
                    anyhow::bail!("Transaction {tx_hash} reverted on-chain");
                }
                Ok(IncludedTransaction {
                    tx_hash,
                    block_number: receipt.block_number,
                })
            }
            None => {
                anyhow::bail!(
                    "Timed out awaiting inclusion of transaction {tx_hash}; the in-flight slot stays occupied and the persisted record reconciles"
                )
            }
        }
    }

    fn ensure_transaction_ready(&self, purpose: TransactionPurpose) -> anyhow::Result<()> {
        if !self.core.is_connected() {
            anyhow::bail!("Blockchain execution client is not connected");
        }

        if let Some(in_flight) = &self.in_flight {
            anyhow::bail!(
                "Transaction {} ({}, nonce {}) is still awaiting inclusion; at most one transaction can be in flight",
                in_flight.tx_hash,
                in_flight.purpose.as_str(),
                in_flight.nonce
            );
        }

        if !self.cache.has_database() {
            anyhow::bail!(
                "No durable store configured; refusing to submit a {} transaction",
                purpose.as_str()
            );
        }
        Ok(())
    }

    /// Polls for the receipt of a broadcast transaction until it exists or the poll bound
    /// is exhausted. A `null` receipt result is a legitimate pending response.
    async fn poll_for_receipt(
        &self,
        tx_hash: &B256,
        max_polls: u32,
        interval: Duration,
    ) -> anyhow::Result<Option<RpcTransactionReceipt>> {
        let mut last_error = None;
        let mut observed_pending = false;

        for attempt in 0..max_polls {
            if attempt > 0 {
                tokio::time::sleep(interval).await;
            }

            match self.http_rpc_client.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => return Ok(Some(receipt)),
                Ok(None) => observed_pending = true,
                Err(e) => {
                    log::warn!(
                        "Receipt poll {}/{} for transaction {tx_hash} failed: {e}",
                        attempt + 1,
                        max_polls
                    );
                    last_error = Some(e);
                }
            }
        }

        if !observed_pending && let Some(e) = last_error {
            return Err(e);
        }

        Ok(None)
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BlockchainExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        self.core.venue
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        todo!("implement get_account")
    }

    fn generate_account_state(
        &self,
        _balances: Vec<AccountBalance>,
        _margins: Vec<MarginBalance>,
        _reported: bool,
        _ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        todo!("implement generate_account_state")
    }

    fn start(&mut self) -> anyhow::Result<()> {
        todo!("implement start")
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        todo!("implement stop")
    }

    fn submit_order(&self, _cmd: SubmitOrder) -> anyhow::Result<()> {
        todo!("implement submit_order")
    }

    fn submit_order_list(&self, _cmd: SubmitOrderList) -> anyhow::Result<()> {
        todo!("implement submit_order_list")
    }

    fn modify_order(&self, _cmd: ModifyOrder) -> anyhow::Result<()> {
        todo!("implement modify_order")
    }

    fn cancel_order(&self, _cmd: CancelOrder) -> anyhow::Result<()> {
        todo!("implement cancel_order")
    }

    fn cancel_all_orders(&self, _cmd: CancelAllOrders) -> anyhow::Result<()> {
        todo!("implement cancel_all_orders")
    }

    fn batch_cancel_orders(&self, _cmd: BatchCancelOrders) -> anyhow::Result<()> {
        todo!("implement batch_cancel_orders")
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        todo!("implement query_account")
    }

    fn query_order(&self, _cmd: QueryOrder) -> anyhow::Result<()> {
        todo!("implement query_order")
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            log::warn!("Blockchain execution client already connected");
            return Ok(());
        }

        log::info!(
            "Connecting to blockchain execution client on chain {}",
            self.chain.name
        );

        self.refresh_wallet_balances().await?;

        // Attach the durable store for execution transaction records when configured
        if let Some(pg_options) = &self.config.postgres_cache_database_config {
            let database =
                crate::cache::database::BlockchainCacheDatabase::connect(pg_options.clone().into())
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to connect to the Postgres cache database: {e}")
                    })?;
            self.cache.database = Some(database);
            self.cache.initialize_chain().await;
        } else {
            log::warn!(
                "No Postgres cache database configured; transactions will be refused (no durable store)"
            );
        }

        // Verify the RPC chain ID against configuration before any signature
        let expected_chain_id = u64::from(self.chain.chain_id);
        let actual_chain_id = self.http_rpc_client.chain_id().await?;
        if actual_chain_id != expected_chain_id {
            anyhow::bail!(
                "Chain ID mismatch at connect: expected {expected_chain_id}, node reported {actual_chain_id}"
            );
        }

        // Load the signer key from the configured environment variable; the key is never
        // logged, serialized, or stored in configuration
        let private_key = std::env::var(&self.config.signer_private_key_env).map_err(|_| {
            anyhow::anyhow!(
                "Signer private key environment variable '{}' is not set",
                self.config.signer_private_key_env
            )
        })?;
        let signer = PrivateKeySigner::from_str(private_key.trim()).map_err(|_| {
            anyhow::anyhow!(
                "Signer private key in '{}' is not a valid hex private key",
                self.config.signer_private_key_env
            )
        })?;

        if signer.address() != self.wallet_address {
            anyhow::bail!(
                "Signer address {} derived from '{}' does not match configured wallet address {}",
                signer.address(),
                self.config.signer_private_key_env,
                self.wallet_address
            );
        }
        self.signer = Some(signer);

        self.core.set_connected();
        log::info!(
            "Blockchain execution client connected on chain {}",
            self.chain.name
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.signer = None;
        self.core.set_disconnected();
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        todo!("implement generate_order_status_report")
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        todo!("implement generate_order_status_reports")
    }

    async fn generate_fill_reports(
        &self,
        _cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        todo!("implement generate_fill_reports")
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        todo!("implement generate_position_status_reports")
    }

    async fn generate_mass_status(
        &self,
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        todo!("implement generate_mass_status")
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use alloy::primitives::address;
    use nautilus_common::cache::Cache;
    use nautilus_infrastructure::sql::pg::{PostgresConnectOptions, get_postgres_connect_options};
    use nautilus_model::{defi::chain::chains, enums::AccountType, identifiers::TraderId};
    use rstest::rstest;
    use sqlx::postgres::PgPoolOptions;

    use super::*;
    use crate::{
        constants::BLOCKCHAIN_VENUE,
        exchanges::arbitrum::UNISWAP_V3,
        rpc::http::{
            EXECUTION_RPC_TIMEOUT_SECS,
            tests::mock::{MockRpcState, start_mock_rpc_server},
        },
    };

    const CHAIN_ID_ARBITRUM: &str =
        include_str!("../../test_data/execution/rpc_eth_chain_id_arbitrum.json");
    const CHAIN_ID_ETHEREUM: &str =
        include_str!("../../test_data/execution/rpc_eth_chain_id_ethereum.json");
    const GET_CODE_DEPLOYED: &str =
        include_str!("../../test_data/execution/rpc_eth_get_code_deployed.json");
    const GET_CODE_EMPTY: &str =
        include_str!("../../test_data/execution/rpc_eth_get_code_empty.json");
    const GET_BALANCE: &str = include_str!("../../test_data/execution/rpc_eth_get_balance.json");
    const GET_BALANCE_ZERO: &str =
        include_str!("../../test_data/execution/rpc_eth_get_balance_zero.json");
    const CALL_BALANCE: &str = include_str!("../../test_data/execution/rpc_eth_call_balance.json");
    const CALL_BALANCE_AFTER_WRAP: &str =
        include_str!("../../test_data/execution/rpc_eth_call_balance_after_wrap.json");
    const CALL_BOOL_TRUE: &str =
        include_str!("../../test_data/execution/rpc_eth_call_bool_true.json");
    const CALL_EMPTY: &str = include_str!("../../test_data/execution/rpc_eth_call_empty.json");
    const CALL_ZERO: &str = include_str!("../../test_data/execution/rpc_eth_call_zero.json");
    const CALL_ALLOWANCE: &str =
        include_str!("../../test_data/execution/rpc_eth_call_allowance.json");
    const TRANSACTION_COUNT: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_count.json");
    const ESTIMATE_GAS: &str = include_str!("../../test_data/execution/rpc_eth_estimate_gas.json");
    const MAX_PRIORITY_FEE: &str =
        include_str!("../../test_data/execution/rpc_eth_max_priority_fee_per_gas.json");
    const BLOCK_BY_NUMBER: &str =
        include_str!("../../test_data/execution/rpc_eth_get_block_by_number.json");
    const RECEIPT_SUCCESS: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_receipt_success.json");
    const RECEIPT_REVERTED: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_receipt_reverted.json");
    const RECEIPT_NULL: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_receipt_null.json");
    const SEND_RAW_TRANSACTION: &str =
        include_str!("../../test_data/execution/rpc_eth_send_raw_transaction.json");
    const SEND_RAW_TRANSACTION_REJECTED: &str =
        include_str!("../../test_data/execution/rpc_eth_send_raw_transaction_rejected.json");
    const RPC_METHOD_NOT_FOUND: &str =
        include_str!("../../test_data/execution/rpc_error_method_not_found.json");

    const WALLET: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    const ROUTER: &str = "0xE592427A0AEce92De3Edee1F18E0157C05861564";
    const WETH: &str = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1";

    // Anvil development key for WALLET (public, test-only)
    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    const BALANCE_OF_SELECTOR: &str = "0x70a08231";
    const ALLOWANCE_SELECTOR: &str = "0xdd62ed3e";

    fn test_pool() -> Pool {
        let chain = Arc::new(chains::ARBITRUM.clone());
        let dex = UNISWAP_V3.dex.clone();
        let weth = Token::new(
            chain.clone(),
            address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            "Wrapped Ether".to_string(),
            "WETH".to_string(),
            18,
        );
        let usdc = Token::new(
            chain.clone(),
            address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
            "USD Coin".to_string(),
            "USDC".to_string(),
            6,
        );

        Pool::new(
            chain,
            dex,
            address!("C6962004f452bE9203591991D15f6b388e09E8D0"),
            PoolIdentifier::from_address(address!("C6962004f452bE9203591991D15f6b388e09E8D0")),
            55_000_000,
            weth,
            usdc,
            Some(500),
            Some(10),
            UnixNanos::default(),
        )
    }

    fn test_config(http_rpc_url: String) -> BlockchainExecutionClientConfig {
        test_config_with_signer_env(http_rpc_url, "BLOCKCHAIN_TEST_PRIVATE_KEY")
    }

    fn test_config_with_signer_env(
        http_rpc_url: String,
        signer_env: &str,
    ) -> BlockchainExecutionClientConfig {
        BlockchainExecutionClientConfig::builder()
            .trader_id(TraderId::from("TRADER-001"))
            .client_id(AccountId::from("BLOCKCHAIN-001"))
            .chain(chains::ARBITRUM.clone())
            .wallet_address(WALLET.to_string())
            .http_rpc_url(http_rpc_url)
            .signer_private_key_env(signer_env.to_string())
            .router_addresses(vec![ROUTER.to_string()])
            .weth_address(WETH.to_string())
            .max_fee_per_gas_wei(1_000_000_000)
            .base_fee_buffer_bps(2_000)
            .gas_limit(1_000_000)
            .gas_buffer_bps(2_000)
            .build()
    }

    fn test_client_from_config(
        config: BlockchainExecutionClientConfig,
        pool: Pool,
    ) -> BlockchainExecutionClient {
        test_client_result(config, pool).unwrap()
    }

    fn test_client_result(
        config: BlockchainExecutionClientConfig,
        pool: Pool,
    ) -> anyhow::Result<BlockchainExecutionClient> {
        let cache = Rc::new(RefCell::new(Cache::default()));
        cache.borrow_mut().add_pool(pool).unwrap();
        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from("BLOCKCHAIN-001"),
            *BLOCKCHAIN_VENUE,
            OmsType::Netting,
            AccountId::from("BLOCKCHAIN-001"),
            AccountType::Wallet,
            None,
            cache,
        );

        BlockchainExecutionClient::new(core, config)
    }

    fn test_client(http_rpc_url: String) -> BlockchainExecutionClient {
        test_client_from_config(test_config(http_rpc_url), test_pool())
    }

    async fn client_with_mock_rpc(
        state: MockRpcState,
    ) -> (BlockchainExecutionClient, MockRpcState) {
        let addr = start_mock_rpc_server(state.clone()).await;
        (test_client(format!("http://{addr}")), state)
    }

    fn execution_rpc_state() -> MockRpcState {
        MockRpcState::default()
            .with_receipt_hash_from_request()
            .with_response("eth_chainId", CHAIN_ID_ARBITRUM)
            .with_response("eth_getCode", GET_CODE_DEPLOYED)
            .with_response("eth_getBalance", GET_BALANCE)
            .with_response("eth_getBlockByNumber", BLOCK_BY_NUMBER)
            .with_response("eth_maxPriorityFeePerGas", MAX_PRIORITY_FEE)
    }

    fn ready_rpc_state() -> MockRpcState {
        execution_rpc_state()
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE)
            .with_call_response(ALLOWANCE_SELECTOR, CALL_ALLOWANCE)
    }

    #[tokio::test]
    async fn preflight_ready_when_all_checks_pass() {
        let (client, _) = client_with_mock_rpc(ready_rpc_state()).await;
        let pool = test_pool();

        let report = client.preflight(&pool.instrument_id).await.unwrap();

        assert!(report.ready, "issues: {:?}", report.issues);
        assert!(report.issues.is_empty());
        assert_eq!(report.expected_chain_id, 42161);
        assert_eq!(report.actual_chain_id, 42161);
        assert!(report.chain_id_matches);
        assert_eq!(
            report.pool.address,
            address!("C6962004f452bE9203591991D15f6b388e09E8D0")
        );
        assert!(report.pool.has_deployed_code);
        assert_eq!(report.pool.fee, Some(500));
        assert_eq!(
            report.pool.base_token,
            address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1")
        );
        assert_eq!(
            report.pool.quote_token,
            address!("af88d065e77c8cC2239327C5EDb3A432268e5831")
        );
        assert_eq!(report.routers.len(), 1);
        assert!(report.routers[0].has_deployed_code);
        assert_eq!(report.tokens.len(), 2);
        assert_eq!(
            report.tokens[0].wallet_balance,
            U256::from(500_000_000_000_000_000u64)
        );
        assert_eq!(
            report.tokens[0].router_allowances,
            vec![(
                address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
                U256::from(1_000_000_000_000_000_000u64)
            )]
        );
        assert_eq!(
            report.native_balance_wei,
            U256::from(1_000_000_000_000_000_000u64)
        );
        assert_eq!(report.base_fee_per_gas_wei, 100_000_000);
        assert_eq!(report.max_priority_fee_per_gas_wei, 10_000_000);
        assert_eq!(report.derived_max_fee_per_gas_wei, 130_000_000);
        assert!(report.fee_within_ceiling);
    }

    #[tokio::test]
    async fn preflight_not_ready_without_pool_fee() {
        let addr = start_mock_rpc_server(ready_rpc_state()).await;
        let mut pool = test_pool();
        pool.fee = None;
        let client = test_client_from_config(test_config(format!("http://{addr}")), pool.clone());

        let report = client.preflight(&pool.instrument_id).await.unwrap();

        assert!(!report.ready);
        assert_eq!(report.pool.fee, None);
        assert_eq!(report.issues, vec!["Pool fee tier is missing"]);
    }

    #[tokio::test]
    async fn preflight_not_ready_on_wrong_chain() {
        let state = ready_rpc_state().with_response("eth_chainId", CHAIN_ID_ETHEREUM);
        let (client, _) = client_with_mock_rpc(state).await;
        let pool = test_pool();

        let report = client.preflight(&pool.instrument_id).await.unwrap();

        assert!(!report.ready);
        assert!(!report.chain_id_matches);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("Chain ID mismatch"))
        );
    }

    #[tokio::test]
    async fn preflight_not_ready_without_deployed_code() {
        let state = ready_rpc_state().with_response("eth_getCode", GET_CODE_EMPTY);
        let (client, _) = client_with_mock_rpc(state).await;
        let pool = test_pool();

        let report = client.preflight(&pool.instrument_id).await.unwrap();

        assert!(!report.ready);
        assert!(!report.pool.has_deployed_code);
        assert!(!report.routers[0].has_deployed_code);
        assert!(report.tokens.iter().all(|t| !t.has_deployed_code));
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("No deployed bytecode at router address"))
        );
    }

    #[tokio::test]
    async fn preflight_not_ready_with_zero_balances_and_allowance() {
        let state = ready_rpc_state()
            .with_response("eth_getBalance", GET_BALANCE_ZERO)
            .with_call_response(BALANCE_OF_SELECTOR, CALL_ZERO)
            .with_call_response(ALLOWANCE_SELECTOR, CALL_ZERO);
        let (client, _) = client_with_mock_rpc(state).await;
        let pool = test_pool();

        let report = client.preflight(&pool.instrument_id).await.unwrap();

        assert!(!report.ready);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("Native currency balance is zero"))
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("balance is zero"))
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("No router allowance"))
        );
    }

    #[tokio::test]
    async fn preflight_not_ready_when_fees_exceed_ceiling() {
        let addr = start_mock_rpc_server(ready_rpc_state()).await;
        let mut config = test_config(format!("http://{addr}"));
        config.max_fee_per_gas_wei = 1;
        let pool = test_pool();
        let client = test_client_from_config(config, pool.clone());

        let report = client.preflight(&pool.instrument_id).await.unwrap();

        assert!(!report.ready);
        assert!(!report.fee_within_ceiling);
        assert_eq!(report.derived_max_fee_per_gas_wei, 130_000_000);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("exceeds ceiling"))
        );
    }

    #[rstest]
    fn resolve_pool_rejects_unknown_pool() {
        let client = test_client("http://127.0.0.1:1".to_string());
        let unknown: InstrumentId = "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45.Arbitrum:UniswapV3"
            .parse()
            .unwrap();

        let error = client.resolve_pool(&unknown).unwrap_err();

        assert!(error.to_string().contains("Unknown pool"), "was: {error}");
    }

    #[rstest]
    fn resolve_pool_rejects_mismatched_chain() {
        let client = test_client("http://127.0.0.1:1".to_string());
        let ethereum_pool: InstrumentId =
            "0xC6962004f452bE9203591991D15f6b388e09E8D0.Ethereum:UniswapV3"
                .parse()
                .unwrap();

        let error = client.resolve_pool(&ethereum_pool).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not match the client chain"),
            "was: {error}"
        );
    }

    #[rstest]
    fn resolve_pool_rejects_unsupported_dex() {
        let client = test_client("http://127.0.0.1:1".to_string());
        let v4_pool: InstrumentId = "0xC6962004f452bE9203591991D15f6b388e09E8D0.Arbitrum:UniswapV4"
            .parse()
            .unwrap();

        let error = client.resolve_pool(&v4_pool).unwrap_err();

        assert!(
            error.to_string().contains("only UniswapV3 is supported"),
            "was: {error}"
        );
    }

    #[rstest]
    fn resolve_pool_rejects_pool_id_identifier() {
        let client = test_client("http://127.0.0.1:1".to_string());
        let pool_id: InstrumentId =
            "0x0000000000000000000000000000000000000000000000000000000000000000.Arbitrum:UniswapV3"
                .parse()
                .unwrap();

        let error = client.resolve_pool(&pool_id).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("only address identifiers are supported"),
            "was: {error}"
        );
    }

    #[rstest]
    fn resolve_pool_rejects_ambiguous_token_priority() {
        let chain = Arc::new(chains::ARBITRUM.clone());
        let dex = UNISWAP_V3.dex.clone();
        let token_a = Token::new(
            chain.clone(),
            address!("1111111111111111111111111111111111111111"),
            "Token A".to_string(),
            "TOKA".to_string(),
            18,
        );
        let token_b = Token::new(
            chain.clone(),
            address!("2222222222222222222222222222222222222222"),
            "Token B".to_string(),
            "TOKB".to_string(),
            18,
        );
        let pool = Pool::new(
            chain,
            dex,
            address!("3333333333333333333333333333333333333333"),
            PoolIdentifier::from_address(address!("3333333333333333333333333333333333333333")),
            55_000_000,
            token_a,
            token_b,
            Some(500),
            Some(10),
            UnixNanos::default(),
        );
        let client =
            test_client_from_config(test_config("http://127.0.0.1:1".to_string()), pool.clone());

        let error = client.resolve_pool(&pool.instrument_id).unwrap_err();

        assert!(error.to_string().contains("ambiguous"), "was: {error}");
    }

    #[rstest]
    fn new_rejects_empty_router_allowlist() {
        let mut config = test_config("http://127.0.0.1:1".to_string());
        config.router_addresses = Vec::new();

        let error = test_client_result(config, test_pool()).unwrap_err();

        assert!(
            error.to_string().contains("at least one router address"),
            "was: {error}"
        );
    }

    #[tokio::test]
    async fn wrap_refuses_without_durable_store() {
        let (mut client, _) = client_with_mock_rpc(ready_rpc_state()).await;
        client.core.set_connected();

        let error = client.wrap(U256::from(1_000u64)).await.unwrap_err();

        assert!(
            error.to_string().contains("No durable store configured"),
            "was: {error}"
        );
    }

    #[tokio::test]
    async fn approve_rejects_router_outside_allowlist() {
        let (mut client, _) = client_with_mock_rpc(ready_rpc_state()).await;

        let error = client
            .approve(
                address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                U256::from(1_000u64),
                address!("68b3465833fb72A70ecDF485E0e4C7bD8665Fc45"),
            )
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("not in the configured `router_addresses` allowlist"),
            "was: {error}"
        );
    }

    #[tokio::test]
    async fn in_flight_guard_rejects_second_transaction() {
        let (mut client, _) = client_with_mock_rpc(ready_rpc_state()).await;
        client.core.set_connected();
        client.in_flight = Some(InFlightTransaction {
            nonce: 7,
            tx_hash: B256::ZERO,
            purpose: TransactionPurpose::Wrap,
        });

        let error = client.wrap(U256::from(1_000u64)).await.unwrap_err();

        assert!(
            error.to_string().contains("still awaiting inclusion"),
            "was: {error}"
        );
    }

    #[tokio::test]
    async fn wrap_rejects_zero_amount() {
        let (mut client, _) = client_with_mock_rpc(ready_rpc_state()).await;

        let error = client.wrap(U256::ZERO).await.unwrap_err();

        assert!(
            error.to_string().contains("Wrap amount must be positive"),
            "was: {error}"
        );
    }

    #[tokio::test]
    async fn wrap_rejects_code_free_target_before_broadcast() {
        let state = execution_rpc_state().with_response("eth_getCode", GET_CODE_EMPTY);
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_wrap_code_free_test", state).await
        else {
            return;
        };

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("No deployed bytecode"),
            "was: {error}"
        );
        assert!(client.in_flight.is_none());
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "eth_getCode");

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn wrap_rejects_unrelated_target_before_broadcast() {
        let state = execution_rpc_state().with_response("eth_call", CALL_EMPTY);
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_wrap_unrelated_test", state).await
        else {
            return;
        };

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("Decoding error"), "was: {error}");
        assert!(client.in_flight.is_none());
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["method"], "eth_getCode");
        assert_eq!(requests[1]["method"], "eth_call");

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn wrap_rejects_included_transaction_without_balance_delta() {
        let state = execution_rpc_state()
            .with_response_sequence("eth_call", &[CALL_BALANCE, CALL_BALANCE, CALL_BALANCE])
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION);
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_wrap_no_delta_test", state).await
        else {
            return;
        };

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("did not increase"),
            "was: {error}"
        );
        assert!(client.in_flight.is_none());
        let status: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT status FROM {schema}.execution_transaction"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(status, "included");
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        assert_eq!(broadcasts, 1);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn wrap_reports_inclusion_when_postcondition_read_fails() {
        let state = execution_rpc_state()
            .with_response_sequence(
                "eth_call",
                &[CALL_BALANCE, CALL_BALANCE, RPC_METHOD_NOT_FOUND],
            )
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION);
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_wrap_postcondition_rpc_test", state).await
        else {
            return;
        };

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("failed to read WETH balance after included transaction 0x"),
            "was: {message}"
        );
        assert!(message.contains("at block 30346561"), "was: {message}");
        assert!(client.in_flight.is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_rejects_false_return_before_broadcast() {
        let state = execution_rpc_state().with_response("eth_call", CALL_ZERO);
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_approve_false_test", state).await
        else {
            return;
        };

        let error = client
            .approve(
                address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                U256::from(1_000u64),
                address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("returned false"), "was: {error}");
        assert!(client.in_flight.is_none());
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["method"], "eth_getCode");
        assert_eq!(requests[1]["method"], "eth_call");
        assert_eq!(requests[1]["params"][0]["from"], WALLET);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_accepts_empty_return_with_sufficient_allowance() {
        let state = execution_rpc_state()
            .with_response_sequence("eth_call", &[CALL_EMPTY, CALL_ALLOWANCE])
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION);
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_approve_empty_test", state).await
        else {
            return;
        };

        let tx_hash = client
            .approve(
                address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                U256::from(1_000u64),
                address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
            )
            .await
            .unwrap();

        let record = client
            .cache
            .get_execution_transaction(42161, &tx_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.purpose, "approve");
        assert_eq!(record.status, "included");
        assert!(client.in_flight.is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_rejects_empty_return_with_insufficient_allowance() {
        let state = execution_rpc_state()
            .with_response_sequence("eth_call", &[CALL_EMPTY, CALL_ZERO])
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION);
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_approve_insufficient_test", state).await
        else {
            return;
        };

        let error = client
            .approve(
                address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                U256::from(1_000u64),
                address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
            )
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("below the requested amount"),
            "was: {error}"
        );
        assert!(client.in_flight.is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_reports_inclusion_when_postcondition_read_fails() {
        let state = execution_rpc_state()
            .with_response_sequence("eth_call", &[CALL_BOOL_TRUE, RPC_METHOD_NOT_FOUND])
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION);
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_approve_postcondition_rpc_test", state).await
        else {
            return;
        };

        let error = client
            .approve(
                address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                U256::from(1_000u64),
                address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
            )
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("failed to read router allowance after included transaction 0x"),
            "was: {message}"
        );
        assert!(message.contains("at block 30346561"), "was: {message}");
        assert!(client.in_flight.is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn connect_rejects_on_chain_mismatch() {
        let state = ready_rpc_state().with_response("eth_chainId", CHAIN_ID_ETHEREUM);
        let (mut client, _) = client_with_mock_rpc(state).await;

        let error = client.connect().await.unwrap_err();

        assert!(
            error.to_string().contains("Chain ID mismatch at connect"),
            "was: {error}"
        );
    }

    #[allow(unsafe_code)] // env-var mutation in tests; unique var names avoid cross-test races
    #[tokio::test]
    async fn connect_rejects_on_signer_wallet_mismatch() {
        let addr = start_mock_rpc_server(ready_rpc_state()).await;
        let config = test_config_with_signer_env(
            format!("http://{addr}"),
            "BLOCKCHAIN_TEST_PRIVATE_KEY_MISMATCH",
        );
        let mut client = test_client_from_config(config, test_pool());
        // Anvil development key #1 derives a different address than the configured wallet
        // SAFETY: this variable name is unique to this test across the test binary
        unsafe {
            std::env::set_var(
                "BLOCKCHAIN_TEST_PRIVATE_KEY_MISMATCH",
                "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
            )
        };

        let error = client.connect().await.unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not match configured wallet address"),
            "was: {error}"
        );
    }

    #[allow(unsafe_code)] // env-var mutation in tests; unique var names avoid cross-test races
    #[tokio::test]
    async fn connect_initializes_signer_from_env() {
        let addr = start_mock_rpc_server(ready_rpc_state()).await;
        let config =
            test_config_with_signer_env(format!("http://{addr}"), "BLOCKCHAIN_TEST_PRIVATE_KEY_OK");
        let mut client = test_client_from_config(config, test_pool());
        // SAFETY: this variable name is unique to this test across the test binary
        unsafe { std::env::set_var("BLOCKCHAIN_TEST_PRIVATE_KEY_OK", TEST_PRIVATE_KEY) };

        client.connect().await.unwrap();

        let signer = client.signer.as_ref().unwrap();
        assert_eq!(
            signer.address(),
            address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266")
        );
    }

    #[tokio::test]
    async fn disconnect_revokes_signer_and_blocks_execution() {
        let (mut client, state) = client_with_mock_rpc(ready_rpc_state()).await;
        client.core.set_connected();
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());

        client.disconnect().await.unwrap();
        let error = client.wrap(U256::from(1_000u64)).await.unwrap_err();

        assert!(!client.is_connected());
        assert!(client.signer.is_none());
        assert!(
            error.to_string().contains("is not connected"),
            "was: {error}"
        );
        assert!(state.recorded_requests().is_empty());
    }

    #[tokio::test]
    async fn poll_for_receipt_returns_none_after_exhaustion() {
        let state = ready_rpc_state().with_response("eth_getTransactionReceipt", RECEIPT_NULL);
        let (client, state) = client_with_mock_rpc(state).await;

        let receipt = client
            .poll_for_receipt(&B256::ZERO, 3, Duration::ZERO)
            .await
            .unwrap();

        assert!(receipt.is_none());
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 3);
    }

    #[tokio::test]
    async fn poll_for_receipt_returns_last_error_when_every_poll_fails() {
        let state =
            ready_rpc_state().with_response("eth_getTransactionReceipt", RPC_METHOD_NOT_FOUND);
        let (client, state) = client_with_mock_rpc(state).await;

        let error = client
            .poll_for_receipt(&B256::ZERO, 3, Duration::ZERO)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "eth_getTransactionReceipt RPC error -32601"
        );
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 3);
    }

    #[tokio::test]
    async fn poll_for_receipt_returns_none_after_pending_then_errors() {
        let state = ready_rpc_state().with_response_sequence(
            "eth_getTransactionReceipt",
            &[RECEIPT_NULL, RPC_METHOD_NOT_FOUND, RPC_METHOD_NOT_FOUND],
        );
        let (client, state) = client_with_mock_rpc(state).await;

        let receipt = client
            .poll_for_receipt(&B256::ZERO, 3, Duration::ZERO)
            .await
            .unwrap();

        assert!(receipt.is_none());
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 3);
    }

    #[tokio::test]
    async fn cancellation_during_persistence_keeps_in_flight_slot() {
        let Some((admin_pool, pg_config)) = connect_test_postgres("persistence cancellation").await
        else {
            return;
        };

        let schema = format!("execution_persist_cancel_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;

        let observer_options: sqlx::postgres::PgConnectOptions = pg_config.clone().into();
        let observer_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(observer_options)
            .await
            .unwrap();
        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();

        let state = ready_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS);
        let addr = start_mock_rpc_server(state.clone()).await;

        let mut client = test_client(format!("http://{addr}"));
        client.cache.database = Some(database);
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        let mut lock_transaction = admin_pool.begin().await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "LOCK TABLE {schema}.execution_transaction IN ACCESS EXCLUSIVE MODE"
        )))
        .execute(&mut *lock_transaction)
        .await
        .unwrap();

        let value = U256::from(1_000_000_000_000_000u64);
        let mut wrap = Box::pin(client.wrap(value));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                tokio::select! {
                    result = &mut wrap => {
                        panic!("persistence completed while the table was locked: {result:?}")
                    }
                    () = tokio::time::sleep(Duration::from_millis(1)) => {}
                }

                let waiting = sqlx::query_scalar::<_, bool>(
                    "
                    SELECT EXISTS (
                        SELECT 1
                        FROM pg_locks AS locks
                        JOIN pg_class AS relations ON relations.oid = locks.relation
                        JOIN pg_namespace AS namespaces ON namespaces.oid = relations.relnamespace
                        WHERE namespaces.nspname = $1
                          AND relations.relname = 'execution_transaction'
                          AND NOT locks.granted
                    )
                    ",
                )
                .bind(&schema)
                .fetch_one(&observer_pool)
                .await
                .unwrap();

                if waiting {
                    break;
                }
            }
        })
        .await
        .unwrap();
        drop(wrap);

        let in_flight = client.in_flight.unwrap();
        let second_error = client
            .wrap(U256::from(2_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        let expected_tx = build_eip1559_transaction(
            42161,
            7,
            78_000,
            130_000_000,
            10_000_000,
            address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            value,
            Bytes::from(nautilus_core::hex::decode("d0e30db0").unwrap()),
        );
        let (expected_hash, _) = sign_eip1559_transaction(
            expected_tx,
            &PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.tx_hash, expected_hash);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert!(
            second_error
                .to_string()
                .contains("still awaiting inclusion"),
            "was: {second_error}"
        );
        assert_eq!(broadcasts, 0);

        lock_transaction.rollback().await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn persistence_failure_keeps_unbroadcast_slot() {
        let Some((admin_pool, pg_config)) = connect_test_postgres("persistence failure").await
        else {
            return;
        };

        let schema = format!("execution_persist_fail_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP TABLE {schema}.execution_transaction"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let state = ready_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS);
        let addr = start_mock_rpc_server(state.clone()).await;

        let mut client = test_client(format!("http://{addr}"));
        client.cache.database = Some(database);
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        let value = U256::from(1_000_000_000_000_000u64);
        let error = client.wrap(value).await.unwrap_err();
        let in_flight = client.in_flight.unwrap();
        let second_error = client
            .wrap(U256::from(2_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        let expected_tx = build_eip1559_transaction(
            42161,
            7,
            78_000,
            130_000_000,
            10_000_000,
            address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            value,
            Bytes::from(nautilus_core::hex::decode("d0e30db0").unwrap()),
        );
        let (expected_hash, _) = sign_eip1559_transaction(
            expected_tx,
            &PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        )
        .await
        .unwrap();

        let error_message = error.to_string();
        assert!(
            error_message.starts_with(&format!(
                "Failed to persist transaction {expected_hash}: Failed to insert into execution_transaction table"
            )),
            "was: {error_message}"
        );
        assert!(
            error_message.ends_with("the in-flight slot stays occupied"),
            "was: {error_message}"
        );
        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.tx_hash, expected_hash);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert!(
            second_error
                .to_string()
                .contains("still awaiting inclusion"),
            "was: {second_error}"
        );
        assert_eq!(broadcasts, 0);

        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cancellation_after_dispatch_keeps_record_and_in_flight_slot() {
        let Some((admin_pool, pg_config)) = connect_test_postgres("broadcast cancellation").await
        else {
            return;
        };

        let schema = format!("execution_cancel_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();

        let state = ready_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_sleep(
                "eth_sendRawTransaction",
                Duration::from_secs(EXECUTION_RPC_TIMEOUT_SECS + 2),
            );
        let addr = start_mock_rpc_server(state.clone()).await;

        let mut client = test_client(format!("http://{addr}"));
        client.cache.database = Some(database);
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        let mut wrap = Box::pin(client.wrap(U256::from(1_000_000_000_000_000u64)));
        tokio::select! {
            result = &mut wrap => panic!("broadcast completed before cancellation: {result:?}"),
            result = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if state.recorded_requests().iter().any(|request| {
                        request["method"] == "eth_sendRawTransaction"
                    }) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }) => result.unwrap(),
        }
        drop(wrap);

        let in_flight = client.in_flight.unwrap();
        let error = client
            .wrap(U256::from(2_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let record = client
            .cache
            .get_execution_transaction(42161, &in_flight.tx_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();

        assert!(
            error.to_string().contains("still awaiting inclusion"),
            "was: {error}"
        );
        assert_eq!(record.nonce, 7);
        assert_eq!(record.purpose, "wrap");
        assert_eq!(record.status, "pending");
        assert_eq!(broadcasts, 1);

        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cancellation_during_receipt_polling_keeps_record_and_in_flight_slot() {
        let state = ready_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION)
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_sleep(
                "eth_getTransactionReceipt",
                Duration::from_secs(EXECUTION_RPC_TIMEOUT_SECS + 2),
            );
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_receipt_cancel_test", state).await
        else {
            return;
        };

        let mut wrap = Box::pin(client.wrap(U256::from(1_000_000_000_000_000u64)));
        tokio::select! {
            result = &mut wrap => panic!("receipt polling completed before cancellation: {result:?}"),
            result = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if state.recorded_requests().iter().any(|request| {
                        request["method"] == "eth_getTransactionReceipt"
                    }) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }) => result.unwrap(),
        }
        drop(wrap);

        let in_flight = client.in_flight.unwrap();
        let second_error = client
            .wrap(U256::from(2_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let record = client
            .cache
            .get_execution_transaction(42161, &in_flight.tx_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        let requests = state.recorded_requests();

        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert!(
            second_error
                .to_string()
                .contains("still awaiting inclusion"),
            "was: {second_error}"
        );
        assert_eq!(record.nonce, 7);
        assert_eq!(record.transaction_hash, in_flight.tx_hash.to_string());
        assert_eq!(record.purpose, "wrap");
        assert_eq!(record.status, "pending");
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_sendRawTransaction")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_getTransactionReceipt")
                .count(),
            1
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn rejected_broadcast_marks_record_rejected_before_releasing_slot() {
        let Some((admin_pool, pg_config)) = connect_test_postgres("broadcast rejection").await
        else {
            return;
        };

        let schema = format!("execution_rejected_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();

        let state = ready_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION_REJECTED);
        let addr = start_mock_rpc_server(state.clone()).await;

        let mut client = test_client(format!("http://{addr}"));
        client.cache.database = Some(database);
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let (purpose, status): (String, String) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT purpose, status FROM {schema}.execution_transaction"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();

        assert!(
            error
                .to_string()
                .contains("Broadcast rejected with RPC error -32000"),
            "was: {error}"
        );
        assert!(client.in_flight.is_none());
        assert_eq!(purpose, "wrap");
        assert_eq!(status, "rejected");
        assert_eq!(broadcasts, 1);

        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rejected_broadcast_keeps_slot_when_status_update_fails() {
        let Some((admin_pool, pg_config)) =
            connect_test_postgres("broadcast rejection persistence failure").await
        else {
            return;
        };

        let schema = format!("execution_rejected_update_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();

        let state = ready_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION_REJECTED)
            .with_sleep("eth_sendRawTransaction", Duration::from_secs(1));
        let addr = start_mock_rpc_server(state.clone()).await;

        let mut client = test_client(format!("http://{addr}"));
        client.cache.database = Some(database);
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        let mut wrap = Box::pin(client.wrap(U256::from(1_000_000_000_000_000u64)));
        tokio::select! {
            result = &mut wrap => panic!("broadcast completed before database failure: {result:?}"),
            result = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if state.recorded_requests().iter().any(|request| {
                        request["method"] == "eth_sendRawTransaction"
                    }) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }) => result.unwrap(),
        }
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP TABLE {schema}.execution_transaction"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let error = wrap.await.unwrap_err();

        assert!(
            error.to_string().contains("Failed to persist rejection"),
            "was: {error}"
        );
        let in_flight = client.in_flight.unwrap();
        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);

        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn included_receipt_keeps_slot_when_status_update_fails() {
        let state = ready_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION)
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_sleep("eth_getTransactionReceipt", Duration::from_secs(1));
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_included_update_test", state).await
        else {
            return;
        };

        let mut wrap = Box::pin(client.wrap(U256::from(1_000_000_000_000_000u64)));
        tokio::select! {
            result = &mut wrap => panic!("receipt completed before database failure: {result:?}"),
            result = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if state.recorded_requests().iter().any(|request| {
                        request["method"] == "eth_getTransactionReceipt"
                    }) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }) => result.unwrap(),
        }
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP TABLE {schema}.execution_transaction"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let error = wrap.await.unwrap_err();
        let in_flight = client.in_flight.unwrap();
        let requests = state.recorded_requests();

        assert!(
            error
                .to_string()
                .contains("Failed to update persisted status"),
            "was: {error}"
        );
        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_sendRawTransaction")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_getTransactionReceipt")
                .count(),
            1
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn broadcast_timeout_after_send_keeps_record_pending() {
        let Some((admin_pool, pg_config)) = connect_test_postgres("broadcast timeout").await else {
            return;
        };

        let schema = format!("execution_timeout_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();

        let state = ready_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_sleep(
                "eth_sendRawTransaction",
                Duration::from_secs(EXECUTION_RPC_TIMEOUT_SECS + 2),
            );
        let addr = start_mock_rpc_server(state).await;

        let mut client = test_client(format!("http://{addr}"));
        client.cache.database = Some(database);
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("timed out after send"),
            "was: {error}"
        );
        let in_flight = client.in_flight.unwrap();
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert_eq!(in_flight.nonce, 7);

        // The persisted record stays pending for reconciliation instead of rebroadcasting
        let record = client
            .cache
            .get_execution_transaction(42161, &in_flight.tx_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.status, "pending");

        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn duplicate_execution_transaction_preserves_original_record() {
        const TRANSACTION_HASH: &str = "0xduplicate-transaction-hash";
        let Some((admin_pool, schema, client, _)) =
            execution_client_with_database("execution_duplicate_record_test", ready_rpc_state())
                .await
        else {
            return;
        };

        client
            .cache
            .add_execution_transaction(42161, 7, TRANSACTION_HASH, "wrap", "pending")
            .await
            .unwrap();
        client
            .cache
            .add_execution_transaction(42161, 8, TRANSACTION_HASH, "approve", "included")
            .await
            .unwrap();

        let record = client
            .cache
            .get_execution_transaction(42161, TRANSACTION_HASH)
            .await
            .unwrap()
            .unwrap();
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert_eq!(record.nonce, 7);
        assert_eq!(record.transaction_hash, TRANSACTION_HASH);
        assert_eq!(record.purpose, "wrap");
        assert_eq!(record.status, "pending");
        assert_eq!(count, 1);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn wrap_then_approve_persists_records_and_clears_in_flight() {
        let Some((admin_pool, pg_config)) = connect_test_postgres("execution persistence").await
        else {
            return;
        };

        let schema = format!("execution_client_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();

        let state = execution_rpc_state()
            .with_response_sequence(
                "eth_call",
                &[
                    CALL_BALANCE,
                    CALL_BALANCE,
                    CALL_BALANCE_AFTER_WRAP,
                    CALL_BOOL_TRUE,
                    CALL_ALLOWANCE,
                ],
            )
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response_sequence(
                "eth_getTransactionReceipt",
                &[RPC_METHOD_NOT_FOUND, RECEIPT_SUCCESS, RECEIPT_SUCCESS],
            )
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION);
        let addr = start_mock_rpc_server(state.clone()).await;

        let mut config = test_config(format!("http://{addr}"));
        config.unlimited_approval = true;
        let mut client = test_client_from_config(config, test_pool());
        client.cache.database = Some(database);
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        let wrap_hash = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap();

        let record = client
            .cache
            .get_execution_transaction(42161, &wrap_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.nonce, 7);
        assert_eq!(record.purpose, "wrap");
        assert_eq!(record.status, "included");

        // The in-flight slot cleared after inclusion, so a second transaction proceeds
        let approve_hash = client
            .approve(
                address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
                U256::from(1_000u64),
                address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
            )
            .await
            .unwrap();

        let record = client
            .cache
            .get_execution_transaction(42161, &approve_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.purpose, "approve");
        assert_eq!(record.status, "included");

        let requests = state.recorded_requests();
        let broadcasts: Vec<_> = requests
            .iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .collect();
        assert_eq!(broadcasts.len(), 2);
        for broadcast in &broadcasts {
            let payload = broadcast["params"][0].as_str().unwrap();
            assert!(payload.starts_with("0x02"), "was: {payload}");
        }
        let receipt_polls = requests
            .iter()
            .filter(|request| request["method"] == "eth_getTransactionReceipt")
            .count();
        assert_eq!(receipt_polls, 3);
        let calls: Vec<_> = requests
            .iter()
            .filter(|request| request["method"] == "eth_call")
            .collect();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[0]["params"][1], "latest");
        assert_eq!(calls[1]["params"][1], "0x1cf0d40");
        assert_eq!(calls[2]["params"][1], "0x1cf0d41");
        assert_eq!(calls[3]["params"][1], "latest");
        assert_eq!(calls[4]["params"][1], "0x1cf0d41");

        // Unlimited approval policy encoded U256::MAX in the approve calldata
        let estimates: Vec<_> = requests
            .iter()
            .filter(|request| request["method"] == "eth_estimateGas")
            .collect();
        let approve_data = estimates[1]["params"][0]["data"].as_str().unwrap();
        assert!(
            approve_data.starts_with("0x095ea7b3"),
            "was: {approve_data}"
        );
        assert!(
            approve_data.ends_with(&"f".repeat(64)),
            "was: {approve_data}"
        );

        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn reverted_receipt_marks_record_reverted_and_errors() {
        let Some((admin_pool, pg_config)) = connect_test_postgres("reverted receipt").await else {
            return;
        };

        let schema = format!("execution_reverted_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();

        let state = ready_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_getTransactionReceipt", RECEIPT_REVERTED)
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION);
        let addr = start_mock_rpc_server(state).await;

        let mut client = test_client(format!("http://{addr}"));
        client.cache.database = Some(database);
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("reverted on-chain"),
            "was: {error}"
        );
        assert!(client.in_flight.is_none());

        // The orchestration's policy math is deterministic: nonce 7 from the fixture,
        // buffered gas 78000, buffered max fee 130000000, priority fee 10000000
        let expected_tx = build_eip1559_transaction(
            42161,
            7,
            78_000,
            130_000_000,
            10_000_000,
            address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            U256::from(1_000_000_000_000_000u64),
            Bytes::from(nautilus_core::hex::decode("d0e30db0").unwrap()),
        );
        let (expected_hash, _) = sign_eip1559_transaction(
            expected_tx,
            &PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        )
        .await
        .unwrap();

        let record = client
            .cache
            .get_execution_transaction(42161, &expected_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.status, "reverted");

        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    async fn connect_test_postgres(
        test_name: &str,
    ) -> Option<(sqlx::PgPool, PostgresConnectOptions)> {
        let pg_config = get_postgres_connect_options(None, None, None, None, None);
        let admin_options: sqlx::postgres::PgConnectOptions = pg_config.clone().into();
        let admin_pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect_with(admin_options)
            .await
        {
            Ok(pool) => pool,
            Err(e) => {
                eprintln!("Postgres unavailable; skipping {test_name} test: {e}");
                return None;
            }
        };

        Some((admin_pool, pg_config))
    }

    async fn execution_client_with_database(
        test_name: &str,
        state: MockRpcState,
    ) -> Option<(
        sqlx::PgPool,
        String,
        BlockchainExecutionClient,
        MockRpcState,
    )> {
        let (admin_pool, pg_config) = connect_test_postgres(test_name).await?;
        let schema = format!("{test_name}_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();
        let addr = start_mock_rpc_server(state.clone()).await;
        let mut client = test_client(format!("http://{addr}"));
        client.cache.database = Some(database);
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        Some((admin_pool, schema, client, state))
    }

    async fn drop_execution_schema(admin_pool: &sqlx::PgPool, schema: &str) {
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(admin_pool)
            .await
            .unwrap();
    }

    async fn setup_execution_schema(admin_pool: &sqlx::PgPool, schema: &str) {
        for statement in [
            format!("CREATE SCHEMA {schema}"),
            format!(
                r#"CREATE TABLE {schema}."chain" (chain_id INTEGER PRIMARY KEY, name TEXT NOT NULL)"#
            ),
            format!(r#"INSERT INTO {schema}."chain" (chain_id, name) VALUES (42161, 'Arbitrum')"#),
            format!(
                r#"CREATE TABLE {schema}."execution_transaction" (
                    id BIGSERIAL PRIMARY KEY,
                    chain_id INTEGER NOT NULL REFERENCES {schema}."chain"(chain_id) ON DELETE CASCADE,
                    nonce BIGINT NOT NULL,
                    transaction_hash TEXT NOT NULL,
                    purpose TEXT NOT NULL,
                    status TEXT NOT NULL,
                    UNIQUE (chain_id, transaction_hash)
                )"#
            ),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(admin_pool)
                .await
                .unwrap();
        }
    }
}
