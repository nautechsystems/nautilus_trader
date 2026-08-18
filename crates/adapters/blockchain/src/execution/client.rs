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

use std::{
    collections::HashSet,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use alloy::{
    primitives::{
        Address, B256, Bytes, I256, U256,
        aliases::{U24, U160},
        keccak256,
    },
    signers::local::PrivateKeySigner,
    sol_types::SolCall,
};
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    live::{get_runtime, runner::get_exec_event_sender, task::TaskHandles},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    Params, UnixNanos, datetime::NANOSECONDS_IN_SECOND, hex, time::get_atomic_clock_realtime,
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    defi::{
        DexType, Pool, PoolIdentifier, SharedChain, Token,
        pool_analysis::quote::SwapQuote,
        validation::validate_address,
        wallet::{TokenBalance, WalletBalance},
    },
    enums::{CurrencyType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, TradeId, Venue, VenueOrderId},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Quantity, fixed::FIXED_PRECISION},
};

use crate::{
    cache::{
        BlockchainCache,
        database::BlockchainCacheDatabase,
        rows::{ExecutionIntentInsert, ExecutionIntentRow, ExecutionTransactionHashRow},
    },
    config::BlockchainExecutionClientConfig,
    contracts::{
        erc20::{ERC20, Erc20Contract},
        uniswap_v3_swap::UniswapV3SwapRouter,
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
        types::{RpcTransaction, RpcTransactionReceipt},
    },
};

/// Interval between receipt polls while awaiting transaction finality.
const RECEIPT_POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Basis points denominator for slippage derivation.
const BPS_DENOMINATOR: u32 = 10_000;
/// Denial reason for order-list submissions, which have no on-chain execution route.
const ORDER_LIST_UNSUPPORTED: &str =
    "Order lists are not supported; submit each order individually";
/// Rejection reason for order modifications, which immutable on-chain swaps cannot support.
const ORDER_MODIFY_UNSUPPORTED: &str = "Order modification is not supported";
/// Rejection reason for order cancellations, which immutable on-chain swaps cannot support.
const ORDER_CANCEL_UNSUPPORTED: &str = "Order cancellation is not supported";
/// Error for venue report probes that cannot answer without implying absence.
const VENUE_EXECUTION_REPORTS_UNSUPPORTED: &str =
    "Venue execution reports are not supported on the blockchain execution client";

// A broadcast transaction awaiting finality, occupying the single in-flight slot.
#[derive(Debug, Clone, Copy)]
struct InFlightTransaction {
    intent_id: i64,
    nonce: u64,
    tx_hash: B256,
    purpose: TransactionPurpose,
}

/// The single in-flight transaction slot.
///
/// The slot is claimed before any preparation RPC call so the `pending` nonce read stays
/// authoritative: a second transaction is rejected before it can sign. A claim is released
/// only when preparation fails before signing; from persistence onward the slot is never
/// released on failure, because the database may have committed before its acknowledgement
/// was lost.
#[derive(Debug, Clone, Copy)]
enum InFlightSlot {
    /// Claimed before preparation; no signed transaction exists yet.
    Preparing(TransactionPurpose),
    /// Signed, persisted, and awaiting finality.
    AwaitingFinality(InFlightTransaction),
}

#[derive(Debug, Clone)]
struct IncludedTransaction {
    intent_id: i64,
    tx_hash: B256,
    block_number: u64,
    receipt: RpcTransactionReceipt,
}

/// The single-in-flight limit error naming the transaction currently occupying the slot.
fn in_flight_limit_error(slot: &InFlightSlot) -> anyhow::Error {
    match slot {
        InFlightSlot::Preparing(purpose) => anyhow::anyhow!(
            "A {} transaction is being prepared; at most one transaction can be in flight",
            purpose.as_str()
        ),
        InFlightSlot::AwaitingFinality(in_flight) => anyhow::anyhow!(
            "Transaction {} (intent {}, {}, nonce {}) is still awaiting finality; at most one transaction can be in flight",
            in_flight.tx_hash,
            in_flight.intent_id,
            in_flight.purpose.as_str(),
            in_flight.nonce
        ),
    }
}

/// Releases a pre-signature slot claim when the slot is still in the preparing state.
///
/// Aborted or failed preparation can leave a claim behind; because no signed transaction
/// exists for a preparing slot, releasing it cannot strand a broadcastable signature.
fn release_preparing_slot(in_flight: &Mutex<Option<InFlightSlot>>) {
    let mut slot = in_flight.lock().expect("in-flight mutex poisoned");
    if matches!(*slot, Some(InFlightSlot::Preparing(_))) {
        *slot = None;
    }
}

#[derive(Debug)]
struct TransactionLimits {
    allowed_token_pairs: HashSet<(Address, Address)>,
    slippage_bps: u32,
    max_slippage_bps: u32,
    max_order_amount: u64,
    deadline_seconds: u64,
    max_quote_age_blocks: u64,
    receipt_timeout_secs: u64,
}

/// Execution client for blockchain interactions including balance tracking and order execution.
#[derive(Debug)]
pub struct BlockchainExecutionClient {
    /// Core execution client providing base functionality.
    core: ExecutionClientCore,
    /// Generates and dispatches execution events.
    emitter: ExecutionEventEmitter,
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
    /// Validated transaction limits required before execution can start.
    transaction_limits: TransactionLimits,
    /// Validated wrapped native token address for wrap operations.
    weth_address: Address,
    /// The transaction currently awaiting finality, occupying the single in-flight slot.
    in_flight: Arc<Mutex<Option<InFlightSlot>>>,
    /// Tracks native currency and ERC-20 token balances.
    wallet_balance: Arc<Mutex<WalletBalance>>,
    /// Contract interface for ERC-20 token interactions.
    erc20_contract: Erc20Contract,
    /// HTTP RPC client for blockchain queries.
    http_rpc_client: Arc<BlockchainHttpRpcClient>,
    /// Handles of spawned order-submission tasks, aborted on stop or disconnect.
    pending_tasks: Arc<TaskHandles>,
}

impl BlockchainExecutionClient {
    /// Creates a new [`BlockchainExecutionClient`] instance for the specified configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if any transaction limit is missing, if the wallet address, any token
    /// address, any router address, any allowed token pair address, or the WETH address in the
    /// config is invalid, if the router allowlist is empty, or if the slippage bounds are
    /// inconsistent or not below 100%.
    pub fn new(
        core_client: ExecutionClientCore,
        config: BlockchainExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        let transaction_limits = Self::transaction_limits(&config)?;
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
        let emitter = ExecutionEventEmitter::new(
            get_atomic_clock_realtime(),
            core_client.trader_id,
            core_client.account_id,
            core_client.account_type,
            core_client.base_currency,
        );

        Ok(Self {
            core: core_client,
            emitter,
            wallet_balance: Arc::new(Mutex::new(wallet_balance)),
            chain,
            cache,
            config,
            signer: None,
            router_addresses,
            transaction_limits,
            weth_address,
            in_flight: Arc::new(Mutex::new(None)),
            erc20_contract,
            http_rpc_client,
            wallet_address,
            pending_tasks: Arc::new(TaskHandles::default()),
        })
    }

    fn transaction_limits(
        config: &BlockchainExecutionClientConfig,
    ) -> anyhow::Result<TransactionLimits> {
        let (
            Some(allowed_token_pairs),
            Some(slippage_bps),
            Some(max_slippage_bps),
            Some(max_order_amount),
            Some(deadline_seconds),
            Some(max_quote_age_blocks),
            Some(receipt_timeout_secs),
        ) = (
            &config.allowed_token_pairs,
            config.slippage_bps,
            config.max_slippage_bps,
            config.max_order_amount,
            config.deadline_seconds,
            config.max_quote_age_blocks,
            config.receipt_timeout_secs,
        )
        else {
            anyhow::bail!(
                "Blockchain execution transaction limits are required: allowed_token_pairs, slippage_bps, max_slippage_bps, max_order_amount, deadline_seconds, max_quote_age_blocks, receipt_timeout_secs"
            );
        };

        let mut parsed_pairs = HashSet::with_capacity(allowed_token_pairs.len());
        for (token_in, token_out) in allowed_token_pairs {
            parsed_pairs.insert((
                validate_address(token_in.as_str())?,
                validate_address(token_out.as_str())?,
            ));
        }

        if slippage_bps > max_slippage_bps {
            anyhow::bail!(
                "`slippage_bps` {slippage_bps} exceeds `max_slippage_bps` {max_slippage_bps}"
            );
        }

        if max_slippage_bps >= BPS_DENOMINATOR {
            anyhow::bail!("`max_slippage_bps` {max_slippage_bps} must be below {BPS_DENOMINATOR}");
        }

        Ok(TransactionLimits {
            allowed_token_pairs: parsed_pairs,
            slippage_bps,
            max_slippage_bps,
            max_order_amount,
            deadline_seconds,
            max_quote_age_blocks,
            receipt_timeout_secs,
        })
    }

    /// Fetches the native currency balance (e.g., ETH) for the wallet from the blockchain.
    async fn fetch_native_currency_balance(&self) -> anyhow::Result<Money> {
        let balance_u256 = self
            .http_rpc_client
            .get_balance_with_timeout(&self.wallet_address, None, Some(EXECUTION_RPC_TIMEOUT_SECS))
            .await?;

        let native_currency = self.chain.native_currency();

        Money::from_u256(balance_u256, native_currency).map_err(Into::into)
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

    /// Refreshes and publishes all native currency and tracked ERC-20 balances.
    async fn refresh_wallet_balances(&mut self) -> anyhow::Result<()> {
        let (wallet_balance, balances) = self.fetch_wallet_balances().await?;
        self.generate_account_state(
            balances,
            vec![],
            true,
            get_atomic_clock_realtime().get_time_ns(),
            None,
        )?;
        *self
            .wallet_balance
            .lock()
            .expect("wallet balance mutex poisoned") = wallet_balance;
        Ok(())
    }

    async fn fetch_wallet_balances(
        &mut self,
    ) -> anyhow::Result<(WalletBalance, Vec<AccountBalance>)> {
        let native_currency_balance = self.fetch_native_currency_balance().await?;
        let token_universe = self
            .wallet_balance
            .lock()
            .expect("wallet balance mutex poisoned")
            .token_universe
            .clone();
        let mut token_addresses = token_universe.iter().copied().collect::<Vec<_>>();
        token_addresses.sort_unstable();

        let mut token_balances = Vec::with_capacity(token_addresses.len());
        for token_address in token_addresses {
            let token_balance = self
                .fetch_token_balance(&token_address)
                .await
                .with_context(|| format!("failed to fetch token balance for {token_address}"))?;
            token_balances.push(token_balance);
        }

        let mut wallet_balance = WalletBalance::new(token_universe);
        let balances = wallet_balance.replace_balances(native_currency_balance, token_balances)?;
        log::debug!(
            "Refreshed wallet balance with {} account balances",
            balances.len()
        );
        Ok((wallet_balance, balances))
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
        let _balance_probe = self
            .erc20_contract
            .balance_of(&self.weth_address, &self.wallet_address)
            .await?;

        let calldata = WETH9::depositCall {}.abi_encode();
        let executor = self.transaction_executor()?;
        let IncludedTransaction {
            intent_id,
            tx_hash,
            block_number,
            ..
        } = executor
            .transact(
                self.weth_address,
                amount_wei,
                Bytes::from(calldata),
                TransactionPurpose::Wrap,
                None,
            )
            .await?;
        self.ensure_wrap_balance_increase(&self.weth_address, amount_wei, tx_hash, block_number)
            .await?;
        executor
            .database
            .mark_execution_event_emitted(intent_id, "terminal")
            .await?;

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

        let executor = self.transaction_executor()?;
        let IncludedTransaction {
            intent_id,
            tx_hash,
            block_number,
            ..
        } = executor
            .transact(
                token,
                U256::ZERO,
                Bytes::from(calldata),
                TransactionPurpose::Approve,
                None,
            )
            .await?;
        self.ensure_approve_allowance(&token, &router, amount, tx_hash, block_number)
            .await?;
        executor
            .database
            .mark_execution_event_emitted(intent_id, "terminal")
            .await?;

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

    /// Ensures the wrapped native token balance increased by exactly `amount_wei` across the
    /// block that included transaction `tx_hash`, reading both balances at their historical
    /// blocks. Shared by the live wrap path and restart reconciliation.
    async fn ensure_wrap_balance_increase(
        &self,
        weth_address: &Address,
        amount_wei: U256,
        tx_hash: B256,
        block_number: u64,
    ) -> anyhow::Result<()> {
        let previous_block = block_number.checked_sub(1).ok_or_else(|| {
            anyhow::anyhow!("Included wrap transaction {tx_hash} has invalid block number 0")
        })?;
        let balance_before = self
            .erc20_contract
            .balance_of_at(weth_address, &self.wallet_address, previous_block)
            .await
            .with_context(|| {
                format!(
                    "failed to read WETH balance before included transaction {tx_hash} at block {previous_block}"
                )
            })?;
        let balance_after = self
            .erc20_contract
            .balance_of_at(weth_address, &self.wallet_address, block_number)
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

        Ok(())
    }

    /// Ensures the router allowance at the block that included transaction `tx_hash` covers
    /// `amount`. Shared by the live approve path and restart reconciliation.
    async fn ensure_approve_allowance(
        &self,
        token: &Address,
        router: &Address,
        amount: U256,
        tx_hash: B256,
        block_number: u64,
    ) -> anyhow::Result<()> {
        let allowance = self
            .erc20_contract
            .allowance_at(token, &self.wallet_address, router, block_number)
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

    /// Builds the shared transaction executor from the connected client state.
    ///
    /// # Errors
    ///
    /// Returns an error if no durable store is configured or the signer is not initialized.
    fn transaction_executor(&self) -> anyhow::Result<TransactionExecutor> {
        let database = self.cache.database.clone().ok_or_else(|| {
            anyhow::anyhow!("No durable store configured; refusing to submit a transaction")
        })?;
        let signer = self
            .signer
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Signer not initialized; connect the client first"))?;

        Ok(TransactionExecutor {
            http_rpc_client: self.http_rpc_client.clone(),
            database,
            signer,
            in_flight: Arc::clone(&self.in_flight),
            wallet_balance: Arc::clone(&self.wallet_balance),
            account_id: self.core.account_id,
            wallet_address: self.wallet_address,
            chain_id: self.chain.chain_id,
            max_fee_per_gas_wei: self.config.max_fee_per_gas_wei,
            base_fee_buffer_bps: self.config.base_fee_buffer_bps,
            gas_limit: self.config.gas_limit,
            gas_buffer_bps: self.config.gas_buffer_bps,
            receipt_timeout: receipt_timeout(self.transaction_limits.receipt_timeout_secs),
            receipt_max_polls: receipt_max_polls(self.transaction_limits.receipt_timeout_secs),
        })
    }

    fn restore_swap_plan(&self, intent: &ExecutionIntentRow) -> anyhow::Result<SwapPlan> {
        let client_order_id = ClientOrderId::new_checked(
            intent
                .client_order_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Persisted swap intent has no client order ID"))?,
        )?;
        let order = self
            .core
            .cache()
            .try_order_owned(&client_order_id)
            .with_context(|| {
                format!(
                    "Cannot reconcile swap intent {} because order {client_order_id} is not restored",
                    intent.id
                )
            })?;
        let instrument_id = InstrumentId::from_str(
            intent
                .instrument_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Persisted swap intent has no instrument ID"))?,
        )?;
        anyhow::ensure!(
            order.instrument_id() == instrument_id,
            "Persisted swap instrument {instrument_id} does not match restored order instrument {}",
            order.instrument_id()
        );
        anyhow::ensure!(
            intent.trader_id.as_deref() == Some(order.trader_id().as_str()),
            "Persisted swap trader does not match restored order"
        );
        anyhow::ensure!(
            intent.strategy_id.as_deref() == Some(order.strategy_id().as_str()),
            "Persisted swap strategy does not match restored order"
        );
        anyhow::ensure!(
            intent.account_id.as_deref() == Some(self.core.account_id.as_str()),
            "Persisted swap account does not match execution client account"
        );

        let pool = self.resolve_pool(&instrument_id)?;
        let pool_address = Address::from_str(
            intent
                .pool_address
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Persisted swap intent has no pool address"))?,
        )?;
        anyhow::ensure!(
            pool.address == pool_address,
            "Persisted pool {pool_address} does not match restored pool {}",
            pool.address
        );
        let amount_in = U256::from_str(
            intent
                .amount_in
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Persisted swap intent has no input amount"))?,
        )?;
        let fee = U24::try_from(
            pool.fee
                .ok_or_else(|| anyhow::anyhow!("Restored pool {instrument_id} has no fee"))?,
        )?;
        let quote_token = pool.get_quote_token();
        let quote_currency = Currency::new_checked(
            &quote_token.symbol,
            quote_token.decimals,
            0,
            &quote_token.name,
            CurrencyType::Crypto,
        )?;
        let token_in = pool.get_base_token().address;
        let token_out = quote_token.address;

        Ok(SwapPlan {
            order,
            quote_currency,
            pool,
            instrument_id,
            pool_address,
            router: Address::from_str(&intent.transaction_to)?,
            token_in,
            token_out,
            fee,
            amount_in,
            min_amount_out: U256::ZERO,
            profiler_block: intent.created_block,
        })
    }

    async fn reconcile_unresolved_execution(&self) -> anyhow::Result<()> {
        let database = self.cache.database.clone().ok_or_else(|| {
            anyhow::anyhow!("No durable store configured for execution reconciliation")
        })?;
        let wallet_address = self.wallet_address.to_string();
        let Some(intent) = database
            .get_active_execution_intent(self.chain.chain_id, &wallet_address)
            .await?
        else {
            return Ok(());
        };
        anyhow::ensure!(
            intent.schema_version == crate::execution::transaction::EXECUTION_SCHEMA_VERSION,
            "Execution intent {} uses unsupported schema version {}",
            intent.id,
            intent.schema_version
        );

        if matches!(intent.status.as_str(), "prepared" | "signed") {
            database
                .mark_execution_intent_recoverable(intent.id)
                .await?;
            release_preparing_slot(&self.in_flight);
            return Ok(());
        }

        let purpose = TransactionPurpose::parse(&intent.purpose).ok_or_else(|| {
            anyhow::anyhow!(
                "Execution intent {} has unknown purpose {}",
                intent.id,
                intent.purpose
            )
        })?;
        let nonce = intent
            .nonce
            .ok_or_else(|| anyhow::anyhow!("Active execution intent {} has no nonce", intent.id))?;
        let hashes = database.get_execution_transaction_hashes(intent.id).await?;
        let current = current_execution_hash(intent.id, &hashes)?;
        let tx_hash = B256::from_str(&current.transaction_hash).with_context(|| {
            format!(
                "Execution intent {} has invalid transaction hash {}",
                intent.id, current.transaction_hash
            )
        })?;
        *self.in_flight.lock().expect("in-flight mutex poisoned") =
            Some(InFlightSlot::AwaitingFinality(InFlightTransaction {
                intent_id: intent.id,
                nonce,
                tx_hash,
                purpose,
            }));

        let plan = if purpose == TransactionPurpose::Swap {
            Some(self.restore_swap_plan(&intent)?)
        } else {
            None
        };

        if let Some(plan) = &plan
            && !intent.acknowledgement_emitted
        {
            if plan.order.ts_submitted().is_none() {
                self.emitter.emit_order_submitted(&plan.order);
            }
            database
                .mark_execution_event_emitted(intent.id, "acknowledgement")
                .await?;
        }

        let executor = self.transaction_executor()?;
        let prepared = PreparedTransaction {
            intent_id: intent.id,
            created_block: intent.created_block,
            nonce,
            tx_hash,
            raw_tx: current.raw_transaction.clone().unwrap_or_default(),
        };
        let outcome = if matches!(intent.status.as_str(), "finalized" | "reverted") {
            let receipt = executor
                .http_rpc_client
                .get_transaction_receipt(&tx_hash)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Finalized execution transaction {tx_hash} no longer has a receipt"
                    )
                })?;
            anyhow::ensure!(
                executor.receipt_is_stably_finalized(&receipt).await?,
                "Persisted terminal transaction {tx_hash} is not stable at the finalized boundary"
            );

            if intent.status == "finalized" {
                InclusionOutcome::Finalized(IncludedTransaction {
                    intent_id: intent.id,
                    tx_hash,
                    block_number: receipt.block_number,
                    receipt,
                })
            } else {
                InclusionOutcome::Reverted(tx_hash)
            }
        } else {
            executor.await_finality(&prepared).await?
        };

        match outcome {
            InclusionOutcome::Finalized(included) => {
                if let Some(plan) = plan {
                    if finalized_transaction_matches(&included, &intent, nonce, &executor).await? {
                        complete_finalized_swap(
                            &plan,
                            intent.id,
                            &included,
                            &executor,
                            &self.emitter,
                        )
                        .await?;
                    } else {
                        if plan.order.status() != OrderStatus::Rejected {
                            self.emitter.emit_order_rejected(
                                &plan.order,
                                &format!(
                                    "Finalized signer-nonce transaction {} does not match the persisted swap intent",
                                    included.tx_hash
                                ),
                                get_atomic_clock_realtime().get_time_ns(),
                                false,
                            );
                        }
                        database
                            .mark_execution_event_emitted(intent.id, "terminal")
                            .await?;
                    }
                    executor.release_slot();
                } else {
                    self.ensure_recovered_operator_transaction(
                        &intent, purpose, nonce, &included, &executor,
                    )
                    .await?;
                    database
                        .mark_execution_event_emitted(intent.id, "terminal")
                        .await?;
                    executor.release_slot();
                }
            }
            InclusionOutcome::Reverted(tx_hash) => {
                if let Some(plan) = plan {
                    if plan.order.status() != OrderStatus::Rejected {
                        self.emitter.emit_order_rejected(
                            &plan.order,
                            &format!("Transaction {tx_hash} reverted on-chain"),
                            get_atomic_clock_realtime().get_time_ns(),
                            false,
                        );
                    }
                    database
                        .mark_execution_event_emitted(intent.id, "terminal")
                        .await?;
                }
                executor.release_slot();
            }
            InclusionOutcome::Pending(message) => log::warn!("{message}"),
        }
        Ok(())
    }

    /// Revalidates a restored wrap or approve against the finalized signer-nonce transaction
    /// and reruns the live postcondition before the restored intent may report success.
    ///
    /// Fails closed: any identity mismatch or unproven postcondition returns an error, which
    /// keeps the in-flight signer slot occupied, leaves the intent active, and fails connect.
    async fn ensure_recovered_operator_transaction(
        &self,
        intent: &ExecutionIntentRow,
        purpose: TransactionPurpose,
        nonce: u64,
        included: &IncludedTransaction,
        executor: &TransactionExecutor,
    ) -> anyhow::Result<()> {
        if !finalized_transaction_matches(included, intent, nonce, executor).await? {
            anyhow::bail!(
                "Finalized signer-nonce transaction {} does not match the persisted {} intent",
                included.tx_hash,
                purpose.as_str()
            );
        }

        let (to, input, value) = persisted_call_fields(intent)?;

        match purpose {
            TransactionPurpose::Wrap => {
                self.ensure_wrap_balance_increase(
                    &to,
                    value,
                    included.tx_hash,
                    included.block_number,
                )
                .await
            }
            TransactionPurpose::Approve => {
                let call = ERC20::approveCall::abi_decode(&input)
                    .with_context(|| "persisted approve calldata is invalid")?;
                self.ensure_approve_allowance(
                    &to,
                    &call.spender,
                    call.amount,
                    included.tx_hash,
                    included.block_number,
                )
                .await
            }
            TransactionPurpose::Swap => {
                unreachable!("swap intents restore a swap plan")
            }
        }
    }

    fn ensure_transaction_ready(&self, purpose: TransactionPurpose) -> anyhow::Result<()> {
        if !self.core.is_connected() {
            anyhow::bail!("Blockchain execution client is not connected");
        }

        {
            let slot = self.in_flight.lock().expect("in-flight mutex poisoned");
            if let Some(in_flight) = *slot {
                return Err(in_flight_limit_error(&in_flight));
            }
        }

        if !self.cache.has_database() {
            anyhow::bail!(
                "No durable store configured; refusing to submit a {} transaction",
                purpose.as_str()
            );
        }
        Ok(())
    }

    /// Validates a submit-order command against the configured policy and builds the swap plan.
    ///
    /// Local checks run first: pool resolution, order semantics, allowlists, amount and
    /// slippage limits, and the quote derived from the live pool profiler. Infrastructure
    /// readiness (connection, in-flight slot, durable store, signer) follows. Chain state is
    /// verified in the spawned task before signing.
    fn prepare_swap(&self, cmd: &SubmitOrder, order: &OrderAny) -> anyhow::Result<SwapPlan> {
        let instrument_id = order.instrument_id();
        let pool = self.resolve_pool(&instrument_id)?;

        if order.order_type() != OrderType::Market {
            anyhow::bail!(
                "Unsupported order type {}; only Market is supported",
                order.order_type()
            );
        }

        if order.order_side() != OrderSide::Sell {
            anyhow::bail!(
                "Unsupported order side {}; only Sell is supported",
                order.order_side()
            );
        }

        if order.is_quote_quantity() {
            anyhow::bail!(
                "Quote-denominated quantities are not supported; quantity must be denominated in the base token"
            );
        }

        let fee = pool
            .fee
            .ok_or_else(|| anyhow::anyhow!("Pool {instrument_id} has no fee tier"))?;
        let fee = U24::try_from(fee)
            .map_err(|_| anyhow::anyhow!("Pool {instrument_id} fee {fee} exceeds uint24"))?;

        let base_token = pool.get_base_token();
        let quote_token = pool.get_quote_token();
        let quote_currency = Currency::new_checked(
            &quote_token.symbol,
            quote_token.decimals,
            0,
            &quote_token.name,
            CurrencyType::Crypto,
        )?;

        if !self
            .transaction_limits
            .allowed_token_pairs
            .contains(&(base_token.address, quote_token.address))
        {
            anyhow::bail!(
                "Token pair {} -> {} is not in the `allowed_token_pairs` allowlist",
                base_token.address,
                quote_token.address
            );
        }

        let amount_in = quantity_to_raw_amount(order.quantity(), base_token.decimals)?;
        if amount_in > U256::from(self.transaction_limits.max_order_amount) {
            anyhow::bail!(
                "Order amount {amount_in} exceeds the configured `max_order_amount` {}",
                self.transaction_limits.max_order_amount
            );
        }

        let slippage_bps = match cmd
            .params
            .as_ref()
            .and_then(|params| params.get_u64("slippage_bps"))
        {
            Some(value) => u32::try_from(value).map_err(|_| {
                anyhow::anyhow!("slippage_bps parameter {value} exceeds the u32 range")
            })?,
            None => self.transaction_limits.slippage_bps,
        };

        if slippage_bps > self.transaction_limits.max_slippage_bps {
            anyhow::bail!(
                "Slippage {slippage_bps} bps exceeds the configured `max_slippage_bps` {}",
                self.transaction_limits.max_slippage_bps
            );
        }

        let profiler = self
            .core
            .cache()
            .pool_profiler(&instrument_id)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No pool profiler for {instrument_id}; an active data subscription is required to quote the swap"
                )
            })?;

        if !profiler.is_initialized {
            anyhow::bail!("Pool profiler for {instrument_id} is not initialized");
        }
        let profiler_block = profiler
            .last_processed_event
            .as_ref()
            .map(|position| position.number)
            .ok_or_else(|| {
                anyhow::anyhow!("Pool profiler for {instrument_id} has processed no events")
            })?;

        let zero_for_one = base_token.address == pool.token0.address;
        let quote = profiler
            .swap_exact_in(amount_in, zero_for_one, None)
            .map_err(|e| anyhow::anyhow!("Swap quote failed for {instrument_id}: {e}"))?;

        let amount_filled = if zero_for_one {
            quote.amount0
        } else {
            quote.amount1
        };

        if amount_filled != I256::from(amount_in) {
            anyhow::bail!(
                "Local quote for {instrument_id} filled {amount_filled} of the {amount_in} order amount; pool liquidity cannot fill the order"
            );
        }

        let quoted_amount_out = exact_output_amount(&quote, zero_for_one)?;
        let min_amount_out = derive_min_amount_out(quoted_amount_out, slippage_bps)?;

        self.ensure_transaction_ready(TransactionPurpose::Swap)?;

        if self.signer.is_none() {
            anyhow::bail!("Signer not initialized; connect the client first");
        }

        let pool_address = pool.address;
        let token_in = base_token.address;
        let token_out = quote_token.address;

        Ok(SwapPlan {
            order: order.clone(),
            pool,
            quote_currency,
            instrument_id,
            pool_address,
            router: self.router_addresses[0],
            token_in,
            token_out,
            fee,
            amount_in,
            min_amount_out,
            profiler_block,
        })
    }
}

/// A locally signed EIP-1559 transaction ready for persist-before-broadcast.
#[derive(Debug)]
struct PreparedTransaction {
    intent_id: i64,
    created_block: u64,
    nonce: u64,
    tx_hash: B256,
    raw_tx: Vec<u8>,
}

/// The classified outcome of a broadcast attempt.
#[derive(Debug)]
enum BroadcastOutcome {
    /// The node accepted the transaction (or reported it already known).
    Accepted,
    /// Acceptance is uncertain; the persisted record stays active and the slot occupied.
    Ambiguous(String),
}

/// The classified outcome of awaiting transaction finality.
#[derive(Debug)]
enum InclusionOutcome {
    /// Canonical successful receipt observed through a stable finalized boundary.
    Finalized(IncludedTransaction),
    /// Canonical failed receipt observed through a stable finalized boundary.
    Reverted(B256),
    /// No receipt arrived within the poll budget or observation failed; the record stays
    /// pending and the in-flight slot occupied.
    Pending(String),
}

/// Shared execution context driving the EIP-1559 transaction pipeline.
///
/// Wrap, approve, and swap submissions share one implementation so the chain, fee, gas,
/// signing, persistence, and broadcast policy cannot drift between operator transactions
/// and order flow. Carries only `Send` state so order submission can run on a spawned task
/// after the synchronous trait method returns.
#[derive(Debug, Clone)]
struct TransactionExecutor {
    http_rpc_client: Arc<BlockchainHttpRpcClient>,
    database: BlockchainCacheDatabase,
    signer: PrivateKeySigner,
    in_flight: Arc<Mutex<Option<InFlightSlot>>>,
    wallet_balance: Arc<Mutex<WalletBalance>>,
    account_id: AccountId,
    wallet_address: Address,
    chain_id: u32,
    max_fee_per_gas_wei: u64,
    base_fee_buffer_bps: u32,
    gas_limit: u64,
    gas_buffer_bps: u32,
    receipt_timeout: Duration,
    receipt_max_polls: u32,
}

impl TransactionExecutor {
    /// Builds, signs, persists, and broadcasts an EIP-1559 transaction, then awaits finality.
    ///
    /// Order of operations: claim the in-flight slot, chain ID verification, nonce and fee and
    /// gas policy checks, local signing, persist-before-broadcast, broadcast, inclusion
    /// observation. The persisted record guarantees a signed transaction is never forgotten.
    async fn transact(
        &self,
        to: Address,
        value: U256,
        input: Bytes,
        purpose: TransactionPurpose,
        client_order_id: Option<ClientOrderId>,
    ) -> anyhow::Result<IncludedTransaction> {
        self.claim_slot(purpose)?;
        let created_block = self.http_rpc_client.latest_block().await?.number;
        let intent = ExecutionIntentInsert {
            chain_id: self.chain_id,
            wallet_address: self.wallet_address.to_string(),
            purpose: purpose.as_str().to_string(),
            client_order_id: client_order_id.map(|id| id.to_string()),
            trader_id: None,
            strategy_id: None,
            account_id: None,
            instrument_id: None,
            pool_address: None,
            transaction_to: to.to_string(),
            transaction_input: hex::encode_prefixed(&input),
            transaction_value: value.to_string(),
            amount_in: None,
            created_block,
        };
        let intent = self.database.reserve_execution_intent(&intent).await?;
        let prepared = match self
            .prepare_and_sign(intent.id, intent.created_block, to, value, input)
            .await
        {
            Ok(prepared) => prepared,
            Err(e) => {
                if self
                    .database
                    .mark_execution_intent_recoverable(intent.id)
                    .await
                    .is_ok()
                {
                    release_preparing_slot(&self.in_flight);
                }
                return Err(e);
            }
        };
        self.fill_and_persist(&prepared, purpose).await?;

        match self.broadcast(&prepared).await? {
            BroadcastOutcome::Accepted => {}
            BroadcastOutcome::Ambiguous(message) => log::warn!("{message}"),
        }

        match self.await_finality(&prepared).await? {
            InclusionOutcome::Finalized(included) => {
                self.release_slot();
                Ok(included)
            }
            InclusionOutcome::Reverted(tx_hash) => {
                self.release_slot();
                anyhow::bail!("Transaction {tx_hash} reverted on-chain")
            }
            InclusionOutcome::Pending(message) => anyhow::bail!(message),
        }
    }

    /// Claims the single in-flight slot before any preparation RPC call, so the `pending`
    /// nonce read stays authoritative: a second transaction is rejected before it can sign.
    fn claim_slot(&self, purpose: TransactionPurpose) -> anyhow::Result<()> {
        let mut slot = self.in_flight.lock().expect("in-flight mutex poisoned");
        if let Some(in_flight) = *slot {
            return Err(in_flight_limit_error(&in_flight));
        }
        *slot = Some(InFlightSlot::Preparing(purpose));
        Ok(())
    }

    /// Runs the read-only pre-signing pipeline: chain ID verification, nonce selection, fee
    /// and gas policy checks, transaction building, and local signing.
    async fn prepare_and_sign(
        &self,
        intent_id: i64,
        created_block: u64,
        to: Address,
        value: U256,
        input: Bytes,
    ) -> anyhow::Result<PreparedTransaction> {
        let expected_chain_id = u64::from(self.chain_id);
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
        self.database
            .assign_execution_intent_nonce(intent_id, nonce)
            .await?;
        let latest_block = self.http_rpc_client.latest_block().await?;
        let base_fee_per_gas_wei = latest_block.base_fee_per_gas.ok_or_else(|| {
            anyhow::anyhow!("Latest block {} has no base fee", latest_block.number)
        })?;
        let priority_fee_per_gas_wei = self.http_rpc_client.max_priority_fee_per_gas().await?;
        let (max_fee_per_gas, max_priority_fee_per_gas) = derive_fees(
            base_fee_per_gas_wei,
            priority_fee_per_gas_wei,
            self.base_fee_buffer_bps,
            u128::from(self.max_fee_per_gas_wei),
        )?;
        let gas_estimate = self
            .http_rpc_client
            .estimate_gas(&self.wallet_address, &to, value, &input)
            .await?;
        let gas_limit = derive_gas_limit(gas_estimate, self.gas_buffer_bps, self.gas_limit)?;
        let max_gas_cost = U256::from(gas_limit)
            .checked_mul(U256::from(max_fee_per_gas))
            .ok_or_else(|| anyhow::anyhow!("Maximum gas cost overflow"))?;
        let max_transaction_cost = value
            .checked_add(max_gas_cost)
            .ok_or_else(|| anyhow::anyhow!("Maximum transaction cost overflow"))?;
        let native_balance = self
            .http_rpc_client
            .get_balance_with_timeout(&self.wallet_address, None, Some(EXECUTION_RPC_TIMEOUT_SECS))
            .await?;

        if native_balance < max_transaction_cost {
            anyhow::bail!(
                "Native currency balance {native_balance} wei is below maximum transaction cost {max_transaction_cost} wei"
            );
        }

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
        let (tx_hash, raw_tx) = sign_eip1559_transaction(tx, &self.signer).await?;

        Ok(PreparedTransaction {
            intent_id,
            created_block,
            nonce,
            tx_hash,
            raw_tx,
        })
    }

    /// Fills the claimed slot with the signed transaction and persists the pending record
    /// before broadcast.
    ///
    /// The slot is filled before the cancellable write: PostgreSQL may commit before this
    /// future resumes, so cancellation cannot safely restore the pre-transaction state. A
    /// persistence failure leaves the slot occupied because commit acknowledgement is
    /// ambiguous.
    async fn fill_and_persist(
        &self,
        prepared: &PreparedTransaction,
        purpose: TransactionPurpose,
    ) -> anyhow::Result<()> {
        {
            let mut slot = self.in_flight.lock().expect("in-flight mutex poisoned");
            *slot = Some(InFlightSlot::AwaitingFinality(InFlightTransaction {
                intent_id: prepared.intent_id,
                nonce: prepared.nonce,
                tx_hash: prepared.tx_hash,
                purpose,
            }));
        }

        let tx_hash = prepared.tx_hash;
        self.database
            .add_execution_transaction_hash(
                prepared.intent_id,
                self.chain_id,
                &tx_hash.to_string(),
                &prepared.raw_tx,
            )
            .await
            .map(|_| ())
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to persist transaction {tx_hash}: {e}; the in-flight slot stays occupied"
                )
            })
    }

    /// Broadcasts the signed transaction and classifies the acceptance outcome.
    async fn broadcast(&self, prepared: &PreparedTransaction) -> anyhow::Result<BroadcastOutcome> {
        let tx_hash = prepared.tx_hash;

        self.database
            .record_execution_status(
                prepared.intent_id,
                &tx_hash.to_string(),
                TransactionStatus::Broadcast,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to persist broadcast attempt for transaction {tx_hash}: {e}; the in-flight slot stays occupied"
                )
            })?;

        match self
            .http_rpc_client
            .send_raw_transaction(&prepared.raw_tx, &tx_hash)
            .await
        {
            Ok(broadcast_hash) => {
                if broadcast_hash != tx_hash {
                    // The node acknowledged a different hash: acceptance of the signed
                    // transaction is unverified, so reconcile through the persisted record
                    // rather than poll a hash that cannot match the chain
                    return Ok(BroadcastOutcome::Ambiguous(format!(
                        "Broadcast of transaction {tx_hash} returned a differing hash {broadcast_hash}; the persisted record reconciles instead of rebroadcasting"
                    )));
                }
                Ok(BroadcastOutcome::Accepted)
            }
            Err(BroadcastError::TimeoutAfterSend) => Ok(BroadcastOutcome::Ambiguous(format!(
                "Broadcast of transaction {tx_hash} timed out after send; the persisted record reconciles instead of rebroadcasting"
            ))),
            Err(BroadcastError::Failed(message)) => {
                // Transport or response failure: acceptance is ambiguous (the failure may also
                // predate dispatch; see BroadcastError::Failed), so occupy the slot and
                // reconcile rather than risk a rebroadcast
                Ok(BroadcastOutcome::Ambiguous(format!(
                    "Broadcast of transaction {tx_hash} failed ambiguously ({message}); the persisted record reconciles instead of rebroadcasting"
                )))
            }
            Err(error @ BroadcastError::Rejected { .. }) => {
                Ok(BroadcastOutcome::Ambiguous(format!(
                    "Broadcast of transaction {tx_hash} was rejected ({error}); the signed hash remains occupied until canonical nonce reconciliation"
                )))
            }
        }
    }

    /// Polls until the receipt is canonical at a stable finalized boundary.
    async fn await_finality(
        &self,
        prepared: &PreparedTransaction,
    ) -> anyhow::Result<InclusionOutcome> {
        let mut tx_hash = prepared.tx_hash;
        let mut inclusion_observed = false;
        let deadline = tokio::time::Instant::now() + self.receipt_timeout;

        for attempt in 0..self.receipt_max_polls {
            if tokio::time::Instant::now() >= deadline {
                break;
            }

            if attempt > 0 {
                tokio::time::sleep(RECEIPT_POLL_INTERVAL).await;
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let receipt_result = match tokio::time::timeout(
                remaining,
                self.http_rpc_client.get_transaction_receipt(&tx_hash),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => break,
            };

            match receipt_result {
                Ok(Some(receipt)) => {
                    let canonical = self
                        .http_rpc_client
                        .block_by_number(receipt.block_number, false)
                        .await?;

                    if canonical.hash != receipt.block_hash {
                        self.database
                            .record_execution_status(
                                prepared.intent_id,
                                &tx_hash.to_string(),
                                TransactionStatus::Reorged,
                                Some(receipt.block_number),
                                Some(&receipt.block_hash.to_string()),
                                Some(receipt.status),
                                Some(receipt.gas_used),
                                Some(&receipt.effective_gas_price.to_string()),
                            )
                            .await?;
                        inclusion_observed = false;
                        continue;
                    }

                    self.database
                        .record_execution_status(
                            prepared.intent_id,
                            &tx_hash.to_string(),
                            TransactionStatus::Included,
                            Some(receipt.block_number),
                            Some(&receipt.block_hash.to_string()),
                            Some(receipt.status),
                            Some(receipt.gas_used),
                            Some(&receipt.effective_gas_price.to_string()),
                        )
                        .await?;
                    inclusion_observed = true;

                    if self.receipt_is_stably_finalized(&receipt).await? {
                        let status = if receipt.status {
                            TransactionStatus::Finalized
                        } else {
                            TransactionStatus::Reverted
                        };
                        self.database
                            .record_execution_status(
                                prepared.intent_id,
                                &tx_hash.to_string(),
                                status,
                                Some(receipt.block_number),
                                Some(&receipt.block_hash.to_string()),
                                Some(receipt.status),
                                Some(receipt.gas_used),
                                Some(&receipt.effective_gas_price.to_string()),
                            )
                            .await?;
                        return if receipt.status {
                            Ok(InclusionOutcome::Finalized(IncludedTransaction {
                                intent_id: prepared.intent_id,
                                tx_hash,
                                block_number: receipt.block_number,
                                receipt,
                            }))
                        } else {
                            Ok(InclusionOutcome::Reverted(tx_hash))
                        };
                    }
                }
                Ok(None) => {
                    if inclusion_observed {
                        self.database
                            .record_execution_status(
                                prepared.intent_id,
                                &tx_hash.to_string(),
                                TransactionStatus::Reorged,
                                None,
                                None,
                                None,
                                None,
                                None,
                            )
                            .await?;
                        inclusion_observed = false;
                    }

                    if self
                        .http_rpc_client
                        .get_transaction_count_latest(&self.wallet_address)
                        .await?
                        > prepared.nonce
                        && let Some(replacement) = self
                            .find_nonce_transaction(prepared.nonce, prepared.created_block)
                            .await?
                        && replacement.hash != tx_hash
                    {
                        self.database
                            .add_execution_replacement_hash(
                                prepared.intent_id,
                                self.chain_id,
                                &replacement.hash.to_string(),
                            )
                            .await?;
                        tx_hash = replacement.hash;
                        inclusion_observed = false;
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Finality poll {}/{} for transaction {tx_hash} failed: {e}",
                        attempt + 1,
                        self.receipt_max_polls
                    );
                }
            }
        }

        self.database
            .record_execution_status(
                prepared.intent_id,
                &tx_hash.to_string(),
                TransactionStatus::Dropped,
                None,
                None,
                None,
                None,
                None,
            )
            .await?;
        Ok(InclusionOutcome::Pending(format!(
            "Timed out awaiting finality of transaction {tx_hash}; the intent stays occupied for reconciliation"
        )))
    }

    async fn receipt_is_stably_finalized(
        &self,
        receipt: &RpcTransactionReceipt,
    ) -> anyhow::Result<bool> {
        let finalized = self.http_rpc_client.finalized_block().await?;
        if finalized.number < receipt.block_number {
            return Ok(false);
        }

        let canonical_again = self
            .http_rpc_client
            .block_by_number(receipt.block_number, false)
            .await?;
        let finalized_again = self
            .http_rpc_client
            .block_by_number(finalized.number, false)
            .await?;
        Ok(canonical_again.hash == receipt.block_hash && finalized_again.hash == finalized.hash)
    }

    async fn find_nonce_transaction(
        &self,
        nonce: u64,
        from_block: u64,
    ) -> anyhow::Result<Option<RpcTransaction>> {
        let head = self.http_rpc_client.latest_block().await?;
        let mut found = None;

        for number in from_block..=head.number {
            let block = self.http_rpc_client.block_by_number(number, true).await?;
            if let Some(transaction) = block.transactions.into_iter().find(|transaction| {
                transaction.from == self.wallet_address && transaction.nonce == nonce
            }) {
                found = Some(transaction);
                break;
            }
        }
        let stable_head = self
            .http_rpc_client
            .block_by_number(head.number, false)
            .await?;
        anyhow::ensure!(
            stable_head.hash == head.hash,
            "Canonical head changed during signer-nonce replacement scan"
        );
        Ok(found)
    }

    fn release_slot(&self) {
        *self.in_flight.lock().expect("in-flight mutex poisoned") = None;
    }
}

fn current_execution_hash(
    intent_id: i64,
    hashes: &[ExecutionTransactionHashRow],
) -> anyhow::Result<&ExecutionTransactionHashRow> {
    let mut current = hashes.iter().filter(|row| row.current);
    let row = current
        .next()
        .ok_or_else(|| anyhow::anyhow!("Execution intent {intent_id} has no current hash"))?;
    anyhow::ensure!(
        current.next().is_none(),
        "Execution intent {intent_id} has more than one current hash"
    );
    Ok(row)
}

/// Polls for the receipt of a broadcast transaction until it exists or the poll bound
/// is exhausted. A `null` receipt result is a legitimate pending response.
#[cfg(test)]
async fn poll_for_receipt(
    http_rpc_client: &BlockchainHttpRpcClient,
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

        match http_rpc_client.get_transaction_receipt(tx_hash).await {
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

/// Derives the receipt poll budget from the configured inclusion timeout in seconds.
fn receipt_max_polls(receipt_timeout_secs: u64) -> u32 {
    u32::try_from(receipt_timeout_secs.max(1)).unwrap_or(u32::MAX)
}

fn receipt_timeout(receipt_timeout_secs: u64) -> Duration {
    Duration::from_secs(receipt_timeout_secs.clamp(1, u64::from(u32::MAX)))
}

/// A fully validated swap ready for asynchronous dispatch.
#[derive(Debug)]
struct SwapPlan {
    order: OrderAny,
    pool: Pool,
    quote_currency: Currency,
    instrument_id: InstrumentId,
    pool_address: Address,
    router: Address,
    token_in: Address,
    token_out: Address,
    fee: U24,
    amount_in: U256,
    min_amount_out: U256,
    profiler_block: u64,
}

/// Executes a validated swap plan: read-only pre-trade checks, signing, persistence,
/// broadcast, and finality observation.
///
/// Emits only events justified by known transaction state: `OrderDenied` before the
/// transaction exists, `OrderSubmitted` after broadcast acceptance, and `OrderRejected` on
/// a definitive node rejection or an on-chain revert. No fill is emitted at broadcast or
/// first inclusion; fills arrive with finality reconciliation. Ambiguous outcomes keep the
/// order submitted untouched while the persisted record reconciles.
async fn execute_swap(
    plan: SwapPlan,
    executor: TransactionExecutor,
    emitter: ExecutionEventEmitter,
    max_quote_age_blocks: u64,
    deadline_seconds: u64,
) -> anyhow::Result<()> {
    let order = &plan.order;

    if let Err(e) = executor.claim_slot(TransactionPurpose::Swap) {
        emitter.emit_order_denied(order, &e.to_string());
        return Ok(());
    }

    let latest_block = match executor.http_rpc_client.latest_block().await {
        Ok(block) => block,
        Err(e) => {
            release_preparing_slot(&executor.in_flight);
            emitter.emit_order_denied(
                order,
                &format!("Failed to read the latest block for the swap: {e}"),
            );
            return Ok(());
        }
    };

    if plan.profiler_block > latest_block.number {
        release_preparing_slot(&executor.in_flight);
        emitter.emit_order_denied(
            order,
            &format!(
                "Pool state for {} at block {} is ahead of the latest block {}; the execution RPC endpoint lags the data feed",
                plan.instrument_id, plan.profiler_block, latest_block.number
            ),
        );
        return Ok(());
    }

    let quote_age = latest_block.number - plan.profiler_block;
    if quote_age > max_quote_age_blocks {
        release_preparing_slot(&executor.in_flight);
        emitter.emit_order_denied(
            order,
            &format!(
                "Stale quote for {}: pool state at block {}, latest block {}, exceeds `max_quote_age_blocks` {max_quote_age_blocks}",
                plan.instrument_id, plan.profiler_block, latest_block.number
            ),
        );
        return Ok(());
    }
    let deadline = match latest_block.timestamp.checked_add(deadline_seconds) {
        Some(deadline) => deadline,
        None => {
            release_preparing_slot(&executor.in_flight);
            emitter.emit_order_denied(
                order,
                &format!(
                    "Swap deadline overflow: latest block timestamp {} plus `deadline_seconds` {deadline_seconds} exceeds u64",
                    latest_block.timestamp
                ),
            );
            return Ok(());
        }
    };

    if let Err(e) = check_swap_preconditions(&plan, &executor).await {
        release_preparing_slot(&executor.in_flight);
        emitter.emit_order_denied(order, &e.to_string());
        return Ok(());
    }

    let calldata = UniswapV3SwapRouter::exactInputSingleCall {
        params: UniswapV3SwapRouter::ExactInputSingleParams {
            tokenIn: plan.token_in,
            tokenOut: plan.token_out,
            fee: plan.fee,
            recipient: executor.wallet_address,
            deadline: U256::from(deadline),
            amountIn: plan.amount_in,
            amountOutMinimum: plan.min_amount_out,
            sqrtPriceLimitX96: U160::ZERO,
        },
    }
    .abi_encode();

    let calldata = Bytes::from(calldata);
    let intent = ExecutionIntentInsert {
        chain_id: executor.chain_id,
        wallet_address: executor.wallet_address.to_string(),
        purpose: TransactionPurpose::Swap.as_str().to_string(),
        client_order_id: Some(order.client_order_id().to_string()),
        trader_id: Some(order.trader_id().to_string()),
        strategy_id: Some(order.strategy_id().to_string()),
        account_id: Some(executor.account_id.to_string()),
        instrument_id: Some(plan.instrument_id.to_string()),
        pool_address: Some(plan.pool_address.to_string()),
        transaction_to: plan.router.to_string(),
        transaction_input: hex::encode_prefixed(&calldata),
        transaction_value: U256::ZERO.to_string(),
        amount_in: Some(plan.amount_in.to_string()),
        created_block: latest_block.number,
    };
    let intent = match executor.database.reserve_execution_intent(&intent).await {
        Ok(intent) => intent,
        Err(e) => {
            emitter.emit_order_denied(order, &e.to_string());
            return Ok(());
        }
    };
    let prepared = match executor
        .prepare_and_sign(
            intent.id,
            intent.created_block,
            plan.router,
            U256::ZERO,
            calldata,
        )
        .await
    {
        Ok(prepared) => prepared,
        Err(e) => {
            if executor
                .database
                .mark_execution_intent_recoverable(intent.id)
                .await
                .is_ok()
            {
                release_preparing_slot(&executor.in_flight);
            }
            emitter.emit_order_denied(order, &e.to_string());
            return Ok(());
        }
    };

    if let Err(e) = executor
        .fill_and_persist(&prepared, TransactionPurpose::Swap)
        .await
    {
        emitter.emit_order_denied(order, &e.to_string());
        return Ok(());
    }

    match executor.broadcast(&prepared).await {
        Ok(BroadcastOutcome::Accepted) => {
            emitter.emit_order_submitted(order);
            executor
                .database
                .mark_execution_event_emitted(intent.id, "acknowledgement")
                .await?;
        }
        Ok(BroadcastOutcome::Ambiguous(message)) => {
            emitter.emit_order_submitted(order);
            executor
                .database
                .mark_execution_event_emitted(intent.id, "acknowledgement")
                .await?;
            log::warn!("{message}");
        }
        Err(e) => {
            return Err(e);
        }
    }

    match executor.await_finality(&prepared).await {
        Ok(InclusionOutcome::Finalized(included)) => {
            if !finalized_transaction_matches(&included, &intent, prepared.nonce, &executor).await?
            {
                emitter.emit_order_rejected(
                    order,
                    &format!(
                        "Finalized signer-nonce transaction {} does not match the persisted swap intent",
                        included.tx_hash
                    ),
                    get_atomic_clock_realtime().get_time_ns(),
                    false,
                );
                executor
                    .database
                    .mark_execution_event_emitted(intent.id, "terminal")
                    .await?;
                executor.release_slot();
                return Ok(());
            }

            complete_finalized_swap(&plan, intent.id, &included, &executor, &emitter).await?;
            executor.release_slot();
            Ok(())
        }
        Ok(InclusionOutcome::Reverted(tx_hash)) => {
            emitter.emit_order_rejected(
                order,
                &format!("Transaction {tx_hash} reverted on-chain"),
                get_atomic_clock_realtime().get_time_ns(),
                false,
            );
            executor
                .database
                .mark_execution_event_emitted(intent.id, "terminal")
                .await?;
            executor.release_slot();
            Ok(())
        }
        Ok(InclusionOutcome::Pending(message)) => anyhow::bail!(message),
        Err(e) => Err(e),
    }
}

async fn finalized_transaction_matches(
    included: &IncludedTransaction,
    intent: &ExecutionIntentRow,
    nonce: u64,
    executor: &TransactionExecutor,
) -> anyhow::Result<bool> {
    let block = executor
        .http_rpc_client
        .block_by_number(included.block_number, true)
        .await?;
    anyhow::ensure!(
        block.hash == included.receipt.block_hash,
        "Finalized block {} changed from {} to {} before intent validation",
        included.block_number,
        included.receipt.block_hash,
        block.hash
    );
    let Some(transaction) = block
        .transactions
        .iter()
        .find(|transaction| transaction.hash == included.tx_hash)
    else {
        anyhow::bail!(
            "Finalized block {} does not contain transaction {}",
            included.block_number,
            included.tx_hash
        );
    };
    let (expected_to, expected_input, expected_value) = persisted_call_fields(intent)?;

    Ok(transaction.from == executor.wallet_address
        && transaction.nonce == nonce
        && transaction.to == Some(expected_to)
        && transaction.input == expected_input
        && transaction.value == expected_value)
}

/// Parses the persisted destination, calldata, and value of an execution intent.
fn persisted_call_fields(intent: &ExecutionIntentRow) -> anyhow::Result<(Address, Bytes, U256)> {
    let to = Address::from_str(&intent.transaction_to)
        .with_context(|| "persisted execution destination is invalid")?;
    let input = hex::decode(
        intent
            .transaction_input
            .strip_prefix("0x")
            .unwrap_or(&intent.transaction_input),
    )
    .with_context(|| "persisted execution calldata is invalid")?;
    let value = U256::from_str(&intent.transaction_value)
        .with_context(|| "persisted execution value is invalid")?;

    Ok((to, Bytes::from(input), value))
}

async fn complete_finalized_swap(
    plan: &SwapPlan,
    intent_id: i64,
    included: &IncludedTransaction,
    executor: &TransactionExecutor,
    emitter: &ExecutionEventEmitter,
) -> anyhow::Result<()> {
    if let Err(e) = emit_finalized_swap_fill(plan, included, executor, emitter).await {
        if plan.order.status() != OrderStatus::Rejected {
            emitter.emit_order_rejected(
                &plan.order,
                &format!(
                    "Finalized transaction {} could not produce an exact fill: {e}",
                    included.tx_hash
                ),
                get_atomic_clock_realtime().get_time_ns(),
                false,
            );
        }
        executor
            .database
            .mark_execution_event_emitted(intent_id, "terminal")
            .await?;
        return Ok(());
    }

    refresh_wallet_after_fill(plan, executor, emitter).await?;
    executor
        .database
        .mark_execution_event_emitted(intent_id, "fill")
        .await
}

async fn emit_finalized_swap_fill(
    plan: &SwapPlan,
    included: &IncludedTransaction,
    executor: &TransactionExecutor,
    emitter: &ExecutionEventEmitter,
) -> anyhow::Result<()> {
    let signature =
        keccak256("Swap(address,address,int256,int256,uint160,uint128,int24)").to_string();
    let swap_logs = included
        .receipt
        .logs
        .iter()
        .filter(|log| log.topics.first().is_some_and(|topic| topic == &signature))
        .collect::<Vec<_>>();
    anyhow::ensure!(
        swap_logs.len() == 1,
        "Finalized transaction {} emitted {} Swap logs; expected exactly one",
        included.tx_hash,
        swap_logs.len()
    );
    let log = swap_logs[0];
    let address = Address::from_str(&log.address)
        .with_context(|| format!("Invalid finalized Swap log address {}", log.address))?;
    anyhow::ensure!(
        address == plan.pool_address,
        "Finalized Swap log came from pool {address}, expected {}",
        plan.pool_address
    );

    let dex = crate::exchanges::get_dex_extended(plan.pool.chain.name, &plan.pool.dex.name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No RPC Swap decoder for {}:{}",
                plan.pool.chain.name,
                plan.pool.dex.name
            )
        })?;
    let event = dex.parse_swap_event_rpc(log)?;
    let base_amount = if plan.pool.get_base_token().address == plan.pool.token0.address {
        event.amount0
    } else {
        event.amount1
    };
    anyhow::ensure!(
        base_amount.is_positive() && base_amount.unsigned_abs() == plan.amount_in,
        "Finalized Swap input {base_amount} does not match the persisted amount {}",
        plan.amount_in
    );

    let block = executor
        .http_rpc_client
        .block_by_number(included.block_number, false)
        .await?;
    anyhow::ensure!(
        block.hash == included.receipt.block_hash,
        "Finalized block {} changed from {} to {} before fill emission",
        included.block_number,
        included.receipt.block_hash,
        block.hash
    );
    let timestamp_ns = block
        .timestamp
        .checked_mul(NANOSECONDS_IN_SECOND)
        .ok_or_else(|| anyhow::anyhow!("Finalized block timestamp overflows nanoseconds"))?;
    let mut swap = event.to_pool_swap(
        plan.pool.chain.clone(),
        plan.instrument_id,
        plan.pool.pool_identifier,
        UnixNanos::from(timestamp_ns),
    );
    swap.calculate_trade_info(&plan.pool.token0, &plan.pool.token1, None)?;
    let trade = swap
        .trade_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Finalized Swap has no calculated trade information"))?;
    anyhow::ensure!(
        trade.order_side == OrderSide::Sell,
        "Finalized Swap side {} does not match Sell order",
        trade.order_side
    );
    let gas_cost = included
        .receipt
        .effective_gas_price
        .checked_mul(U256::from(included.receipt.gas_used))
        .ok_or_else(|| anyhow::anyhow!("Finalized transaction gas commission overflow"))?;
    let commission = Money::from_u256(gas_cost, plan.pool.chain.native_currency())?;
    let trade_digest = keccak256(format!("{}:{}", included.tx_hash, swap.log_index));
    let trade_digest = trade_digest.to_string();
    let trade_id = TradeId::new_checked(&trade_digest[2..38])?;

    if plan
        .order
        .trade_ids()
        .iter()
        .any(|existing| **existing == trade_id)
    {
        return Ok(());
    }

    emitter.emit_order_filled(
        &plan.order,
        VenueOrderId::new_checked(included.tx_hash.to_string())?,
        None,
        trade_id,
        plan.order.quantity(),
        trade.execution_price,
        plan.quote_currency,
        Some(commission),
        LiquiditySide::Taker,
        UnixNanos::from(timestamp_ns),
    );
    Ok(())
}

async fn refresh_wallet_after_fill(
    plan: &SwapPlan,
    executor: &TransactionExecutor,
    emitter: &ExecutionEventEmitter,
) -> anyhow::Result<()> {
    let mut token_universe = executor
        .wallet_balance
        .lock()
        .expect("wallet balance mutex poisoned")
        .token_universe
        .clone();
    token_universe.insert(plan.pool.token0.address);
    token_universe.insert(plan.pool.token1.address);

    let native_amount = executor
        .http_rpc_client
        .get_balance_with_timeout(
            &executor.wallet_address,
            None,
            Some(EXECUTION_RPC_TIMEOUT_SECS),
        )
        .await?;
    let native_balance = Money::from_u256(native_amount, plan.pool.chain.native_currency())?;
    let erc20 = Erc20Contract::new_with_timeout(
        executor.http_rpc_client.clone(),
        Some(EXECUTION_RPC_TIMEOUT_SECS),
        true,
    );
    let mut token_addresses = token_universe.iter().copied().collect::<Vec<_>>();
    token_addresses.sort_unstable();
    let mut token_balances = Vec::with_capacity(token_addresses.len());
    for address in token_addresses {
        let token = if address == plan.pool.token0.address {
            plan.pool.token0.clone()
        } else if address == plan.pool.token1.address {
            plan.pool.token1.clone()
        } else {
            let metadata = erc20.fetch_token_info(&address).await?;
            Token::new(
                plan.pool.chain.clone(),
                address,
                metadata.name,
                metadata.symbol,
                metadata.decimals,
            )
        };
        let amount = erc20.balance_of(&address, &executor.wallet_address).await?;
        token_balances.push(TokenBalance::new(amount, token));
    }

    let mut wallet_balance = WalletBalance::new(token_universe);
    let balances = wallet_balance.replace_balances(native_balance, token_balances)?;
    *executor
        .wallet_balance
        .lock()
        .expect("wallet balance mutex poisoned") = wallet_balance;
    emitter.try_emit_account_state(
        balances,
        vec![],
        true,
        get_atomic_clock_realtime().get_time_ns(),
        None,
    )
}

/// Runs the read-only pre-trade checks for a swap: deployed bytecode at the pool, router,
/// and token addresses, and an operator-prepared router allowance and input-token balance
/// covering the amount. The shared signing pipeline checks the exact maximum native cost.
/// Never wraps or approves.
async fn check_swap_preconditions(
    plan: &SwapPlan,
    executor: &TransactionExecutor,
) -> anyhow::Result<()> {
    for (address, description) in [
        (plan.pool_address, "pool"),
        (plan.router, "router"),
        (plan.token_in, "input token"),
        (plan.token_out, "output token"),
    ] {
        let code = executor.http_rpc_client.get_code(&address).await?;
        if code.is_empty() {
            anyhow::bail!("No deployed bytecode at {description} address {address}");
        }
    }

    let erc20_contract = Erc20Contract::new_with_timeout(
        executor.http_rpc_client.clone(),
        Some(EXECUTION_RPC_TIMEOUT_SECS),
        true,
    );

    let allowance = erc20_contract
        .allowance(&plan.token_in, &executor.wallet_address, &plan.router)
        .await?;

    if allowance < plan.amount_in {
        anyhow::bail!(
            "Router allowance {allowance} is below the swap amount {} for input token {}; approve the router explicitly before submitting",
            plan.amount_in,
            plan.token_in
        );
    }

    let balance = erc20_contract
        .balance_of(&plan.token_in, &executor.wallet_address)
        .await?;

    if balance < plan.amount_in {
        anyhow::bail!(
            "Input token {} balance {balance} is below the swap amount {}",
            plan.token_in,
            plan.amount_in
        );
    }

    Ok(())
}

/// Converts a base-denominated order quantity into raw token units with exact integer
/// scaling by the base token decimals.
///
/// `Quantity::raw` is scaled to the greater of its declared precision and `FIXED_PRECISION`;
/// token amounts are scaled to the token's decimals. Quantities not exactly representable in
/// token units are rejected rather than rounded.
fn quantity_to_raw_amount(quantity: Quantity, decimals: u8) -> anyhow::Result<U256> {
    if quantity.is_zero() {
        anyhow::bail!("Order quantity must be positive");
    }

    let raw = U256::from(quantity.raw);
    let raw_precision = quantity.precision.max(FIXED_PRECISION);
    if decimals >= raw_precision {
        let scale = U256::from(10u64)
            .checked_pow(U256::from(decimals - raw_precision))
            .ok_or_else(|| anyhow::anyhow!("Order amount scaling overflow"))?;
        raw.checked_mul(scale).ok_or_else(|| {
            anyhow::anyhow!("Order amount overflow scaling quantity to raw token units")
        })
    } else {
        let divisor = U256::from(10u64)
            .checked_pow(U256::from(raw_precision - decimals))
            .ok_or_else(|| anyhow::anyhow!("Order amount scaling overflow"))?;
        if !(raw % divisor).is_zero() {
            anyhow::bail!(
                "Order quantity {quantity} is not exactly representable in {decimals} base token decimals"
            );
        }
        Ok(raw / divisor)
    }
}

/// Extracts the positive output amount from an exact-input swap quote.
fn exact_output_amount(quote: &SwapQuote, zero_for_one: bool) -> anyhow::Result<U256> {
    let amount = if zero_for_one {
        quote.amount1
    } else {
        quote.amount0
    };

    if !amount.is_negative() {
        anyhow::bail!("Swap quote output amount {amount} is not a positive output");
    }
    Ok(amount.unsigned_abs())
}

/// Derives the minimum output accepted for the swap: the quoted output reduced by
/// `slippage_bps`, in exact integer arithmetic. Rejects a zero minimum, which would leave
/// the swap without slippage protection.
fn derive_min_amount_out(quoted_amount_out: U256, slippage_bps: u32) -> anyhow::Result<U256> {
    if slippage_bps >= BPS_DENOMINATOR {
        anyhow::bail!("Slippage {slippage_bps} bps must be below {BPS_DENOMINATOR}");
    }
    let min_amount_out = quoted_amount_out
        .checked_mul(U256::from(BPS_DENOMINATOR - slippage_bps))
        .and_then(|scaled| scaled.checked_div(U256::from(BPS_DENOMINATOR)))
        .ok_or_else(|| anyhow::anyhow!("Minimum output derivation overflow"))?;
    if min_amount_out.is_zero() {
        anyhow::bail!(
            "Derived minimum output is zero for quoted output {quoted_amount_out} at {slippage_bps} bps slippage"
        );
    }
    Ok(min_amount_out)
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

    fn handles_order_venue(&self, venue: Venue) -> bool {
        venue.parse_dex().is_ok_and(|(blockchain, dex_type)| {
            blockchain == self.chain.name && dex_type == DexType::UniswapV3
        })
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        self.emitter
            .try_emit_account_state(balances, margins, reported, ts_event, info)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }

        self.emitter.set_sender(get_exec_event_sender());
        self.core.set_started();
        log::info!(
            "Started: client_id={}, account_id={}",
            self.core.client_id,
            self.core.account_id
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        self.pending_tasks.abort_all_retained();
        self.signer = None;
        self.core.set_stopped();
        self.core.set_disconnected();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        let plan = match self.prepare_swap(&cmd, &order) {
            Ok(plan) => plan,
            Err(e) => {
                self.emitter.emit_order_denied(&order, &e.to_string());
                return Ok(());
            }
        };

        let executor = match self.transaction_executor() {
            Ok(executor) => executor,
            Err(e) => {
                self.emitter.emit_order_denied(&order, &e.to_string());
                return Ok(());
            }
        };

        let emitter = self.emitter.clone();
        let max_quote_age_blocks = self.transaction_limits.max_quote_age_blocks;
        let deadline_seconds = self.transaction_limits.deadline_seconds;
        let client_order_id = order.client_order_id();

        let handle = get_runtime().spawn(async move {
            if let Err(e) = execute_swap(
                plan,
                executor,
                emitter,
                max_quote_age_blocks,
                deadline_seconds,
            )
            .await
            {
                log::warn!("Swap execution for order {client_order_id} failed: {e:?}");
            }
        });
        self.pending_tasks.push(handle);

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let orders = self
            .core
            .cache()
            .orders_for_ids(&cmd.order_list.client_order_ids, &cmd);

        for order in &orders {
            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                continue;
            }

            self.emitter
                .emit_order_denied(order, ORDER_LIST_UNSUPPORTED);
        }

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let Ok(order) = self.core.cache().try_order_owned(&cmd.client_order_id) else {
            log::warn!("Cannot modify unknown order {}", cmd.client_order_id);
            return Ok(());
        };

        self.emitter.emit_order_modify_rejected(
            &order,
            cmd.venue_order_id,
            ORDER_MODIFY_UNSUPPORTED,
            get_atomic_clock_realtime().get_time_ns(),
        );
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let Ok(order) = self.core.cache().try_order_owned(&cmd.client_order_id) else {
            log::warn!("Cannot cancel unknown order {}", cmd.client_order_id);
            return Ok(());
        };

        self.emitter.emit_order_cancel_rejected(
            &order,
            cmd.venue_order_id,
            ORDER_CANCEL_UNSUPPORTED,
            get_atomic_clock_realtime().get_time_ns(),
        );
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        log::warn!(
            "Cancel-all for {} is not supported on the blockchain execution client",
            cmd.instrument_id
        );
        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        for cancel in cmd.cancels {
            self.cancel_order(cancel)?;
        }
        Ok(())
    }

    fn query_account(&self, cmd: QueryAccount) -> anyhow::Result<()> {
        anyhow::ensure!(
            cmd.account_id == self.core.account_id,
            "Query account ID {} does not match client account ID {}",
            cmd.account_id,
            self.core.account_id
        );
        anyhow::ensure!(self.core.is_started(), "Execution client is not started");

        let balances = self
            .wallet_balance
            .lock()
            .expect("wallet balance mutex poisoned")
            .as_account_balances()?;
        self.generate_account_state(
            balances,
            vec![],
            true,
            get_atomic_clock_realtime().get_time_ns(),
            None,
        )
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        log::warn!(
            "Order queries are not supported on the blockchain execution client; cannot query {}",
            cmd.client_order_id
        );
        Ok(())
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

        // A prior stop or disconnect aborted spawned submission tasks; only after their
        // termination is it safe to release a leftover pre-signature claim, because a
        // terminated task can never touch the slot again
        let reaped = tokio::time::timeout(Duration::from_secs(5), async {
            while !self.pending_tasks.all_finished() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .is_ok();

        if reaped {
            release_preparing_slot(&self.in_flight);
        } else {
            log::error!(
                "Submission tasks did not terminate after disconnect; keeping any in-flight slot claim"
            );
        }

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
            self.cache.ensure_execution_transaction_schema().await?;
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

        if self.cache.has_database()
            && let Err(e) = self.reconcile_unresolved_execution().await
        {
            self.signer = None;
            return Err(e);
        }

        if let Err(e) = self.refresh_wallet_balances().await {
            self.signer = None;
            return Err(e);
        }
        self.core.set_connected();
        log::info!(
            "Blockchain execution client connected on chain {}",
            self.chain.name
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        // Task handles stay registered so a later `connect` can prove their termination
        // before releasing any stale pre-signature slot claim
        self.pending_tasks.abort_all_retained();
        self.signer = None;
        self.core.set_disconnected();
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        anyhow::bail!("{VENUE_EXECUTION_REPORTS_UNSUPPORTED}");
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        anyhow::bail!("{VENUE_EXECUTION_REPORTS_UNSUPPORTED}");
    }

    async fn generate_fill_reports(
        &self,
        _cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        anyhow::bail!("{VENUE_EXECUTION_REPORTS_UNSUPPORTED}");
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        anyhow::bail!("{VENUE_EXECUTION_REPORTS_UNSUPPORTED}");
    }

    async fn generate_mass_status(
        &self,
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        // Venue mass status is unsupported; durable intent reconciliation at connect
        // covers restart recovery, and Ok(None) keeps LiveNode startup safe
        log::warn!(
            "Mass status is not supported on the blockchain execution client; skipping venue reconciliation"
        );
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use alloy::{
        primitives::{address, aliases::I24},
        sol_types::SolValue,
    };
    use nautilus_common::{
        cache::Cache, live::runner::replace_exec_event_sender, messages::ExecutionEvent,
    };
    use nautilus_core::UUID4;
    use nautilus_infrastructure::sql::pg::{PostgresConnectOptions, get_postgres_connect_options};
    use nautilus_model::{
        defi::{
            PoolProfiler,
            chain::chains,
            data::block::BlockPosition,
            pool_analysis::{
                position::PoolPosition,
                snapshot::{PoolAnalytics, PoolSnapshot, PoolState},
            },
            tick_map::{tick::PoolTick, tick_math::get_tick_at_sqrt_ratio},
        },
        enums::AccountType,
        events::OrderEventAny,
        identifiers::{OrderListId, StrategyId, TraderId},
        orders::{OrderList, OrderTestBuilder},
        types::Price,
    };
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
    const GET_BALANCE_INSUFFICIENT: &str =
        include_str!("../../test_data/execution/rpc_eth_get_balance_insufficient.json");
    const CALL_BALANCE: &str = include_str!("../../test_data/execution/rpc_eth_call_balance.json");
    const CALL_BALANCE_AFTER_WRAP: &str =
        include_str!("../../test_data/execution/rpc_eth_call_balance_after_wrap.json");
    const CALL_BALANCE_WETH: &str =
        include_str!("../../test_data/execution/rpc_eth_call_balance_weth.json");
    const CALL_BALANCE_USDC: &str =
        include_str!("../../test_data/execution/rpc_eth_call_balance_usdc.json");
    const CALL_BALANCE_WETH_UPDATED: &str =
        include_str!("../../test_data/execution/rpc_eth_call_balance_weth_updated.json");
    const CALL_BALANCE_USDC_UPDATED: &str =
        include_str!("../../test_data/execution/rpc_eth_call_balance_usdc_updated.json");
    const CALL_BOOL_TRUE: &str =
        include_str!("../../test_data/execution/rpc_eth_call_bool_true.json");
    const CALL_EMPTY: &str = include_str!("../../test_data/execution/rpc_eth_call_empty.json");
    const CALL_ZERO: &str = include_str!("../../test_data/execution/rpc_eth_call_zero.json");
    const CALL_ALLOWANCE: &str =
        include_str!("../../test_data/execution/rpc_eth_call_allowance.json");
    const TRANSACTION_COUNT: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_count.json");
    const TRANSACTION_COUNT_NEXT: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_count_next.json");
    const ESTIMATE_GAS: &str = include_str!("../../test_data/execution/rpc_eth_estimate_gas.json");
    const MAX_PRIORITY_FEE: &str =
        include_str!("../../test_data/execution/rpc_eth_max_priority_fee_per_gas.json");
    const BLOCK_BY_NUMBER: &str =
        include_str!("../../test_data/execution/rpc_eth_get_block_by_number.json");
    const BLOCK_CANONICAL: &str =
        include_str!("../../test_data/execution/rpc_eth_get_block_canonical.json");
    const BLOCK_FINALIZED: &str =
        include_str!("../../test_data/execution/rpc_eth_get_block_finalized.json");
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
    const SEND_RAW_TRANSACTION_NONCE_TOO_LOW: &str =
        include_str!("../../test_data/execution/rpc_eth_send_raw_transaction_nonce_too_low.json");
    const RPC_METHOD_NOT_FOUND: &str =
        include_str!("../../test_data/execution/rpc_error_method_not_found.json");

    const WALLET: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    const ROUTER: &str = "0xE592427A0AEce92De3Edee1F18E0157C05861564";
    const WETH: &str = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1";
    const USDC: &str = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831";

    const WETH_ADDRESS: Address = address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1");
    const ROUTER_ADDRESS: Address = address!("E592427A0AEce92De3Edee1F18E0157C05861564");

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
            .allowed_token_pairs(vec![(WETH.to_string(), USDC.to_string())])
            .slippage_bps(50)
            .max_slippage_bps(200)
            .max_order_amount(1_000_000_000_000_000_000)
            .deadline_seconds(300)
            .max_quote_age_blocks(100)
            .receipt_timeout_secs(1)
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
        test_client_and_cache(config, pool).map(|(client, _)| client)
    }

    fn test_client_and_cache(
        config: BlockchainExecutionClientConfig,
        pool: Pool,
    ) -> anyhow::Result<(BlockchainExecutionClient, Rc<RefCell<Cache>>)> {
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
            cache.clone(),
        );

        let client = BlockchainExecutionClient::new(core, config)?;
        Ok((client, cache))
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

    async fn client_with_token_mock_rpc(
        state: MockRpcState,
        signer_env: &str,
    ) -> (BlockchainExecutionClient, MockRpcState, Rc<RefCell<Cache>>) {
        let addr = start_mock_rpc_server(state.clone()).await;
        let pool = test_pool();
        let tokens = [pool.token0.clone(), pool.token1.clone()];
        let mut config = test_config_with_signer_env(format!("http://{addr}"), signer_env);
        config.tokens = Some(vec![WETH.to_string(), USDC.to_string()]);
        let (mut client, cache) = test_client_and_cache(config, pool).unwrap();
        for token in tokens {
            client.cache.add_token(token).await.unwrap();
        }
        (client, state, cache)
    }

    fn execution_rpc_state() -> MockRpcState {
        MockRpcState::default()
            .with_receipt_hash_from_request()
            .with_response("eth_chainId", CHAIN_ID_ARBITRUM)
            .with_response("eth_getCode", GET_CODE_DEPLOYED)
            .with_response("eth_getBalance", GET_BALANCE)
            .with_response("eth_getBlockByNumber", BLOCK_BY_NUMBER)
            .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", BLOCK_CANONICAL)
            .with_parameter_response("eth_getBlockByNumber", "finalized", BLOCK_FINALIZED)
            .with_parameter_response("eth_getBlockByNumber", "0x1cf0d42", BLOCK_FINALIZED)
            .with_response("eth_maxPriorityFeePerGas", MAX_PRIORITY_FEE)
    }

    fn ready_rpc_state() -> MockRpcState {
        execution_rpc_state()
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE)
            .with_call_response(ALLOWANCE_SELECTOR, CALL_ALLOWANCE)
    }

    fn signing_rpc_state() -> MockRpcState {
        ready_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
    }

    fn broadcast_rpc_state() -> MockRpcState {
        execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_send_raw_transaction_echo()
    }

    async fn expected_wrap_tx_hash(value: U256) -> B256 {
        expected_tx_hash(
            WETH_ADDRESS,
            value,
            Bytes::from(nautilus_core::hex::decode("d0e30db0").unwrap()),
        )
        .await
    }

    async fn expected_approve_tx_hash(amount: U256) -> B256 {
        let calldata = ERC20::approveCall {
            spender: ROUTER_ADDRESS,
            amount,
        }
        .abi_encode();
        expected_tx_hash(WETH_ADDRESS, U256::ZERO, Bytes::from(calldata)).await
    }

    async fn expected_tx_hash(to: Address, value: U256, input: Bytes) -> B256 {
        // The orchestration's policy math is deterministic: nonce 7 from the fixture,
        // buffered gas 78000, buffered max fee 130000000, priority fee 10000000
        let expected_tx =
            build_eip1559_transaction(42161, 7, 78_000, 130_000_000, 10_000_000, to, value, input);
        let (expected_hash, _) = sign_eip1559_transaction(
            expected_tx,
            &PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        )
        .await
        .unwrap();
        expected_hash
    }

    async fn await_recorded_request(state: &MockRpcState, method: &str) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if state
                    .recorded_requests()
                    .iter()
                    .any(|request| request["method"] == method)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
    }

    /// The block number served by the `eth_getBlockByNumber` fixture; swap quotes pin their
    /// profiler state to it so the freshness check passes.
    const FIXTURE_BLOCK: u64 = 30_346_560;
    /// The timestamp served by the `eth_getBlockByNumber` fixture.
    const FIXTURE_BLOCK_TIMESTAMP: u64 = 1_761_888_800;
    /// Synthetic full-range liquidity for the test pool profiler.
    const TEST_LIQUIDITY: u128 = 1_000_000_000_000_000_000_000;

    fn test_profiler(pool: &Pool, block_number: u64) -> PoolProfiler {
        test_profiler_with_state(pool, block_number, U160::from(1u128 << 96), TEST_LIQUIDITY)
    }

    fn test_profiler_with_state(
        pool: &Pool,
        block_number: u64,
        sqrt_price_x96: U160,
        liquidity: u128,
    ) -> PoolProfiler {
        test_profiler_with_range(
            pool,
            block_number,
            sqrt_price_x96,
            -887_220,
            887_220,
            liquidity,
        )
    }

    /// Builds a profiler from a synthetic snapshot with a single position over
    /// `[tick_lower, tick_upper]`: exact-input quotes within the range match the
    /// constant-product curve, and a tiny test swap never crosses a tick.
    fn test_profiler_with_range(
        pool: &Pool,
        block_number: u64,
        sqrt_price_x96: U160,
        tick_lower: i32,
        tick_upper: i32,
        liquidity: u128,
    ) -> PoolProfiler {
        let snapshot = PoolSnapshot::new(
            pool.instrument_id,
            PoolState {
                current_tick: get_tick_at_sqrt_ratio(sqrt_price_x96),
                price_sqrt_ratio_x96: sqrt_price_x96,
                liquidity,
                protocol_fees_token0: U256::ZERO,
                protocol_fees_token1: U256::ZERO,
                fee_protocol: 0,
                fee_protocol0_basis_points: None,
                fee_protocol1_basis_points: None,
                fee_growth_global_0: U256::ZERO,
                fee_growth_global_1: U256::ZERO,
            },
            vec![PoolPosition::new(
                address!("DeaDbeefdEAdbeefdEadbEEFdeadbeEFdEaDbeeF"),
                tick_lower,
                tick_upper,
                liquidity as i128,
            )],
            vec![
                PoolTick::new(
                    tick_lower,
                    liquidity,
                    liquidity as i128,
                    U256::ZERO,
                    U256::ZERO,
                    true,
                    0,
                ),
                PoolTick::new(
                    tick_upper,
                    liquidity,
                    -(liquidity as i128),
                    U256::ZERO,
                    U256::ZERO,
                    true,
                    0,
                ),
            ],
            PoolAnalytics::default(),
            BlockPosition::new(
                block_number,
                "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                0,
                0,
            ),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let mut profiler = PoolProfiler::new(Arc::new(pool.clone()));
        profiler.restore_from_snapshot(snapshot).unwrap();
        profiler
    }

    fn test_market_sell_order(instrument_id: InstrumentId) -> OrderAny {
        market_sell_order_with_id(instrument_id, "O-SWAP-001")
    }

    fn submit_order_cmd(order: &OrderAny) -> SubmitOrder {
        SubmitOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("BLOCKCHAIN-001")),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.init_event().clone(),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        )
    }

    /// Builds a client whose shared cache holds the test pool, a live profiler at
    /// `FIXTURE_BLOCK`, and a SELL market order, without a durable store.
    fn swap_client_with_cache(
        config: BlockchainExecutionClientConfig,
    ) -> (BlockchainExecutionClient, Rc<RefCell<Cache>>) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let pool = test_pool();
        cache.borrow_mut().add_pool(pool.clone()).unwrap();
        cache
            .borrow_mut()
            .add_order(
                test_market_sell_order(pool.instrument_id),
                None,
                None,
                false,
            )
            .unwrap();
        cache
            .borrow_mut()
            .add_pool_profiler(test_profiler(&pool, FIXTURE_BLOCK))
            .unwrap();
        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from("BLOCKCHAIN-001"),
            *BLOCKCHAIN_VENUE,
            OmsType::Netting,
            AccountId::from("BLOCKCHAIN-001"),
            AccountType::Wallet,
            None,
            cache.clone(),
        );

        let client = BlockchainExecutionClient::new(core, config).unwrap();
        (client, cache)
    }

    /// Builds a connected, signer-equipped swap client backed by a fresh Postgres schema.
    async fn swap_client_with_database(
        test_name: &str,
        state: MockRpcState,
    ) -> Option<(
        sqlx::PgPool,
        String,
        BlockchainExecutionClient,
        MockRpcState,
        Rc<RefCell<Cache>>,
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
        let (mut client, cache) = swap_client_with_cache(test_config(format!("http://{addr}")));
        client.cache.database = Some(database);
        // Mirror the connect-time migration: tests create the pre-submission table shape
        client
            .cache
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        Some((admin_pool, schema, client, state, cache))
    }

    async fn swap_rpc_state() -> MockRpcState {
        let min_amount_out = expected_min_amount_out(50);
        swap_rpc_state_with_min_amount_out(min_amount_out).await
    }

    async fn swap_rpc_state_with_min_amount_out(min_amount_out: U256) -> MockRpcState {
        let (tx_hash, _) = expected_swap_tx(min_amount_out).await;
        finalized_swap_rpc_state(tx_hash, min_amount_out)
    }

    /// The swap state with a broadcast response whose hash differs from the signed hash.
    async fn swap_rpc_state_for_mismatch() -> MockRpcState {
        swap_rpc_state()
            .await
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION)
    }

    /// Extracts the transaction awaiting finality in the in-flight slot.
    fn awaiting_in_flight(client: &BlockchainExecutionClient) -> InFlightTransaction {
        let slot = *client.in_flight.lock().unwrap();
        let Some(InFlightSlot::AwaitingFinality(in_flight)) = slot else {
            panic!("expected an awaiting-finality transaction, was {slot:?}");
        };
        in_flight
    }

    fn start_with_events(
        client: &mut BlockchainExecutionClient,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        client.start().unwrap();
        receiver
    }

    async fn await_pending_tasks(client: &BlockchainExecutionClient) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !client.pending_tasks.all_finished() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
    }

    fn collect_order_events(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Vec<OrderEventAny> {
        let mut events = Vec::new();

        while let Ok(event) = receiver.try_recv() {
            if let ExecutionEvent::Order(order_event) = event {
                events.push(order_event);
            }
        }
        events
    }

    fn assert_missing_swap_log_rejected(events: &[OrderEventAny]) {
        assert_eq!(events.len(), 2, "was: {events:?}");
        assert!(matches!(&events[0], OrderEventAny::Submitted(_)));
        let OrderEventAny::Rejected(rejected) = &events[1] else {
            panic!("expected OrderRejected, was {:?}", events[1]);
        };
        assert!(
            rejected.reason.as_str().contains("emitted 0 Swap logs"),
            "was: {}",
            rejected.reason
        );
    }

    fn assert_swap_submitted_and_filled(events: &[OrderEventAny]) {
        assert_eq!(events.len(), 2, "was: {events:?}");
        assert!(matches!(&events[0], OrderEventAny::Submitted(_)));
        assert!(matches!(&events[1], OrderEventAny::Filled(_)));
    }

    fn expected_swap_calldata(min_amount_out: U256) -> Vec<u8> {
        UniswapV3SwapRouter::exactInputSingleCall {
            params: UniswapV3SwapRouter::ExactInputSingleParams {
                tokenIn: WETH_ADDRESS,
                tokenOut: address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
                fee: U24::try_from(500u32).unwrap(),
                recipient: address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
                deadline: U256::from(FIXTURE_BLOCK_TIMESTAMP + 300),
                amountIn: U256::from(1_000_000_000_000_000u64),
                amountOutMinimum: min_amount_out,
                sqrtPriceLimitX96: U160::ZERO,
            },
        }
        .abi_encode()
    }

    /// Derives the expected minimum output with the same live profiler the plan used.
    fn expected_min_amount_out(slippage_bps: u32) -> U256 {
        expected_min_amount_out_for(&test_pool(), true, slippage_bps)
    }

    fn expected_min_amount_out_for(pool: &Pool, zero_for_one: bool, slippage_bps: u32) -> U256 {
        let profiler = test_profiler(pool, FIXTURE_BLOCK);
        let quote = profiler
            .swap_exact_in(U256::from(1_000_000_000_000_000u64), zero_for_one, None)
            .unwrap();
        let quoted = exact_output_amount(&quote, zero_for_one).unwrap();
        derive_min_amount_out(quoted, slippage_bps).unwrap()
    }

    async fn expected_swap_tx(min_amount_out: U256) -> (B256, String) {
        // The orchestration's policy math is deterministic: nonce 7 from the fixture,
        // buffered gas 78000, buffered max fee 130000000, priority fee 10000000
        let expected_tx = build_eip1559_transaction(
            42161,
            7,
            78_000,
            130_000_000,
            10_000_000,
            ROUTER_ADDRESS,
            U256::ZERO,
            Bytes::from(expected_swap_calldata(min_amount_out)),
        );
        sign_eip1559_transaction(
            expected_tx,
            &PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        )
        .await
        .map(|(hash, raw)| (hash, nautilus_core::hex::encode_prefixed(&raw)))
        .unwrap()
    }

    fn finalized_swap_receipt(tx_hash: B256) -> String {
        let data = (
            I256::try_from(1_000_000_000_000_000_i128).unwrap(),
            I256::try_from(-1_000_000_i128).unwrap(),
            U160::from(1_u128 << 96),
            TEST_LIQUIDITY,
            I24::try_from(0).unwrap(),
        )
            .abi_encode();
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "transactionHash": tx_hash.to_string(),
                "blockHash": "0x2222222222222222222222222222222222222222222222222222222222222222",
                "blockNumber": "0x1cf0d41",
                "transactionIndex": "0x2",
                "gasUsed": "0xc3c0",
                "effectiveGasPrice": "0x5f5e100",
                "status": "0x1",
                "logs": [{
                    "removed": false,
                    "logIndex": "0x6",
                    "transactionIndex": "0x2",
                    "transactionHash": tx_hash.to_string(),
                    "blockHash": "0x2222222222222222222222222222222222222222222222222222222222222222",
                    "blockNumber": "0x1cf0d41",
                    "address": test_pool().address.to_string(),
                    "data": hex::encode_prefixed(data),
                    "topics": [
                        "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",
                        "0x000000000000000000000000f39fd6e51aad88f6f4ce6ab8827279cfffb92266",
                        "0x000000000000000000000000f39fd6e51aad88f6f4ce6ab8827279cfffb92266"
                    ]
                }]
            }
        })
        .to_string()
    }

    fn finalized_swap_block(tx_hash: B256, min_amount_out: U256) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d41",
                "hash": "0x2222222222222222222222222222222222222222222222222222222222222222",
                "timestamp": "0x69044a21",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": tx_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "to": ROUTER,
                    "input": hex::encode_prefixed(expected_swap_calldata(min_amount_out)),
                    "value": "0x0"
                }]
            }
        })
        .to_string()
    }

    fn finalized_swap_rpc_state(tx_hash: B256, min_amount_out: U256) -> MockRpcState {
        let receipt = finalized_swap_receipt(tx_hash);
        let block = finalized_swap_block(tx_hash, min_amount_out);
        signing_rpc_state()
            .with_response("eth_getTransactionReceipt", &receipt)
            .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
            .with_send_raw_transaction_echo()
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE)
            .with_call_response(ALLOWANCE_SELECTOR, CALL_ALLOWANCE)
    }

    fn replacement_head_block(tx_hash: B256) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d40",
                "hash": "0x1111111111111111111111111111111111111111111111111111111111111111",
                "timestamp": "0x69044a20",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": tx_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "to": WETH,
                    "input": "0xd0e30db0",
                    "value": "0x38d7ea4c68000"
                }]
            }
        })
        .to_string()
    }

    /// The canonical block at the receipt height containing the given wrap transaction with
    /// the exact persisted call fields.
    fn finalized_wrap_block(tx_hash: B256) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d41",
                "hash": "0x2222222222222222222222222222222222222222222222222222222222222222",
                "timestamp": "0x69044a21",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": tx_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "to": WETH,
                    "input": "0xd0e30db0",
                    "value": "0x38d7ea4c68000"
                }]
            }
        })
        .to_string()
    }

    /// The canonical block at the receipt height containing the given approve transaction
    /// with the exact persisted call fields.
    fn finalized_approve_block(tx_hash: B256, amount: U256) -> String {
        let calldata = ERC20::approveCall {
            spender: ROUTER_ADDRESS,
            amount,
        }
        .abi_encode();
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d41",
                "hash": "0x2222222222222222222222222222222222222222222222222222222222222222",
                "timestamp": "0x69044a21",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": tx_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "to": WETH,
                    "input": hex::encode_prefixed(calldata),
                    "value": "0x0"
                }]
            }
        })
        .to_string()
    }

    #[rstest]
    fn quantity_to_raw_amount_scales_by_token_decimals() {
        assert_eq!(
            quantity_to_raw_amount(Quantity::from("0.001"), 18).unwrap(),
            U256::from(1_000_000_000_000_000u64)
        );
        assert_eq!(
            quantity_to_raw_amount(Quantity::from("1.5"), 18).unwrap(),
            U256::from(1_500_000_000_000_000_000u128)
        );
        assert_eq!(
            quantity_to_raw_amount(Quantity::from("12.5"), 6).unwrap(),
            U256::from(12_500_000u64)
        );
    }

    #[rstest]
    fn quantity_to_raw_amount_uses_defi_quantity_precision() {
        let amount = U256::from(10_000_000_000_000_000u64);
        let quantity = Quantity::from_u256(amount, 18).unwrap();

        assert_eq!(quantity_to_raw_amount(quantity, 18).unwrap(), amount);
    }

    #[rstest]
    fn quantity_to_raw_amount_rejects_zero() {
        let error = quantity_to_raw_amount(Quantity::from("0.0"), 18).unwrap_err();

        assert_eq!(error.to_string(), "Order quantity must be positive");
    }

    #[rstest]
    fn quantity_to_raw_amount_rejects_inexact_token_units() {
        let error = quantity_to_raw_amount(Quantity::from("0.0000001"), 6).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("is not exactly representable in 6 base token decimals"),
            "was: {error}"
        );
    }

    #[rstest]
    #[case(1_000_000, 50, 995_000)]
    #[case(1_000_000, 0, 1_000_000)]
    #[case(1_000_000, 200, 980_000)]
    #[case(10_000, 9_999, 1)]
    fn derive_min_amount_out_applies_slippage(
        #[case] quoted: u64,
        #[case] slippage_bps: u32,
        #[case] expected: u64,
    ) {
        assert_eq!(
            derive_min_amount_out(U256::from(quoted), slippage_bps).unwrap(),
            U256::from(expected)
        );
    }

    #[rstest]
    fn derive_min_amount_out_rejects_zero_result() {
        let error = derive_min_amount_out(U256::from(9_999u64), 9_999).unwrap_err();

        assert!(
            error.to_string().contains("Derived minimum output is zero"),
            "was: {error}"
        );
    }

    #[rstest]
    fn derive_min_amount_out_rejects_full_slippage() {
        let error = derive_min_amount_out(U256::from(1_000_000u64), 10_000).unwrap_err();

        assert!(
            error.to_string().contains("must be below 10000"),
            "was: {error}"
        );
    }

    #[rstest]
    fn exact_output_amount_extracts_negative_leg() {
        let pool = test_pool();
        let profiler = test_profiler(&pool, FIXTURE_BLOCK);
        let quote = profiler
            .swap_exact_in(U256::from(1_000_000_000_000_000u64), true, None)
            .unwrap();

        let amount = exact_output_amount(&quote, true).unwrap();

        assert!(amount < U256::from(1_000_000_000_000_000u64));
        assert!(amount > U256::from(990_000_000_000_000u64));

        let error = exact_output_amount(&quote, false).unwrap_err();
        assert!(
            error.to_string().contains("is not a positive output"),
            "was: {error}"
        );
    }

    #[rstest]
    #[case("0xC6962004f452bE9203591991D15f6b388e09E8D0.Arbitrum:UniswapV3", true)]
    #[case("0xC6962004f452bE9203591991D15f6b388e09E8D0.Ethereum:UniswapV3", false)]
    #[case("0xC6962004f452bE9203591991D15f6b388e09E8D0.Arbitrum:UniswapV4", false)]
    #[case("ETHUSDT-PERP.BINANCE", false)]
    fn handles_order_venue_matches_chain_and_dex(#[case] instrument: &str, #[case] expected: bool) {
        let client = test_client("http://127.0.0.1:1".to_string());
        let instrument_id: InstrumentId = instrument.parse().unwrap();

        assert_eq!(client.handles_order_venue(instrument_id.venue), expected);
    }

    #[tokio::test]
    async fn submit_order_denies_buy_side() {
        let (mut client, cache) =
            swap_client_with_cache(test_config("http://127.0.0.1:1".to_string()));
        let pool = test_pool();
        let order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("S-001"))
            .instrument_id(pool.instrument_id)
            .client_order_id(ClientOrderId::from("O-SWAP-001"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.001"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("only Sell is supported"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_non_market_order_type() {
        let (mut client, cache) =
            swap_client_with_cache(test_config("http://127.0.0.1:1".to_string()));
        let pool = test_pool();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("S-001"))
            .instrument_id(pool.instrument_id)
            .client_order_id(ClientOrderId::from("O-SWAP-001"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.001"))
            .price(Price::from("2000"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("only Market is supported"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_quote_denominated_quantity() {
        let (mut client, cache) =
            swap_client_with_cache(test_config("http://127.0.0.1:1".to_string()));
        let pool = test_pool();
        let order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("S-001"))
            .instrument_id(pool.instrument_id)
            .client_order_id(ClientOrderId::from("O-SWAP-001"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.001"))
            .quote_quantity(true)
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("Quote-denominated"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_unknown_pool() {
        let (mut client, cache) =
            swap_client_with_cache(test_config("http://127.0.0.1:1".to_string()));
        let unknown: InstrumentId = "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45.Arbitrum:UniswapV3"
            .parse()
            .unwrap();
        let order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("S-001"))
            .instrument_id(unknown)
            .client_order_id(ClientOrderId::from("O-SWAP-001"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.001"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("Unknown pool"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_token_pair_outside_allowlist() {
        let mut config = test_config("http://127.0.0.1:1".to_string());
        config.allowed_token_pairs = Some(Vec::new());
        let (mut client, _) = swap_client_with_cache(config);
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied
                .reason
                .as_str()
                .contains("not in the `allowed_token_pairs` allowlist"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_amount_above_max_order_amount() {
        let mut config = test_config("http://127.0.0.1:1".to_string());
        config.max_order_amount = Some(999_999_999_999_999); // 0.001 WETH in raw units minus one
        let (mut client, _) = swap_client_with_cache(config);
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied
                .reason
                .as_str()
                .contains("exceeds the configured `max_order_amount`"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_slippage_param_above_ceiling() {
        let (mut client, _) = swap_client_with_cache(test_config("http://127.0.0.1:1".to_string()));
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut cmd = submit_order_cmd(&order);
        cmd.params = Some(serde_json::from_str(r#"{"slippage_bps": 201}"#).unwrap());
        let mut receiver = start_with_events(&mut client);

        client.submit_order(cmd).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied
                .reason
                .as_str()
                .contains("exceeds the configured `max_slippage_bps`"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_pool_without_fee_tier() {
        let addr = start_mock_rpc_server(MockRpcState::default()).await;
        let mut pool = test_pool();
        pool.fee = None;
        let cache = Rc::new(RefCell::new(Cache::default()));
        cache.borrow_mut().add_pool(pool.clone()).unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
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
        let mut client =
            BlockchainExecutionClient::new(core, test_config(format!("http://{addr}"))).unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("no fee tier"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_without_live_profiler() {
        let addr = start_mock_rpc_server(MockRpcState::default()).await;
        let cache = Rc::new(RefCell::new(Cache::default()));
        let pool = test_pool();
        cache.borrow_mut().add_pool(pool.clone()).unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
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
        let mut client =
            BlockchainExecutionClient::new(core, test_config(format!("http://{addr}"))).unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("No pool profiler"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_when_not_connected() {
        let (mut client, _) = swap_client_with_cache(test_config("http://127.0.0.1:1".to_string()));
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("is not connected"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_without_durable_store() {
        let (mut client, _) = swap_client_with_cache(test_config("http://127.0.0.1:1".to_string()));
        client.core.set_connected();
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied
                .reason
                .as_str()
                .contains("No durable store configured"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_when_transaction_in_flight() {
        let (mut client, _) = swap_client_with_cache(test_config("http://127.0.0.1:1".to_string()));
        client.core.set_connected();
        *client.in_flight.lock().unwrap() =
            Some(InFlightSlot::AwaitingFinality(InFlightTransaction {
                intent_id: 1,
                nonce: 7,
                tx_hash: B256::ZERO,
                purpose: TransactionPurpose::Wrap,
            }));
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("still awaiting finality"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_without_signer() {
        let Some((admin_pool, schema, mut client, _, _)) =
            swap_client_with_database("execution_submit_no_signer_test", swap_rpc_state().await)
                .await
        else {
            return;
        };
        client.signer = None;
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("Signer not initialized"),
            "was: {}",
            denied.reason
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_broadcasts_swap_and_records_client_order_id() {
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_success_test", swap_rpc_state().await)
                .await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);
        let expected_min_out = expected_min_amount_out(50);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);
        let OrderEventAny::Submitted(submitted) = &events[0] else {
            panic!("expected OrderSubmitted, was {:?}", events[0]);
        };
        assert_eq!(submitted.client_order_id, order.client_order_id());

        // The recorded broadcast matches the fully signed expected transaction
        let (expected_hash, expected_raw) = expected_swap_tx(expected_min_out).await;
        let broadcasts: Vec<_> = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .collect();
        assert_eq!(broadcasts.len(), 1);
        assert_eq!(broadcasts[0]["params"][0].as_str().unwrap(), expected_raw);

        let record = client
            .cache
            .get_execution_transaction(42161, &expected_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.nonce, 7);
        assert_eq!(record.purpose, "swap");
        assert_eq!(record.status, "finalized");
        assert_eq!(
            record.client_order_id.as_deref(),
            Some(order.client_order_id().as_str())
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn finalized_swap_emits_exact_fill_once_and_refreshes_wallet() {
        let min_amount_out = expected_min_amount_out(50);
        let (expected_hash, _) = expected_swap_tx(min_amount_out).await;
        let state = finalized_swap_rpc_state(expected_hash, min_amount_out);
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_fill_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        let plan = client
            .prepare_swap(&submit_order_cmd(&order), &order)
            .unwrap();
        execute_swap(
            plan,
            client.transaction_executor().unwrap(),
            client.emitter.clone(),
            client.transaction_limits.max_quote_age_blocks,
            client.transaction_limits.deadline_seconds,
        )
        .await
        .unwrap();

        let (nonce, wallet_address, transaction_to, transaction_input, transaction_value): (
            i64,
            String,
            String,
            String,
            String,
        ) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT nonce, wallet_address, transaction_to, transaction_input, transaction_value \
             FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let finalized_block = client
            .http_rpc_client
            .block_by_number(FIXTURE_BLOCK + 1, true)
            .await
            .unwrap();
        let finalized_transaction = finalized_block
            .transactions
            .iter()
            .find(|transaction| transaction.hash == expected_hash)
            .unwrap();
        assert_eq!(finalized_transaction.from.to_string(), wallet_address);
        assert_eq!(finalized_transaction.nonce, u64::try_from(nonce).unwrap());
        assert_eq!(
            finalized_transaction.to,
            Some(Address::from_str(&transaction_to).unwrap())
        );
        assert_eq!(
            finalized_transaction.input.as_ref(),
            hex::decode(transaction_input.strip_prefix("0x").unwrap()).unwrap()
        );
        assert_eq!(
            finalized_transaction.value,
            U256::from_str(&transaction_value).unwrap()
        );

        let mut order_events = Vec::new();
        let mut account_states = Vec::new();

        while let Ok(event) = receiver.try_recv() {
            match event {
                ExecutionEvent::Order(event) => order_events.push(event),
                ExecutionEvent::Account(state) => account_states.push(state),
                other => panic!("unexpected execution event: {other:?}"),
            }
        }
        assert_eq!(order_events.len(), 2, "was: {order_events:?}");
        assert!(matches!(&order_events[0], OrderEventAny::Submitted(_)));
        let OrderEventAny::Filled(fill) = &order_events[1] else {
            panic!("expected OrderFilled, was {:?}", order_events[1]);
        };
        let expected_commission = Money::from_u256(
            U256::from(50_112_u64) * U256::from(100_000_000_u64),
            test_pool().chain.native_currency(),
        )
        .unwrap();
        assert_eq!(fill.client_order_id, order.client_order_id());
        assert_eq!(fill.venue_order_id.as_str(), expected_hash.to_string());
        assert_eq!(fill.order_side, OrderSide::Sell);
        assert_eq!(fill.last_qty, Quantity::from("0.001"));
        assert_eq!(fill.last_px, Price::from("1000"));
        assert_eq!(fill.currency.code.as_str(), "USDC");
        assert_eq!(fill.commission, Some(expected_commission));
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
        assert_eq!(account_states.len(), 1);
        let account_state = &account_states[0];
        assert_eq!(account_state.account_id, AccountId::from("BLOCKCHAIN-001"));
        assert_eq!(account_state.account_type, AccountType::Wallet);
        assert_eq!(account_state.base_currency, None);
        assert_eq!(account_state.balances.len(), 3);
        assert!(account_state.margins.is_empty());
        assert!(account_state.is_reported);
        assert_eq!(
            account_state.balances,
            client
                .wallet_balance
                .lock()
                .unwrap()
                .as_account_balances()
                .unwrap()
        );

        let (fill_emitted, active): (bool, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT fill_emitted, active FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert!(fill_emitted);
        assert!(!active);

        let database = client.cache.database.as_ref().unwrap().clone();
        let restart_config = client.config.clone();
        drop(client);
        let (mut restarted, _) = swap_client_with_cache(restart_config);
        restarted.cache.database = Some(database);
        restarted.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        let mut restart_receiver = start_with_events(&mut restarted);
        restarted.reconcile_unresolved_execution().await.unwrap();
        restarted.reconcile_unresolved_execution().await.unwrap();
        assert!(collect_order_events(&mut restart_receiver).is_empty());
        let requests = state.recorded_requests();
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
                .filter(|request| {
                    request["method"] == "eth_call"
                        && request["params"][0]["data"]
                            .as_str()
                            .is_some_and(|data| data.starts_with(BALANCE_OF_SELECTOR))
                })
                .count(),
            3
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_getBalance")
                .count(),
            2
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn finalized_swap_without_log_rejects_and_releases_ownership() {
        let state = swap_rpc_state()
            .await
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS);
        let Some((admin_pool, schema, mut client, _, _)) =
            swap_client_with_database("execution_submit_missing_log_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        let (status, terminal_emitted, active): (String, bool, bool) =
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT status, terminal_emitted, active FROM {schema}.execution_intent"
            )))
            .fetch_one(&admin_pool)
            .await
            .unwrap();

        assert_missing_swap_log_rejected(&events);
        assert_eq!(status, "finalized");
        assert!(terminal_emitted);
        assert!(!active);
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn finalized_swap_refresh_failure_stays_owned_for_reconciliation() {
        let min_amount_out = expected_min_amount_out(50);
        let (expected_hash, _) = expected_swap_tx(min_amount_out).await;
        let state = finalized_swap_rpc_state(expected_hash, min_amount_out)
            .with_response_sequence("eth_getBalance", &[GET_BALANCE, RPC_METHOD_NOT_FOUND]);
        let Some((admin_pool, schema, mut client, _state, _)) =
            swap_client_with_database("execution_submit_refresh_fail_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);
        let plan = client
            .prepare_swap(&submit_order_cmd(&order), &order)
            .unwrap();

        let error = execute_swap(
            plan,
            client.transaction_executor().unwrap(),
            client.emitter.clone(),
            client.transaction_limits.max_quote_age_blocks,
            client.transaction_limits.deadline_seconds,
        )
        .await
        .unwrap_err();
        let events = collect_order_events(&mut receiver);
        let (status, fill_emitted, active): (String, bool, bool) =
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT status, fill_emitted, active FROM {schema}.execution_intent"
            )))
            .fetch_one(&admin_pool)
            .await
            .unwrap();

        assert!(
            error.to_string().contains("RPC error -32601"),
            "was: {error}"
        );
        assert_eq!(events.len(), 2, "was: {events:?}");
        assert!(matches!(&events[0], OrderEventAny::Submitted(_)));
        assert!(matches!(&events[1], OrderEventAny::Filled(_)));
        assert_eq!(status, "finalized");
        assert!(!fill_emitted);
        assert!(active);
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_on_insufficient_router_allowance() {
        let state = swap_rpc_state()
            .await
            .with_call_response(ALLOWANCE_SELECTOR, CALL_ZERO);
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_allowance_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("below the swap amount"),
            "was: {}",
            denied.reason
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction"),
            "no broadcast may follow a pre-trade denial"
        );
        let row_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(row_count, 0);
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_on_insufficient_input_balance() {
        let state = swap_rpc_state()
            .await
            .with_call_response(BALANCE_OF_SELECTOR, CALL_ZERO);
        let Some((admin_pool, schema, mut client, _, _)) =
            swap_client_with_database("execution_submit_balance_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("is below the swap amount"),
            "was: {}",
            denied.reason
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_on_insufficient_native_balance() {
        let state = swap_rpc_state()
            .await
            .with_response("eth_getBalance", GET_BALANCE_INSUFFICIENT);
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_native_balance_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied
                .reason
                .as_str()
                .contains("below maximum transaction cost"),
            "was: {}",
            denied.reason
        );
        assert!(
            state
                .recorded_requests()
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction"),
            "no broadcast may follow an insufficient native balance"
        );
        let (row_count, status): (i64, Option<String>) = sqlx::query_as(sqlx::AssertSqlSafe(
            format!("SELECT COUNT(*), MAX(status) FROM {schema}.execution_intent"),
        ))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(row_count, 1);
        assert_eq!(status.as_deref(), Some("recoverable"));
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_on_stale_quote() {
        let Some((admin_pool, schema, mut client, state, cache)) =
            swap_client_with_database("execution_submit_stale_quote_test", swap_rpc_state().await)
                .await
        else {
            return;
        };
        let pool = test_pool();
        cache
            .borrow_mut()
            .add_pool_profiler(test_profiler(&pool, FIXTURE_BLOCK - 101))
            .unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("Stale quote"),
            "was: {}",
            denied.reason
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction"),
            "no broadcast may follow a pre-trade denial"
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_quote_ahead_of_chain_head() {
        let Some((admin_pool, schema, mut client, state, cache)) =
            swap_client_with_database("execution_submit_ahead_quote_test", swap_rpc_state().await)
                .await
        else {
            return;
        };
        let pool = test_pool();
        cache
            .borrow_mut()
            .add_pool_profiler(test_profiler(&pool, FIXTURE_BLOCK + 1))
            .unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied
                .reason
                .as_str()
                .contains("is ahead of the latest block"),
            "was: {}",
            denied.reason
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction"),
            "no broadcast may follow a pre-trade denial"
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_on_deadline_overflow() {
        let Some((admin_pool, schema, mut client, state, _)) = swap_client_with_database(
            "execution_submit_deadline_overflow_test",
            swap_rpc_state().await,
        )
        .await
        else {
            return;
        };
        client.transaction_limits.deadline_seconds = u64::MAX;
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("deadline overflow"),
            "was: {}",
            denied.reason
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction"),
            "no broadcast may follow a pre-trade denial"
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_reconciles_node_rejection_to_finalized_receipt() {
        let state = swap_rpc_state()
            .await
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION_REJECTED);
        let Some((admin_pool, schema, mut client, _state, _)) =
            swap_client_with_database("execution_submit_node_rejected_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);
        let (purpose, status, client_order_id): (String, String, Option<String>) =
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT purpose, status, client_order_id FROM {schema}.execution_intent"
            )))
            .fetch_one(&admin_pool)
            .await
            .unwrap();
        assert_eq!(purpose, "swap");
        assert_eq!(status, "finalized");
        assert_eq!(
            client_order_id.as_deref(),
            Some(order.client_order_id().as_str())
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_acknowledges_uncertain_nonce_too_low() {
        let state = swap_rpc_state()
            .await
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION_NONCE_TOO_LOW);
        let Some((admin_pool, schema, mut client, _state, _)) =
            swap_client_with_database("execution_submit_nonce_too_low_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);
        let (purpose, status, client_order_id): (String, String, Option<String>) =
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT purpose, status, client_order_id FROM {schema}.execution_intent"
            )))
            .fetch_one(&admin_pool)
            .await
            .unwrap();
        assert_eq!(purpose, "swap");
        assert_eq!(status, "finalized");
        assert_eq!(
            client_order_id.as_deref(),
            Some(order.client_order_id().as_str())
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_rejected_on_reverted_receipt() {
        let state = swap_rpc_state()
            .await
            .with_response("eth_getTransactionReceipt", RECEIPT_REVERTED);
        let Some((admin_pool, schema, mut client, _state, _)) =
            swap_client_with_database("execution_submit_reverted_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);
        let expected_min_out = expected_min_amount_out(50);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], OrderEventAny::Submitted(_)),
            "was: {:?}",
            events[0]
        );
        let OrderEventAny::Rejected(rejected) = &events[1] else {
            panic!("expected OrderRejected, was {:?}", events[1]);
        };
        assert!(
            rejected.reason.as_str().contains("reverted on-chain"),
            "was: {}",
            rejected.reason
        );

        let (expected_hash, _) = expected_swap_tx(expected_min_out).await;
        let record = client
            .cache
            .get_execution_transaction(42161, &expected_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.purpose, "swap");
        assert_eq!(record.status, "reverted");
        assert_eq!(
            record.client_order_id.as_deref(),
            Some(order.client_order_id().as_str())
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_acknowledges_ambiguous_broadcast() {
        // A response the client cannot parse classifies as an ambiguous broadcast failure:
        // the transaction may be live, so the durable intent remains submitted and occupied
        let state = swap_rpc_state()
            .await
            .with_response("eth_sendRawTransaction", "not json");
        let Some((admin_pool, schema, mut client, _state, _)) =
            swap_client_with_database("execution_submit_ambiguous_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);

        let record = client
            .cache
            .get_execution_transaction(
                42161,
                &expected_swap_tx(expected_min_amount_out(50))
                    .await
                    .0
                    .to_string(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.purpose, "swap");
        assert_eq!(record.status, "finalized");
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_acknowledges_broadcast_hash_mismatch() {
        // The static fixture's hash differs from the locally signed hash, so acceptance of the
        // signed transaction is unverified until the independent receipt reaches finality
        let state = swap_rpc_state_for_mismatch().await;
        let Some((admin_pool, schema, mut client, _state, _)) =
            swap_client_with_database("execution_submit_hash_mismatch_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);

        let record = client
            .cache
            .get_execution_transaction(
                42161,
                &expected_swap_tx(expected_min_amount_out(50))
                    .await
                    .0
                    .to_string(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.purpose, "swap");
        assert_eq!(record.status, "finalized");
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_persist_failure_denies_without_broadcast() {
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_persist_fail_test", swap_rpc_state().await)
                .await
        else {
            return;
        };
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP TABLE {schema}.execution_intent CASCADE"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied
                .reason
                .as_str()
                .contains("Failed to reserve execution intent"),
            "was: {}",
            denied.reason
        );
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        assert_eq!(broadcasts, 0);
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn execution_fresh_schema_preserves_nullable_wallet_address() {
        let Some((admin_pool, _)) = connect_test_postgres("fresh execution schema").await else {
            return;
        };
        let schema = format!("execution_fresh_test_{}", std::process::id());
        let mut transaction = admin_pool.begin().await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SET LOCAL search_path TO {schema}"
        )))
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE chain (chain_id INTEGER PRIMARY KEY)")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("INSERT INTO chain (chain_id) VALUES (42161)")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(execution_transaction_create_sql()))
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO execution_transaction \
             (chain_id, nonce, transaction_hash, purpose, status) \
             VALUES (42161, 7, '0xfresh-legacy', 'wrap', 'rejected')",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();

        let is_nullable: String = sqlx::query_scalar(
            "SELECT is_nullable FROM information_schema.columns \
             WHERE table_schema = $1 AND table_name = 'execution_transaction' \
             AND column_name = 'wallet_address'",
        )
        .bind(&schema)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        let wallet_address: Option<String> = sqlx::query_scalar(
            "SELECT wallet_address FROM execution_transaction \
             WHERE transaction_hash = '0xfresh-legacy'",
        )
        .fetch_one(&mut *transaction)
        .await
        .unwrap();

        assert_eq!(is_nullable, "YES");
        assert_eq!(wallet_address, None);
        transaction.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn execution_schema_migration_preserves_existing_rows() {
        let Some((admin_pool, pg_config)) =
            connect_test_postgres("execution schema migration").await
        else {
            return;
        };
        let schema = format!("execution_migration_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {schema}.execution_transaction \
             (chain_id, nonce, transaction_hash, purpose, status) \
             VALUES (42161, 7, '0xlegacy', 'wrap', 'rejected')"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = BlockchainCacheDatabase::connect(db_options).await.unwrap();
        database
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        let legacy = database
            .get_execution_transaction(42161, "0xlegacy")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(legacy.purpose, "wrap");
        assert_eq!(legacy.status, "rejected");
        assert_eq!(legacy.client_order_id, None);
        assert_eq!(legacy.wallet_address, None);
        let is_nullable: String = sqlx::query_scalar(
            "SELECT is_nullable FROM information_schema.columns \
             WHERE table_schema = $1 AND table_name = 'execution_transaction' \
             AND column_name = 'wallet_address'",
        )
        .bind(&schema)
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let fence_error = database
            .add_execution_transaction(
                42161,
                WALLET,
                8,
                "0xswap",
                "swap",
                "pending",
                Some("O-SWAP-001"),
            )
            .await
            .unwrap_err();
        assert!(
            fence_error
                .to_string()
                .contains("Legacy execution writer refused"),
            "was: {fence_error}"
        );
        assert_eq!(is_nullable, "YES");

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn execution_schema_migration_drops_legacy_wallet_not_null() {
        let Some((admin_pool, pg_config)) =
            connect_test_postgres("legacy wallet address schema migration").await
        else {
            return;
        };
        let schema = format!("execution_wallet_migration_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {schema}.execution_transaction \
             ADD COLUMN wallet_address TEXT NOT NULL"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {schema}.execution_transaction \
             (chain_id, wallet_address, nonce, transaction_hash, purpose, status) \
             VALUES (42161, '{WALLET}', 7, '0xlegacy-wallet', 'wrap', 'rejected')"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = BlockchainCacheDatabase::connect(db_options).await.unwrap();
        database
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        let legacy = database
            .get_execution_transaction(42161, "0xlegacy-wallet")
            .await
            .unwrap()
            .unwrap();
        let is_nullable: String = sqlx::query_scalar(
            "SELECT is_nullable FROM information_schema.columns \
             WHERE table_schema = $1 AND table_name = 'execution_transaction' \
             AND column_name = 'wallet_address'",
        )
        .bind(&schema)
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert_eq!(legacy.wallet_address.as_deref(), Some(WALLET));
        assert_eq!(is_nullable, "YES");
        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn execution_schema_migration_refuses_unresolved_legacy_rows() {
        let Some((admin_pool, pg_config)) =
            connect_test_postgres("unsafe execution schema migration").await
        else {
            return;
        };
        let schema = format!("execution_unsafe_migration_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {schema}.execution_transaction \
             (chain_id, nonce, transaction_hash, purpose, status) \
             VALUES (42161, 7, '0xunresolved', 'wrap', 'pending')"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();
        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = BlockchainCacheDatabase::connect(db_options).await.unwrap();

        let error = database
            .ensure_execution_transaction_schema()
            .await
            .unwrap_err();
        let legacy_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction \
             WHERE transaction_hash = '0xunresolved' AND status = 'pending'"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let v2_table: Option<String> = sqlx::query_scalar("SELECT to_regclass($1)::TEXT")
            .bind(format!("{schema}.execution_intent"))
            .fetch_one(&admin_pool)
            .await
            .unwrap();

        assert!(
            error
                .to_string()
                .contains("Cannot safely migrate 1 unresolved execution schema version 1"),
            "was: {error}"
        );
        assert_eq!(legacy_count, 1);
        assert_eq!(v2_table, None);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[rstest]
    fn receipt_max_polls_derives_from_timeout() {
        assert_eq!(receipt_max_polls(0), 1);
        assert_eq!(receipt_max_polls(1), 1);
        assert_eq!(receipt_max_polls(60), 60);
        assert_eq!(receipt_max_polls(u64::MAX), u32::MAX);
        assert_eq!(receipt_timeout(0), Duration::from_secs(1));
        assert_eq!(receipt_timeout(60), Duration::from_secs(60));
        assert_eq!(
            receipt_timeout(u64::MAX),
            Duration::from_secs(u64::from(u32::MAX))
        );
    }

    #[rstest]
    fn submit_order_errors_when_order_not_cached() {
        let client = swap_client_with_cache(test_config("http://127.0.0.1:1".to_string())).0;
        let mut cmd = submit_order_cmd(&test_market_sell_order(test_pool().instrument_id));
        cmd.client_order_id = ClientOrderId::from("O-UNKNOWN");

        let error = client.submit_order(cmd).unwrap_err();

        assert!(!error.to_string().is_empty());
    }

    #[tokio::test]
    async fn submit_order_denies_pool_fee_above_uint24() {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut pool = test_pool();
        pool.fee = Some(16_777_216); // 2^24, above the uint24 calldata range
        cache.borrow_mut().add_pool(pool.clone()).unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
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
        let mut client =
            BlockchainExecutionClient::new(core, test_config("http://127.0.0.1:1".to_string()))
                .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("exceeds uint24"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_uninitialized_profiler() {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let pool = test_pool();
        cache.borrow_mut().add_pool(pool.clone()).unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_pool_profiler(PoolProfiler::new(Arc::new(pool)))
            .unwrap();
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
        let mut client =
            BlockchainExecutionClient::new(core, test_config("http://127.0.0.1:1".to_string()))
                .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("is not initialized"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_when_quote_cannot_fill_order() {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let pool = test_pool();
        cache.borrow_mut().add_pool(pool.clone()).unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        // A narrow one-unit-of-liquidity position cannot fill the order
        cache
            .borrow_mut()
            .add_pool_profiler(test_profiler_with_range(
                &pool,
                FIXTURE_BLOCK,
                U160::from(1u128 << 96),
                -10,
                10,
                1,
            ))
            .unwrap();
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
        let mut client =
            BlockchainExecutionClient::new(core, test_config("http://127.0.0.1:1".to_string()))
                .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("cannot fill the order"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_applies_slippage_param_override_at_ceiling() {
        let Some((admin_pool, schema, mut client, state, _)) = swap_client_with_database(
            "execution_submit_slippage_override_test",
            swap_rpc_state_with_min_amount_out(expected_min_amount_out(200)).await,
        )
        .await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut cmd = submit_order_cmd(&order);
        // Exactly at the configured ceiling (200 bps): accepted, not denied
        cmd.params = Some(serde_json::from_str(r#"{"slippage_bps": 200}"#).unwrap());
        let mut receiver = start_with_events(&mut client);
        let expected_min_out = expected_min_amount_out(200);

        client.submit_order(cmd).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);

        let (_, expected_raw) = expected_swap_tx(expected_min_out).await;
        let broadcasts: Vec<_> = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .collect();
        assert_eq!(broadcasts.len(), 1);
        assert_eq!(broadcasts[0]["params"][0].as_str().unwrap(), expected_raw);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_accepts_quote_fresh_at_max_age_boundary() {
        let Some((admin_pool, schema, mut client, _state, cache)) = swap_client_with_database(
            "execution_submit_fresh_boundary_test",
            swap_rpc_state().await,
        )
        .await
        else {
            return;
        };
        let pool = test_pool();
        cache
            .borrow_mut()
            .add_pool_profiler(test_profiler(&pool, FIXTURE_BLOCK - 100))
            .unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_sells_base_token_from_token1_position() {
        // A pool with the token order reversed relative to the convention: USDC is token0
        // and WETH is token1, so the base token (WETH, by token priority) sits in the
        // token1 position and the swap quotes zero_for_one = false
        let chain = Arc::new(chains::ARBITRUM.clone());
        let dex = UNISWAP_V3.dex.clone();
        let usdc = Token::new(
            chain.clone(),
            address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
            "USD Coin".to_string(),
            "USDC".to_string(),
            6,
        );
        let weth = Token::new(
            chain.clone(),
            address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            "Wrapped Ether".to_string(),
            "WETH".to_string(),
            18,
        );
        let pool = Pool::new(
            chain,
            dex,
            address!("C6962004f452bE9203591991D15f6b388e09E8D0"),
            PoolIdentifier::from_address(address!("C6962004f452bE9203591991D15f6b388e09E8D0")),
            55_000_000,
            usdc,
            weth,
            Some(500),
            Some(10),
            UnixNanos::default(),
        );

        let cache = Rc::new(RefCell::new(Cache::default()));
        cache.borrow_mut().add_pool(pool.clone()).unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        // Asymmetric price so the zero_for_one direction changes the quote
        cache
            .borrow_mut()
            .add_pool_profiler(test_profiler_with_state(
                &pool,
                FIXTURE_BLOCK,
                U160::from(2u128 << 96),
                TEST_LIQUIDITY,
            ))
            .unwrap();

        let (admin_pool, pg_config) = match connect_test_postgres("orientation").await {
            Some(setup) => setup,
            None => return,
        };
        let schema = format!("execution_submit_orientation_test_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;
        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();

        let state = swap_rpc_state()
            .await
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL);
        let addr = start_mock_rpc_server(state.clone()).await;
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
        let mut client =
            BlockchainExecutionClient::new(core, test_config(format!("http://{addr}"))).unwrap();
        client.cache.database = Some(database);
        client
            .cache
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();
        let mut receiver = start_with_events(&mut client);

        let profiler = test_profiler_with_state(
            &pool,
            FIXTURE_BLOCK,
            U160::from(2u128 << 96),
            TEST_LIQUIDITY,
        );
        let quote = profiler
            .swap_exact_in(U256::from(1_000_000_000_000_000u64), false, None)
            .unwrap();
        let quoted = exact_output_amount(&quote, false).unwrap();
        let expected_min_out = derive_min_amount_out(quoted, 50).unwrap();
        let wrong_direction_quote = profiler
            .swap_exact_in(U256::from(1_000_000_000_000_000u64), true, None)
            .unwrap();
        let wrong_direction_out = exact_output_amount(&wrong_direction_quote, true).unwrap();
        assert_ne!(
            quoted, wrong_direction_out,
            "the asymmetric price must make the quote direction observable"
        );

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], OrderEventAny::Submitted(_)),
            "was: {:?}",
            events[0]
        );

        let (_, expected_raw) = expected_swap_tx(expected_min_out).await;
        let broadcasts: Vec<_> = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .collect();
        assert_eq!(broadcasts.len(), 1);
        assert_eq!(broadcasts[0]["params"][0].as_str().unwrap(), expected_raw);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_on_chain_mismatch() {
        let state = swap_rpc_state()
            .await
            .with_response("eth_chainId", CHAIN_ID_ETHEREUM);
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_chain_mismatch_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("Chain ID mismatch"),
            "was: {}",
            denied.reason
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction"),
            "no broadcast may follow a chain mismatch"
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_finality_timeout_marks_dropped_and_keeps_ownership() {
        let state = swap_rpc_state()
            .await
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_sleep("eth_getTransactionReceipt", Duration::from_secs(30));
        let Some((admin_pool, schema, mut client, _state, _)) =
            swap_client_with_database("execution_submit_inclusion_timeout_test", state).await
        else {
            return;
        };
        // The wall-clock timeout must cap a stalled receipt RPC.
        client.transaction_limits.receipt_timeout_secs = 1;
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);
        let started = tokio::time::Instant::now();

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;
        assert!(started.elapsed() < Duration::from_secs(3));

        // Broadcast acceptance was observed, so the order is submitted; the unobserved
        // inclusion justifies no further event
        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], OrderEventAny::Submitted(_)),
            "was: {:?}",
            events[0]
        );

        let record = client
            .cache
            .get_execution_transaction(
                42161,
                &expected_swap_tx(expected_min_amount_out(50))
                    .await
                    .0
                    .to_string(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.status, "dropped");
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn included_receipt_finality_timeout_marks_dropped_and_keeps_ownership() {
        let state = execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_call", CALL_BALANCE)
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_parameter_response("eth_getBlockByNumber", "finalized", BLOCK_BY_NUMBER)
            .with_send_raw_transaction_echo();
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_included_timeout_test", state).await
        else {
            return;
        };
        client.transaction_limits.receipt_timeout_secs = 1;

        let error = client
            .wrap(U256::from(1_000_000_000_000_000_u64))
            .await
            .unwrap_err();
        let transitions: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT to_status FROM {schema}.execution_transaction_transition ORDER BY id"
        )))
        .fetch_all(&admin_pool)
        .await
        .unwrap();

        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        assert_eq!(
            transitions,
            ["prepared", "signed", "broadcast", "included", "dropped"]
        );
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_single_in_flight_rejects_concurrent_swap() {
        let state = swap_rpc_state()
            .await
            .with_sleep("eth_sendRawTransaction", Duration::from_millis(500));
        let Some((admin_pool, schema, mut client, state, cache)) =
            swap_client_with_database("execution_submit_concurrent_test", state).await
        else {
            return;
        };
        let pool = test_pool();
        let first = test_market_sell_order(pool.instrument_id);
        let second = OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("S-001"))
            .instrument_id(pool.instrument_id)
            .client_order_id(ClientOrderId::from("O-SWAP-002"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.001"))
            .build();
        cache
            .borrow_mut()
            .add_order(second.clone(), None, None, false)
            .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&first)).unwrap();
        client.submit_order(submit_order_cmd(&second)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        let submitted = events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Submitted(_)))
            .count();
        let denied_in_flight = events
            .iter()
            .filter(|event| {
                matches!(event, OrderEventAny::Denied(denied) if denied.reason.as_str().contains("at most one transaction can be in flight"))
            })
            .count();
        assert_eq!(submitted, 1, "was: {events:?}");
        assert_eq!(denied_in_flight, 1, "was: {events:?}");

        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        assert_eq!(broadcasts, 1);
        let row_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(row_count, 1);
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
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
    #[case::allowed_token_pairs("allowed_token_pairs")]
    #[case::slippage_bps("slippage_bps")]
    #[case::max_slippage_bps("max_slippage_bps")]
    #[case::max_order_amount("max_order_amount")]
    #[case::deadline_seconds("deadline_seconds")]
    #[case::max_quote_age_blocks("max_quote_age_blocks")]
    #[case::receipt_timeout_secs("receipt_timeout_secs")]
    fn new_rejects_each_missing_transaction_limit(#[case] missing: &str) {
        let mut config = test_config("http://127.0.0.1:1".to_string());
        match missing {
            "allowed_token_pairs" => config.allowed_token_pairs = None,
            "slippage_bps" => config.slippage_bps = None,
            "max_slippage_bps" => config.max_slippage_bps = None,
            "max_order_amount" => config.max_order_amount = None,
            "deadline_seconds" => config.deadline_seconds = None,
            "max_quote_age_blocks" => config.max_quote_age_blocks = None,
            "receipt_timeout_secs" => config.receipt_timeout_secs = None,
            _ => unreachable!(),
        }

        let error = test_client_result(config, test_pool()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Blockchain execution transaction limits are required: allowed_token_pairs, slippage_bps, max_slippage_bps, max_order_amount, deadline_seconds, max_quote_age_blocks, receipt_timeout_secs"
        );
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
                WETH_ADDRESS,
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
        *client.in_flight.lock().unwrap() =
            Some(InFlightSlot::AwaitingFinality(InFlightTransaction {
                intent_id: 1,
                nonce: 7,
                tx_hash: B256::ZERO,
                purpose: TransactionPurpose::Wrap,
            }));

        let error = client.wrap(U256::from(1_000u64)).await.unwrap_err();

        assert!(
            error.to_string().contains("still awaiting finality"),
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
        assert!(client.in_flight.lock().unwrap().is_none());
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
        assert!(client.in_flight.lock().unwrap().is_none());
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["method"], "eth_getCode");
        assert_eq!(requests[1]["method"], "eth_call");

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn wrap_rejects_included_transaction_without_balance_delta() {
        let state = broadcast_rpc_state()
            .with_response_sequence("eth_call", &[CALL_BALANCE, CALL_BALANCE, CALL_BALANCE]);
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
        assert!(client.in_flight.lock().unwrap().is_none());
        let status: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT status FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(status, "finalized");
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
        let state = broadcast_rpc_state().with_response_sequence(
            "eth_call",
            &[CALL_BALANCE, CALL_BALANCE, RPC_METHOD_NOT_FOUND],
        );
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
        assert!(client.in_flight.lock().unwrap().is_none());

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
            .approve(WETH_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("returned false"), "was: {error}");
        assert!(client.in_flight.lock().unwrap().is_none());
        let requests = state.recorded_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["method"], "eth_getCode");
        assert_eq!(requests[1]["method"], "eth_call");
        assert_eq!(requests[1]["params"][0]["from"], WALLET);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_accepts_empty_return_with_sufficient_allowance() {
        let state =
            broadcast_rpc_state().with_response_sequence("eth_call", &[CALL_EMPTY, CALL_ALLOWANCE]);
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_approve_empty_test", state).await
        else {
            return;
        };

        let tx_hash = client
            .approve(WETH_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap();

        let record = client
            .cache
            .get_execution_transaction(42161, &tx_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.purpose, "approve");
        assert_eq!(record.status, "finalized");
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_rejects_empty_return_with_insufficient_allowance() {
        let state =
            broadcast_rpc_state().with_response_sequence("eth_call", &[CALL_EMPTY, CALL_ZERO]);
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_approve_insufficient_test", state).await
        else {
            return;
        };

        let error = client
            .approve(WETH_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("below the requested amount"),
            "was: {error}"
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_reports_inclusion_when_postcondition_read_fails() {
        let state = broadcast_rpc_state()
            .with_response_sequence("eth_call", &[CALL_BOOL_TRUE, RPC_METHOD_NOT_FOUND]);
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_approve_postcondition_rpc_test", state).await
        else {
            return;
        };

        let error = client
            .approve(WETH_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("failed to read router allowance after included transaction 0x"),
            "was: {message}"
        );
        assert!(message.contains("at block 30346561"), "was: {message}");
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn wallet_balance_refresh_replaces_complete_snapshot_with_exact_precision() {
        let state = execution_rpc_state()
            .with_response_sequence("eth_getBalance", &[GET_BALANCE, GET_BALANCE_ZERO])
            .with_response_sequence(
                "eth_call",
                &[
                    CALL_BALANCE_WETH,
                    CALL_BALANCE_USDC,
                    CALL_BALANCE_WETH_UPDATED,
                    CALL_BALANCE_USDC_UPDATED,
                ],
            );
        let (mut client, _, _) =
            client_with_token_mock_rpc(state, "BLOCKCHAIN_TEST_BALANCE_REPLACE").await;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        client.start().unwrap();

        client.refresh_wallet_balances().await.unwrap();
        let balances = client
            .wallet_balance
            .lock()
            .unwrap()
            .as_account_balances()
            .unwrap();

        assert_eq!(balances.len(), 3);
        assert_eq!(balances[0].currency.code.as_str(), "ETH");
        assert_eq!(balances[0].currency.name.as_str(), "Ethereum");
        assert_eq!(balances[0].currency.precision, 18);
        assert_eq!(balances[0].total.raw, 1_000_000_000_000_000_000);
        assert_eq!(balances[0].free, balances[0].total);
        assert_eq!(balances[0].locked, Money::zero(balances[0].currency));
        assert_eq!(balances[1].currency.code.as_str(), "WETH");
        assert_eq!(balances[1].currency.name.as_str(), "Wrapped Ether");
        assert_eq!(balances[1].currency.precision, 18);
        assert_eq!(balances[1].total.raw, 1_234_567_890_123_456_789);
        assert_eq!(balances[1].free, balances[1].total);
        assert_eq!(balances[1].locked, Money::zero(balances[1].currency));
        assert_eq!(balances[2].currency.code.as_str(), "USDC");
        assert_eq!(balances[2].currency.name.as_str(), "USD Coin");
        assert_eq!(balances[2].currency.precision, 6);
        assert_eq!(balances[2].total.raw, 9_876_543_210_000_000_000);
        assert_eq!(balances[2].free, balances[2].total);
        assert_eq!(balances[2].locked, Money::zero(balances[2].currency));

        client.refresh_wallet_balances().await.unwrap();
        let balances = client
            .wallet_balance
            .lock()
            .unwrap()
            .as_account_balances()
            .unwrap();

        assert_eq!(balances.len(), 3);
        assert_eq!(
            client.wallet_balance.lock().unwrap().token_balances.len(),
            2
        );
        assert_eq!(balances[0].total.raw, 0);
        assert_eq!(balances[1].total.raw, 2_000_000_000_000_000_000);
        assert_eq!(balances[2].total.raw, 12_345_670_000_000_000);
    }

    #[allow(unsafe_code)] // env-var mutation in tests; unique var names avoid cross-test races
    #[tokio::test]
    async fn failed_connect_refresh_retains_snapshot_and_publishes_nothing() {
        let state = execution_rpc_state()
            .with_response_sequence("eth_getBalance", &[GET_BALANCE, GET_BALANCE_ZERO])
            .with_response_sequence(
                "eth_call",
                &[
                    CALL_BALANCE_WETH,
                    CALL_BALANCE_USDC,
                    CALL_BALANCE_WETH_UPDATED,
                    RPC_METHOD_NOT_FOUND,
                ],
            );
        let (mut client, _, _) =
            client_with_token_mock_rpc(state, "BLOCKCHAIN_TEST_BALANCE_ATOMIC").await;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        client.start().unwrap();
        client.refresh_wallet_balances().await.unwrap();
        let retained = client
            .wallet_balance
            .lock()
            .unwrap()
            .as_account_balances()
            .unwrap();
        receiver.try_recv().unwrap();
        // SAFETY: this variable name is unique to this test across the test binary
        unsafe { std::env::set_var("BLOCKCHAIN_TEST_BALANCE_ATOMIC", TEST_PRIVATE_KEY) };

        let error = client.connect().await.unwrap_err();
        let failed_address = Address::from_str(USDC).unwrap();

        assert!(
            error.to_string().contains(&format!(
                "failed to fetch token balance for {failed_address}"
            )),
            "was: {error}"
        );
        assert!(!client.is_connected());
        assert!(client.signer.is_none());
        assert_eq!(
            client
                .wallet_balance
                .lock()
                .unwrap()
                .as_account_balances()
                .unwrap(),
            retained
        );
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[allow(unsafe_code)] // env-var mutation in tests; unique var names avoid cross-test races
    #[tokio::test]
    async fn failed_connect_publication_retains_snapshot() {
        let state = execution_rpc_state()
            .with_response_sequence("eth_getBalance", &[GET_BALANCE, GET_BALANCE_ZERO])
            .with_response_sequence(
                "eth_call",
                &[
                    CALL_BALANCE_WETH,
                    CALL_BALANCE_USDC,
                    CALL_BALANCE_WETH_UPDATED,
                    CALL_BALANCE_USDC_UPDATED,
                ],
            );
        let (mut client, _, _) =
            client_with_token_mock_rpc(state, "BLOCKCHAIN_TEST_BALANCE_PUBLICATION").await;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        client.start().unwrap();
        client.refresh_wallet_balances().await.unwrap();
        let retained = client
            .wallet_balance
            .lock()
            .unwrap()
            .as_account_balances()
            .unwrap();
        receiver.try_recv().unwrap();
        drop(receiver);
        // SAFETY: this variable name is unique to this test across the test binary
        unsafe { std::env::set_var("BLOCKCHAIN_TEST_BALANCE_PUBLICATION", TEST_PRIVATE_KEY) };

        let error = client.connect().await.unwrap_err();

        assert!(
            error.to_string().contains("Failed to send account state"),
            "was: {error}"
        );
        assert!(!client.is_connected());
        assert!(client.signer.is_none());
        assert_eq!(
            client
                .wallet_balance
                .lock()
                .unwrap()
                .as_account_balances()
                .unwrap(),
            retained
        );
    }

    #[allow(unsafe_code)] // env-var mutation in tests; unique var names avoid cross-test races
    #[tokio::test]
    async fn connect_and_repeated_query_publish_wallet_account_state() {
        let state = execution_rpc_state()
            .with_response_sequence("eth_call", &[CALL_BALANCE_WETH, CALL_BALANCE_USDC]);
        let (mut client, state, cache) =
            client_with_token_mock_rpc(state, "BLOCKCHAIN_TEST_ACCOUNT_STATE").await;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        client.start().unwrap();
        // SAFETY: this variable name is unique to this test across the test binary
        unsafe { std::env::set_var("BLOCKCHAIN_TEST_ACCOUNT_STATE", TEST_PRIVATE_KEY) };

        client.connect().await.unwrap();

        let ExecutionEvent::Account(connected) = receiver.try_recv().unwrap() else {
            panic!("expected account state event")
        };
        assert_eq!(connected.account_id, AccountId::from("BLOCKCHAIN-001"));
        assert_eq!(connected.account_type, AccountType::Wallet);
        assert_eq!(connected.base_currency, None);
        assert_eq!(connected.balances.len(), 3);
        assert!(connected.margins.is_empty());
        assert!(connected.is_reported);
        cache.borrow_mut().update_account_state(&connected).unwrap();

        let account = client.get_account().unwrap();
        assert!(matches!(account, AccountAny::Wallet(_)));
        assert_eq!(account.id(), AccountId::from("BLOCKCHAIN-001"));
        assert_eq!(account.last_event(), Some(connected.clone()));

        let requests_before = state.recorded_requests().len();
        let query = || {
            QueryAccount::new(
                TraderId::from("TRADER-001"),
                Some(ClientId::from("BLOCKCHAIN-001")),
                AccountId::from("BLOCKCHAIN-001"),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        };
        client.query_account(query()).unwrap();
        client.query_account(query()).unwrap();

        let ExecutionEvent::Account(first_query) = receiver.try_recv().unwrap() else {
            panic!("expected first query account state event")
        };
        let ExecutionEvent::Account(second_query) = receiver.try_recv().unwrap() else {
            panic!("expected second query account state event")
        };
        assert!(connected.has_same_balances_and_margins(&first_query));
        assert!(first_query.has_same_balances_and_margins(&second_query));
        assert_eq!(first_query.account_id, connected.account_id);
        assert_eq!(first_query.account_type, connected.account_type);
        assert_eq!(first_query.base_currency, connected.base_currency);
        assert_eq!(first_query.is_reported, connected.is_reported);
        assert_ne!(first_query.event_id, second_query.event_id);
        assert_eq!(state.recorded_requests().len(), requests_before);

        client.stop().unwrap();
        assert!(client.core.is_stopped());
        assert!(!client.is_connected());
        assert!(client.signer.is_none());
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
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        client.start().unwrap();
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

    #[allow(unsafe_code)] // env-var mutation in tests; unique var names avoid cross-test races
    #[tokio::test]
    async fn connect_releases_stale_preparing_claim_after_tasks_finish() {
        let addr = start_mock_rpc_server(ready_rpc_state()).await;
        let config = test_config_with_signer_env(
            format!("http://{addr}"),
            "BLOCKCHAIN_TEST_RECONNECT_CLAIM",
        );
        let mut client = test_client_from_config(config, test_pool());
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        client.start().unwrap();
        // SAFETY: this variable name is unique to this test across the test binary
        unsafe { std::env::set_var("BLOCKCHAIN_TEST_RECONNECT_CLAIM", TEST_PRIVATE_KEY) };
        *client.in_flight.lock().unwrap() = Some(InFlightSlot::Preparing(TransactionPurpose::Swap));
        client.pending_tasks.push(get_runtime().spawn(async {}));

        client.connect().await.unwrap();

        assert!(client.in_flight.lock().unwrap().is_none());
    }

    #[allow(unsafe_code)] // env-var mutation in tests; unique var names avoid cross-test races
    #[tokio::test]
    async fn reconnect_after_aborted_submission_releases_preparing_claim() {
        // The swap task blocks in its first RPC; disconnect aborts it mid-preparation, and the
        // following connect awaits termination and releases the stale claim
        let state = ready_rpc_state().with_sleep("eth_getBlockByNumber", Duration::from_secs(30));
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_reconnect_claim_test", state).await
        else {
            return;
        };
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        client.start().unwrap();
        // SAFETY: this variable name is unique to this test across the test binary
        unsafe { std::env::set_var("BLOCKCHAIN_TEST_PRIVATE_KEY", TEST_PRIVATE_KEY) };

        let order = test_market_sell_order(test_pool().instrument_id);
        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_recorded_request(&state, "eth_getBlockByNumber").await;
        assert!(client.in_flight.lock().unwrap().is_some());

        client.disconnect().await.unwrap();
        client.connect().await.unwrap();

        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn poll_for_receipt_returns_none_after_exhaustion() {
        let state = ready_rpc_state().with_response("eth_getTransactionReceipt", RECEIPT_NULL);
        let (client, state) = client_with_mock_rpc(state).await;

        let receipt = poll_for_receipt(&client.http_rpc_client, &B256::ZERO, 3, Duration::ZERO)
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

        let error = poll_for_receipt(&client.http_rpc_client, &B256::ZERO, 3, Duration::ZERO)
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

        let receipt = poll_for_receipt(&client.http_rpc_client, &B256::ZERO, 3, Duration::ZERO)
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

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = crate::cache::database::BlockchainCacheDatabase::connect(db_options)
            .await
            .unwrap();

        let state = signing_rpc_state();
        let addr = start_mock_rpc_server(state.clone()).await;

        let mut client = test_client(format!("http://{addr}"));
        client.cache.database = Some(database);
        // Mirror the connect-time migration: tests create the pre-submission table shape
        client
            .cache
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        client.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        client.core.set_connected();

        let advisory_lock = i64::from(std::process::id());

        for statement in [
            format!(
                "CREATE FUNCTION {schema}.block_execution_hash_insert() RETURNS trigger \
                 LANGUAGE plpgsql AS 'BEGIN PERFORM pg_advisory_xact_lock({advisory_lock}); \
                 RETURN NEW; END'"
            ),
            format!(
                "CREATE TRIGGER block_execution_hash_insert BEFORE INSERT ON \
                 {schema}.execution_transaction_hash FOR EACH ROW EXECUTE FUNCTION \
                 {schema}.block_execution_hash_insert()"
            ),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(&admin_pool)
                .await
                .unwrap();
        }
        let mut lock_transaction = admin_pool.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(advisory_lock)
            .execute(&mut *lock_transaction)
            .await
            .unwrap();

        let value = U256::from(1_000_000_000_000_000u64);
        let in_flight = Arc::clone(&client.in_flight);
        let mut wrap = Box::pin(client.wrap(value));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                tokio::select! {
                    result = &mut wrap => {
                        panic!("persistence completed while the table was locked: {result:?}")
                    }
                    () = tokio::time::sleep(Duration::from_millis(1)) => {}
                }

                if matches!(
                    *in_flight.lock().unwrap(),
                    Some(InFlightSlot::AwaitingFinality(_))
                ) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        drop(wrap);
        lock_transaction.rollback().await.unwrap();

        let slot = *client.in_flight.lock().unwrap();
        let second_error = client
            .wrap(U256::from(2_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        let status: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT status FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert!(matches!(slot, Some(InFlightSlot::AwaitingFinality(_))));
        assert_eq!(status, "prepared");
        assert!(
            second_error.to_string().contains("awaiting finality"),
            "was: {second_error}"
        );
        assert_eq!(broadcasts, 0);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn persistence_failure_keeps_unbroadcast_slot() {
        let state = signing_rpc_state();
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_persist_fail_test", state).await
        else {
            return;
        };
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {schema}.execution_transaction_hash \
             ADD CONSTRAINT execution_hash_reject CHECK (FALSE)"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let value = U256::from(1_000_000_000_000_000u64);
        let error = client.wrap(value).await.unwrap_err();
        let in_flight = awaiting_in_flight(&client);
        let second_error = client
            .wrap(U256::from(2_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        let expected_hash = expected_wrap_tx_hash(value).await;

        let error_message = error.to_string();
        assert!(error_message.starts_with(&format!(
            "Failed to persist transaction {expected_hash}: Failed to persist signed transaction"
        )), "was: {error_message}");
        assert!(
            error_message.ends_with("the in-flight slot stays occupied"),
            "was: {error_message}"
        );
        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.tx_hash, expected_hash);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert!(
            second_error.to_string().contains("still awaiting finality"),
            "was: {second_error}"
        );
        assert_eq!(broadcasts, 0);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn cancellation_after_dispatch_keeps_record_and_in_flight_slot() {
        let state = signing_rpc_state().with_sleep(
            "eth_sendRawTransaction",
            Duration::from_secs(EXECUTION_RPC_TIMEOUT_SECS + 2),
        );
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_cancel_test", state).await
        else {
            return;
        };

        let mut wrap = Box::pin(client.wrap(U256::from(1_000_000_000_000_000u64)));
        tokio::select! {
            result = &mut wrap => panic!("broadcast completed before cancellation: {result:?}"),
            () = await_recorded_request(&state, "eth_sendRawTransaction") => {}
        }
        drop(wrap);

        let in_flight = awaiting_in_flight(&client);
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
            error.to_string().contains("still awaiting finality"),
            "was: {error}"
        );
        assert_eq!(record.nonce, 7);
        assert_eq!(record.purpose, "wrap");
        assert_eq!(record.status, "broadcast");
        assert_eq!(broadcasts, 1);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn cancellation_during_receipt_polling_keeps_record_and_in_flight_slot() {
        let state = signing_rpc_state()
            .with_send_raw_transaction_echo()
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
            () = await_recorded_request(&state, "eth_getTransactionReceipt") => {}
        }
        drop(wrap);

        let in_flight = awaiting_in_flight(&client);
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
            second_error.to_string().contains("still awaiting finality"),
            "was: {second_error}"
        );
        assert_eq!(record.nonce, 7);
        assert_eq!(record.transaction_hash, in_flight.tx_hash.to_string());
        assert_eq!(record.purpose, "wrap");
        assert_eq!(record.status, "broadcast");
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
    async fn rejected_broadcast_stays_dropped_and_occupied() {
        let state = signing_rpc_state()
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION_REJECTED);
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_rejected_test", state).await
        else {
            return;
        };

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let (purpose, status): (String, String) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT purpose, status FROM {schema}.execution_intent"
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
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        assert!(client.in_flight.lock().unwrap().is_some());
        assert_eq!(purpose, "wrap");
        assert_eq!(status, "dropped");
        assert_eq!(broadcasts, 1);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn legacy_table_loss_does_not_release_rejected_broadcast() {
        let state = signing_rpc_state()
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION_REJECTED)
            .with_sleep("eth_sendRawTransaction", Duration::from_secs(1));
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_rejected_update_test", state).await
        else {
            return;
        };

        let mut wrap = Box::pin(client.wrap(U256::from(1_000_000_000_000_000u64)));
        tokio::select! {
            result = &mut wrap => panic!("broadcast completed before database failure: {result:?}"),
            () = await_recorded_request(&state, "eth_sendRawTransaction") => {}
        }
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP TABLE {schema}.execution_transaction"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let error = wrap.await.unwrap_err();

        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        let in_flight = awaiting_in_flight(&client);
        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn included_receipt_keeps_slot_when_status_update_fails() {
        let state = signing_rpc_state()
            .with_send_raw_transaction_echo()
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
            () = await_recorded_request(&state, "eth_getTransactionReceipt") => {}
        }
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {schema}.execution_transaction_hash \
             RENAME TO execution_transaction_hash_unavailable"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let error = wrap.await.unwrap_err();
        let in_flight = awaiting_in_flight(&client);
        let requests = state.recorded_requests();

        assert!(
            error
                .to_string()
                .contains("Failed to update execution hash"),
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
    async fn broadcast_timeout_marks_dropped_and_keeps_ownership() {
        let state = signing_rpc_state().with_sleep(
            "eth_sendRawTransaction",
            Duration::from_secs(EXECUTION_RPC_TIMEOUT_SECS + 2),
        );
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_timeout_test", state).await
        else {
            return;
        };

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        let in_flight = awaiting_in_flight(&client);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert_eq!(in_flight.nonce, 7);

        // The persisted record stays active for reconciliation instead of rebroadcasting.
        let record = client
            .cache
            .get_execution_transaction(42161, &in_flight.tx_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.status, "dropped");

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_reconciles_dropped_transaction_without_rebroadcast() {
        let initial_state = broadcast_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE);
        let Some((admin_pool, schema, mut first_client, _)) =
            execution_client_with_database("execution_restart_test", initial_state).await
        else {
            return;
        };
        let expected_hash = expected_wrap_tx_hash(U256::from(1_000_000_000_000_000_u64)).await;
        let error = first_client
            .wrap(U256::from(1_000_000_000_000_000_u64))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        let database = first_client.cache.database.as_ref().unwrap().clone();
        drop(first_client);

        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_parameter_response(
                "eth_getBlockByNumber",
                "0x1cf0d41",
                &finalized_wrap_block(expected_hash),
            )
            .with_response_sequence("eth_call", &[CALL_BALANCE, CALL_BALANCE_AFTER_WRAP]);
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());

        restarted.reconcile_unresolved_execution().await.unwrap();
        restarted.reconcile_unresolved_execution().await.unwrap();

        let record = restarted
            .cache
            .get_execution_transaction(42161, &expected_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        let requests = restart_state.recorded_requests();
        assert_eq!(record.status, "finalized");
        assert!(restarted.in_flight.lock().unwrap().is_none());
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_getTransactionReceipt")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_call")
                .count(),
            2
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_sendRawTransaction")
                .count(),
            0
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_marks_prepared_intent_recoverable_without_broadcast() {
        let Some((admin_pool, schema, client, state)) =
            execution_client_with_database("execution_prepared_restart_test", ready_rpc_state())
                .await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap();
        let intent = database
            .reserve_execution_intent(&ExecutionIntentInsert {
                chain_id: 42161,
                wallet_address: WALLET.to_string(),
                purpose: "wrap".to_string(),
                client_order_id: None,
                trader_id: None,
                strategy_id: None,
                account_id: None,
                instrument_id: None,
                pool_address: None,
                transaction_to: WETH_ADDRESS.to_string(),
                transaction_input: "0xd0e30db0".to_string(),
                transaction_value: "1".to_string(),
                amount_in: None,
                created_block: FIXTURE_BLOCK,
            })
            .await
            .unwrap();
        *client.in_flight.lock().unwrap() = Some(InFlightSlot::Preparing(TransactionPurpose::Wrap));

        client.reconcile_unresolved_execution().await.unwrap();

        let (status, active): (String, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT status, active FROM {schema}.execution_intent WHERE id = {}",
            intent.id
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let transitions: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT to_status FROM {schema}.execution_transaction_transition ORDER BY id"
        )))
        .fetch_all(&admin_pool)
        .await
        .unwrap();

        assert_eq!(status, "recoverable");
        assert!(!active);
        assert_eq!(transitions, ["prepared", "recoverable"]);
        assert!(client.in_flight.lock().unwrap().is_none());
        assert!(
            state
                .recorded_requests()
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_wrap_identity_mismatch_keeps_signer_ownership() {
        let initial_state = broadcast_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE);
        let Some((admin_pool, schema, mut first_client, _)) =
            execution_client_with_database("execution_restart_mismatch_test", initial_state).await
        else {
            return;
        };
        let expected_hash = expected_wrap_tx_hash(U256::from(1_000_000_000_000_000_u64)).await;
        let error = first_client
            .wrap(U256::from(1_000_000_000_000_000_u64))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        let database = first_client.cache.database.as_ref().unwrap().clone();
        drop(first_client);

        // The finalized transaction carries no value, so it cannot be the persisted wrap
        let mismatched_block = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d41",
                "hash": "0x2222222222222222222222222222222222222222222222222222222222222222",
                "timestamp": "0x69044a21",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": expected_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "to": WETH,
                    "input": "0xd0e30db0",
                    "value": "0x0"
                }]
            }
        })
        .to_string();
        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &mismatched_block)
            .with_response_sequence("eth_call", &[CALL_BALANCE, CALL_BALANCE_AFTER_WRAP]);
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());

        let error = restarted
            .reconcile_unresolved_execution()
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not match the persisted wrap intent"),
            "was: {error}"
        );
        let in_flight = awaiting_in_flight(&restarted);
        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert_eq!(in_flight.tx_hash, expected_hash);
        let requests = restart_state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_call"),
            "the postcondition must not run when call identity is unproven"
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );
        let (status, active): (String, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT status, active FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(status, "finalized");
        assert!(active);

        let database = restarted.cache.database.as_ref().unwrap().clone();
        drop(restarted);
        let mut second = test_client(format!("http://{addr}"));
        second.cache.database = Some(database);
        second.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        let error = second.reconcile_unresolved_execution().await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not match the persisted wrap intent"),
            "was: {error}"
        );
        let active: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT active FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert!(active);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_wrap_postcondition_failure_keeps_signer_ownership() {
        let initial_state = broadcast_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE);
        let Some((admin_pool, schema, mut first_client, _)) =
            execution_client_with_database("execution_restart_postcondition_test", initial_state)
                .await
        else {
            return;
        };
        let expected_hash = expected_wrap_tx_hash(U256::from(1_000_000_000_000_000_u64)).await;
        let error = first_client
            .wrap(U256::from(1_000_000_000_000_000_u64))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        let database = first_client.cache.database.as_ref().unwrap().clone();
        drop(first_client);

        // Call identity matches, but the wrapped balance does not increase
        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_parameter_response(
                "eth_getBlockByNumber",
                "0x1cf0d41",
                &finalized_wrap_block(expected_hash),
            )
            .with_response_sequence("eth_call", &[CALL_BALANCE, CALL_BALANCE]);
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());

        let error = restarted
            .reconcile_unresolved_execution()
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("did not increase by"),
            "was: {error}"
        );
        let in_flight = awaiting_in_flight(&restarted);
        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert!(
            restart_state
                .recorded_requests()
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_reconciles_same_nonce_wrap_replacement_after_validation() {
        let initial_state = broadcast_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE);
        let Some((admin_pool, schema, mut first_client, _)) =
            execution_client_with_database("execution_restart_replacement_test", initial_state)
                .await
        else {
            return;
        };
        let original_hash = expected_wrap_tx_hash(U256::from(1_000_000_000_000_000_u64)).await;
        let error = first_client
            .wrap(U256::from(1_000_000_000_000_000_u64))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        let database = first_client.cache.database.as_ref().unwrap().clone();
        drop(first_client);

        // The replacement consumes the signer nonce with identical call fields
        let replacement_hash = B256::from([0x44; 32]);
        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT_NEXT)
            .with_response_sequence("eth_call", &[CALL_BALANCE, CALL_BALANCE_AFTER_WRAP])
            .with_response_sequence(
                "eth_getTransactionReceipt",
                &[RECEIPT_NULL, RECEIPT_SUCCESS],
            )
            .with_parameter_response(
                "eth_getBlockByNumber",
                "0x1cf0d40",
                &replacement_head_block(replacement_hash),
            )
            .with_parameter_response(
                "eth_getBlockByNumber",
                "0x1cf0d41",
                &finalized_wrap_block(replacement_hash),
            );
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());
        restarted.transaction_limits.receipt_timeout_secs = 2;

        restarted.reconcile_unresolved_execution().await.unwrap();

        let hashes: Vec<(String, String, bool)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT transaction_hash, status, current FROM \
                 {schema}.execution_transaction_hash ORDER BY id"
        )))
        .fetch_all(&admin_pool)
        .await
        .unwrap();
        let requests = restart_state.recorded_requests();

        assert_eq!(
            hashes,
            [
                (original_hash.to_string(), "replaced".to_string(), false),
                (replacement_hash.to_string(), "finalized".to_string(), true),
            ]
        );
        assert!(restarted.in_flight.lock().unwrap().is_none());
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_getTransactionReceipt")
                .count(),
            2
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_call")
                .count(),
            2
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_reconciles_finalized_approve_after_validation() {
        let initial_state = broadcast_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_response("eth_call", CALL_BOOL_TRUE);
        let Some((admin_pool, schema, mut first_client, _)) =
            execution_client_with_database("execution_restart_approve_test", initial_state).await
        else {
            return;
        };
        let expected_hash = expected_approve_tx_hash(U256::from(1_000u64)).await;
        let error = first_client
            .approve(WETH_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        let database = first_client.cache.database.as_ref().unwrap().clone();
        drop(first_client);

        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_parameter_response(
                "eth_getBlockByNumber",
                "0x1cf0d41",
                &finalized_approve_block(expected_hash, U256::from(1_000u64)),
            )
            .with_call_response(ALLOWANCE_SELECTOR, CALL_ALLOWANCE);
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());

        restarted.reconcile_unresolved_execution().await.unwrap();

        let record = restarted
            .cache
            .get_execution_transaction(42161, &expected_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        let requests = restart_state.recorded_requests();
        assert_eq!(record.status, "finalized");
        assert_eq!(record.purpose, "approve");
        assert!(restarted.in_flight.lock().unwrap().is_none());
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_call")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_sendRawTransaction")
                .count(),
            0
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_approve_postcondition_failure_keeps_signer_ownership() {
        let initial_state = broadcast_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_response("eth_call", CALL_BOOL_TRUE);
        let Some((admin_pool, schema, mut first_client, _)) =
            execution_client_with_database("execution_restart_approve_post_test", initial_state)
                .await
        else {
            return;
        };
        let expected_hash = expected_approve_tx_hash(U256::from(1_000u64)).await;
        let error = first_client
            .approve(WETH_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        let database = first_client.cache.database.as_ref().unwrap().clone();
        drop(first_client);

        // Call identity matches, but the allowance does not cover the approved amount
        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_parameter_response(
                "eth_getBlockByNumber",
                "0x1cf0d41",
                &finalized_approve_block(expected_hash, U256::from(1_000u64)),
            )
            .with_call_response(ALLOWANCE_SELECTOR, CALL_ZERO);
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.signer = Some(PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap());

        let error = restarted
            .reconcile_unresolved_execution()
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("below the requested amount"),
            "was: {error}"
        );
        let in_flight = awaiting_in_flight(&restarted);
        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.purpose, TransactionPurpose::Approve);
        assert!(
            restart_state
                .recorded_requests()
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn disappearing_included_receipt_records_reorg_before_drop() {
        let state = execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_call", CALL_BALANCE)
            .with_response_sequence(
                "eth_getTransactionReceipt",
                &[RECEIPT_SUCCESS, RECEIPT_NULL],
            )
            .with_parameter_response("eth_getBlockByNumber", "finalized", BLOCK_BY_NUMBER)
            .with_send_raw_transaction_echo();
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_receipt_disappeared_test", state).await
        else {
            return;
        };
        client.transaction_limits.receipt_timeout_secs = 2;

        let error = client
            .wrap(U256::from(1_000_000_000_000_000_u64))
            .await
            .unwrap_err();
        let transitions: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT to_status FROM {schema}.execution_transaction_transition ORDER BY id"
        )))
        .fetch_all(&admin_pool)
        .await
        .unwrap();

        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        assert_eq!(
            transitions,
            [
                "prepared",
                "signed",
                "broadcast",
                "included",
                "reorged",
                "dropped"
            ]
        );
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn canonical_block_change_records_reorg_before_drop() {
        let changed_block = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d41",
                "hash": "0x4444444444444444444444444444444444444444444444444444444444444444",
                "timestamp": "0x69044a21",
                "baseFeePerGas": "0x5f5e100",
                "transactions": []
            }
        })
        .to_string();
        let state = execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_call", CALL_BALANCE)
            .with_response_sequence(
                "eth_getTransactionReceipt",
                &[RECEIPT_SUCCESS, RECEIPT_SUCCESS],
            )
            .with_parameter_response_sequence(
                "eth_getBlockByNumber",
                "0x1cf0d41",
                &[BLOCK_CANONICAL, &changed_block],
            )
            .with_parameter_response("eth_getBlockByNumber", "finalized", BLOCK_BY_NUMBER)
            .with_send_raw_transaction_echo();
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_reorg_test", state).await
        else {
            return;
        };
        client.transaction_limits.receipt_timeout_secs = 2;

        let error = client
            .wrap(U256::from(1_000_000_000_000_000_u64))
            .await
            .unwrap_err();
        let transitions: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT to_status FROM {schema}.execution_transaction_transition ORDER BY id"
        )))
        .fetch_all(&admin_pool)
        .await
        .unwrap();

        assert!(
            error.to_string().contains("Timed out awaiting finality"),
            "was: {error}"
        );
        assert_eq!(
            transitions,
            [
                "prepared",
                "signed",
                "broadcast",
                "included",
                "reorged",
                "dropped"
            ]
        );
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn same_nonce_replacement_preserves_intent_and_finalizes_new_hash() {
        let replacement_hash = B256::from([0x44; 32]);
        let replacement_block = replacement_head_block(replacement_hash);
        let state = execution_rpc_state()
            .with_response_sequence(
                "eth_call",
                &[CALL_BALANCE, CALL_BALANCE, CALL_BALANCE_AFTER_WRAP],
            )
            .with_response_sequence(
                "eth_getTransactionCount",
                &[TRANSACTION_COUNT, TRANSACTION_COUNT_NEXT],
            )
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response_sequence(
                "eth_getTransactionReceipt",
                &[RECEIPT_NULL, RECEIPT_SUCCESS],
            )
            .with_parameter_response("eth_getBlockByNumber", "0x1cf0d40", &replacement_block)
            .with_send_raw_transaction_echo();
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_replacement_test", state).await
        else {
            return;
        };
        client.transaction_limits.receipt_timeout_secs = 2;
        let original_hash = expected_wrap_tx_hash(U256::from(1_000_000_000_000_000_u64)).await;

        let observed_hash = client
            .wrap(U256::from(1_000_000_000_000_000_u64))
            .await
            .unwrap();
        let hashes: Vec<(String, String, bool)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT transaction_hash, status, current FROM \
                 {schema}.execution_transaction_hash ORDER BY id"
        )))
        .fetch_all(&admin_pool)
        .await
        .unwrap();
        let client_order_ids: Vec<Option<String>> = sqlx::query_scalar(sqlx::AssertSqlSafe(
            format!("SELECT client_order_id FROM {schema}.execution_intent"),
        ))
        .fetch_all(&admin_pool)
        .await
        .unwrap();

        assert_eq!(observed_hash, replacement_hash);
        assert_eq!(
            hashes,
            [
                (original_hash.to_string(), "replaced".to_string(), false),
                (replacement_hash.to_string(), "finalized".to_string(), true),
            ]
        );
        assert_eq!(client_order_ids, [None]);
        assert_eq!(
            state
                .recorded_requests()
                .iter()
                .filter(|request| request["method"] == "eth_sendRawTransaction")
                .count(),
            1
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn execution_transaction_constraints_reject_conflicting_identity() {
        const TRANSACTION_HASH: &str = "0xduplicate-transaction-hash";
        const OTHER_WALLET: &str = "0x0000000000000000000000000000000000000001";
        let Some((admin_pool, schema, client, _)) =
            execution_client_with_database("execution_duplicate_record_test", ready_rpc_state())
                .await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap();
        let operator = database
            .reserve_execution_intent(&ExecutionIntentInsert {
                chain_id: 42161,
                wallet_address: WALLET.to_string(),
                purpose: "wrap".to_string(),
                client_order_id: None,
                trader_id: None,
                strategy_id: None,
                account_id: None,
                instrument_id: None,
                pool_address: None,
                transaction_to: WETH_ADDRESS.to_string(),
                transaction_input: "0xd0e30db0".to_string(),
                transaction_value: "1".to_string(),
                amount_in: None,
                created_block: FIXTURE_BLOCK,
            })
            .await
            .unwrap();
        database
            .assign_execution_intent_nonce(operator.id, 7)
            .await
            .unwrap();
        database
            .add_execution_transaction_hash(operator.id, 42161, TRANSACTION_HASH, &[1, 2, 3])
            .await
            .unwrap();
        database
            .add_execution_transaction_hash(operator.id, 42161, TRANSACTION_HASH, &[1, 2, 3])
            .await
            .unwrap();

        let signer_conflict = database
            .reserve_execution_intent(&ExecutionIntentInsert {
                chain_id: 42161,
                wallet_address: WALLET.to_string(),
                purpose: "approve".to_string(),
                client_order_id: None,
                trader_id: None,
                strategy_id: None,
                account_id: None,
                instrument_id: None,
                pool_address: None,
                transaction_to: ROUTER_ADDRESS.to_string(),
                transaction_input: "0x01".to_string(),
                transaction_value: "0".to_string(),
                amount_in: None,
                created_block: FIXTURE_BLOCK,
            })
            .await
            .unwrap_err();
        let other = database
            .reserve_execution_intent(&ExecutionIntentInsert {
                chain_id: 42161,
                wallet_address: OTHER_WALLET.to_string(),
                purpose: "approve".to_string(),
                client_order_id: None,
                trader_id: None,
                strategy_id: None,
                account_id: None,
                instrument_id: None,
                pool_address: None,
                transaction_to: ROUTER_ADDRESS.to_string(),
                transaction_input: "0x01".to_string(),
                transaction_value: "0".to_string(),
                amount_in: None,
                created_block: FIXTURE_BLOCK,
            })
            .await
            .unwrap();
        database
            .assign_execution_intent_nonce(other.id, 7)
            .await
            .unwrap();
        let hash_conflict = database
            .add_execution_transaction_hash(other.id, 42161, TRANSACTION_HASH, &[4, 5, 6])
            .await
            .unwrap_err();

        let record = database
            .get_execution_transaction(42161, TRANSACTION_HASH)
            .await
            .unwrap()
            .unwrap();
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert_eq!(record.wallet_address.as_deref(), Some(WALLET));
        assert_eq!(record.nonce, 7);
        assert_eq!(record.transaction_hash, TRANSACTION_HASH);
        assert_eq!(record.purpose, "wrap");
        assert_eq!(record.status, "signed");
        assert_eq!(record.client_order_id, None);
        assert!(
            hash_conflict
                .to_string()
                .contains("conflicts with its persisted identity"),
            "was: {hash_conflict}"
        );
        assert!(
            signer_conflict
                .to_string()
                .contains("execution_intent_active_signer_key"),
            "was: {signer_conflict}"
        );
        assert_eq!(count, 2);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn replacement_hashes_retain_swap_order_identity() {
        const ORIGINAL_HASH: &str =
            "0x5555555555555555555555555555555555555555555555555555555555555555";
        const REPLACEMENT_HASH: &str =
            "0x6666666666666666666666666666666666666666666666666666666666666666";
        let Some((admin_pool, schema, client, _)) = execution_client_with_database(
            "execution_swap_replacement_identity_test",
            ready_rpc_state(),
        )
        .await
        else {
            return;
        };
        let pool = test_pool();
        let database = client.cache.database.as_ref().unwrap();
        let intent = database
            .reserve_execution_intent(&ExecutionIntentInsert {
                chain_id: 42161,
                wallet_address: WALLET.to_string(),
                purpose: "swap".to_string(),
                client_order_id: Some("O-SWAP-001".to_string()),
                trader_id: Some("TRADER-001".to_string()),
                strategy_id: Some("S-001".to_string()),
                account_id: Some("BLOCKCHAIN-001".to_string()),
                instrument_id: Some(pool.instrument_id.to_string()),
                pool_address: Some(pool.address.to_string()),
                transaction_to: ROUTER_ADDRESS.to_string(),
                transaction_input: "0x010203".to_string(),
                transaction_value: "0".to_string(),
                amount_in: Some("1000000000000000".to_string()),
                created_block: FIXTURE_BLOCK,
            })
            .await
            .unwrap();
        database
            .assign_execution_intent_nonce(intent.id, 7)
            .await
            .unwrap();
        database
            .add_execution_transaction_hash(intent.id, 42161, ORIGINAL_HASH, &[1, 2, 3])
            .await
            .unwrap();

        database
            .add_execution_replacement_hash(intent.id, 42161, REPLACEMENT_HASH)
            .await
            .unwrap();

        let rows: Vec<(String, String, bool, String)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT hash.transaction_hash, hash.status, hash.current, intent.client_order_id \
                 FROM {schema}.execution_transaction_hash AS hash \
                 JOIN {schema}.execution_intent AS intent ON intent.id = hash.intent_id \
                 ORDER BY hash.id"
            )))
            .fetch_all(&admin_pool)
            .await
            .unwrap();

        assert_eq!(
            rows,
            [
                (
                    ORIGINAL_HASH.to_string(),
                    "replaced".to_string(),
                    false,
                    "O-SWAP-001".to_string(),
                ),
                (
                    REPLACEMENT_HASH.to_string(),
                    "replaced".to_string(),
                    true,
                    "O-SWAP-001".to_string(),
                ),
            ]
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn execution_status_transitions_are_idempotent() {
        const TRANSACTION_HASH: &str =
            "0x5555555555555555555555555555555555555555555555555555555555555555";
        let Some((admin_pool, schema, client, _)) =
            execution_client_with_database("execution_transition_test", ready_rpc_state()).await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap();
        let intent = database
            .reserve_execution_intent(&ExecutionIntentInsert {
                chain_id: 42161,
                wallet_address: WALLET.to_string(),
                purpose: "wrap".to_string(),
                client_order_id: None,
                trader_id: None,
                strategy_id: None,
                account_id: None,
                instrument_id: None,
                pool_address: None,
                transaction_to: WETH_ADDRESS.to_string(),
                transaction_input: "0xd0e30db0".to_string(),
                transaction_value: "1".to_string(),
                amount_in: None,
                created_block: FIXTURE_BLOCK,
            })
            .await
            .unwrap();
        database
            .assign_execution_intent_nonce(intent.id, 7)
            .await
            .unwrap();
        database
            .add_execution_transaction_hash(intent.id, 42161, TRANSACTION_HASH, &[1, 2, 3])
            .await
            .unwrap();

        for _ in 0..2 {
            database
                .record_execution_status(
                    intent.id,
                    TRANSACTION_HASH,
                    TransactionStatus::Broadcast,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .await
                .unwrap();
        }

        for status in [TransactionStatus::Included, TransactionStatus::Included] {
            database
                .record_execution_status(
                    intent.id,
                    TRANSACTION_HASH,
                    status,
                    Some(FIXTURE_BLOCK + 1),
                    Some("0x2222222222222222222222222222222222222222222222222222222222222222"),
                    Some(true),
                    Some(50_112),
                    Some("100000000"),
                )
                .await
                .unwrap();
        }

        for _ in 0..2 {
            database
                .record_execution_status(
                    intent.id,
                    TRANSACTION_HASH,
                    TransactionStatus::Finalized,
                    Some(FIXTURE_BLOCK + 1),
                    Some("0x2222222222222222222222222222222222222222222222222222222222222222"),
                    Some(true),
                    Some(50_112),
                    Some("100000000"),
                )
                .await
                .unwrap();
        }

        let transitions: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT to_status FROM {schema}.execution_transaction_transition ORDER BY id"
        )))
        .fetch_all(&admin_pool)
        .await
        .unwrap();
        let (status, active): (String, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT status, active FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let append_only_error = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {schema}.execution_transaction_transition WHERE intent_id = {}",
            intent.id
        )))
        .execute(&admin_pool)
        .await
        .unwrap_err();

        assert_eq!(
            transitions,
            ["prepared", "signed", "broadcast", "included", "finalized"]
        );
        assert_eq!(status, "finalized");
        assert!(!active);
        assert!(
            append_only_error
                .to_string()
                .contains("Execution transitions are append-only"),
            "was: {append_only_error}"
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn wrap_then_approve_persists_records_and_clears_in_flight() {
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
            .with_response_sequence(
                "eth_getTransactionCount",
                &[TRANSACTION_COUNT, TRANSACTION_COUNT_NEXT],
            )
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response_sequence(
                "eth_getTransactionReceipt",
                &[RPC_METHOD_NOT_FOUND, RECEIPT_SUCCESS, RECEIPT_SUCCESS],
            )
            .with_send_raw_transaction_echo();
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_client_test", state).await
        else {
            return;
        };
        client.config.unlimited_approval = true;
        client.transaction_limits.receipt_timeout_secs = 3;

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
        assert_eq!(record.status, "finalized");

        // The in-flight slot cleared after finality, so a second transaction proceeds.
        let approve_hash = client
            .approve(WETH_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap();

        let record = client
            .cache
            .get_execution_transaction(42161, &approve_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.nonce, 8);
        assert_eq!(record.purpose, "approve");
        assert_eq!(record.status, "finalized");

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

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn reverted_receipt_marks_record_reverted_and_errors() {
        let state = signing_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_REVERTED)
            .with_send_raw_transaction_echo();
        let Some((admin_pool, schema, mut client, _)) =
            execution_client_with_database("execution_reverted_test", state).await
        else {
            return;
        };

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("reverted on-chain"),
            "was: {error}"
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        let expected_hash = expected_wrap_tx_hash(U256::from(1_000_000_000_000_000u64)).await;

        let record = client
            .cache
            .get_execution_transaction(42161, &expected_hash.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.status, "reverted");

        drop_execution_schema(&admin_pool, &schema).await;
    }

    fn market_sell_order_with_id(instrument_id: InstrumentId, client_order_id: &str) -> OrderAny {
        OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("S-001"))
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from(client_order_id))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.001"))
            .build()
    }

    fn submit_order_list_cmd(orders: &[OrderAny]) -> SubmitOrderList {
        let order_list = OrderList::new(
            OrderListId::from("OL-001"),
            orders[0].instrument_id(),
            orders[0].strategy_id(),
            orders.iter().map(|order| order.client_order_id()).collect(),
            UnixNanos::default(),
        );
        SubmitOrderList::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("BLOCKCHAIN-001")),
            orders[0].strategy_id(),
            order_list,
            orders
                .iter()
                .map(|order| order.init_event().clone())
                .collect(),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        )
    }

    fn modify_order_cmd(
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
    ) -> ModifyOrder {
        ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("BLOCKCHAIN-001")),
            StrategyId::from("S-001"),
            instrument_id,
            client_order_id,
            None,
            Some(Quantity::from("0.002")),
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    }

    fn cancel_order_cmd(
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
    ) -> CancelOrder {
        CancelOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("BLOCKCHAIN-001")),
            StrategyId::from("S-001"),
            instrument_id,
            client_order_id,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    }

    fn cancel_all_orders_cmd(instrument_id: InstrumentId) -> CancelAllOrders {
        CancelAllOrders::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("BLOCKCHAIN-001")),
            StrategyId::from("S-001"),
            instrument_id,
            OrderSide::Sell,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    }

    fn batch_cancel_orders_cmd(cancels: Vec<CancelOrder>) -> BatchCancelOrders {
        BatchCancelOrders::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("BLOCKCHAIN-001")),
            StrategyId::from("S-001"),
            cancels[0].instrument_id,
            cancels,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    }

    fn query_order_cmd(instrument_id: InstrumentId, client_order_id: ClientOrderId) -> QueryOrder {
        QueryOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("BLOCKCHAIN-001")),
            StrategyId::from("S-001"),
            instrument_id,
            client_order_id,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    }

    async fn unsupported_client_with_mock_rpc()
    -> (BlockchainExecutionClient, MockRpcState, Rc<RefCell<Cache>>) {
        let state = ready_rpc_state();
        let addr = start_mock_rpc_server(state.clone()).await;
        let (client, cache) = swap_client_with_cache(test_config(format!("http://{addr}")));
        (client, state, cache)
    }

    #[tokio::test]
    async fn submit_order_list_denies_every_order_without_side_effects() {
        let (mut client, state, cache) = unsupported_client_with_mock_rpc().await;
        let pool = test_pool();
        let first = test_market_sell_order(pool.instrument_id);
        let second = market_sell_order_with_id(pool.instrument_id, "O-SWAP-002");
        cache
            .borrow_mut()
            .add_order(second.clone(), None, None, false)
            .unwrap();
        let orders = [first.clone(), second.clone()];
        let mut receiver = start_with_events(&mut client);

        client
            .submit_order_list(submit_order_list_cmd(&orders))
            .unwrap();

        let events = collect_order_events(&mut receiver);
        let mut denied_ids = Vec::new();

        for event in &events {
            let OrderEventAny::Denied(denied) = event else {
                panic!("expected OrderDenied, was {event:?}");
            };
            assert_eq!(denied.reason.as_str(), ORDER_LIST_UNSUPPORTED);
            denied_ids.push(denied.client_order_id);
        }
        denied_ids.sort();
        assert_eq!(
            denied_ids,
            [first.client_order_id(), second.client_order_id()]
        );
        assert!(state.recorded_requests().is_empty());
        assert!(client.in_flight.lock().unwrap().is_none());

        for order in &orders {
            let cache_ref = cache.borrow();
            let cached = cache_ref.order(&order.client_order_id()).unwrap();
            assert_eq!(cached.status(), OrderStatus::Initialized);
        }
    }

    #[tokio::test]
    async fn modify_order_rejects_without_side_effects() {
        let (mut client, state, cache) = unsupported_client_with_mock_rpc().await;
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client
            .modify_order(modify_order_cmd(
                order.instrument_id(),
                order.client_order_id(),
            ))
            .unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1, "was: {events:?}");
        let OrderEventAny::ModifyRejected(rejected) = &events[0] else {
            panic!("expected OrderModifyRejected, was {:?}", events[0]);
        };
        assert_eq!(rejected.client_order_id, order.client_order_id());
        assert_eq!(rejected.reason.as_str(), ORDER_MODIFY_UNSUPPORTED);
        assert!(state.recorded_requests().is_empty());
        assert!(client.in_flight.lock().unwrap().is_none());
        let cache_ref = cache.borrow();
        let cached = cache_ref.order(&order.client_order_id()).unwrap();
        assert_eq!(cached.status(), OrderStatus::Initialized);
    }

    #[tokio::test]
    async fn cancel_order_rejects_without_side_effects() {
        let (mut client, state, cache) = unsupported_client_with_mock_rpc().await;
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client
            .cancel_order(cancel_order_cmd(
                order.instrument_id(),
                order.client_order_id(),
            ))
            .unwrap();

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1, "was: {events:?}");
        let OrderEventAny::CancelRejected(rejected) = &events[0] else {
            panic!("expected OrderCancelRejected, was {:?}", events[0]);
        };
        assert_eq!(rejected.client_order_id, order.client_order_id());
        assert_eq!(rejected.reason.as_str(), ORDER_CANCEL_UNSUPPORTED);
        assert!(state.recorded_requests().is_empty());
        assert!(client.in_flight.lock().unwrap().is_none());
        let cache_ref = cache.borrow();
        let cached = cache_ref.order(&order.client_order_id()).unwrap();
        assert_eq!(cached.status(), OrderStatus::Initialized);
    }

    #[tokio::test]
    async fn batch_cancel_orders_rejects_each_order_without_side_effects() {
        let (mut client, state, cache) = unsupported_client_with_mock_rpc().await;
        let pool = test_pool();
        let first = test_market_sell_order(pool.instrument_id);
        let second = market_sell_order_with_id(pool.instrument_id, "O-SWAP-002");
        cache
            .borrow_mut()
            .add_order(second.clone(), None, None, false)
            .unwrap();
        let orders = [first.clone(), second.clone()];
        let cancels = orders
            .iter()
            .map(|order| cancel_order_cmd(order.instrument_id(), order.client_order_id()))
            .collect();
        let mut receiver = start_with_events(&mut client);

        client
            .batch_cancel_orders(batch_cancel_orders_cmd(cancels))
            .unwrap();

        let events = collect_order_events(&mut receiver);
        let mut rejected_ids = Vec::new();

        for event in &events {
            let OrderEventAny::CancelRejected(rejected) = event else {
                panic!("expected OrderCancelRejected, was {event:?}");
            };
            assert_eq!(rejected.reason.as_str(), ORDER_CANCEL_UNSUPPORTED);
            rejected_ids.push(rejected.client_order_id);
        }
        rejected_ids.sort();
        assert_eq!(
            rejected_ids,
            [first.client_order_id(), second.client_order_id()]
        );
        assert!(state.recorded_requests().is_empty());
        assert!(client.in_flight.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn cancel_all_and_query_order_log_unsupported_without_side_effects() {
        let (mut client, state, cache) = unsupported_client_with_mock_rpc().await;
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client
            .cancel_all_orders(cancel_all_orders_cmd(order.instrument_id()))
            .unwrap();
        client
            .query_order(query_order_cmd(
                order.instrument_id(),
                order.client_order_id(),
            ))
            .unwrap();

        let events = collect_order_events(&mut receiver);
        assert!(events.is_empty(), "was: {events:?}");
        assert!(state.recorded_requests().is_empty());
        assert!(client.in_flight.lock().unwrap().is_none());
        let cache_ref = cache.borrow();
        let cached = cache_ref.order(&order.client_order_id()).unwrap();
        assert_eq!(cached.status(), OrderStatus::Initialized);
    }

    #[tokio::test]
    async fn unsupported_commands_handle_unknown_orders_without_panic() {
        let (mut client, state, _) = unsupported_client_with_mock_rpc().await;
        let instrument_id = test_pool().instrument_id;
        let unknown = ClientOrderId::from("O-UNKNOWN");
        let mut receiver = start_with_events(&mut client);

        client
            .modify_order(modify_order_cmd(instrument_id, unknown))
            .unwrap();
        client
            .cancel_order(cancel_order_cmd(instrument_id, unknown))
            .unwrap();
        client
            .batch_cancel_orders(batch_cancel_orders_cmd(vec![cancel_order_cmd(
                instrument_id,
                unknown,
            )]))
            .unwrap();
        client
            .query_order(query_order_cmd(instrument_id, unknown))
            .unwrap();

        let events = collect_order_events(&mut receiver);
        assert!(events.is_empty(), "was: {events:?}");
        assert!(state.recorded_requests().is_empty());
        assert!(client.in_flight.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn report_generators_error_except_mass_status_without_side_effects() {
        let (client, state, _) = unsupported_client_with_mock_rpc().await;

        let report = client
            .generate_order_status_report(&GenerateOrderStatusReport::new(
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap_err();
        assert_eq!(report.to_string(), VENUE_EXECUTION_REPORTS_UNSUPPORTED);

        let reports = client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                false,
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap_err();
        assert_eq!(reports.to_string(), VENUE_EXECUTION_REPORTS_UNSUPPORTED);

        let fills = client
            .generate_fill_reports(GenerateFillReports::new(
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap_err();
        assert_eq!(fills.to_string(), VENUE_EXECUTION_REPORTS_UNSUPPORTED);

        let positions = client
            .generate_position_status_reports(&GeneratePositionStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap_err();
        assert_eq!(positions.to_string(), VENUE_EXECUTION_REPORTS_UNSUPPORTED);

        let mass_status = client.generate_mass_status(None).await.unwrap();
        assert!(mass_status.is_none());

        let mass_status = client.generate_mass_status(Some(60)).await.unwrap();
        assert!(mass_status.is_none());

        assert!(state.recorded_requests().is_empty());
        assert!(client.in_flight.lock().unwrap().is_none());
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
        // Mirror the connect-time migration: tests create the pre-submission table shape
        client
            .cache
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
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

    fn execution_transaction_create_sql() -> &'static str {
        const TABLES_SQL: &str = include_str!("../../../../../schema/sql/tables.sql");
        const START: &str = "CREATE TABLE IF NOT EXISTS \"execution_transaction\"";
        let start = TABLES_SQL
            .find(START)
            .expect("execution_transaction table is missing from tables.sql");
        let statement = &TABLES_SQL[start..];
        let end = statement
            .find(";\n")
            .expect("execution_transaction CREATE TABLE is unterminated")
            + 1;
        &statement[..end]
    }
}
