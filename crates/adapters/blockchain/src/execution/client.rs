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
    collections::{HashMap, HashSet},
    fmt::Debug,
    ops::RangeInclusive,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    live::runner::get_exec_event_sender,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    Params, UUID4, UnixNanos, datetime::NANOSECONDS_IN_SECOND, hex, time::get_atomic_clock_realtime,
};
use nautilus_live::{
    ExecutionClientCore, ExecutionEventEmitter,
    task::{TaskGroup, TaskGroupGuard},
};
use nautilus_model::{
    accounts::AccountAny,
    defi::{
        DexType, Pool, PoolIdentifier, SharedChain, Token,
        data::block::{BLOCK_SCOPED_SNAPSHOT_INDEX, BlockPosition},
        pool_analysis::quote::SwapQuote,
        validation::validate_address,
        wallet::{TokenBalance, WalletBalance},
    },
    enums::{CurrencyType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType},
    events::{OrderCanceled, OrderEventAny, OrderFilled, OrderRejected},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, TradeId, Venue, VenueOrderId},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{
        AccountBalance, Currency, MarginBalance, Money, Price, Quantity, fixed::FIXED_PRECISION,
    },
};
use zeroize::Zeroizing;

use crate::{
    cache::{
        BlockchainCache,
        database::{
            BlockchainCacheDatabase, ExecutionFinalityTransition, ExecutionNonceAssignment,
            ExecutionPayloadCheck, ExecutionPayloadLease, ExecutionReplacementScan,
            ExecutionVerificationBatch, ExecutionVerificationBootstrap,
            ExecutionVerificationDecision, ExecutionVerificationMigration,
            ExecutionVerificationMigrationRecord, ExecutionVerificationMigrationSnapshot,
            ExecutionVerifiedHeader, reservation_failure_proven_not_committed,
        },
        rows::{ExecutionIntentInsert, ExecutionIntentRow, ExecutionTransactionHashRow},
    },
    config::{
        BlockchainContractRole, BlockchainDeploymentManifest, BlockchainExecutionClientConfig,
    },
    contracts::{
        erc20::{ERC20, Erc20Contract},
        uniswap_v3_quote::UniswapV3Quote,
        uniswap_v3_swap::{UniswapV3Factory, UniswapV3RouterState, UniswapV3SwapRouter},
        weth::WETH9,
    },
    execution::{
        preflight::{
            BlockchainPreflightReport, ContractCodeCheck, PoolPreflightCheck, TokenPreflightCheck,
        },
        sealing::{
            PayloadKeySet, PayloadPolicy, authenticate_payload, authenticate_payload_identity,
            authenticate_retained_payload, payload_context, payload_context_identity,
            persisted_call_fields, retained_payload_requires_policy,
        },
        transaction::{
            TransactionPurpose, TransactionStatus, build_eip1559_transaction, compute_max_fee,
            decode_signed_transaction, derive_fees, derive_gas_limit, sign_eip1559_transaction,
        },
    },
    rpc::{
        error::BroadcastError,
        helpers as rpc_helpers,
        http::{BlockchainHttpRpcClient, EXECUTION_RPC_TIMEOUT_SECS},
        types::{RpcCallType, RpcTransaction, RpcTransactionReceipt},
        verification::{
            VerificationCoordinator, VerificationOutcome, Verified, VerifiedBlockHeader,
            VerifiedCallTrace, VerifiedSimulation,
        },
    },
};

/// Interval between receipt polls while awaiting transaction finality.
const RECEIPT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_PAYLOAD_OPERATION_BATCH_SIZE: usize = 1_000;
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
/// Maximum historical block range inspected to identify a signer-nonce replacement.
const MAX_REPLACEMENT_SCAN_BLOCKS: u64 = 4_096;

/// Result of authenticating every persisted signed transaction in one execution database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadStorageCheck {
    /// Whether payload protection is active.
    pub protected: bool,
    /// Durable deployment identity when protection is active.
    pub deployment_id: Option<String>,
    /// Rows which still contain plaintext signed transaction bytes.
    pub plaintext_rows: u64,
    /// Signed transaction rows which require a payload.
    pub original_rows: u64,
    /// Canonical replacement rows whose original bytes are unavailable.
    pub replacement_rows: u64,
    /// Payload rows successfully opened and authenticated.
    pub authenticated_rows: u64,
    /// Key IDs referenced by protected payloads.
    pub key_ids: Vec<String>,
    /// Database roles with direct table ownership or `SELECT` grants.
    pub read_roles: Vec<String>,
}

impl From<ExecutionPayloadCheck> for PayloadStorageCheck {
    fn from(value: ExecutionPayloadCheck) -> Self {
        Self {
            protected: value.protected,
            deployment_id: value.deployment_id,
            plaintext_rows: value.plaintext_rows,
            original_rows: value.original_rows,
            replacement_rows: value.replacement_rows,
            authenticated_rows: value.authenticated_rows,
            key_ids: value.key_ids,
            read_roles: value.read_roles,
        }
    }
}

// A broadcast transaction awaiting finality, occupying the single in-flight slot.
#[derive(Debug, Clone, Copy)]
struct InFlightTransaction {
    intent_id: i64,
    nonce: u64,
    tx_hash: B256,
    purpose: TransactionPurpose,
}

#[derive(Debug, Clone, Copy)]
struct RecoveryTransaction {
    intent_id: i64,
    nonce: u64,
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
    /// Restored durable ownership retained while persisted transaction data is authenticated.
    Recovering(RecoveryTransaction),
    /// Signed, persisted, and awaiting finality.
    AwaitingFinality(InFlightTransaction),
}

#[derive(Debug, Clone)]
struct IncludedTransaction {
    intent_id: i64,
    nonce: u64,
    tx_hash: B256,
    block_number: u64,
    receipt: RpcTransactionReceipt,
    finality: StableFinality,
}

#[derive(Debug, Clone)]
struct StableFinality {
    decisions: Vec<ExecutionVerificationDecision>,
    inclusion_header: ExecutionVerifiedHeader,
    finalized_headers: Vec<ExecutionVerifiedHeader>,
}

#[derive(Debug, Clone, Copy)]
enum TransactionAuthorization {
    Wrap {
        weth: Address,
    },
    Approve {
        token: Address,
        router: Address,
        amount: U256,
    },
}

/// The single-in-flight limit error naming the transaction currently occupying the slot.
fn in_flight_limit_error(slot: &InFlightSlot) -> anyhow::Error {
    match slot {
        InFlightSlot::Preparing(purpose) => anyhow::anyhow!(
            "A {} transaction is being prepared; at most one transaction can be in flight",
            purpose.as_str()
        ),
        InFlightSlot::Recovering(recovery) => anyhow::anyhow!(
            "Execution intent {} ({}, nonce {}) retains signer ownership pending recovery; at most one transaction can be in flight",
            recovery.intent_id,
            recovery.purpose.as_str(),
            recovery.nonce
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

fn release_preparing_if_reservation_not_committed(
    in_flight: &Mutex<Option<InFlightSlot>>,
    error: &anyhow::Error,
) {
    if reservation_failure_proven_not_committed(error) {
        release_preparing_slot(in_flight);
    }
}

#[derive(Debug)]
struct TransactionLimits {
    allowed_token_pairs: HashSet<(Address, Address)>,
    quote_spend_limits: HashMap<(Address, Address), QuoteSpendCeiling>,
    slippage_bps: u32,
    max_slippage_bps: u32,
    max_order_amount: u64,
    deadline_seconds: u64,
    max_quote_age_blocks: u64,
    receipt_timeout_secs: u64,
}

#[derive(Debug, Clone, Copy)]
struct QuoteSpendCeiling {
    spend_token: Address,
    spend_token_decimals: u8,
    max_amount: U256,
}

/// Execution client for blockchain interactions including balance tracking and order execution.
#[derive(Debug)]
pub struct BlockchainExecutionClient {
    core: ExecutionClientCore,
    emitter: ExecutionEventEmitter,
    cache: BlockchainCache,
    config: BlockchainExecutionClientConfig,
    chain: SharedChain,
    wallet_address: Address,
    signer: Option<Arc<PrivateKeySigner>>,
    payload_keys: Option<Arc<PayloadKeySet>>,
    router_addresses: Vec<Address>,
    transaction_limits: TransactionLimits,
    weth_address: Address,
    in_flight: Arc<Mutex<Option<InFlightSlot>>>,
    wallet_balance: Arc<Mutex<WalletBalance>>,
    erc20_contract: Erc20Contract,
    http_rpc_client: Arc<BlockchainHttpRpcClient>,
    verification: VerificationCoordinator,
    pending_tasks: TaskGroup,
}

impl BlockchainExecutionClient {
    /// Creates a new [`BlockchainExecutionClient`] instance for the specified configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if transaction limits are missing or invalid, independent verification is
    /// missing or conflicts with the configured chain, the deployment manifest or provider
    /// topology is invalid, a configured address or token pair is invalid, the router allowlist is
    /// empty, or the slippage bounds are inconsistent or not below 100%.
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
        let verification_config = config.verification.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Independent Blockchain execution verification is required")
        })?;
        anyhow::ensure!(
            verification_config.chain_anchor.chain_id == config.chain.chain_id,
            "Verification chain anchor ID does not match the configured chain"
        );
        anyhow::ensure!(
            verification_config.chain_anchor.chain_name == config.chain.name.to_string(),
            "Verification chain anchor name does not match the configured chain"
        );
        let verification = VerificationCoordinator::new(
            http_rpc_client.clone(),
            &config.http_rpc_url,
            verification_config,
            config.rpc_requests_per_second,
        )?;
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
        Self::validate_manifest_contracts(&config, &router_addresses, weth_address)?;

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

        let pending_tasks = TaskGroup::new();

        Ok(Self {
            core: core_client,
            emitter,
            wallet_balance: Arc::new(Mutex::new(wallet_balance)),
            chain,
            cache,
            config,
            signer: None,
            payload_keys: None,
            router_addresses,
            transaction_limits,
            weth_address,
            in_flight: Arc::new(Mutex::new(None)),
            erc20_contract,
            http_rpc_client,
            verification,
            wallet_address,
            pending_tasks,
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

        let quote_spend_limits = config.quote_spend_limits.as_deref().unwrap_or_default();
        let mut parsed_quote_spend_limits = HashMap::with_capacity(quote_spend_limits.len());
        for limit in quote_spend_limits {
            let token_in = validate_address(limit.token_in.as_str())?;
            let token_out = validate_address(limit.token_out.as_str())?;
            let spend_token = validate_address(limit.spend_token.as_str())?;

            if !parsed_pairs.contains(&(token_in, token_out)) {
                anyhow::bail!(
                    "Quote spend limit pair {token_in} -> {token_out} is not in the `allowed_token_pairs` allowlist"
                );
            }

            if spend_token != token_in {
                anyhow::bail!(
                    "Quote spend limit for {token_in} -> {token_out} is denominated in {spend_token}; `spend_token` must match `token_in`"
                );
            }

            if limit.max_amount.is_empty()
                || !limit.max_amount.bytes().all(|byte| byte.is_ascii_digit())
            {
                anyhow::bail!(
                    "Quote spend limit `max_amount` '{}' must be a base-10 unsigned integer string",
                    limit.max_amount
                );
            }
            let max_amount = U256::from_str(&limit.max_amount).map_err(|_| {
                anyhow::anyhow!(
                    "Quote spend limit `max_amount` '{}' exceeds the U256 range",
                    limit.max_amount
                )
            })?;
            let ceiling = QuoteSpendCeiling {
                spend_token,
                spend_token_decimals: limit.spend_token_decimals,
                max_amount,
            };

            if parsed_quote_spend_limits
                .insert((token_in, token_out), ceiling)
                .is_some()
            {
                anyhow::bail!(
                    "Duplicate quote spend limit for token pair {token_in} -> {token_out}"
                );
            }
        }

        if slippage_bps > max_slippage_bps {
            anyhow::bail!(
                "`slippage_bps` {slippage_bps} exceeds `max_slippage_bps` {max_slippage_bps}"
            );
        }

        if max_slippage_bps >= BPS_DENOMINATOR {
            anyhow::bail!("`max_slippage_bps` {max_slippage_bps} must be below {BPS_DENOMINATOR}");
        }

        if !(1..=4_095).contains(&max_quote_age_blocks) {
            anyhow::bail!("`max_quote_age_blocks` must be in 1..=4095");
        }

        Ok(TransactionLimits {
            allowed_token_pairs: parsed_pairs,
            quote_spend_limits: parsed_quote_spend_limits,
            slippage_bps,
            max_slippage_bps,
            max_order_amount,
            deadline_seconds,
            max_quote_age_blocks,
            receipt_timeout_secs,
        })
    }

    fn validate_manifest_contracts(
        config: &BlockchainExecutionClientConfig,
        routers: &[Address],
        weth: Address,
    ) -> anyhow::Result<()> {
        let verification = config.verification.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Independent Blockchain execution verification is required")
        })?;
        let manifest = &verification.deployment_manifest;
        let role_addresses = |role| {
            manifest
                .contracts
                .iter()
                .filter(|contract| contract.role == role)
                .map(|contract| {
                    Address::from_str(&contract.address)
                        .map_err(|_| anyhow::anyhow!("Deployment manifest address is invalid"))
                })
                .collect::<anyhow::Result<HashSet<_>>>()
        };
        let singleton = |role, description: &str| {
            let addresses = role_addresses(role)?;
            anyhow::ensure!(
                addresses.len() == 1,
                "Deployment manifest must contain exactly one {description} contract"
            );
            Ok(*addresses.iter().next().expect("singleton role address"))
        };

        let configured_routers = routers.iter().copied().collect::<HashSet<_>>();
        anyhow::ensure!(
            role_addresses(BlockchainContractRole::Router)? == configured_routers,
            "Deployment manifest router set does not match `router_addresses`"
        );
        anyhow::ensure!(
            singleton(BlockchainContractRole::WrappedNative, "wrapped native")? == weth,
            "Deployment manifest wrapped native contract does not match `weth_address`"
        );
        let factory = singleton(BlockchainContractRole::Factory, "factory")?;
        let registered_factory =
            crate::exchanges::get_dex_extended(config.chain.name, &DexType::UniswapV3)
                .map(|dex| dex.factory)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No registered Uniswap V3 deployment for chain {}",
                        config.chain.name
                    )
                })?;
        anyhow::ensure!(
            factory == registered_factory,
            "Deployment manifest factory does not match the registered Uniswap V3 factory"
        );
        let quote_contract = singleton(BlockchainContractRole::Quote, "quote")?;

        let mut token_decimals = HashMap::new();

        for token in &manifest.tokens {
            let address = Address::from_str(&token.address)
                .map_err(|_| anyhow::anyhow!("Deployment manifest token address is invalid"))?;
            anyhow::ensure!(
                token_decimals.insert(address, token.decimals).is_none(),
                "Deployment manifest contains a duplicate token identity"
            );
        }
        anyhow::ensure!(
            token_decimals.contains_key(&weth),
            "Deployment manifest has no wrapped native token identity"
        );

        if let Some(tokens) = &config.tokens {
            for token in tokens {
                let address = validate_address(token)?;
                anyhow::ensure!(
                    token_decimals.contains_key(&address),
                    "Configured token {address} has no deployment manifest identity"
                );
            }
        }

        for (token_in, token_out) in config.allowed_token_pairs.as_deref().unwrap_or_default() {
            let token_in = validate_address(token_in)?;
            let token_out = validate_address(token_out)?;
            anyhow::ensure!(
                token_in != token_out
                    && token_decimals.contains_key(&token_in)
                    && token_decimals.contains_key(&token_out),
                "Allowed token pair {token_in} -> {token_out} is not fully pinned by the deployment manifest"
            );
        }

        for limit in config.quote_spend_limits.as_deref().unwrap_or_default() {
            let spend_token = validate_address(&limit.spend_token)?;
            anyhow::ensure!(
                token_decimals.get(&spend_token) == Some(&limit.spend_token_decimals),
                "Quote spend limit decimals do not match the deployment manifest"
            );
        }

        let pool_contracts = role_addresses(BlockchainContractRole::Pool)?;

        for pool in &manifest.pools {
            let pool_address = Address::from_str(&pool.address)
                .map_err(|_| anyhow::anyhow!("Deployment manifest pool address is invalid"))?;
            let pool_factory = Address::from_str(&pool.factory)
                .map_err(|_| anyhow::anyhow!("Deployment manifest pool factory is invalid"))?;
            let pool_quote = Address::from_str(&pool.quote_contract).map_err(|_| {
                anyhow::anyhow!("Deployment manifest pool quote contract is invalid")
            })?;
            anyhow::ensure!(
                pool_contracts.contains(&pool_address)
                    && pool_factory == factory
                    && pool_quote == quote_contract,
                "Deployment manifest pool does not use the pinned pool, factory, and quote identities"
            );
        }
        Ok(())
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
    /// persistence, or broadcast step fails. A persistence failure after signing, or a failed
    /// postcondition after finality, leaves the in-flight slot occupied.
    pub async fn wrap(&mut self, amount_wei: U256) -> anyhow::Result<B256> {
        if amount_wei.is_zero() {
            anyhow::bail!("Wrap amount must be positive");
        }

        self.ensure_transaction_ready(TransactionPurpose::Wrap)?;

        let calldata = WETH9::depositCall {}.abi_encode();
        let executor = self.transaction_executor()?;
        let included = executor
            .transact(
                self.weth_address,
                amount_wei,
                Bytes::from(calldata),
                TransactionPurpose::Wrap,
                None,
                TransactionAuthorization::Wrap {
                    weth: self.weth_address,
                },
            )
            .await?;
        let postconditions =
            verify_wrap_balance_increase(&executor, &self.weth_address, amount_wei, &included)
                .await?;
        executor
            .commit_verified_finality(&included, TransactionStatus::Finalized, &postconditions)
            .await?;
        executor
            .database
            .mark_execution_event_emitted(included.intent_id, "terminal")
            .await?;
        executor.release_slot();

        Ok(included.tx_hash)
    }

    /// Approves an allowlisted SwapRouter to spend `amount` of `token` via an ERC-20
    /// `approve` transaction. Zero always revokes the allowance. For nonzero requests,
    /// `unlimited_approval` changes the target to `U256::MAX`.
    ///
    /// This is an explicit operator operation; it never runs inside `submit_order`.
    ///
    /// # Errors
    ///
    /// Returns an error if the router or token fails policy and deployment checks, a nonzero
    /// allowance was not cleared first, approval simulation returns false or malformed data, the
    /// client is not connected, another transaction is in flight, no durable store is configured,
    /// the resulting allowance differs from the target, or any RPC, signing, persistence, or
    /// broadcast step fails. A persistence failure after signing, or a failed postcondition after
    /// finality, leaves the in-flight slot occupied.
    pub async fn approve(
        &mut self,
        token: Address,
        amount: U256,
        router: Address,
    ) -> anyhow::Result<B256> {
        if !self.router_addresses.contains(&router) {
            anyhow::bail!("Router {router} is not in the configured `router_addresses` allowlist");
        }

        if !amount.is_zero()
            && !self
                .transaction_limits
                .allowed_token_pairs
                .iter()
                .any(|(token_in, _)| *token_in == token)
        {
            anyhow::bail!(
                "Token {token} is not an input token in the configured `allowed_token_pairs`"
            );
        }

        self.ensure_transaction_ready(TransactionPurpose::Approve)?;

        let approval_amount = if amount.is_zero() {
            U256::ZERO
        } else if self.config.unlimited_approval {
            U256::MAX
        } else {
            amount
        };
        let calldata = ERC20::approveCall {
            spender: router,
            amount: approval_amount,
        }
        .abi_encode();

        let executor = self.transaction_executor()?;
        let included = executor
            .transact(
                token,
                U256::ZERO,
                Bytes::from(calldata),
                TransactionPurpose::Approve,
                None,
                TransactionAuthorization::Approve {
                    token,
                    router,
                    amount: approval_amount,
                },
            )
            .await?;
        let postconditions =
            verify_approve_allowance(&executor, &token, &router, approval_amount, &included)
                .await?;
        executor
            .commit_verified_finality(&included, TransactionStatus::Finalized, &postconditions)
            .await?;
        executor
            .database
            .mark_execution_event_emitted(included.intent_id, "terminal")
            .await?;
        executor.release_slot();

        Ok(included.tx_hash)
    }

    fn uniswap_v3_factory(&self) -> anyhow::Result<Address> {
        crate::exchanges::get_dex_extended(self.chain.name, &DexType::UniswapV3)
            .map(|dex| dex.factory)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No registered Uniswap V3 deployment for chain {}",
                    self.chain.name
                )
            })
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

    /// Authenticates every persisted signed transaction in this execution database.
    ///
    /// Run this while the execution client is disconnected. The check takes a stable table lock,
    /// reads in bounded batches, and returns counts, deployment identity, key IDs, and database
    /// roles with direct ownership or `SELECT` grants.
    ///
    /// # Errors
    ///
    /// Returns an error for inconsistent storage state, missing keys, or any payload which cannot
    /// be opened and authenticated against its durable intent.
    pub async fn check_payload_storage(
        &self,
        batch_size: usize,
    ) -> anyhow::Result<PayloadStorageCheck> {
        let batch_size = validate_payload_operation_batch_size(batch_size)?;
        let database = self.payload_operation_database().await?;
        let keys = self.load_payload_keys()?;
        database
            .check_execution_payload_storage(keys.as_ref(), None, batch_size)
            .await
            .map(Into::into)
    }

    /// Activates or resumes protected signed-transaction storage for this execution database.
    ///
    /// Run this while the execution client is disconnected. A full payload check must succeed
    /// before a later execution connection is permitted.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is connected, an active key or deployment identity is
    /// unavailable, or any stored payload cannot be migrated and authenticated.
    pub async fn protect_payload_storage(&self) -> anyhow::Result<()> {
        let database = self.payload_operation_database().await?;
        database.ensure_execution_transaction_schema().await?;
        let keys = self
            .load_payload_keys()?
            .ok_or_else(|| anyhow::anyhow!("Payload protection requires an active payload key"))?;
        database.ensure_execution_payload_storage(&keys).await
    }

    /// Rewraps all protected payloads in this execution database with the configured active key.
    ///
    /// The prior active key must remain configured as a retired key until this method and a
    /// subsequent full check both succeed.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is connected, storage is not protected, required keys are
    /// unavailable, or any bounded rewrap batch fails authentication.
    pub async fn rewrap_payload_storage(&self, batch_size: usize) -> anyhow::Result<()> {
        let batch_size = validate_payload_operation_batch_size(batch_size)?;
        let database = self.payload_operation_database().await?;
        let keys = self
            .load_payload_keys()?
            .ok_or_else(|| anyhow::anyhow!("Payload rewrap requires an active payload key"))?;
        database
            .rewrap_execution_payload_storage(&keys, batch_size)
            .await
    }

    /// Restores authenticated plaintext payloads and removes protection from this database.
    ///
    /// This incident-only operation is resumable. Keep the complete key set configured until it
    /// succeeds and the unprotected database passes a full payload check.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is connected, storage is not protected, required keys are
    /// unavailable, or any bounded rollback batch fails authentication.
    pub async fn rollback_payload_storage(&self, batch_size: usize) -> anyhow::Result<()> {
        let batch_size = validate_payload_operation_batch_size(batch_size)?;
        let database = self.payload_operation_database().await?;
        let keys = self
            .load_payload_keys()?
            .ok_or_else(|| anyhow::anyhow!("Payload rollback requires an active payload key"))?;
        database
            .rollback_execution_payload_storage(&keys, batch_size)
            .await
    }

    async fn payload_operation_database(&self) -> anyhow::Result<BlockchainCacheDatabase> {
        anyhow::ensure!(
            !self.core.is_connected(),
            "Disconnect the execution client before payload storage operations"
        );

        if let Some(database) = &self.cache.database {
            return Ok(database.clone());
        }
        let options = self
            .config
            .postgres_cache_database_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No Postgres cache database is configured"))?;
        BlockchainCacheDatabase::connect(options.clone().into())
            .await
            .context("failed to connect to the execution database")
    }

    fn load_payload_keys(&self) -> anyhow::Result<Option<PayloadKeySet>> {
        PayloadKeySet::load(
            self.config.payload_key_env.as_deref(),
            &self.config.payload_key_retired_env,
            self.config.payload_deployment_id.as_deref(),
        )
    }

    fn payload_policy(&self) -> PayloadPolicy {
        PayloadPolicy {
            chain_id: self.chain.chain_id,
            signer: self.wallet_address,
            gas_limit: self.config.gas_limit,
            max_fee_per_gas: self.config.max_fee_per_gas_wei,
        }
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
        let payload_keys = self.payload_keys.clone().ok_or_else(|| {
            anyhow::anyhow!("Protected payload keys are not initialized; connect the client first")
        })?;
        let verification_config = self
            .config
            .verification
            .as_ref()
            .expect("verification config validated at construction");
        let identities = std::iter::once(&verification_config.authoritative)
            .chain(
                verification_config
                    .verifiers
                    .iter()
                    .map(|provider| &provider.identity),
            )
            .collect::<Vec<_>>();

        Ok(TransactionExecutor {
            http_rpc_client: self.http_rpc_client.clone(),
            verification: self.verification.clone(),
            manifest_version: verification_config.manifest_version.clone(),
            manifest_digest: verification_config.manifest_digest.clone(),
            deployment_manifest: Arc::new(verification_config.deployment_manifest.clone()),
            provider_ids: identities
                .iter()
                .map(|identity| identity.provider_id.clone())
                .collect(),
            operator_ids: identities
                .iter()
                .map(|identity| identity.operator_id.clone())
                .collect(),
            failure_domain_ids: identities
                .iter()
                .flat_map(|identity| identity.failure_domain_ids.iter().cloned())
                .collect(),
            database,
            signer,
            payload_keys,
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
        let (token_in, token_out) = swap_token_pair(
            order.order_side(),
            pool.get_base_token().address,
            quote_token.address,
        )?;
        let factory = self.uniswap_v3_factory()?;
        anyhow::ensure!(
            pool.dex.factory == factory,
            "Restored pool {instrument_id} references factory {}, expected registered factory {factory}",
            pool.dex.factory
        );

        Ok(SwapPlan {
            order,
            quote_currency,
            pool,
            instrument_id,
            pool_address,
            router: Address::from_str(&intent.transaction_to)?,
            factory,
            weth: self.weth_address,
            token_in,
            token_out,
            fee,
            amount_in,
            min_amount_out: U256::ZERO,
            slippage_bps: 0,
            quote_spend_ceiling: None,
            profiler_position: None,
        })
    }

    async fn reconcile_unresolved_execution(&self) -> anyhow::Result<()> {
        let database = self.cache.database.clone().ok_or_else(|| {
            anyhow::anyhow!("No durable store configured for execution reconciliation")
        })?;
        let _payload_lease = database
            .acquire_execution_payload_lease(self.payload_keys.as_deref().ok_or_else(|| {
                anyhow::anyhow!("Protected payload keys are required for execution recovery")
            })?)
            .await?;
        let wallet_address = self.wallet_address.to_string();
        anyhow::ensure!(
            !database
                .has_recoverable_signed_execution(self.chain.chain_id, &wallet_address)
                .await?,
            "A recoverable execution for wallet {} retains signed transaction bytes; refusing to reuse its nonce without explicit recovery",
            self.wallet_address
        );
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

        if intent.status == "prepared" {
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
        *self.in_flight.lock().expect("in-flight mutex poisoned") =
            Some(InFlightSlot::Recovering(RecoveryTransaction {
                intent_id: intent.id,
                nonce,
                purpose,
            }));
        let hashes = database.get_execution_transaction_hashes(intent.id).await?;
        let current = current_execution_hash(intent.id, &hashes)?;
        let tx_hash = B256::from_str(&current.transaction_hash).with_context(|| {
            format!(
                "Execution intent {} has invalid transaction hash {}",
                intent.id, current.transaction_hash
            )
        })?;
        let policy = PayloadPolicy {
            chain_id: self.chain.chain_id,
            signer: self.wallet_address,
            gas_limit: self.config.gas_limit,
            max_fee_per_gas: self.config.max_fee_per_gas_wei,
        };
        let mut authenticated_payloads = HashMap::new();
        let mut current_payload = None;

        for hash in &hashes {
            anyhow::ensure!(
                hash.intent_id == intent.id,
                "Persisted transaction row references intent {}, expected {}",
                hash.intent_id,
                intent.id
            );
            anyhow::ensure!(
                hash.chain_id == self.chain.chain_id,
                "Persisted transaction row chain ID {} does not match configured chain ID {}",
                hash.chain_id,
                self.chain.chain_id
            );

            if !hash.payload_expected {
                anyhow::ensure!(
                    hash.raw_transaction.is_none() && hash.sealed_transaction.is_none(),
                    "Replacement transaction {} unexpectedly retains signed bytes",
                    hash.transaction_hash
                );
                continue;
            }
            let raw_transaction = open_execution_payload(
                self.payload_keys
                    .as_deref()
                    .expect("payload keys checked above"),
                policy,
                &intent,
                hash,
                "recovery",
            )
            .map_err(|e| {
                anyhow::anyhow!(
                    "Execution intent {} signed transaction {} failed authentication: {e}",
                    intent.id,
                    hash.transaction_hash
                )
            })?;
            let authenticated_hash = B256::from_str(&hash.transaction_hash).with_context(|| {
                format!(
                    "Execution intent {} has invalid transaction hash {}",
                    intent.id, hash.transaction_hash
                )
            })?;
            anyhow::ensure!(
                authenticated_payloads
                    .insert(authenticated_hash, raw_transaction.clone())
                    .is_none(),
                "Execution intent {} has duplicate authenticated transaction hash {}",
                intent.id,
                hash.transaction_hash
            );

            if hash.id == current.id {
                current_payload = Some(raw_transaction);
            }
        }
        anyhow::ensure!(
            !authenticated_payloads.is_empty(),
            "Execution intent {} has no persisted signed transaction bytes",
            intent.id
        );

        if intent.status == "broadcast" {
            anyhow::ensure!(
                current_payload.is_some(),
                "Broadcast execution intent {} has no persisted signed transaction bytes",
                intent.id
            );
        }

        if intent.status == "signed" {
            anyhow::bail!(
                "Execution intent {} has a signed transaction {} that was not authorized for broadcast; its nonce remains reserved pending explicit recovery",
                intent.id,
                tx_hash
            );
        }

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
        let mut prepared = PreparedTransaction {
            intent_id: intent.id,
            created_block: intent.created_block,
            nonce,
            tx_hash,
            raw_tx: current_payload.unwrap_or_default(),
            payload_lease: None,
        };

        let finality_already_committed = matches!(intent.status.as_str(), "finalized" | "reverted");
        if !finality_already_committed {
            match executor
                .authorize_rebroadcast(&prepared, &intent, purpose)
                .await?
            {
                ReconciliationAuthorization::Rebroadcast => {
                    match executor.broadcast(&prepared).await? {
                        BroadcastOutcome::Accepted => {}
                        BroadcastOutcome::Ambiguous(message) => log::warn!("{message}"),
                    }
                }
                ReconciliationAuthorization::Retain => {
                    log::warn!(
                        "Rebroadcast of transaction {} was suppressed by verified reconciliation state",
                        prepared.tx_hash
                    );
                }
                ReconciliationAuthorization::ScanReplacement(head) => {
                    let Some((replacement_hash, replacement_payload)) = executor
                        .scan_canonical_replacement(&intent, nonce, head, &authenticated_payloads)
                        .await?
                    else {
                        log::warn!(
                            "Canonical replacement scan for intent {} reached its bounded verified window",
                            intent.id
                        );
                        return Ok(());
                    };
                    prepared.tx_hash = replacement_hash;
                    prepared.raw_tx = replacement_payload;
                    *self.in_flight.lock().expect("in-flight mutex poisoned") =
                        Some(InFlightSlot::AwaitingFinality(InFlightTransaction {
                            intent_id: intent.id,
                            nonce,
                            tx_hash: replacement_hash,
                            purpose,
                        }));
                }
            }
        }
        let outcome = if finality_already_committed {
            let receipt = verified_value(
                executor.verification.verify_receipt(&tx_hash).await,
                "persisted terminal receipt",
            )?;
            let finality = executor
                .receipt_is_stably_finalized(&receipt)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Persisted terminal transaction {tx_hash} is not stable at the finalized boundary"
                    )
                })?;
            let included = IncludedTransaction {
                intent_id: intent.id,
                nonce,
                tx_hash,
                block_number: receipt.block_number,
                receipt,
                finality,
            };

            if intent.status == "finalized" {
                InclusionOutcome::Finalized(included)
            } else {
                InclusionOutcome::Reverted(included)
            }
        } else {
            executor.await_finality(&prepared).await?
        };

        match outcome {
            InclusionOutcome::Finalized(mut included) => {
                let trace_purpose = match (&plan, purpose) {
                    (Some(plan), TransactionPurpose::Swap) => match plan.order.order_side() {
                        OrderSide::Sell => "swap_sell",
                        OrderSide::Buy => "swap_buy",
                    },
                    (None, TransactionPurpose::Wrap) => "wrap",
                    (None, TransactionPurpose::Approve) => "approve",
                    _ => anyhow::bail!("Restored transaction purpose is inconsistent"),
                };
                included.finality.decisions.extend(
                    verify_finalized_transaction(
                        &included,
                        &intent,
                        nonce,
                        &prepared.raw_tx,
                        &executor,
                        trace_purpose,
                    )
                    .await?,
                );

                if let Some(plan) = plan {
                    let fill = validate_finalized_swap_fill(&plan, &included)?;
                    let wallet =
                        load_verified_wallet_after_fill(&plan, &included, &executor).await?;
                    if !finality_already_committed {
                        executor
                            .commit_verified_finality(
                                &included,
                                TransactionStatus::Finalized,
                                &wallet.decisions,
                            )
                            .await?;
                    }
                    complete_finalized_swap(
                        &plan,
                        intent.id,
                        included.tx_hash,
                        fill,
                        wallet,
                        &executor,
                        &self.emitter,
                    )
                    .await?;
                    executor.release_slot();
                } else {
                    let postconditions = self
                        .verify_recovered_operator_transaction(
                            &intent, purpose, &included, &executor,
                        )
                        .await?;

                    if !finality_already_committed {
                        executor
                            .commit_verified_finality(
                                &included,
                                TransactionStatus::Finalized,
                                &postconditions,
                            )
                            .await?;
                    }
                    database
                        .mark_execution_event_emitted(intent.id, "terminal")
                        .await?;
                    executor.release_slot();
                }
            }
            InclusionOutcome::Reverted(mut included) => {
                let trace_purpose = match (&plan, purpose) {
                    (Some(plan), TransactionPurpose::Swap) => match plan.order.order_side() {
                        OrderSide::Sell => "swap_sell",
                        OrderSide::Buy => "swap_buy",
                    },
                    (None, TransactionPurpose::Wrap) => "wrap",
                    (None, TransactionPurpose::Approve) => "approve",
                    _ => anyhow::bail!("Restored transaction purpose is inconsistent"),
                };
                included.finality.decisions.extend(
                    verify_finalized_transaction(
                        &included,
                        &intent,
                        nonce,
                        &prepared.raw_tx,
                        &executor,
                        trace_purpose,
                    )
                    .await?,
                );

                if !finality_already_committed {
                    executor
                        .commit_verified_finality(&included, TransactionStatus::Reverted, &[])
                        .await?;
                }

                if let Some(plan) = plan
                    && plan.order.status() != OrderStatus::Rejected
                {
                    send_reverted_order(&self.emitter, &plan.order, &included)?;
                }
                database
                    .mark_execution_event_emitted(intent.id, "terminal")
                    .await?;
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
    async fn verify_recovered_operator_transaction(
        &self,
        intent: &ExecutionIntentRow,
        purpose: TransactionPurpose,
        included: &IncludedTransaction,
        executor: &TransactionExecutor,
    ) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
        let (to, input, value) = persisted_call_fields(intent)?;

        match purpose {
            TransactionPurpose::Wrap => {
                verify_wrap_balance_increase(executor, &to, value, included).await
            }
            TransactionPurpose::Approve => {
                let call = ERC20::approveCall::abi_decode(&input)
                    .with_context(|| "persisted approve calldata is invalid")?;
                verify_approve_allowance(executor, &to, &call.spender, call.amount, included).await
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

        if !matches!(order.order_side(), OrderSide::Buy | OrderSide::Sell) {
            anyhow::bail!(
                "Unsupported order side {}; only Buy and Sell are supported",
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
        let (token_in, token_out) =
            swap_token_pair(order.order_side(), base_token.address, quote_token.address)?;

        if !self
            .transaction_limits
            .allowed_token_pairs
            .contains(&(token_in, token_out))
        {
            anyhow::bail!(
                "Token pair {token_in} -> {token_out} is not in the `allowed_token_pairs` allowlist"
            );
        }

        let base_amount = quantity_to_raw_amount(order.quantity(), base_token.decimals)?;
        if base_amount > U256::from(self.transaction_limits.max_order_amount) {
            anyhow::bail!(
                "Order amount {base_amount} exceeds the configured `max_order_amount` {}",
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

        let quote_spend_ceiling = if order.order_side() == OrderSide::Buy {
            let ceiling = self
                .transaction_limits
                .quote_spend_limits
                .get(&(token_in, token_out))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No `quote_spend_limits` entry for BUY token pair {token_in} -> {token_out}"
                    )
                })?;
            anyhow::ensure!(
                ceiling.spend_token == quote_token.address,
                "Quote spend limit for {token_in} -> {token_out} is denominated in {}, expected quote token {}",
                ceiling.spend_token,
                quote_token.address
            );
            anyhow::ensure!(
                ceiling.spend_token_decimals == quote_token.decimals,
                "Quote spend limit for token {} uses {} decimals, expected pool quote-token decimals {}",
                ceiling.spend_token,
                ceiling.spend_token_decimals,
                quote_token.decimals
            );
            Some(ceiling)
        } else {
            None
        };

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
        let profiler_position = profiler.last_processed_event.clone().ok_or_else(|| {
            anyhow::anyhow!("Pool profiler for {instrument_id} has processed no events")
        })?;

        let zero_for_one = token_in == pool.token0.address;
        let (amount_in, quoted_amount_out) = match order.order_side() {
            OrderSide::Sell => {
                let quote = profiler
                    .swap_exact_in(base_amount, zero_for_one, None)
                    .map_err(|e| anyhow::anyhow!("Swap quote failed for {instrument_id}: {e}"))?;
                let amount_filled = if zero_for_one {
                    quote.amount0
                } else {
                    quote.amount1
                };

                if amount_filled != I256::from(base_amount) {
                    anyhow::bail!(
                        "Local quote for {instrument_id} filled {amount_filled} of the {base_amount} order amount; pool liquidity cannot fill the order"
                    );
                }
                (base_amount, exact_output_amount(&quote, zero_for_one)?)
            }
            OrderSide::Buy => {
                let quote = profiler
                    .swap_exact_out(base_amount, zero_for_one, None)
                    .map_err(|e| anyhow::anyhow!("Swap quote failed for {instrument_id}: {e}"))?;
                let amount_in = quote.get_input_amount();
                if amount_in.is_zero() {
                    anyhow::bail!("Local quote for {instrument_id} produced a zero quote input");
                }
                let ceiling = quote_spend_ceiling.ok_or_else(|| {
                    anyhow::anyhow!(
                        "No `quote_spend_limits` entry for BUY token pair {token_in} -> {token_out}"
                    )
                })?;

                if amount_in > ceiling.max_amount {
                    anyhow::bail!(
                        "BUY quote amount {amount_in} exceeds the configured `quote_spend_limits` maximum {} for {token_in} -> {token_out}",
                        ceiling.max_amount
                    );
                }
                (amount_in, base_amount)
            }
        };
        let min_amount_out = derive_min_amount_out(quoted_amount_out, slippage_bps)?;

        self.ensure_transaction_ready(TransactionPurpose::Swap)?;

        if self.signer.is_none() {
            anyhow::bail!("Signer not initialized; connect the client first");
        }

        let pool_address = pool.address;
        let factory = self.uniswap_v3_factory()?;
        anyhow::ensure!(
            pool.dex.factory == factory,
            "Pool {instrument_id} references factory {}, expected registered factory {factory}",
            pool.dex.factory
        );

        Ok(SwapPlan {
            order: order.clone(),
            pool,
            quote_currency,
            instrument_id,
            pool_address,
            router: self.router_addresses[0],
            factory,
            weth: self.weth_address,
            token_in,
            token_out,
            fee,
            amount_in,
            min_amount_out,
            slippage_bps,
            quote_spend_ceiling: quote_spend_ceiling.copied(),
            profiler_position: Some(profiler_position),
        })
    }
}

/// A locally signed EIP-1559 transaction ready for persist-before-broadcast.
struct PreparedTransaction {
    intent_id: i64,
    created_block: u64,
    nonce: u64,
    tx_hash: B256,
    raw_tx: Vec<u8>,
    payload_lease: Option<ExecutionPayloadLease>,
}

impl Debug for PreparedTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PreparedTransaction))
            .field("intent_id", &self.intent_id)
            .field("created_block", &self.created_block)
            .field("nonce", &self.nonce)
            .field("tx_hash", &self.tx_hash)
            .field("raw_tx", &"<redacted>")
            .field(
                "payload_lease",
                &self.payload_lease.as_ref().map(|_| "held"),
            )
            .finish()
    }
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
    Reverted(IncludedTransaction),
    /// No receipt arrived within the poll budget or observation failed; the record stays
    /// pending and the in-flight slot occupied.
    Pending(String),
}

enum ReconciliationAuthorization {
    Rebroadcast,
    Retain,
    ScanReplacement(VerifiedBlockHeader),
}

fn verified_value<T>(outcome: VerificationOutcome<T>, context: &str) -> anyhow::Result<T> {
    required_verification(outcome, context).map(|verified| verified.value)
}

fn required_verification<T>(
    outcome: VerificationOutcome<T>,
    context: &str,
) -> anyhow::Result<Verified<T>> {
    match outcome {
        VerificationOutcome::Verified(verified) => Ok(verified),
        VerificationOutcome::Disagreement(_) => {
            anyhow::bail!("{context} verification disagreed")
        }
        VerificationOutcome::Unavailable(_) => {
            anyhow::bail!("{context} verification is unavailable")
        }
        VerificationOutcome::Retryable(_) => {
            anyhow::bail!("{context} verification is retryable")
        }
        VerificationOutcome::LocallyInvalid(_) => {
            anyhow::bail!("{context} verification is locally invalid")
        }
    }
}

fn validate_transaction_authorization(
    authorization: Option<&TransactionAuthorization>,
    to: Address,
    value: U256,
    input: &[u8],
) -> anyhow::Result<()> {
    match authorization {
        None => Ok(()),
        Some(TransactionAuthorization::Wrap { weth }) => {
            anyhow::ensure!(
                to == *weth && !value.is_zero() && input == WETH9::depositCall::SELECTOR,
                "Wrap authorization does not match the transaction call"
            );
            Ok(())
        }
        Some(TransactionAuthorization::Approve {
            token,
            router,
            amount,
        }) => {
            let expected = ERC20::approveCall {
                spender: *router,
                amount: *amount,
            }
            .abi_encode();
            anyhow::ensure!(
                to == *token && value.is_zero() && input == expected,
                "Approve authorization does not match the transaction call"
            );
            Ok(())
        }
    }
}

fn verification_decision<T>(
    verified: &Verified<T>,
    height_start: Option<u64>,
    height_end: Option<u64>,
) -> ExecutionVerificationDecision {
    ExecutionVerificationDecision {
        read_class: verified.read.as_str(),
        height_start,
        height_end,
        normalized_value_digest: verified.normalized_value_digest.to_string(),
    }
}

fn parse_verified_header(header: &ExecutionVerifiedHeader) -> anyhow::Result<VerifiedBlockHeader> {
    Ok(VerifiedBlockHeader {
        number: header.number,
        hash: B256::from_str(&header.hash).context("Durable finalized header hash is invalid")?,
        parent_hash: B256::from_str(&header.parent_hash)
            .context("Durable finalized parent hash is invalid")?,
        timestamp: header.timestamp,
        base_fee_per_gas: header.base_fee_per_gas,
    })
}

fn durable_verified_header(header: &VerifiedBlockHeader) -> ExecutionVerifiedHeader {
    ExecutionVerifiedHeader {
        number: header.number,
        hash: header.hash.to_string(),
        parent_hash: header.parent_hash.to_string(),
        timestamp: header.timestamp,
        base_fee_per_gas: header.base_fee_per_gas,
    }
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
    verification: VerificationCoordinator,
    manifest_version: String,
    manifest_digest: String,
    deployment_manifest: Arc<BlockchainDeploymentManifest>,
    provider_ids: Vec<String>,
    operator_ids: Vec<String>,
    failure_domain_ids: Vec<String>,
    database: BlockchainCacheDatabase,
    signer: Arc<PrivateKeySigner>,
    payload_keys: Arc<PayloadKeySet>,
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
        authorization: TransactionAuthorization,
    ) -> anyhow::Result<IncludedTransaction> {
        self.claim_slot(purpose)?;
        let now_unix_secs = current_unix_secs()?;
        let decision_header = match required_verification(
            self.verification
                .verify_decision_header(now_unix_secs)
                .await,
            "operator decision header",
        ) {
            Ok(header) => header,
            Err(e) => {
                release_preparing_slot(&self.in_flight);
                return Err(e);
            }
        };
        let created_block = decision_header.value.number;
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
        let intent = match self.database.reserve_execution_intent(&intent).await {
            Ok(intent) => intent,
            Err(e) => {
                release_preparing_if_reservation_not_committed(&self.in_flight, &e);
                return Err(e);
            }
        };
        let prepared = match self
            .prepare_and_sign(
                intent.id,
                intent.created_block,
                to,
                value,
                input,
                &authorization,
                decision_header,
            )
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
            InclusionOutcome::Finalized(mut included) => {
                included.finality.decisions.extend(
                    verify_finalized_transaction(
                        &included,
                        &intent,
                        prepared.nonce,
                        &prepared.raw_tx,
                        self,
                        purpose.as_str(),
                    )
                    .await?,
                );
                Ok(included)
            }
            InclusionOutcome::Reverted(mut included) => {
                included.finality.decisions.extend(
                    verify_finalized_transaction(
                        &included,
                        &intent,
                        prepared.nonce,
                        &prepared.raw_tx,
                        self,
                        purpose.as_str(),
                    )
                    .await?,
                );
                self.commit_verified_finality(&included, TransactionStatus::Reverted, &[])
                    .await?;
                self.database
                    .mark_execution_event_emitted(prepared.intent_id, "terminal")
                    .await?;
                self.release_slot();
                anyhow::bail!("Transaction {} reverted on-chain", included.tx_hash)
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
    #[expect(
        clippy::too_many_arguments,
        reason = "Security-critical transaction fields stay explicit at the signing boundary"
    )]
    async fn prepare_and_sign(
        &self,
        intent_id: i64,
        created_block: u64,
        to: Address,
        value: U256,
        input: Bytes,
        authorization: &TransactionAuthorization,
        decision_header: Verified<VerifiedBlockHeader>,
    ) -> anyhow::Result<PreparedTransaction> {
        self.prepare_and_sign_with_anchors(
            intent_id,
            created_block,
            to,
            value,
            input,
            None,
            Some(authorization),
            Some(decision_header),
        )
        .await
    }

    async fn prepare_and_sign_swap(
        &self,
        intent_id: i64,
        created_block: u64,
        to: Address,
        value: U256,
        input: Bytes,
        anchors: &SwapQuoteAnchors,
    ) -> anyhow::Result<PreparedTransaction> {
        self.prepare_and_sign_with_anchors(
            intent_id,
            created_block,
            to,
            value,
            input,
            Some(anchors),
            None,
            None,
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Security-critical transaction fields and verification anchors stay explicit"
    )]
    async fn prepare_and_sign_with_anchors(
        &self,
        intent_id: i64,
        created_block: u64,
        to: Address,
        value: U256,
        input: Bytes,
        swap_anchors: Option<&SwapQuoteAnchors>,
        authorization: Option<&TransactionAuthorization>,
        decision_header: Option<Verified<VerifiedBlockHeader>>,
    ) -> anyhow::Result<PreparedTransaction> {
        let expected_chain_id = u64::from(self.chain_id);
        let chain_id_verification = required_verification(
            self.verification.verify_chain_id().await,
            "pre-sign chain ID",
        )?;
        let actual_chain_id = chain_id_verification.value;
        anyhow::ensure!(
            actual_chain_id == expected_chain_id,
            "Verified chain ID does not match the transaction chain"
        );
        let decision_header_verification = if let Some(anchors) = swap_anchors {
            required_verification(
                self.verification.verify_block(anchors.state.number).await,
                "pre-sign swap decision header reread",
            )?
        } else {
            match decision_header {
                Some(verified) => verified,
                None => {
                    let now_unix_secs = current_unix_secs()?;
                    required_verification(
                        self.verification
                            .verify_decision_header(now_unix_secs)
                            .await,
                        "pre-sign decision header",
                    )?
                }
            }
        };
        let decision_header = decision_header_verification.value;

        if let Some(anchors) = swap_anchors {
            anyhow::ensure!(
                decision_header == anchors.state,
                "Verified swap decision header changed before signing"
            );
        }
        let decision_ancestry = self.verify_decision_ancestry(decision_header).await?;
        validate_transaction_authorization(authorization, to, value, &input)?;
        let deployment_verification = required_verification(
            self.verification
                .verify_deployment_manifest(&self.deployment_manifest, decision_header.number)
                .await,
            "pre-sign deployment manifest",
        )?;
        let authorization_decisions = match authorization {
            Some(authorization) => {
                self.verify_transaction_authorization(authorization, decision_header.number)
                    .await?
            }
            None => Vec::new(),
        };
        let base_fee_per_gas_wei = decision_header.base_fee_per_gas.ok_or_else(|| {
            anyhow::anyhow!(
                "Verified decision block {} has no base fee",
                decision_header.number
            )
        })?;
        let priority_fee_verification = required_verification(
            self.verification.verify_priority_fee().await,
            "pre-sign priority fee",
        )?;
        let priority_fee_per_gas_wei = priority_fee_verification.value;
        let (max_fee_per_gas, max_priority_fee_per_gas) = derive_fees(
            base_fee_per_gas_wei,
            priority_fee_per_gas_wei,
            self.base_fee_buffer_bps,
            u128::from(self.max_fee_per_gas_wei),
        )?;
        let gas_estimate_verification = required_verification(
            self.verification
                .verify_gas_estimate(
                    &self.wallet_address,
                    &to,
                    value,
                    &input,
                    decision_header.number,
                )
                .await,
            "pre-sign gas estimate",
        )?;
        let gas_estimate = gas_estimate_verification.value;
        let gas_limit = derive_gas_limit(gas_estimate, self.gas_buffer_bps, self.gas_limit)?;
        let max_gas_cost = U256::from(gas_limit)
            .checked_mul(U256::from(max_fee_per_gas))
            .ok_or_else(|| anyhow::anyhow!("Maximum gas cost overflow"))?;
        let max_transaction_cost = value
            .checked_add(max_gas_cost)
            .ok_or_else(|| anyhow::anyhow!("Maximum transaction cost overflow"))?;
        let native_balance_verification = required_verification(
            self.verification
                .verify_balance(&self.wallet_address, decision_header.number)
                .await,
            "pre-sign native balance",
        )?;
        let native_balance = native_balance_verification.value;

        if native_balance < max_transaction_cost {
            anyhow::bail!(
                "Native currency balance {native_balance} wei is below maximum transaction cost {max_transaction_cost} wei"
            );
        }
        let decision_height = Some(decision_header.number);
        let mut decisions = vec![
            verification_decision(&chain_id_verification, None, None),
            verification_decision(
                &decision_header_verification,
                decision_height,
                decision_height,
            ),
            verification_decision(&deployment_verification, decision_height, decision_height),
            verification_decision(&priority_fee_verification, None, None),
            verification_decision(&gas_estimate_verification, decision_height, decision_height),
            verification_decision(
                &native_balance_verification,
                decision_height,
                decision_height,
            ),
        ];
        decisions.extend(decision_ancestry);
        decisions.extend(authorization_decisions);
        if let Some(anchors) = swap_anchors {
            decisions.extend(self.verify_swap_anchors_before_sign(anchors).await?);
            decisions.extend(anchors.precondition_decisions.iter().cloned());
        } else {
            decisions.extend(self.verify_pre_sign_header_fence(decision_header).await?);
        }
        let canonical_nonce_verification = required_verification(
            self.verification
                .verify_transaction_count(&self.wallet_address, decision_header.number)
                .await,
            "pre-sign canonical nonce reread",
        )?;
        let pending_nonce_verification = required_verification(
            self.verification
                .verify_pending_transaction_count(&self.wallet_address)
                .await,
            "pre-sign pending nonce reread",
        )?;
        anyhow::ensure!(
            pending_nonce_verification.value == canonical_nonce_verification.value,
            "Pending nonce does not match the verified canonical nonce"
        );
        let nonce = canonical_nonce_verification.value;
        decisions.push(verification_decision(
            &canonical_nonce_verification,
            decision_height,
            decision_height,
        ));
        decisions.push(verification_decision(
            &pending_nonce_verification,
            None,
            None,
        ));
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
        let wallet_address = self.wallet_address.to_string();
        self.database
            .assign_execution_intent_nonce_verified(&ExecutionNonceAssignment {
                intent_id,
                chain_id: self.chain_id,
                wallet_address: &wallet_address,
                nonce,
                manifest_version: &self.manifest_version,
                manifest_digest: &self.manifest_digest,
                provider_ids: &self.provider_ids,
                operator_ids: &self.operator_ids,
                failure_domain_ids: &self.failure_domain_ids,
                decisions: &decisions,
            })
            .await?;
        let payload_lease = self
            .database
            .acquire_execution_payload_lease(&self.payload_keys)
            .await?;
        let (tx_hash, raw_tx) = sign_eip1559_transaction(tx, &self.signer).await?;

        Ok(PreparedTransaction {
            intent_id,
            created_block,
            nonce,
            tx_hash,
            raw_tx,
            payload_lease: Some(payload_lease),
        })
    }

    async fn verify_pre_sign_header_fence(
        &self,
        target: VerifiedBlockHeader,
    ) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
        let checkpoint = required_verification(
            self.verification.verify_checkpoint().await,
            "pre-sign checkpoint reread",
        )?;
        let header = required_verification(
            self.verification.verify_block(target.number).await,
            "pre-sign decision header reread",
        )?;
        anyhow::ensure!(
            header.value == target,
            "Decision header changed before signing"
        );
        Ok(vec![
            verification_decision(
                &checkpoint,
                Some(checkpoint.value.number),
                Some(checkpoint.value.number),
            ),
            verification_decision(&header, Some(target.number), Some(target.number)),
        ])
    }

    async fn verify_decision_ancestry(
        &self,
        target: VerifiedBlockHeader,
    ) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
        let checkpoint = required_verification(
            self.verification.verify_checkpoint().await,
            "pre-sign checkpoint",
        )?;
        anyhow::ensure!(
            checkpoint.value.number <= target.number,
            "Pre-sign decision header precedes the trusted checkpoint"
        );
        let mut decisions = vec![verification_decision(
            &checkpoint,
            Some(checkpoint.value.number),
            Some(checkpoint.value.number),
        )];
        let wallet_address = self.wallet_address.to_string();
        let position = self
            .database
            .load_execution_verification_position(
                self.chain_id,
                &wallet_address,
                &self.manifest_version,
                &self.manifest_digest,
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("Execution verification ledger is not initialized"))?;
        let durable_tip = parse_verified_header(&position.finalized_tip)?;
        anyhow::ensure!(
            durable_tip.number >= checkpoint.value.number && durable_tip.number <= target.number,
            "Pre-sign decision header does not extend the durable finalized header tip"
        );
        let durable_tip_verification = required_verification(
            self.verification.verify_block(durable_tip.number).await,
            "pre-sign durable finalized tip",
        )?;
        anyhow::ensure!(
            durable_tip_verification.value == durable_tip,
            "Durable finalized header tip conflicts with independent sources"
        );

        if durable_tip != checkpoint.value {
            decisions.push(verification_decision(
                &durable_tip_verification,
                Some(durable_tip.number),
                Some(durable_tip.number),
            ));
        }
        let mut cursor = durable_tip;
        while cursor.number < target.number {
            let end = cursor.number.saturating_add(4_096).min(target.number);
            let start = cursor.number.saturating_add(1);
            let ancestry = required_verification(
                self.verification.verify_header_window(cursor, end).await,
                "pre-sign decision ancestry",
            )?;
            cursor = *ancestry
                .value
                .last()
                .expect("nonempty decision ancestry advances the cursor");
            decisions.push(verification_decision(&ancestry, Some(start), Some(end)));
        }
        anyhow::ensure!(
            cursor == target,
            "Pre-sign decision header conflicts with its trusted ancestry"
        );
        Ok(decisions)
    }

    async fn verify_transaction_authorization(
        &self,
        authorization: &TransactionAuthorization,
        block: u64,
    ) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
        match *authorization {
            TransactionAuthorization::Wrap { weth } => {
                let call = ERC20::balanceOfCall {
                    account: self.wallet_address,
                }
                .abi_encode();
                let balance = required_verification(
                    self.verification
                        .verify_decoded_call(None, &weth, U256::ZERO, &call, block, |result| {
                            ERC20::balanceOfCall::abi_decode_returns(result).map_err(Into::into)
                        })
                        .await,
                    "pre-sign wrapped token probe",
                )?;
                Ok(vec![verification_decision(
                    &balance,
                    Some(block),
                    Some(block),
                )])
            }
            TransactionAuthorization::Approve {
                token,
                router,
                amount,
            } => {
                let allowance_call = ERC20::allowanceCall {
                    owner: self.wallet_address,
                    spender: router,
                }
                .abi_encode();
                let allowance = required_verification(
                    self.verification
                        .verify_decoded_call(
                            None,
                            &token,
                            U256::ZERO,
                            &allowance_call,
                            block,
                            |result| {
                                ERC20::allowanceCall::abi_decode_returns(result).map_err(Into::into)
                            },
                        )
                        .await,
                    "pre-sign router allowance",
                )?;
                anyhow::ensure!(
                    allowance.value.is_zero() || amount.is_zero(),
                    "Router allowance for token {token} is already {}; approve zero before setting a new nonzero allowance",
                    allowance.value
                );

                let approve_call = ERC20::approveCall {
                    spender: router,
                    amount,
                }
                .abi_encode();
                let simulation = required_verification(
                    self.verification
                        .verify_decoded_simulation(
                            &self.wallet_address,
                            &token,
                            U256::ZERO,
                            &approve_call,
                            block,
                            |result| {
                                if result.is_empty() {
                                    Ok(true)
                                } else {
                                    ERC20::approveCall::abi_decode_returns_validate(result)
                                        .map_err(Into::into)
                                }
                            },
                        )
                        .await,
                    "pre-sign approval simulation",
                )?;

                match &simulation.value {
                    VerifiedSimulation::Succeeded(true) => {}
                    VerifiedSimulation::Succeeded(false) => {
                        anyhow::bail!("ERC-20 approve returned false for token {token}")
                    }
                    VerifiedSimulation::Denied => {
                        anyhow::bail!("ERC-20 approve simulation reverted for token {token}")
                    }
                }
                Ok(vec![
                    verification_decision(&allowance, Some(block), Some(block)),
                    verification_decision(&simulation, Some(block), Some(block)),
                ])
            }
        }
    }

    async fn verify_swap_anchors_before_sign(
        &self,
        anchors: &SwapQuoteAnchors,
    ) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
        let checkpoint = required_verification(
            self.verification.verify_checkpoint().await,
            "pre-sign checkpoint reread",
        )?;
        let watermark = required_verification(
            self.verification
                .verify_block(anchors.watermark.number)
                .await,
            "pre-sign profiler watermark reread",
        )?;
        anyhow::ensure!(
            watermark.value == anchors.watermark,
            "Pool state header changed before signing"
        );
        let ancestry = required_verification(
            self.verification
                .verify_header_window(watermark.value, anchors.state.number)
                .await,
            "pre-sign profiler ancestry reread",
        )?;

        if let Some(last) = ancestry.value.last() {
            anyhow::ensure!(
                *last == anchors.state,
                "Swap decision header changed in the pre-sign ancestry reread"
            );
        } else {
            anyhow::ensure!(
                watermark.value == anchors.state,
                "Empty pre-sign ancestry does not end at the swap decision header"
            );
        }
        let quote = match anchors.quote_kind {
            SwapQuoteKind::ExactInput(amount_in) => required_verification(
                self.verification
                    .verify_quote_exact_input_single(
                        &anchors.quote_contract,
                        anchors.token_in,
                        anchors.token_out,
                        amount_in,
                        anchors.fee,
                        anchors.state.number,
                    )
                    .await,
                "pre-sign exact-input quote reread",
            )?,
            SwapQuoteKind::ExactOutput(amount_out) => required_verification(
                self.verification
                    .verify_quote_exact_output_single(
                        &anchors.quote_contract,
                        anchors.token_in,
                        anchors.token_out,
                        amount_out,
                        anchors.fee,
                        anchors.state.number,
                    )
                    .await,
                "pre-sign exact-output quote reread",
            )?,
        };
        anyhow::ensure!(
            quote.value == anchors.quote,
            "Independent swap quote changed before signing"
        );
        Ok(vec![
            verification_decision(
                &checkpoint,
                Some(checkpoint.value.number),
                Some(checkpoint.value.number),
            ),
            verification_decision(
                &watermark,
                Some(anchors.watermark.number),
                Some(anchors.watermark.number),
            ),
            verification_decision(
                &ancestry,
                Some(anchors.watermark.number),
                Some(anchors.state.number),
            ),
            verification_decision(
                &quote,
                Some(anchors.state.number),
                Some(anchors.state.number),
            ),
        ])
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
        let transaction_hash = tx_hash.to_string();
        let policy = self.payload_policy();
        let intent = self
            .database
            .get_execution_intent(prepared.intent_id)
            .await?;
        authenticate_payload_identity(
            &prepared.raw_tx,
            &intent,
            &transaction_hash,
            self.chain_id,
            policy,
        )
        .with_context(|| format!("Newly signed transaction {tx_hash} failed authentication"))?;

        self.database
            .reserve_execution_payload_seal(&self.payload_keys)
            .await?;
        let context = payload_context_identity(
            &intent,
            &transaction_hash,
            self.chain_id,
            self.payload_keys.deployment_id(),
        )?;
        let envelope = self.payload_keys.seal(&prepared.raw_tx, &context)?;
        let row = self
            .database
            .add_execution_transaction_envelope(
                prepared.intent_id,
                self.chain_id,
                &transaction_hash,
                &envelope,
            )
            .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to persist transaction {tx_hash}: {e}; the in-flight slot stays occupied"
            )
        })?;
        let stored = open_execution_payload(
            &self.payload_keys,
            policy,
            &intent,
            &row,
            "initial persistence",
        )?;
        anyhow::ensure!(
            stored == prepared.raw_tx,
            "Persisted transaction {tx_hash} does not match the signed bytes"
        );
        Ok(())
    }

    fn payload_policy(&self) -> PayloadPolicy {
        PayloadPolicy {
            chain_id: self.chain_id,
            signer: self.wallet_address,
            gas_limit: self.gas_limit,
            max_fee_per_gas: self.max_fee_per_gas_wei,
        }
    }

    async fn authorize_rebroadcast(
        &self,
        prepared: &PreparedTransaction,
        intent: &ExecutionIntentRow,
        purpose: TransactionPurpose,
    ) -> anyhow::Result<ReconciliationAuthorization> {
        let now_unix_secs = current_unix_secs()?;
        let decision_header = required_verification(
            self.verification
                .verify_decision_header(now_unix_secs)
                .await,
            "rebroadcast decision header",
        )?;
        let block = decision_header.value.number;
        let mut decisions = vec![verification_decision(
            &decision_header,
            Some(block),
            Some(block),
        )];
        decisions.extend(self.verify_decision_ancestry(decision_header.value).await?);
        let deployment = required_verification(
            self.verification
                .verify_deployment_manifest(&self.deployment_manifest, block)
                .await,
            "rebroadcast deployment manifest",
        )?;
        decisions.push(verification_decision(&deployment, Some(block), Some(block)));
        let canonical_nonce = required_verification(
            self.verification
                .verify_transaction_count(&self.wallet_address, block)
                .await,
            "rebroadcast canonical nonce",
        )?;
        decisions.push(verification_decision(
            &canonical_nonce,
            Some(block),
            Some(block),
        ));
        let pending_nonce = required_verification(
            self.verification
                .verify_reconciliation_pending_transaction_count(
                    &self.wallet_address,
                    prepared.nonce,
                )
                .await,
            "rebroadcast pending nonce",
        )?;
        decisions.push(verification_decision(&pending_nonce, None, None));
        let receipt_absence = required_verification(
            self.verification
                .verify_receipt_absence(&prepared.tx_hash)
                .await,
            "rebroadcast receipt absence",
        )?;
        decisions.push(verification_decision(&receipt_absence, None, None));

        let next_nonce = prepared
            .nonce
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Owned signer nonce overflow"))?;
        anyhow::ensure!(
            (prepared.nonce..=next_nonce).contains(&pending_nonce.value),
            "Pending nonce {} is outside the owned reconciliation range {}..={next_nonce}",
            pending_nonce.value,
            prepared.nonce
        );
        anyhow::ensure!(
            (prepared.nonce..=next_nonce).contains(&canonical_nonce.value),
            "Canonical nonce {} is outside the owned reconciliation range {}..={next_nonce}",
            canonical_nonce.value,
            prepared.nonce
        );

        if !receipt_absence.value {
            self.persist_rebroadcast_decisions(intent.id, prepared.nonce, &decisions)
                .await?;
            return Ok(ReconciliationAuthorization::Retain);
        }

        if canonical_nonce.value == next_nonce {
            self.persist_rebroadcast_decisions(intent.id, prepared.nonce, &decisions)
                .await?;
            return Ok(ReconciliationAuthorization::ScanReplacement(
                decision_header.value,
            ));
        }

        let (to, input, value) = persisted_call_fields(intent)?;
        let authorized = match purpose {
            TransactionPurpose::Wrap => {
                let simulation = required_verification(
                    self.verification
                        .verify_decoded_simulation(
                            &self.wallet_address,
                            &to,
                            value,
                            &input,
                            block,
                            |result| Ok(result.is_empty()),
                        )
                        .await,
                    "rebroadcast wrap simulation",
                )?;
                let authorized = matches!(&simulation.value, VerifiedSimulation::Succeeded(true));
                decisions.push(verification_decision(&simulation, Some(block), Some(block)));
                authorized
            }
            TransactionPurpose::Approve => {
                let simulation = required_verification(
                    self.verification
                        .verify_decoded_simulation(
                            &self.wallet_address,
                            &to,
                            value,
                            &input,
                            block,
                            |result| {
                                if result.is_empty() {
                                    Ok(true)
                                } else {
                                    ERC20::approveCall::abi_decode_returns_validate(result)
                                        .map_err(Into::into)
                                }
                            },
                        )
                        .await,
                    "rebroadcast approve simulation",
                )?;
                let authorized = matches!(&simulation.value, VerifiedSimulation::Succeeded(true));
                decisions.push(verification_decision(&simulation, Some(block), Some(block)));
                authorized
            }
            TransactionPurpose::Swap => {
                let call = UniswapV3SwapRouter::exactInputSingleCall::abi_decode(&input)
                    .with_context(|| "persisted swap calldata is invalid")?;

                if U256::from(decision_header.value.timestamp) > call.params.deadline {
                    false
                } else {
                    let simulation = required_verification(
                        self.verification
                            .verify_decoded_simulation(
                                &self.wallet_address,
                                &to,
                                value,
                                &input,
                                block,
                                |result| {
                                    UniswapV3SwapRouter::exactInputSingleCall::abi_decode_returns(
                                        result,
                                    )
                                    .map_err(Into::into)
                                },
                            )
                            .await,
                        "rebroadcast swap simulation",
                    )?;
                    let authorized = match &simulation.value {
                        VerifiedSimulation::Succeeded(amount_out) => {
                            *amount_out >= call.params.amountOutMinimum
                        }
                        VerifiedSimulation::Denied => false,
                    };
                    decisions.push(verification_decision(&simulation, Some(block), Some(block)));
                    authorized
                }
            }
        };
        self.persist_rebroadcast_decisions(intent.id, prepared.nonce, &decisions)
            .await?;
        Ok(if authorized {
            ReconciliationAuthorization::Rebroadcast
        } else {
            ReconciliationAuthorization::Retain
        })
    }

    async fn scan_canonical_replacement(
        &self,
        intent: &ExecutionIntentRow,
        nonce: u64,
        head: VerifiedBlockHeader,
        authenticated_payloads: &HashMap<B256, Vec<u8>>,
    ) -> anyhow::Result<Option<(B256, Vec<u8>)>> {
        let wallet_address = self.wallet_address.to_string();
        let cursor = self
            .database
            .load_execution_replacement_cursor(
                intent.id,
                self.chain_id,
                &wallet_address,
                nonce,
                &self.manifest_digest,
            )
            .await?;
        let start = cursor.as_ref().map_or(intent.created_block, |header| {
            header.number.saturating_add(1)
        });
        anyhow::ensure!(
            start <= head.number,
            "Canonical nonce advanced without an authenticated signer transaction in the scanned canonical range"
        );
        let scan_range = replacement_scan_range(start, head.number)?;
        let end = *scan_range.end();
        let mut decisions = Vec::new();
        let mut blocks = Vec::new();

        if let Some(cursor) = cursor.as_ref() {
            let parent = parse_verified_header(cursor)?;
            let window = required_verification(
                self.verification
                    .verify_replacement_window(parent, end)
                    .await,
                "canonical replacement window",
            )?;
            decisions.push(verification_decision(&window, Some(start), Some(end)));
            blocks = window.value;
        } else {
            let start_header = required_verification(
                self.verification.verify_block(start).await,
                "canonical replacement start header",
            )?;
            decisions.push(verification_decision(
                &start_header,
                Some(start),
                Some(start),
            ));
            let start_block = required_verification(
                self.verification.verify_replacement_block(start).await,
                "canonical replacement start block",
            )?;
            anyhow::ensure!(
                VerifiedBlockHeader::from(start_block.value.clone()) == start_header.value,
                "Replacement block conflicts with its canonical header"
            );
            decisions.push(verification_decision(
                &start_block,
                Some(start),
                Some(start),
            ));
            blocks.push(start_block.value);

            if end > start {
                let window = required_verification(
                    self.verification
                        .verify_replacement_window(start_header.value, end)
                        .await,
                    "canonical replacement window",
                )?;
                decisions.push(verification_decision(&window, Some(start + 1), Some(end)));
                blocks.extend(window.value);
            }
        }

        let scanned_tip = blocks
            .last()
            .map(|block| VerifiedBlockHeader::from(block.clone()))
            .ok_or_else(|| anyhow::anyhow!("Verified replacement scan returned no blocks"))?;
        if end == head.number {
            anyhow::ensure!(
                scanned_tip == head,
                "Replacement scan tip conflicts with the verified canonical head"
            );
        }
        let mut candidates = blocks
            .iter()
            .flat_map(|block| block.transactions.iter())
            .filter(|transaction| {
                transaction.from == self.wallet_address && transaction.nonce == nonce
            });
        let candidate = candidates.next();
        anyhow::ensure!(
            candidates.next().is_none(),
            "Canonical replacement scan found duplicate signer-nonce transactions"
        );

        let finalized_cursor = self
            .database
            .load_execution_verified_header(
                self.chain_id,
                &wallet_address,
                end,
                &self.manifest_digest,
            )
            .await?;

        if let Some(cursor) = finalized_cursor.as_ref() {
            anyhow::ensure!(
                parse_verified_header(cursor)? == scanned_tip,
                "Replacement scan conflicts with the durable finalized header ledger"
            );
        }

        let mut mismatch = None;
        let matched = candidate.and_then(|transaction| {
            let Some(raw_transaction) = authenticated_payloads.get(&transaction.hash).cloned()
            else {
                mismatch = Some(anyhow::anyhow!(
                    "Canonical signer-nonce transaction {} has no authenticated retained payload",
                    transaction.hash
                ));
                return None;
            };

            if let Err(e) = validate_rpc_transaction_matches_payload(transaction, &raw_transaction)
            {
                mismatch = Some(e.context(format!(
                    "Canonical signer-nonce transaction {} failed authenticated payload validation",
                    transaction.hash
                )));
                return None;
            }
            Some((transaction.hash, raw_transaction))
        });
        let matched_hash = matched.as_ref().map(|(hash, _)| hash.to_string());
        self.database
            .record_execution_replacement_scan(&ExecutionReplacementScan {
                intent_id: intent.id,
                chain_id: self.chain_id,
                wallet_address: &wallet_address,
                nonce,
                finalized_cursor: finalized_cursor.as_ref(),
                matched_transaction_hash: matched_hash.as_deref(),
                manifest_version: &self.manifest_version,
                manifest_digest: &self.manifest_digest,
                provider_ids: &self.provider_ids,
                operator_ids: &self.operator_ids,
                failure_domain_ids: &self.failure_domain_ids,
                decisions: &decisions,
            })
            .await?;

        if let Some(e) = mismatch {
            return Err(e);
        }

        if matched.is_none() && end == head.number {
            anyhow::bail!(
                "Canonical nonce advanced without an authenticated signer transaction in the canonical range"
            );
        }
        Ok(matched)
    }

    async fn persist_rebroadcast_decisions(
        &self,
        intent_id: i64,
        nonce: u64,
        decisions: &[ExecutionVerificationDecision],
    ) -> anyhow::Result<()> {
        let wallet_address = self.wallet_address.to_string();
        self.database
            .record_execution_verification_batch(&ExecutionVerificationBatch {
                intent_id,
                chain_id: self.chain_id,
                wallet_address: &wallet_address,
                nonce,
                decision_class: "rebroadcast",
                manifest_version: &self.manifest_version,
                manifest_digest: &self.manifest_digest,
                provider_ids: &self.provider_ids,
                operator_ids: &self.operator_ids,
                failure_domain_ids: &self.failure_domain_ids,
                decisions,
            })
            .await
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
        let tx_hash = prepared.tx_hash;
        let deadline = tokio::time::Instant::now() + self.receipt_timeout;

        for attempt in 0..self.receipt_max_polls {
            if tokio::time::Instant::now() >= deadline {
                break;
            }

            if attempt > 0 {
                tokio::time::sleep(RECEIPT_POLL_INTERVAL).await;
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let receipt_result =
                match tokio::time::timeout(remaining, self.verification.verify_receipt(&tx_hash))
                    .await
                {
                    Ok(result) => result,
                    Err(_) => break,
                };

            match receipt_result {
                VerificationOutcome::Verified(verified_receipt) => {
                    let receipt = &verified_receipt.value;
                    let canonical_verification = required_verification(
                        self.verification.verify_block(receipt.block_number).await,
                        "receipt inclusion header",
                    )?;
                    let canonical = canonical_verification.value;

                    if canonical.hash != receipt.block_hash {
                        continue;
                    }

                    if let Some(mut finality) = self.receipt_is_stably_finalized(receipt).await? {
                        finality.decisions.insert(
                            0,
                            verification_decision(
                                &canonical_verification,
                                Some(receipt.block_number),
                                Some(receipt.block_number),
                            ),
                        );
                        finality.decisions.insert(
                            0,
                            verification_decision(
                                &verified_receipt,
                                Some(receipt.block_number),
                                Some(receipt.block_number),
                            ),
                        );
                        let included = IncludedTransaction {
                            intent_id: prepared.intent_id,
                            nonce: prepared.nonce,
                            tx_hash,
                            block_number: receipt.block_number,
                            receipt: receipt.clone(),
                            finality,
                        };
                        return if receipt.status {
                            Ok(InclusionOutcome::Finalized(included))
                        } else {
                            Ok(InclusionOutcome::Reverted(included))
                        };
                    }
                }
                VerificationOutcome::Retryable(_) => {
                    continue;
                }
                VerificationOutcome::Disagreement(_) => {
                    return Ok(InclusionOutcome::Pending(format!(
                        "Receipt verification disagreed for transaction {tx_hash}; the intent stays occupied for reconciliation"
                    )));
                }
                VerificationOutcome::Unavailable(_) => {
                    log::warn!(
                        "Finality poll {}/{} for transaction {tx_hash} was unavailable",
                        attempt + 1,
                        self.receipt_max_polls
                    );
                }
                VerificationOutcome::LocallyInvalid(_) => {
                    anyhow::bail!(
                        "Receipt verification is locally invalid for transaction {tx_hash}"
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
    ) -> anyhow::Result<Option<StableFinality>> {
        let finalized_verification = match self.verification.verify_finalized_header().await {
            VerificationOutcome::Verified(verified) => verified,
            VerificationOutcome::Retryable(_) | VerificationOutcome::Unavailable(_) => {
                return Ok(None);
            }
            VerificationOutcome::Disagreement(_) => {
                anyhow::bail!("Finalized header verification disagreed")
            }
            VerificationOutcome::LocallyInvalid(_) => {
                anyhow::bail!("Finalized header verification is locally invalid")
            }
        };
        let finalized = finalized_verification.value;
        if finalized.number < receipt.block_number {
            return Ok(None);
        }

        let checkpoint_verification = required_verification(
            self.verification.verify_checkpoint().await,
            "finality checkpoint reread",
        )?;
        let checkpoint = checkpoint_verification.value;
        let mut decisions = vec![verification_decision(
            &checkpoint_verification,
            Some(checkpoint.number),
            Some(checkpoint.number),
        )];
        let position = self
            .database
            .load_execution_verification_position(
                self.chain_id,
                &self.wallet_address.to_string(),
                &self.manifest_version,
                &self.manifest_digest,
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("Execution verification ledger is not initialized"))?;
        let durable_tip = parse_verified_header(&position.finalized_tip)?;
        anyhow::ensure!(
            durable_tip.number >= checkpoint.number,
            "Durable finalized header tip precedes the trusted checkpoint"
        );
        let mut finalized_headers = vec![durable_tip];
        let durable_tip_verification = required_verification(
            self.verification.verify_block(durable_tip.number).await,
            "finality durable header tip",
        )?;
        anyhow::ensure!(
            durable_tip_verification.value == durable_tip,
            "Durable finalized header tip conflicts with independent sources"
        );
        decisions.push(verification_decision(
            &durable_tip_verification,
            Some(durable_tip.number),
            Some(durable_tip.number),
        ));
        anyhow::ensure!(
            finalized.number >= durable_tip.number,
            "Verified finalized height regressed below the durable finalized header tip"
        );
        let mut ancestry_cursor = durable_tip;
        while ancestry_cursor.number < finalized.number {
            let end = ancestry_cursor
                .number
                .saturating_add(4_096)
                .min(finalized.number);
            let start = ancestry_cursor.number.saturating_add(1);
            let ancestry_verification = required_verification(
                self.verification
                    .verify_header_window(ancestry_cursor, end)
                    .await,
                "finality ancestry",
            )?;
            let ancestry = &ancestry_verification.value;
            ancestry_cursor = *ancestry
                .last()
                .expect("nonempty finality ancestry advances the cursor");
            decisions.push(verification_decision(
                &ancestry_verification,
                Some(start),
                Some(end),
            ));
            finalized_headers.extend(ancestry.iter().copied());
        }
        anyhow::ensure!(
            ancestry_cursor == finalized,
            "Finalized header conflicts with its verified ancestry"
        );

        let canonical_again_verification = required_verification(
            self.verification.verify_block(receipt.block_number).await,
            "finality inclusion header reread",
        )?;
        let canonical_again = canonical_again_verification.value;
        let finalized_again_verification = required_verification(
            self.verification.verify_block(finalized.number).await,
            "finalized header reread",
        )?;
        let finalized_again = finalized_again_verification.value;
        anyhow::ensure!(
            canonical_again.hash == receipt.block_hash && finalized_again == finalized,
            "Finality verification disagreed with the receipt or finalized header"
        );
        decisions.extend([
            verification_decision(
                &finalized_verification,
                Some(finalized.number),
                Some(finalized.number),
            ),
            verification_decision(
                &canonical_again_verification,
                Some(receipt.block_number),
                Some(receipt.block_number),
            ),
            verification_decision(
                &finalized_again_verification,
                Some(finalized.number),
                Some(finalized.number),
            ),
        ]);
        Ok(Some(StableFinality {
            decisions,
            inclusion_header: durable_verified_header(&canonical_again),
            finalized_headers: finalized_headers
                .iter()
                .map(durable_verified_header)
                .collect(),
        }))
    }

    async fn commit_verified_finality(
        &self,
        included: &IncludedTransaction,
        status: TransactionStatus,
        verified_postconditions: &[ExecutionVerificationDecision],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(
                status,
                TransactionStatus::Finalized | TransactionStatus::Reverted
            ) && included.receipt.status == (status == TransactionStatus::Finalized),
            "Finality status conflicts with the verified transaction receipt"
        );
        let mut decisions = included.finality.decisions.clone();
        decisions.extend_from_slice(verified_postconditions);
        let wallet_address = self.wallet_address.to_string();
        let transaction_hash = included.tx_hash.to_string();
        let block_hash = included.receipt.block_hash.to_string();
        let effective_gas_price = included.receipt.effective_gas_price.to_string();
        self.database
            .record_execution_finality_verified(&ExecutionFinalityTransition {
                intent_id: included.intent_id,
                chain_id: self.chain_id,
                wallet_address: &wallet_address,
                nonce: included.nonce,
                transaction_hash: &transaction_hash,
                status,
                block_number: included.block_number,
                block_hash: &block_hash,
                receipt_success: included.receipt.status,
                gas_used: included.receipt.gas_used,
                effective_gas_price: &effective_gas_price,
                manifest_version: &self.manifest_version,
                manifest_digest: &self.manifest_digest,
                provider_ids: &self.provider_ids,
                operator_ids: &self.operator_ids,
                failure_domain_ids: &self.failure_domain_ids,
                decisions: &decisions,
                finalized_headers: &included.finality.finalized_headers,
            })
            .await
    }

    fn release_slot(&self) {
        *self.in_flight.lock().expect("in-flight mutex poisoned") = None;
    }
}

fn replacement_scan_range(from_block: u64, head_block: u64) -> anyhow::Result<RangeInclusive<u64>> {
    anyhow::ensure!(
        head_block >= from_block,
        "Canonical head {head_block} is behind execution creation block {from_block}"
    );
    let max_end = from_block.saturating_add(MAX_REPLACEMENT_SCAN_BLOCKS - 1);
    Ok(from_block..=head_block.min(max_end))
}

fn current_unix_secs() -> anyhow::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow::anyhow!("Trusted host clock precedes the Unix epoch"))
        .map(|duration| duration.as_secs())
}

fn validate_payload_operation_batch_size(batch_size: usize) -> anyhow::Result<i64> {
    anyhow::ensure!(
        (1..=MAX_PAYLOAD_OPERATION_BATCH_SIZE).contains(&batch_size),
        "Payload operation batch size must be between 1 and {MAX_PAYLOAD_OPERATION_BATCH_SIZE}"
    );
    Ok(i64::try_from(batch_size).expect("bounded payload batch size fits i64"))
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

fn open_execution_payload(
    keys: &PayloadKeySet,
    policy: PayloadPolicy,
    intent: &ExecutionIntentRow,
    hash: &ExecutionTransactionHashRow,
    reason: &str,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        hash.payload_expected,
        "Execution transaction {} has no signed payload",
        hash.transaction_hash
    );
    anyhow::ensure!(
        hash.raw_transaction.is_none(),
        "Protected execution transaction {} contains plaintext",
        hash.transaction_hash
    );
    let envelope = hash.sealed_transaction.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Protected execution transaction {} has no sealed payload",
            hash.transaction_hash
        )
    })?;
    let context = payload_context(intent, hash, keys.deployment_id())?;
    let raw_transaction = keys.unseal(envelope, &context)?;
    log::info!(
        "Unsealed execution payload for intent {} transaction {} during {reason}",
        intent.id,
        hash.transaction_hash
    );
    authenticate_retained_payload(&raw_transaction, intent, hash, keys.deployment_id())?;
    if retained_payload_requires_policy(intent, hash, policy)? {
        authenticate_payload(&raw_transaction, intent, hash, policy, keys.deployment_id())
            .with_context(|| {
                format!(
                    "execution intent {} transaction {} violates current execution policy",
                    intent.id, hash.transaction_hash
                )
            })?;
    }
    Ok(raw_transaction)
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
    factory: Address,
    weth: Address,
    token_in: Address,
    token_out: Address,
    fee: U24,
    amount_in: U256,
    min_amount_out: U256,
    slippage_bps: u32,
    quote_spend_ceiling: Option<QuoteSpendCeiling>,
    profiler_position: Option<BlockPosition>,
}

#[derive(Debug, Clone)]
struct SwapQuoteAnchors {
    watermark: VerifiedBlockHeader,
    state: VerifiedBlockHeader,
    quote_contract: Address,
    token_in: Address,
    token_out: Address,
    fee: U24,
    quote_kind: SwapQuoteKind,
    quote: UniswapV3Quote,
    precondition_decisions: Vec<ExecutionVerificationDecision>,
}

#[derive(Debug, Clone, Copy)]
enum SwapQuoteKind {
    ExactInput(U256),
    ExactOutput(U256),
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
    mut plan: SwapPlan,
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

    let Some(profiler_position) = plan.profiler_position.as_ref() else {
        release_preparing_slot(&executor.in_flight);
        emitter.emit_order_denied(order, "Pool profiler has no quote provenance");
        return Ok(());
    };
    let mut swap_anchors = match validate_swap_quote(
        profiler_position,
        &plan,
        max_quote_age_blocks,
        &executor.verification,
        &executor.deployment_manifest,
    )
    .await
    {
        Ok(anchors) => anchors,
        Err(e) => {
            release_preparing_slot(&executor.in_flight);
            emitter.emit_order_denied(order, &e.to_string());
            return Ok(());
        }
    };
    let (amount_in, min_amount_out) = match verified_swap_amounts(&plan, swap_anchors.quote) {
        Ok(amounts) => amounts,
        Err(e) => {
            release_preparing_slot(&executor.in_flight);
            emitter.emit_order_denied(order, &e.to_string());
            return Ok(());
        }
    };
    plan.amount_in = amount_in;
    plan.min_amount_out = min_amount_out;
    let deadline = match swap_anchors.state.timestamp.checked_add(deadline_seconds) {
        Some(deadline) => deadline,
        None => {
            release_preparing_slot(&executor.in_flight);
            emitter.emit_order_denied(
                order,
                &format!(
                    "Swap deadline overflow: anchor timestamp {} plus `deadline_seconds` {deadline_seconds} exceeds u64",
                    swap_anchors.state.timestamp
                ),
            );
            return Ok(());
        }
    };

    swap_anchors.precondition_decisions =
        match check_swap_preconditions(&plan, swap_anchors.state.number, &executor).await {
            Ok(decisions) => decisions,
            Err(e) => {
                release_preparing_slot(&executor.in_flight);
                emitter.emit_order_denied(order, &e.to_string());
                return Ok(());
            }
        };

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
        created_block: swap_anchors.state.number,
    };
    let intent = match executor.database.reserve_execution_intent(&intent).await {
        Ok(intent) => intent,
        Err(e) => {
            release_preparing_if_reservation_not_committed(&executor.in_flight, &e);
            emitter.emit_order_denied(order, &e.to_string());
            return Ok(());
        }
    };
    let prepared = match executor
        .prepare_and_sign_swap(
            intent.id,
            intent.created_block,
            plan.router,
            U256::ZERO,
            calldata,
            &swap_anchors,
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

    let broadcast = executor.broadcast(&prepared).await?;
    emitter.emit_order_submitted(order);
    executor
        .database
        .mark_execution_event_emitted(intent.id, "acknowledgement")
        .await?;

    if let BroadcastOutcome::Ambiguous(message) = broadcast {
        log::warn!("{message}");
    }

    let trace_purpose = match plan.order.order_side() {
        OrderSide::Sell => "swap_sell",
        OrderSide::Buy => "swap_buy",
    };

    match executor.await_finality(&prepared).await? {
        InclusionOutcome::Finalized(mut included) => {
            included.finality.decisions.extend(
                verify_finalized_transaction(
                    &included,
                    &intent,
                    prepared.nonce,
                    &prepared.raw_tx,
                    &executor,
                    trace_purpose,
                )
                .await?,
            );
            let fill = validate_finalized_swap_fill(&plan, &included)?;
            let wallet = load_verified_wallet_after_fill(&plan, &included, &executor).await?;
            executor
                .commit_verified_finality(
                    &included,
                    TransactionStatus::Finalized,
                    &wallet.decisions,
                )
                .await?;
            complete_finalized_swap(
                &plan,
                intent.id,
                included.tx_hash,
                fill,
                wallet,
                &executor,
                &emitter,
            )
            .await?;
            executor.release_slot();
            Ok(())
        }
        InclusionOutcome::Reverted(mut included) => {
            included.finality.decisions.extend(
                verify_finalized_transaction(
                    &included,
                    &intent,
                    prepared.nonce,
                    &prepared.raw_tx,
                    &executor,
                    trace_purpose,
                )
                .await?,
            );
            executor
                .commit_verified_finality(&included, TransactionStatus::Reverted, &[])
                .await?;
            send_reverted_order(&emitter, order, &included)?;
            executor
                .database
                .mark_execution_event_emitted(intent.id, "terminal")
                .await?;
            executor.release_slot();
            Ok(())
        }
        InclusionOutcome::Pending(message) => anyhow::bail!(message),
    }
}

async fn validate_swap_quote(
    position: &BlockPosition,
    plan: &SwapPlan,
    max_age_blocks: u64,
    verification: &VerificationCoordinator,
    manifest: &BlockchainDeploymentManifest,
) -> anyhow::Result<SwapQuoteAnchors> {
    let block_hash = position.block_hash.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Pool state at block {} has no ingestion-time block hash; refresh the profiler before execution",
            position.number
        )
    })?;
    let expected_block_hash = B256::from_str(block_hash)
        .with_context(|| format!("Invalid profiler block hash {block_hash}"))?;
    let now_unix_secs = current_unix_secs()?;
    let head = verified_value(
        verification.verify_decision_header(now_unix_secs).await,
        "swap decision header",
    )?;
    validate_quote_age(position.number, head.number, max_age_blocks)?;
    let canonical_block = verified_value(
        verification.verify_block(position.number).await,
        "profiler watermark header",
    )?;
    anyhow::ensure!(
        canonical_block.hash == expected_block_hash,
        "Pool state block {} changed from {} to {}; refresh the profiler before execution",
        position.number,
        expected_block_hash,
        canonical_block.hash
    );

    let snapshot_transaction = position.transaction_index == BLOCK_SCOPED_SNAPSHOT_INDEX;
    let snapshot_log = position.log_index == BLOCK_SCOPED_SNAPSHOT_INDEX;
    anyhow::ensure!(
        snapshot_transaction == snapshot_log,
        "Pool state at block {} has an invalid partial snapshot watermark",
        position.number
    );

    if snapshot_transaction {
        let snapshot_hash = B256::from_str(&position.transaction_hash)
            .with_context(|| "Invalid block-scoped snapshot hash")?;
        anyhow::ensure!(
            snapshot_hash == expected_block_hash,
            "Block-scoped snapshot hash {snapshot_hash} does not match ingestion hash {expected_block_hash}"
        );
    } else {
        validate_profiler_event_verified(
            position,
            expected_block_hash,
            plan.pool_address,
            &plan.pool,
            verification,
        )
        .await?;
    }

    let ancestry = verified_value(
        verification
            .verify_header_window(canonical_block, head.number)
            .await,
        "profiler-to-decision ancestry",
    )?;

    if let Some(last) = ancestry.last() {
        anyhow::ensure!(
            *last == head,
            "Swap decision header conflicts with profiler ancestry"
        );
    } else {
        anyhow::ensure!(
            canonical_block == head,
            "Empty profiler ancestry does not end at the decision header"
        );
    }
    verified_value(
        verification
            .verify_deployment_manifest(manifest, head.number)
            .await,
        "swap deployment manifest",
    )?;
    let quote_contract = validate_manifest_pool(plan, manifest)?;
    let quote_kind = match plan.order.order_side() {
        OrderSide::Sell => SwapQuoteKind::ExactInput(quantity_to_raw_amount(
            plan.order.quantity(),
            plan.pool.get_base_token().decimals,
        )?),
        OrderSide::Buy => SwapQuoteKind::ExactOutput(quantity_to_raw_amount(
            plan.order.quantity(),
            plan.pool.get_base_token().decimals,
        )?),
    };
    let quote = verified_value(
        verify_swap_quote(verification, quote_contract, plan, quote_kind, head.number).await,
        "independent swap quote",
    )?;

    Ok(SwapQuoteAnchors {
        watermark: canonical_block,
        state: head,
        quote_contract,
        token_in: plan.token_in,
        token_out: plan.token_out,
        fee: plan.fee,
        quote_kind,
        quote,
        precondition_decisions: Vec::new(),
    })
}

async fn verify_swap_quote(
    verification: &VerificationCoordinator,
    quote_contract: Address,
    plan: &SwapPlan,
    kind: SwapQuoteKind,
    block: u64,
) -> VerificationOutcome<UniswapV3Quote> {
    match kind {
        SwapQuoteKind::ExactInput(amount_in) => {
            verification
                .verify_quote_exact_input_single(
                    &quote_contract,
                    plan.token_in,
                    plan.token_out,
                    amount_in,
                    plan.fee,
                    block,
                )
                .await
        }
        SwapQuoteKind::ExactOutput(amount_out) => {
            verification
                .verify_quote_exact_output_single(
                    &quote_contract,
                    plan.token_in,
                    plan.token_out,
                    amount_out,
                    plan.fee,
                    block,
                )
                .await
        }
    }
}

fn verified_swap_amounts(plan: &SwapPlan, quote: UniswapV3Quote) -> anyhow::Result<(U256, U256)> {
    anyhow::ensure!(
        !quote.amount.is_zero(),
        "Independent swap quote returned zero"
    );
    let base_amount =
        quantity_to_raw_amount(plan.order.quantity(), plan.pool.get_base_token().decimals)?;
    let slippage_bps = plan.slippage_bps;
    match plan.order.order_side() {
        OrderSide::Sell => Ok((
            base_amount,
            derive_min_amount_out(quote.amount, slippage_bps)?,
        )),
        OrderSide::Buy => {
            let ceiling = plan.quote_spend_ceiling.ok_or_else(|| {
                anyhow::anyhow!(
                    "No quote spend ceiling for BUY token pair {} -> {}",
                    plan.token_in,
                    plan.token_out
                )
            })?;
            anyhow::ensure!(
                quote.amount <= ceiling.max_amount,
                "BUY quote amount {} exceeds the configured quote-spend maximum {} for {} -> {}",
                quote.amount,
                ceiling.max_amount,
                plan.token_in,
                plan.token_out
            );
            Ok((
                quote.amount,
                derive_min_amount_out(base_amount, slippage_bps)?,
            ))
        }
    }
}

fn validate_manifest_pool(
    plan: &SwapPlan,
    manifest: &BlockchainDeploymentManifest,
) -> anyhow::Result<Address> {
    let matching = manifest
        .pools
        .iter()
        .filter(|pool| Address::from_str(&pool.address).ok() == Some(plan.pool_address))
        .collect::<Vec<_>>();
    anyhow::ensure!(
        matching.len() == 1,
        "Pool {} does not have exactly one deployment manifest definition",
        plan.pool_address
    );
    let pool = matching[0];
    let token0 = Address::from_str(&pool.token0)?;
    let token1 = Address::from_str(&pool.token1)?;
    let factory = Address::from_str(&pool.factory)?;
    let quote_contract = Address::from_str(&pool.quote_contract)?;
    anyhow::ensure!(
        token0 == plan.pool.token0.address
            && token1 == plan.pool.token1.address
            && pool.fee == plan.pool.fee.expect("validated pool fee")
            && factory == plan.factory,
        "Cached pool {} does not match its deployment manifest identity",
        plan.pool_address
    );

    for token in [&plan.pool.token0, &plan.pool.token1] {
        let identities = manifest
            .tokens
            .iter()
            .filter(|identity| Address::from_str(&identity.address).ok() == Some(token.address))
            .collect::<Vec<_>>();
        anyhow::ensure!(
            identities.len() == 1,
            "Token {} does not have exactly one deployment manifest identity",
            token.address
        );
        let identity = identities[0];
        anyhow::ensure!(
            identity.name == token.name
                && identity.symbol == token.symbol
                && identity.decimals == token.decimals,
            "Cached token {} does not match its deployment manifest identity",
            token.address
        );
        let expected_role = if token.address == plan.pool.get_base_token().address {
            "base"
        } else {
            "quote"
        };
        anyhow::ensure!(
            matches!(identity.asset_role.as_str(), "both") || identity.asset_role == expected_role,
            "Token {} is not permitted as the pool {expected_role} asset",
            token.address
        );
    }
    Ok(quote_contract)
}

async fn validate_profiler_event_verified(
    position: &BlockPosition,
    expected_block_hash: B256,
    pool_address: Address,
    pool: &Pool,
    verification: &VerificationCoordinator,
) -> anyhow::Result<()> {
    let transaction_hash = B256::from_str(&position.transaction_hash).with_context(|| {
        format!(
            "Invalid profiler transaction hash {}",
            position.transaction_hash
        )
    })?;
    let receipt = verified_value(
        verification.verify_receipt(&transaction_hash).await,
        "profiler watermark receipt",
    )?;
    anyhow::ensure!(
        receipt.status,
        "Profiler transaction did not execute successfully"
    );
    anyhow::ensure!(
        receipt.transaction_hash == transaction_hash,
        "Profiler receipt transaction hash does not match its ingestion watermark"
    );
    anyhow::ensure!(
        receipt.block_number == position.number
            && receipt.block_hash == expected_block_hash
            && receipt.transaction_index == u64::from(position.transaction_index),
        "Profiler receipt position does not match its ingestion watermark"
    );
    let matching_logs = receipt
        .logs
        .iter()
        .filter(|log| rpc_helpers::extract_log_index(log).ok() == Some(position.log_index))
        .collect::<Vec<_>>();
    anyhow::ensure!(
        matching_logs.len() == 1,
        "Profiler receipt contains {} logs at global index {}; expected exactly one",
        matching_logs.len(),
        position.log_index
    );
    let log = matching_logs[0];
    let log_transaction_hash = B256::from_str(&rpc_helpers::extract_transaction_hash(log)?)
        .with_context(|| "Invalid profiler log transaction hash")?;
    let log_block_hash = log
        .block_hash
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Profiler log has no block hash"))?;
    anyhow::ensure!(
        !log.removed
            && log_transaction_hash == transaction_hash
            && rpc_helpers::extract_block_number(log)? == position.number
            && rpc_helpers::extract_transaction_index(log)? == position.transaction_index
            && B256::from_str(log_block_hash)? == expected_block_hash,
        "Profiler log position does not match its ingestion watermark"
    );
    anyhow::ensure!(
        rpc_helpers::extract_address(log)? == pool_address,
        "Profiler watermark log did not come from expected pool {pool_address}"
    );
    let signature = log
        .topics
        .first()
        .ok_or_else(|| anyhow::anyhow!("Profiler watermark log has no event signature"))?;
    let supported =
        profiler_event_signatures(pool).any(|expected| expected.eq_ignore_ascii_case(signature));
    anyhow::ensure!(
        supported,
        "Profiler watermark log has an unsupported event signature"
    );
    Ok(())
}

fn profiler_event_signatures(pool: &Pool) -> impl Iterator<Item = &str> {
    [
        Some(pool.dex.swap_created_event.as_ref()),
        Some(pool.dex.mint_created_event.as_ref()),
        Some(pool.dex.burn_created_event.as_ref()),
        Some(pool.dex.collect_created_event.as_ref()),
        pool.dex.flash_created_event.as_deref(),
        pool.dex.fee_protocol_update_event.as_deref(),
        pool.dex.fee_protocol_collect_event.as_deref(),
    ]
    .into_iter()
    .flatten()
}

fn validate_quote_age(
    profiler_block: u64,
    latest_block: u64,
    max_age_blocks: u64,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        profiler_block <= latest_block,
        "Pool state at block {profiler_block} is ahead of the latest block {latest_block}; the execution RPC endpoint lags the data feed"
    );
    let quote_age = latest_block - profiler_block;
    anyhow::ensure!(
        quote_age <= max_age_blocks,
        "Stale quote: pool state at block {profiler_block}, latest block {latest_block}, exceeds `max_quote_age_blocks` {max_age_blocks}"
    );
    Ok(())
}

fn validate_rpc_transaction_matches_payload(
    transaction: &RpcTransaction,
    raw_transaction: &[u8],
) -> anyhow::Result<()> {
    let signed = decode_signed_transaction(raw_transaction)?;
    anyhow::ensure!(
        transaction.hash == signed.hash
            && transaction.from == signed.signer
            && transaction.nonce == signed.nonce
            && transaction.chain_id == Some(signed.chain_id)
            && transaction.transaction_type == Some(2)
            && transaction.to == Some(signed.to)
            && transaction.input == signed.input
            && transaction.value == signed.value
            && transaction.gas == Some(signed.gas_limit)
            && transaction.max_fee_per_gas == Some(U256::from(signed.max_fee_per_gas))
            && transaction.max_priority_fee_per_gas
                == Some(U256::from(signed.max_priority_fee_per_gas)),
        "Verified transaction fields differ from the authenticated signed payload"
    );
    Ok(())
}

async fn verify_finalized_transaction(
    included: &IncludedTransaction,
    intent: &ExecutionIntentRow,
    nonce: u64,
    raw_transaction: &[u8],
    executor: &TransactionExecutor,
    trace_purpose: &str,
) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
    verify_finalized_transaction_identity(
        included,
        intent,
        nonce,
        raw_transaction,
        &executor.verification,
        executor.wallet_address,
        executor.chain_id,
        &executor.deployment_manifest,
        trace_purpose,
    )
    .await
}

#[expect(clippy::too_many_arguments)]
async fn verify_finalized_transaction_identity(
    included: &IncludedTransaction,
    intent: &ExecutionIntentRow,
    nonce: u64,
    raw_transaction: &[u8],
    verification: &VerificationCoordinator,
    wallet_address: Address,
    chain_id: u32,
    deployment_manifest: &BlockchainDeploymentManifest,
    trace_purpose: &str,
) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
    let transaction_verification = required_verification(
        verification.verify_transaction(&included.tx_hash).await,
        "finalized transaction",
    )?;
    let transaction = &transaction_verification.value;
    let signed = decode_signed_transaction(raw_transaction)?;
    let (expected_to, expected_input, expected_value) = persisted_call_fields(intent)?;
    anyhow::ensure!(
        included.receipt.transaction_hash == included.tx_hash
            && signed.hash == included.tx_hash
            && signed.signer == wallet_address
            && signed.chain_id == u64::from(chain_id)
            && signed.nonce == nonce
            && signed.to == expected_to
            && signed.input == expected_input
            && signed.value == expected_value,
        "Finalized transaction does not match the authenticated signed payload and persisted intent"
    );
    validate_rpc_transaction_matches_payload(transaction, raw_transaction)
        .context("finalized transaction identity mismatch")?;

    let trace_verification = required_verification(
        verification.verify_call_trace(&included.tx_hash).await,
        "finalized call trace",
    )?;
    validate_call_trace(
        &trace_verification.value,
        &signed,
        included.receipt.status,
        trace_purpose,
        deployment_manifest,
    )?;
    let deployment_verification = required_verification(
        verification
            .verify_deployment_manifest(deployment_manifest, included.block_number)
            .await,
        "inclusion deployment manifest",
    )?;

    Ok(vec![
        verification_decision(
            &transaction_verification,
            Some(included.block_number),
            Some(included.block_number),
        ),
        verification_decision(
            &trace_verification,
            Some(included.block_number),
            Some(included.block_number),
        ),
        verification_decision(
            &deployment_verification,
            Some(included.block_number),
            Some(included.block_number),
        ),
    ])
}

fn validate_call_trace(
    trace: &VerifiedCallTrace,
    signed: &crate::execution::transaction::DecodedSignedTransaction,
    receipt_success: bool,
    purpose: &str,
    manifest: &BlockchainDeploymentManifest,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        trace.call_type == RpcCallType::Call
            && trace.from == signed.signer
            && trace.to == Some(signed.to)
            && trace.value == signed.value
            && trace.input_digest == keccak256(&signed.input)
            && trace.success == receipt_success,
        "Verified call-trace root differs from the authenticated transaction"
    );
    validate_internal_calls(&trace.calls, signed.to, purpose, manifest)
}

fn validate_internal_calls(
    calls: &[VerifiedCallTrace],
    caller_context: Address,
    purpose: &str,
    manifest: &BlockchainDeploymentManifest,
) -> anyhow::Result<()> {
    for call in calls {
        anyhow::ensure!(
            call.from == caller_context,
            "Verified call trace child has an invalid caller context"
        );
        let target = call.to.ok_or_else(|| {
            anyhow::anyhow!("Verified call trace contains an operation without a target")
        })?;
        let call_type = match call.call_type {
            RpcCallType::Call => "call",
            RpcCallType::Callcode => "callcode",
            RpcCallType::Delegatecall => "delegatecall",
            RpcCallType::Staticcall => "staticcall",
            RpcCallType::Create | RpcCallType::Create2 | RpcCallType::Selfdestruct => {
                anyhow::bail!("Verified call trace contains a forbidden state-changing operation")
            }
        };
        let permitted = manifest.call_edges.iter().any(|edge| {
            edge.purpose == purpose
                && edge.call_type.eq_ignore_ascii_case(call_type)
                && Address::from_str(&edge.caller).ok() == Some(call.from)
                && Address::from_str(&edge.target).ok() == Some(target)
        });
        anyhow::ensure!(
            permitted,
            "Verified call trace contains an unreviewed {call_type} edge {} -> {target} for {purpose}",
            call.from
        );
        let child_context = match call.call_type {
            RpcCallType::Call | RpcCallType::Staticcall => target,
            RpcCallType::Callcode | RpcCallType::Delegatecall => caller_context,
            RpcCallType::Create | RpcCallType::Create2 | RpcCallType::Selfdestruct => {
                unreachable!("forbidden operations return before child traversal")
            }
        };
        validate_internal_calls(&call.calls, child_context, purpose, manifest)?;
    }
    Ok(())
}

async fn verify_wrap_balance_increase(
    executor: &TransactionExecutor,
    weth_address: &Address,
    amount_wei: U256,
    included: &IncludedTransaction,
) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
    let previous_block = included.block_number.checked_sub(1).ok_or_else(|| {
        anyhow::anyhow!(
            "Included wrap transaction {} has invalid block number 0",
            included.tx_hash
        )
    })?;
    let call = ERC20::balanceOfCall {
        account: executor.wallet_address,
    }
    .abi_encode();
    let balance_before = required_verification(
        executor
            .verification
            .verify_decoded_call(
                None,
                weth_address,
                U256::ZERO,
                &call,
                previous_block,
                |result| ERC20::balanceOfCall::abi_decode_returns(result).map_err(Into::into),
            )
            .await,
        "wrapped balance before finality",
    )
    .with_context(|| {
        format!(
            "failed to verify WETH balance before included transaction {} at block {previous_block}",
            included.tx_hash
        )
    })?;
    let balance_after = required_verification(
        executor
            .verification
            .verify_decoded_call(
                None,
                weth_address,
                U256::ZERO,
                &call,
                included.block_number,
                |result| ERC20::balanceOfCall::abi_decode_returns(result).map_err(Into::into),
            )
            .await,
        "wrapped balance after finality",
    )
    .with_context(|| {
        format!(
            "failed to verify WETH balance after included transaction {} at block {}",
            included.tx_hash, included.block_number
        )
    })?;
    let expected_balance = balance_before
        .value
        .checked_add(amount_wei)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "WETH balance overflow for included transaction {} at block {}",
                included.tx_hash,
                included.block_number
            )
        })?;
    anyhow::ensure!(
        balance_after.value == expected_balance,
        "WETH balance after transaction {} did not increase by {amount_wei}: expected {expected_balance}, was {}",
        included.tx_hash,
        balance_after.value
    );

    Ok(vec![
        verification_decision(&balance_before, Some(previous_block), Some(previous_block)),
        verification_decision(
            &balance_after,
            Some(included.block_number),
            Some(included.block_number),
        ),
    ])
}

async fn verify_approve_allowance(
    executor: &TransactionExecutor,
    token: &Address,
    router: &Address,
    amount: U256,
    included: &IncludedTransaction,
) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
    let call = ERC20::allowanceCall {
        owner: executor.wallet_address,
        spender: *router,
    }
    .abi_encode();
    let allowance = required_verification(
        executor
            .verification
            .verify_decoded_call(
                None,
                token,
                U256::ZERO,
                &call,
                included.block_number,
                |result| ERC20::allowanceCall::abi_decode_returns(result).map_err(Into::into),
            )
            .await,
        "router allowance after finality",
    )
    .with_context(|| {
        format!(
            "failed to verify router allowance after included transaction {} at block {}",
            included.tx_hash, included.block_number
        )
    })?;
    anyhow::ensure!(
        allowance.value == amount,
        "Router allowance after transaction {} does not equal the requested amount {amount}: was {}",
        included.tx_hash,
        allowance.value
    );

    Ok(vec![verification_decision(
        &allowance,
        Some(included.block_number),
        Some(included.block_number),
    )])
}

async fn complete_finalized_swap(
    plan: &SwapPlan,
    intent_id: i64,
    tx_hash: B256,
    fill: Option<FinalizedSwapFill>,
    wallet: VerifiedWalletRefresh,
    executor: &TransactionExecutor,
    emitter: &ExecutionEventEmitter,
) -> anyhow::Result<()> {
    if let Some(fill) = fill {
        let filled = OrderFilled::new(
            emitter.trader_id(),
            plan.order.strategy_id(),
            plan.order.instrument_id(),
            plan.order.client_order_id(),
            fill.venue_order_id,
            emitter.account_id(),
            fill.trade_id,
            plan.order.order_side(),
            plan.order.order_type(),
            fill.last_qty,
            fill.last_px,
            plan.quote_currency,
            LiquiditySide::Taker,
            execution_event_id(tx_hash, b"fill"),
            fill.ts_event,
            fill.ts_event,
            false,
            None,
            Some(fill.commission),
            None,
        );
        emitter.try_send_order_event(OrderEventAny::Filled(filled))?;

        if fill.last_qty < plan.order.quantity() {
            let canceled = OrderCanceled::new(
                emitter.trader_id(),
                plan.order.strategy_id(),
                plan.order.instrument_id(),
                plan.order.client_order_id(),
                execution_event_id(tx_hash, b"partial_cancel"),
                fill.ts_event,
                fill.ts_event,
                false,
                Some(fill.venue_order_id),
                Some(emitter.account_id()),
            );
            emitter.try_send_order_event(OrderEventAny::Canceled(canceled))?;
        }
    }

    *executor
        .wallet_balance
        .lock()
        .expect("wallet balance mutex poisoned") = wallet.wallet_balance;
    emitter.try_emit_account_state(
        wallet.balances,
        vec![],
        true,
        get_atomic_clock_realtime().get_time_ns(),
        None,
    )?;
    executor
        .database
        .mark_execution_event_emitted(intent_id, "fill")
        .await
}

fn send_reverted_order(
    emitter: &ExecutionEventEmitter,
    order: &OrderAny,
    included: &IncludedTransaction,
) -> anyhow::Result<()> {
    let ts_event = finalized_inclusion_time(included)?;
    let rejected = OrderRejected::new(
        emitter.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        emitter.account_id(),
        format!("Transaction {} reverted on-chain", included.tx_hash).into(),
        execution_event_id(included.tx_hash, b"reverted"),
        ts_event,
        ts_event,
        false,
        false,
    );
    emitter.try_send_order_event(OrderEventAny::Rejected(rejected))
}

fn finalized_inclusion_time(included: &IncludedTransaction) -> anyhow::Result<UnixNanos> {
    let inclusion = &included.finality.inclusion_header;
    anyhow::ensure!(
        inclusion.number == included.block_number
            && inclusion.hash == included.receipt.block_hash.to_string(),
        "Verified inclusion header does not match the finalized receipt"
    );
    let timestamp = inclusion.timestamp;
    let nanos = timestamp
        .checked_mul(NANOSECONDS_IN_SECOND)
        .ok_or_else(|| anyhow::anyhow!("Verified inclusion timestamp exceeds nanoseconds"))?;
    Ok(UnixNanos::from(nanos))
}

fn execution_event_id(tx_hash: B256, event: &[u8]) -> UUID4 {
    let mut identity = Vec::with_capacity(tx_hash.len() + event.len());
    identity.extend_from_slice(tx_hash.as_slice());
    identity.extend_from_slice(event);
    let digest = keccak256(identity);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    UUID4::from_bytes(bytes)
}

struct FinalizedSwapFill {
    venue_order_id: VenueOrderId,
    trade_id: TradeId,
    last_qty: Quantity,
    last_px: Price,
    commission: Money,
    ts_event: UnixNanos,
}

struct VerifiedWalletRefresh {
    wallet_balance: WalletBalance,
    balances: Vec<AccountBalance>,
    decisions: Vec<ExecutionVerificationDecision>,
}

fn validate_finalized_swap_fill(
    plan: &SwapPlan,
    included: &IncludedTransaction,
) -> anyhow::Result<Option<FinalizedSwapFill>> {
    let signature =
        keccak256("Swap(address,address,int256,int256,uint160,uint128,int24)").to_string();
    let swap_logs = included
        .receipt
        .logs
        .iter()
        .filter(|log| {
            !log.removed
                && log.topics.first().is_some_and(|topic| topic == &signature)
                && Address::from_str(&log.address).ok() == Some(plan.pool_address)
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        swap_logs.len() == 1,
        "Finalized transaction {} emitted {} Swap logs from expected pool {}; expected exactly one",
        included.tx_hash,
        swap_logs.len(),
        plan.pool_address
    );
    let log = swap_logs[0];
    let log_transaction_hash = B256::from_str(&rpc_helpers::extract_transaction_hash(log)?)
        .with_context(|| "Invalid finalized Swap log transaction hash")?;
    let log_block_hash = log
        .block_hash
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Finalized Swap log has no block hash"))?;
    anyhow::ensure!(
        log_transaction_hash == included.tx_hash
            && rpc_helpers::extract_block_number(log)? == included.block_number
            && u64::from(rpc_helpers::extract_transaction_index(log)?)
                == included.receipt.transaction_index
            && B256::from_str(log_block_hash)
                .with_context(|| "Invalid finalized Swap log block hash")?
                == included.receipt.block_hash,
        "Finalized Swap log position does not match transaction {}",
        included.tx_hash
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
    let (base_amount, quote_amount) =
        if plan.pool.get_base_token().address == plan.pool.token0.address {
            (event.amount0, event.amount1)
        } else {
            (event.amount1, event.amount0)
        };
    let last_qty = match plan.order.order_side() {
        OrderSide::Sell => {
            anyhow::ensure!(
                base_amount.is_positive() && base_amount.unsigned_abs() == plan.amount_in,
                "Finalized Swap input {base_amount} does not match the persisted amount {}",
                plan.amount_in
            );
            plan.order.quantity()
        }
        OrderSide::Buy => {
            anyhow::ensure!(
                quote_amount.is_positive() && quote_amount.unsigned_abs() == plan.amount_in,
                "Finalized Swap input {quote_amount} does not match the persisted amount {}",
                plan.amount_in
            );
            anyhow::ensure!(
                base_amount.is_negative(),
                "Finalized Swap base amount {base_amount} is not a BUY output"
            );
            raw_amount_to_quantity(
                base_amount.unsigned_abs(),
                plan.pool.get_base_token().decimals,
            )?
        }
    };

    let block = &included.finality.inclusion_header;
    anyhow::ensure!(
        block.number == included.block_number
            && block.hash == included.receipt.block_hash.to_string(),
        "Verified inclusion header {} does not match receipt hash {}",
        included.block_number,
        included.receipt.block_hash
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
        trade.order_side == plan.order.order_side(),
        "Finalized Swap side {} does not match {} order",
        trade.order_side,
        plan.order.order_side()
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
        return Ok(None);
    }

    let venue_order_id = VenueOrderId::new_checked(included.tx_hash.to_string())?;
    let ts_event = UnixNanos::from(timestamp_ns);
    let last_px = match plan.order.order_side() {
        OrderSide::Buy => fill_price_from_quote(last_qty, plan.amount_in, plan.quote_currency)?,
        _ => trade.execution_price,
    };
    Ok(Some(FinalizedSwapFill {
        venue_order_id,
        trade_id,
        last_qty,
        last_px,
        commission,
        ts_event,
    }))
}

async fn load_verified_wallet_after_fill(
    plan: &SwapPlan,
    included: &IncludedTransaction,
    executor: &TransactionExecutor,
) -> anyhow::Result<VerifiedWalletRefresh> {
    let mut token_universe = executor
        .wallet_balance
        .lock()
        .expect("wallet balance mutex poisoned")
        .token_universe
        .clone();
    token_universe.insert(plan.pool.token0.address);
    token_universe.insert(plan.pool.token1.address);

    let native_amount = required_verification(
        executor
            .verification
            .verify_balance(&executor.wallet_address, included.block_number)
            .await,
        "finalized native balance",
    )?;
    let native_balance = Money::from_u256(native_amount.value, plan.pool.chain.native_currency())?;
    let mut decisions = vec![verification_decision(
        &native_amount,
        Some(included.block_number),
        Some(included.block_number),
    )];
    let mut token_addresses = token_universe.iter().copied().collect::<Vec<_>>();
    token_addresses.sort_unstable();
    let mut token_balances = Vec::with_capacity(token_addresses.len());
    for address in token_addresses {
        let identities = executor
            .deployment_manifest
            .tokens
            .iter()
            .filter(|identity| Address::from_str(&identity.address).ok() == Some(address))
            .collect::<Vec<_>>();
        anyhow::ensure!(
            identities.len() == 1,
            "Wallet token {address} does not have exactly one deployment manifest identity"
        );
        let identity = identities[0];
        let token = Token::new(
            plan.pool.chain.clone(),
            address,
            identity.name.clone(),
            identity.symbol.clone(),
            identity.decimals,
        );
        let call = ERC20::balanceOfCall {
            account: executor.wallet_address,
        }
        .abi_encode();
        let amount = required_verification(
            executor
                .verification
                .verify_decoded_call(
                    None,
                    &address,
                    U256::ZERO,
                    &call,
                    included.block_number,
                    |result| ERC20::balanceOfCall::abi_decode_returns(result).map_err(Into::into),
                )
                .await,
            "finalized token balance",
        )?;
        decisions.push(verification_decision(
            &amount,
            Some(included.block_number),
            Some(included.block_number),
        ));
        token_balances.push(TokenBalance::new(amount.value, token));
    }

    let mut wallet_balance = WalletBalance::new(token_universe);
    let balances = wallet_balance.replace_balances(native_balance, token_balances)?;
    Ok(VerifiedWalletRefresh {
        wallet_balance,
        balances,
        decisions,
    })
}

async fn verify_connect_capabilities(
    verification: &VerificationCoordinator,
    manifest: &BlockchainDeploymentManifest,
    wallet: Address,
    weth: Address,
    block: u64,
) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
    let contract = manifest
        .contracts
        .first()
        .ok_or_else(|| anyhow::anyhow!("Deployment manifest has no capability probe target"))?;
    let contract_address = Address::from_str(&contract.address)
        .with_context(|| "Deployment manifest capability probe target is invalid")?;
    let storage = required_verification(
        verification
            .verify_storage(&contract_address, &B256::ZERO, block)
            .await,
        "Blockchain explicit-height storage capability",
    )?;

    let balance_call = ERC20::balanceOfCall { account: wallet }.abi_encode();
    let gas = required_verification(
        verification
            .verify_gas_estimate(&wallet, &weth, U256::ZERO, &balance_call, block)
            .await,
        "Blockchain explicit-height gas capability",
    )?;

    let mut decisions = vec![
        verification_decision(&storage, Some(block), Some(block)),
        verification_decision(&gas, Some(block), Some(block)),
    ];

    for pool in &manifest.pools {
        let quote_contract = Address::from_str(&pool.quote_contract)
            .with_context(|| "Deployment manifest quote capability target is invalid")?;
        let token_in = Address::from_str(&pool.token0)
            .with_context(|| "Deployment manifest quote input token is invalid")?;
        let token_out = Address::from_str(&pool.token1)
            .with_context(|| "Deployment manifest quote output token is invalid")?;
        let fee = U24::try_from(pool.fee)
            .map_err(|_| anyhow::anyhow!("Deployment manifest pool fee is invalid"))?;
        let quote = required_verification(
            verification
                .verify_quote_exact_input_single(
                    &quote_contract,
                    token_in,
                    token_out,
                    U256::from(1u64),
                    fee,
                    block,
                )
                .await,
            "Blockchain explicit-height quote capability",
        )?;
        decisions.push(verification_decision(&quote, Some(block), Some(block)));
    }

    let trace = required_verification(
        verification.verify_call_trace_capability().await,
        "Blockchain call trace capability",
    )?;
    decisions.push(verification_decision(&trace, None, None));
    Ok(decisions)
}

/// Runs the read-only pre-trade checks for a swap: deployed bytecode at the pool, router,
/// and token addresses, and an operator-prepared router allowance and input-token balance
/// covering the amount. The shared signing pipeline checks the exact maximum native cost.
/// Never wraps or approves.
async fn check_swap_preconditions(
    plan: &SwapPlan,
    block: u64,
    executor: &TransactionExecutor,
) -> anyhow::Result<Vec<ExecutionVerificationDecision>> {
    let mut decisions = Vec::new();
    let factory_call = UniswapV3RouterState::factoryCall.abi_encode();
    let router_factory = required_verification(
        executor
            .verification
            .verify_decoded_call(
                None,
                &plan.router,
                U256::ZERO,
                &factory_call,
                block,
                |result| {
                    UniswapV3RouterState::factoryCall::abi_decode_returns(result)
                        .map_err(Into::into)
                },
            )
            .await,
        "swap router factory",
    )?;
    anyhow::ensure!(
        router_factory.value == plan.factory,
        "Router {} reports an unexpected factory",
        plan.router,
    );
    decisions.push(verification_decision(
        &router_factory,
        Some(block),
        Some(block),
    ));

    let weth_call = UniswapV3RouterState::WETH9Call.abi_encode();
    let router_weth = required_verification(
        executor
            .verification
            .verify_decoded_call(
                None,
                &plan.router,
                U256::ZERO,
                &weth_call,
                block,
                |result| {
                    UniswapV3RouterState::WETH9Call::abi_decode_returns(result).map_err(Into::into)
                },
            )
            .await,
        "swap router wrapped native",
    )?;
    anyhow::ensure!(
        router_weth.value == plan.weth,
        "Router {} reports an unexpected wrapped native contract",
        plan.router,
    );
    decisions.push(verification_decision(
        &router_weth,
        Some(block),
        Some(block),
    ));

    let pool_call = UniswapV3Factory::getPoolCall {
        tokenA: plan.token_in,
        tokenB: plan.token_out,
        fee: plan.fee,
    }
    .abi_encode();
    let registered_pool = required_verification(
        executor
            .verification
            .verify_decoded_call(
                None,
                &plan.factory,
                U256::ZERO,
                &pool_call,
                block,
                |result| {
                    UniswapV3Factory::getPoolCall::abi_decode_returns(result).map_err(Into::into)
                },
            )
            .await,
        "swap factory pool",
    )?;
    anyhow::ensure!(
        registered_pool.value == plan.pool_address,
        "Factory resolves an unexpected pool for the swap token pair and fee"
    );
    decisions.push(verification_decision(
        &registered_pool,
        Some(block),
        Some(block),
    ));

    for token in [&plan.pool.token0, &plan.pool.token1] {
        let decimals_call = ERC20::decimalsCall.abi_encode();
        let decimals = required_verification(
            executor
                .verification
                .verify_decoded_call(
                    None,
                    &token.address,
                    U256::ZERO,
                    &decimals_call,
                    block,
                    |result| ERC20::decimalsCall::abi_decode_returns(result).map_err(Into::into),
                )
                .await,
            "swap token decimals",
        )?;
        anyhow::ensure!(
            decimals.value == token.decimals,
            "Token {} reports unexpected decimals",
            token.address,
        );
        decisions.push(verification_decision(&decimals, Some(block), Some(block)));
    }

    let allowance_call = ERC20::allowanceCall {
        owner: executor.wallet_address,
        spender: plan.router,
    }
    .abi_encode();
    let allowance = required_verification(
        executor
            .verification
            .verify_decoded_call(
                None,
                &plan.token_in,
                U256::ZERO,
                &allowance_call,
                block,
                |result| ERC20::allowanceCall::abi_decode_returns(result).map_err(Into::into),
            )
            .await,
        "swap input allowance",
    )?;

    if allowance.value < plan.amount_in {
        anyhow::bail!(
            "Router allowance {} is below the swap amount {} for input token {}; approve the router explicitly before submitting",
            allowance.value,
            plan.amount_in,
            plan.token_in
        );
    }
    decisions.push(verification_decision(&allowance, Some(block), Some(block)));

    let balance_call = ERC20::balanceOfCall {
        account: executor.wallet_address,
    }
    .abi_encode();
    let balance = required_verification(
        executor
            .verification
            .verify_decoded_call(
                None,
                &plan.token_in,
                U256::ZERO,
                &balance_call,
                block,
                |result| ERC20::balanceOfCall::abi_decode_returns(result).map_err(Into::into),
            )
            .await,
        "swap input balance",
    )?;

    if balance.value < plan.amount_in {
        anyhow::bail!(
            "Input token {} balance {} is below the swap amount {}",
            plan.token_in,
            balance.value,
            plan.amount_in
        );
    }
    decisions.push(verification_decision(&balance, Some(block), Some(block)));

    Ok(decisions)
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

fn swap_token_pair(
    side: OrderSide,
    base: Address,
    quote: Address,
) -> anyhow::Result<(Address, Address)> {
    match side {
        OrderSide::Sell => Ok((base, quote)),
        OrderSide::Buy => Ok((quote, base)),
    }
}

fn fill_price_from_quote(
    last_qty: Quantity,
    quote_amount: U256,
    quote_currency: Currency,
) -> anyhow::Result<Price> {
    let quote = Money::from_u256(quote_amount, quote_currency)?;
    Price::from_decimal_dp(quote.as_decimal() / last_qty.as_decimal(), FIXED_PRECISION)
        .map_err(anyhow::Error::from)
}

fn raw_amount_to_quantity(amount: U256, decimals: u8) -> anyhow::Result<Quantity> {
    if amount.is_zero() {
        anyhow::bail!("Executed amount must be positive");
    }
    let quantity = if decimals >= FIXED_PRECISION {
        let scale = U256::from(10u64)
            .checked_pow(U256::from(decimals - FIXED_PRECISION))
            .ok_or_else(|| anyhow::anyhow!("Executed amount scaling overflow"))?;
        Quantity::from_u256(amount / scale, FIXED_PRECISION).map_err(anyhow::Error::from)?
    } else {
        Quantity::from_u256(amount, decimals).map_err(anyhow::Error::from)?
    };

    if quantity.is_zero() {
        anyhow::bail!(
            "Executed amount {amount} is below representable quantity precision {FIXED_PRECISION}"
        );
    }
    Ok(quantity)
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

impl BlockchainExecutionClient {
    async fn build_execution_verification_migration(
        &self,
        snapshot: ExecutionVerificationMigrationSnapshot,
        finalized: VerifiedBlockHeader,
        finalized_headers: &[VerifiedBlockHeader],
        nonce_verification: &Verified<u64>,
    ) -> anyhow::Result<ExecutionVerificationMigration> {
        let next_canonical_nonce = nonce_verification.value;
        let mut hashes_by_intent: HashMap<i64, Vec<&ExecutionTransactionHashRow>> = HashMap::new();
        for hash in &snapshot.hashes {
            hashes_by_intent
                .entry(hash.intent_id)
                .or_default()
                .push(hash);
        }
        let active_count = snapshot
            .intents
            .iter()
            .filter(|intent| intent.active)
            .count();
        anyhow::ensure!(
            active_count <= 1,
            "Retained execution history has multiple active signer owners"
        );

        let mut nonce_owners = HashMap::new();
        let mut records = Vec::with_capacity(snapshot.intents.len());
        let finalized_headers = finalized_headers
            .iter()
            .map(durable_verified_header)
            .collect::<Vec<_>>();

        for intent in &snapshot.intents {
            anyhow::ensure!(
                intent.chain_id == self.chain.chain_id
                    && intent.wallet_address == self.config.wallet_address,
                "Retained execution intent belongs to another signer"
            );
            let purpose = TransactionPurpose::parse(&intent.purpose).ok_or_else(|| {
                anyhow::anyhow!("Retained execution intent has an unsupported purpose")
            })?;
            let hashes = hashes_by_intent
                .get(&intent.id)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let current = hashes
                .iter()
                .copied()
                .filter(|hash| hash.current)
                .collect::<Vec<_>>();
            anyhow::ensure!(
                current.len() <= 1,
                "Retained execution intent {} has multiple current hashes",
                intent.id
            );
            let current = current.first().copied();
            let mut authenticated = HashMap::new();

            for hash in hashes {
                if hash.payload_expected {
                    let raw = open_execution_payload(
                        self.payload_keys
                            .as_deref()
                            .expect("Postgres execution requires payload keys"),
                        self.payload_policy(),
                        intent,
                        hash,
                        "verification migration",
                    )?;
                    authenticated.insert(hash.id, raw);
                } else {
                    anyhow::ensure!(
                        hash.raw_transaction.is_none() && hash.sealed_transaction.is_none(),
                        "Unowned replacement hash retains signed bytes"
                    );
                }
            }

            if let Some(nonce) = intent.nonce {
                anyhow::ensure!(
                    nonce_owners.insert(nonce, intent.id).is_none(),
                    "Retained execution history has duplicate signer nonce ownership"
                );
            }

            let base_decision = verification_decision(
                nonce_verification,
                Some(finalized.number),
                Some(finalized.number),
            );

            if !intent.active {
                if matches!(intent.status.as_str(), "finalized" | "reverted") {
                    let expected_marker =
                        if purpose == TransactionPurpose::Swap && intent.status == "finalized" {
                            intent.fill_emitted
                        } else {
                            intent.terminal_emitted
                        };
                    anyhow::ensure!(
                        expected_marker,
                        "Released terminal intent {} has no durable event marker",
                        intent.id
                    );
                } else {
                    anyhow::ensure!(
                        matches!(intent.status.as_str(), "dropped" | "recoverable")
                            && authenticated.is_empty(),
                        "Released nonterminal intent {} retains signed ownership",
                        intent.id
                    );
                    records.push(ExecutionVerificationMigrationRecord {
                        intent_id: intent.id,
                        nonce: intent.nonce,
                        transaction_hash: None,
                        terminal_status: None,
                        block_number: None,
                        block_hash: None,
                        receipt_success: None,
                        gas_used: None,
                        effective_gas_price: None,
                        recover_prepared: false,
                        decisions: vec![base_decision],
                    });
                    continue;
                }
            }

            if intent.active && intent.nonce.is_none() {
                anyhow::ensure!(
                    intent.status == "prepared" && hashes.is_empty(),
                    "Unassigned active intent {} is not an unsigned preparation",
                    intent.id
                );
                records.push(ExecutionVerificationMigrationRecord {
                    intent_id: intent.id,
                    nonce: None,
                    transaction_hash: None,
                    terminal_status: None,
                    block_number: None,
                    block_hash: None,
                    receipt_success: None,
                    gas_used: None,
                    effective_gas_price: None,
                    recover_prepared: true,
                    decisions: vec![base_decision],
                });
                continue;
            }

            let nonce = intent.nonce.ok_or_else(|| {
                anyhow::anyhow!("Retained signed intent {} has no nonce", intent.id)
            })?;
            let current = current.ok_or_else(|| {
                anyhow::anyhow!("Retained signed intent {} has no current hash", intent.id)
            })?;
            let raw_transaction = authenticated.get(&current.id).ok_or_else(|| {
                anyhow::anyhow!(
                    "Retained signed intent {} has no authenticated current payload",
                    intent.id
                )
            })?;

            if intent.active && nonce == next_canonical_nonce {
                anyhow::ensure!(
                    !matches!(intent.status.as_str(), "finalized" | "reverted"),
                    "Active terminal intent conflicts with the canonical nonce ledger"
                );
                records.push(ExecutionVerificationMigrationRecord {
                    intent_id: intent.id,
                    nonce: Some(nonce),
                    transaction_hash: Some(current.transaction_hash.clone()),
                    terminal_status: None,
                    block_number: None,
                    block_hash: None,
                    receipt_success: None,
                    gas_used: None,
                    effective_gas_price: None,
                    recover_prepared: false,
                    decisions: vec![base_decision],
                });
                continue;
            }
            anyhow::ensure!(
                nonce < next_canonical_nonce,
                "Retained active nonce {nonce} is above canonical nonce {next_canonical_nonce}"
            );

            let receipt_verification = required_verification(
                self.verification
                    .verify_receipt(&B256::from_str(&current.transaction_hash).with_context(
                        || {
                            format!(
                                "Retained transaction hash {} is invalid",
                                current.transaction_hash
                            )
                        },
                    )?)
                    .await,
                "migration receipt",
            )?;
            let receipt = receipt_verification.value.clone();
            anyhow::ensure!(
                receipt.block_number <= finalized.number,
                "Retained terminal receipt is above the verified finalized boundary"
            );
            let inclusion_verification = required_verification(
                self.verification.verify_block(receipt.block_number).await,
                "migration inclusion header",
            )?;
            anyhow::ensure!(
                inclusion_verification.value.hash == receipt.block_hash
                    && finalized_headers.iter().any(|header| {
                        header.number == receipt.block_number
                            && header.hash == receipt.block_hash.to_string()
                    }),
                "Retained terminal receipt is not on the verified finalized ancestry"
            );
            let tx_hash = B256::from_str(&current.transaction_hash)
                .context("Retained transaction hash is invalid")?;
            let included = IncludedTransaction {
                intent_id: intent.id,
                nonce,
                tx_hash,
                block_number: receipt.block_number,
                receipt: receipt.clone(),
                finality: StableFinality {
                    decisions: Vec::new(),
                    inclusion_header: durable_verified_header(&inclusion_verification.value),
                    finalized_headers: finalized_headers.clone(),
                },
            };
            let trace_purpose = match purpose {
                TransactionPurpose::Wrap => "wrap",
                TransactionPurpose::Approve => "approve",
                TransactionPurpose::Swap => {
                    match self.restore_swap_plan(intent)?.order.order_side() {
                        OrderSide::Sell => "swap_sell",
                        OrderSide::Buy => "swap_buy",
                    }
                }
            };
            let mut decisions = vec![
                verification_decision(
                    &receipt_verification,
                    Some(receipt.block_number),
                    Some(receipt.block_number),
                ),
                verification_decision(
                    &inclusion_verification,
                    Some(receipt.block_number),
                    Some(receipt.block_number),
                ),
            ];
            decisions.extend(
                verify_finalized_transaction_identity(
                    &included,
                    intent,
                    nonce,
                    raw_transaction,
                    &self.verification,
                    self.wallet_address,
                    self.chain.chain_id,
                    &self
                        .config
                        .verification
                        .as_ref()
                        .expect("verification config validated")
                        .deployment_manifest,
                    trace_purpose,
                )
                .await?,
            );
            let terminal_status = if receipt.status {
                TransactionStatus::Finalized
            } else {
                TransactionStatus::Reverted
            };

            if !intent.active {
                anyhow::ensure!(
                    intent.status == terminal_status.as_str(),
                    "Released terminal intent status conflicts with its verified receipt"
                );
            }
            records.push(ExecutionVerificationMigrationRecord {
                intent_id: intent.id,
                nonce: Some(nonce),
                transaction_hash: Some(current.transaction_hash.clone()),
                terminal_status: Some(terminal_status),
                block_number: Some(receipt.block_number),
                block_hash: Some(receipt.block_hash.to_string()),
                receipt_success: Some(receipt.status),
                gas_used: Some(receipt.gas_used),
                effective_gas_price: Some(receipt.effective_gas_price.to_string()),
                recover_prepared: false,
                decisions,
            });
        }

        Ok(ExecutionVerificationMigration { snapshot, records })
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

        self.pending_tasks.begin_shutdown();
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

        if !self.pending_tasks.is_open() {
            self.emitter
                .emit_order_denied(&order, "Blockchain execution client is shutting down");
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

        let future = async move {
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
        };

        if let Err(e) = self.pending_tasks.spawn(future) {
            release_preparing_slot(&self.in_flight);
            log::warn!("Skipping blockchain swap after shutdown began: {e}");
        }

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

        if !self.pending_tasks.is_open() || !self.pending_tasks.is_empty() {
            self.pending_tasks.begin_shutdown();
            self.pending_tasks
                .finish_shutdown(Duration::from_secs(5), Duration::from_secs(2))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to terminate blockchain submissions: {e}"))?;
            self.signer = None;
            self.pending_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start blockchain task generation: {e}"))?;
        }
        release_preparing_slot(&self.in_flight);

        let setup_guard = TaskGroupGuard::new(&[&self.pending_tasks], || {});

        let payload_keys = PayloadKeySet::load(
            self.config.payload_key_env.as_deref(),
            &self.config.payload_key_retired_env,
            self.config.payload_deployment_id.as_deref(),
        )?
        .map(Arc::new);

        // Attach or reuse the durable store for execution transaction records
        if self.cache.database.is_some() || self.config.postgres_cache_database_config.is_some() {
            let keys = payload_keys.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Postgres execution requires an active payload key and deployment identity"
                )
            })?;

            if self.cache.database.is_none() {
                let pg_options = self
                    .config
                    .postgres_cache_database_config
                    .as_ref()
                    .expect("Postgres configuration checked above");
                let database = crate::cache::database::BlockchainCacheDatabase::connect(
                    pg_options.clone().into(),
                )
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed to connect to the Postgres cache database: {e}")
                })?;
                self.cache.database = Some(database);
            }
            self.cache
                .database
                .as_ref()
                .expect("database was attached")
                .require_execution_payload_storage_ready(keys)
                .await?;
            self.cache.initialize_chain().await;
            self.cache.ensure_execution_transaction_schema().await?;
            let check = self
                .cache
                .database
                .as_ref()
                .expect("database was attached")
                .check_execution_payload_storage(
                    Some(keys),
                    Some(PayloadPolicy {
                        chain_id: self.chain.chain_id,
                        signer: self.wallet_address,
                        gas_limit: self.config.gas_limit,
                        max_fee_per_gas: self.config.max_fee_per_gas_wei,
                    }),
                    100,
                )
                .await?;
            anyhow::ensure!(
                check.protected,
                "Postgres execution requires protected payload storage"
            );
        } else {
            log::warn!(
                "No Postgres cache database configured; transactions will be refused (no durable store)"
            );
        }
        self.payload_keys = payload_keys;

        let verification = self
            .config
            .verification
            .as_ref()
            .expect("verification config validated at construction");
        let position = if let Some(database) = self.cache.database.as_ref() {
            database
                .load_execution_verification_position(
                    self.chain.chain_id,
                    &self.config.wallet_address,
                    &verification.manifest_version,
                    &verification.manifest_digest,
                )
                .await?
        } else {
            None
        };
        let migration_snapshot = if position.is_none() {
            if let Some(database) = self.cache.database.as_ref() {
                let snapshot = database
                    .load_execution_verification_migration_snapshot(
                        self.chain.chain_id,
                        &self.config.wallet_address,
                    )
                    .await?;

                if snapshot.intents.is_empty() {
                    None
                } else {
                    Some(snapshot)
                }
            } else {
                None
            }
        } else {
            None
        };

        let chain_id_verification = required_verification(
            self.verification.verify_chain_id().await,
            "Blockchain chain ID",
        )?;
        let checkpoint_verification = required_verification(
            self.verification.verify_checkpoint().await,
            "Blockchain checkpoint",
        )?;
        let checkpoint = checkpoint_verification.value;
        let finalized_verification = required_verification(
            self.verification.verify_finalized_header().await,
            "Blockchain finalized header",
        )?;
        let finalized = finalized_verification.value;
        let mut connect_decisions = vec![
            verification_decision(&chain_id_verification, None, None),
            verification_decision(
                &checkpoint_verification,
                Some(checkpoint.number),
                Some(checkpoint.number),
            ),
            verification_decision(
                &finalized_verification,
                Some(finalized.number),
                Some(finalized.number),
            ),
        ];
        let mut finalized_headers = if let Some(position) = position.as_ref() {
            let durable_tip = parse_verified_header(&position.finalized_tip)?;
            anyhow::ensure!(
                durable_tip.number >= checkpoint.number,
                "Durable finalized header tip precedes the trusted checkpoint"
            );
            let durable_tip_verification = required_verification(
                self.verification.verify_block(durable_tip.number).await,
                "Blockchain durable finalized tip",
            )?;
            anyhow::ensure!(
                durable_tip_verification.value == durable_tip,
                "Durable finalized header tip conflicts with independent sources"
            );
            connect_decisions.push(verification_decision(
                &durable_tip_verification,
                Some(durable_tip.number),
                Some(durable_tip.number),
            ));
            vec![durable_tip]
        } else {
            vec![checkpoint]
        };
        let mut ancestry_cursor = *finalized_headers
            .last()
            .expect("finalized header ledger is nonempty");
        anyhow::ensure!(
            finalized.number >= ancestry_cursor.number,
            "Verified finalized height regressed below the durable finalized header tip"
        );

        while ancestry_cursor.number < finalized.number {
            let end = ancestry_cursor
                .number
                .saturating_add(4_096)
                .min(finalized.number);
            let start = ancestry_cursor.number.saturating_add(1);
            let headers_verification = required_verification(
                self.verification
                    .verify_header_window(ancestry_cursor, end)
                    .await,
                "Blockchain finalized ancestry",
            )?;
            let headers = &headers_verification.value;
            ancestry_cursor = *headers
                .last()
                .expect("nonempty ancestry window advances the cursor");
            finalized_headers.extend(headers.iter().copied());
            connect_decisions.push(verification_decision(
                &headers_verification,
                Some(start),
                Some(end),
            ));
        }
        anyhow::ensure!(
            ancestry_cursor == finalized,
            "Verified finalized header conflicts with its ancestry window"
        );
        let nonce_verification = required_verification(
            self.verification
                .verify_transaction_count(&self.wallet_address, finalized.number)
                .await,
            "Blockchain finalized transaction count",
        )?;
        let observed_canonical_nonce = nonce_verification.value;
        let next_canonical_nonce = position
            .as_ref()
            .map_or(observed_canonical_nonce, |position| {
                position.next_canonical_nonce
            });

        if let Some(position) = position.as_ref() {
            log::debug!(
                "Resumed execution verification ledger at nonce revision {} with observed finalized nonce {}",
                position.revision,
                observed_canonical_nonce,
            );
        }
        connect_decisions.push(verification_decision(
            &nonce_verification,
            Some(finalized.number),
            Some(finalized.number),
        ));
        let deployment_verification = required_verification(
            self.verification
                .verify_deployment_manifest(&verification.deployment_manifest, finalized.number)
                .await,
            "Blockchain deployment manifest",
        )?;
        connect_decisions.push(verification_decision(
            &deployment_verification,
            Some(finalized.number),
            Some(finalized.number),
        ));
        connect_decisions.extend(
            verify_connect_capabilities(
                &self.verification,
                &verification.deployment_manifest,
                self.wallet_address,
                self.weth_address,
                finalized.number,
            )
            .await?,
        );
        let migration = if let Some(snapshot) = migration_snapshot {
            Some(
                self.build_execution_verification_migration(
                    snapshot,
                    finalized,
                    &finalized_headers,
                    &nonce_verification,
                )
                .await?,
            )
        } else {
            None
        };

        if let Some(database) = self.cache.database.as_ref() {
            let identities = std::iter::once(&verification.authoritative)
                .chain(
                    verification
                        .verifiers
                        .iter()
                        .map(|provider| &provider.identity),
                )
                .collect::<Vec<_>>();
            let provider_ids = identities
                .iter()
                .map(|identity| identity.provider_id.clone())
                .collect::<Vec<_>>();
            let operator_ids = identities
                .iter()
                .map(|identity| identity.operator_id.clone())
                .collect::<Vec<_>>();
            let failure_domain_ids = identities
                .iter()
                .flat_map(|identity| identity.failure_domain_ids.iter().cloned())
                .collect::<Vec<_>>();
            let finalized_headers = finalized_headers
                .iter()
                .map(durable_verified_header)
                .collect::<Vec<_>>();
            database
                .ensure_execution_verification_schema(&ExecutionVerificationBootstrap {
                    chain_id: self.chain.chain_id,
                    wallet_address: &self.config.wallet_address,
                    manifest_version: &verification.manifest_version,
                    manifest_digest: &verification.manifest_digest,
                    checkpoint_number: checkpoint.number,
                    checkpoint_hash: &checkpoint.hash.to_string(),
                    checkpoint_parent_hash: &checkpoint.parent_hash.to_string(),
                    checkpoint_timestamp: checkpoint.timestamp,
                    checkpoint_base_fee_per_gas: checkpoint.base_fee_per_gas,
                    finalized_headers: &finalized_headers,
                    next_canonical_nonce,
                    observed_canonical_nonce,
                    provider_ids: &provider_ids,
                    operator_ids: &operator_ids,
                    failure_domain_ids: &failure_domain_ids,
                    decisions: &connect_decisions,
                    migration: migration.as_ref(),
                })
                .await?;
        }

        let payload_connect_lease = if let (Some(database), Some(keys)) =
            (self.cache.database.as_ref(), self.payload_keys.as_deref())
        {
            Some(
                database
                    .require_execution_payload_storage(keys, self.payload_policy(), 100)
                    .await?,
            )
        } else {
            None
        };

        // Load the signer key from the configured environment variable; the key is never
        // logged, serialized, or stored in configuration
        let private_key = Zeroizing::new(
            std::env::var(&self.config.signer_private_key_env).map_err(|_| {
                anyhow::anyhow!(
                    "Signer private key environment variable '{}' is not set",
                    self.config.signer_private_key_env
                )
            })?,
        );
        let encoded_key = private_key.trim();
        let encoded_key = encoded_key.strip_prefix("0x").unwrap_or(encoded_key);
        let key_bytes = Zeroizing::new(hex::decode_array::<32>(encoded_key).map_err(|_| {
            anyhow::anyhow!(
                "Signer private key in '{}' is not a valid hex private key",
                self.config.signer_private_key_env
            )
        })?);
        let signer = PrivateKeySigner::from_slice(&key_bytes[..]).map_err(|_| {
            anyhow::anyhow!(
                "Signer private key in '{}' is not a valid secp256k1 private key",
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

        self.signer = Some(Arc::new(signer));
        drop(payload_connect_lease);

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
        setup_guard.disarm();
        log::info!(
            "Blockchain execution client connected on chain {}",
            self.chain.name
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.pending_tasks.begin_shutdown();
        let tasks_result = self
            .pending_tasks
            .finish_shutdown(Duration::from_secs(5), Duration::from_secs(2))
            .await;
        self.signer = None;
        self.core.set_disconnected();
        tasks_result
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to terminate blockchain submissions: {e}"))?;
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
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::atomic::{AtomicU64, Ordering},
    };

    use alloy::{
        primitives::{address, aliases::I24},
        sol_types::SolValue,
    };
    use nautilus_common::{
        cache::Cache, live::runner::replace_exec_event_sender, messages::ExecutionEvent,
        testing::wait_until_async,
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
    use sqlx::postgres::{PgAdvisoryLock, PgAdvisoryLockKey, PgPoolOptions};

    use super::*;
    use crate::{
        cache::database::tests::connect_test_database,
        config::{
            BlockchainCallEdgeManifest, BlockchainChainAnchorConfig, BlockchainContractManifest,
            BlockchainContractProbe, BlockchainContractRole, BlockchainDeploymentManifest,
            BlockchainPoolManifest, BlockchainProviderIdentity, BlockchainTokenManifest,
            BlockchainVerificationConfig, BlockchainVerificationProviderConfig, QuoteSpendLimit,
        },
        constants::BLOCKCHAIN_VENUE,
        exchanges::arbitrum::UNISWAP_V3,
        rpc::http::{
            EXECUTION_RPC_TIMEOUT_SECS,
            tests::mock::{MockRpcState, start_mock_rpc_server},
        },
    };

    /// Polls for the receipt of a broadcast transaction until it exists or the poll bound
    /// is exhausted. A `null` receipt result is a legitimate pending response.
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
    const CALL_ALLOWANCE_1000: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x00000000000000000000000000000000000000000000000000000000000003e8\"}";
    const CALL_ALLOWANCE_MAX: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\"}";
    const CALL_FACTORY: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x0000000000000000000000001f98431c8ad98523631ae4a59f267346ea31f984\"}";
    const CALL_WETH: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x00000000000000000000000082af49447d8a07e3bd95bd0d56f35241523fbab1\"}";
    const CALL_USDC: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x000000000000000000000000af88d065e77c8cc2239327c5edb3a432268e5831\"}";
    const CALL_FEE_500: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x00000000000000000000000000000000000000000000000000000000000001f4\"}";
    const CALL_POOL: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x000000000000000000000000c6962004f452be9203591991d15f6b388e09e8d0\"}";
    const CALL_REVERTED: &str =
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":3,"message":"execution reverted"}}"#;
    const STORAGE_ZERO: &str = r#"{"jsonrpc":"2.0","id":1,"result":"0x0000000000000000000000000000000000000000000000000000000000000000"}"#;
    const TRACE_UNKNOWN_TRANSACTION: &str =
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"transaction not found"}}"#;
    const CALL_DECIMALS_18: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x0000000000000000000000000000000000000000000000000000000000000012\"}";
    const CALL_DECIMALS_6: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x0000000000000000000000000000000000000000000000000000000000000006\"}";
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
    const LIVE_READ_SMOKE_ENV: &str = "BLOCKCHAIN_LIVE_READ_SMOKE";
    const LIVE_READ_SMOKE_RPC: &str = "https://arb1.arbitrum.io/rpc";
    const TEST_TIMEOUT: Duration = Duration::from_secs(10);

    const WETH_ADDRESS: Address = address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1");
    const USDC_ADDRESS: Address = address!("af88d065e77c8cC2239327C5EDb3A432268e5831");
    const ROUTER_ADDRESS: Address = address!("E592427A0AEce92De3Edee1F18E0157C05861564");

    // Anvil development key for WALLET (public, test-only)
    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    const BALANCE_OF_SELECTOR: &str = "0x70a08231";
    const ALLOWANCE_SELECTOR: &str = "0xdd62ed3e";
    const POOL_TOKEN0_SELECTOR: &str = "0x0dfe1681";
    const POOL_TOKEN1_SELECTOR: &str = "0xd21220a7";
    const POOL_FEE_SELECTOR: &str = "0xddca3f43";
    const DECIMALS_SELECTOR: &str = "0x313ce567";
    const FACTORY_SELECTOR: &str = "0xc45a0155";
    const WETH9_SELECTOR: &str = "0x4aa4a4fc";
    const GET_POOL_SELECTOR: &str = "0x1698ee82";
    const QUOTE_EXACT_INPUT_SELECTOR: &str = "0xc6a5026a";
    const QUOTE_EXACT_OUTPUT_SELECTOR: &str = "0xbd21704a";

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
        let verifier_separator = if http_rpc_url.contains('?') { '&' } else { '?' };
        let code_hash = keccak256(
            hex::decode("6080604052348015600e575f5ffd5b5060").expect("valid test bytecode"),
        )
        .to_string();
        let result = |response: &str| {
            serde_json::from_str::<serde_json::Value>(response).unwrap()["result"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let probe = |call_data: &str, expected_output: String| BlockchainContractProbe {
            call_data: call_data.to_string(),
            expected_output,
        };
        let contract = |address: &str, role| {
            let probes = match role {
                BlockchainContractRole::Router => vec![
                    probe(FACTORY_SELECTOR, result(CALL_FACTORY)),
                    probe(WETH9_SELECTOR, result(CALL_WETH)),
                ],
                BlockchainContractRole::Factory => vec![probe(
                    &hex::encode_prefixed(
                        UniswapV3Factory::getPoolCall {
                            tokenA: WETH_ADDRESS,
                            tokenB: USDC_ADDRESS,
                            fee: U24::try_from(500u32).unwrap(),
                        }
                        .abi_encode(),
                    ),
                    result(CALL_POOL),
                )],
                BlockchainContractRole::WrappedNative => {
                    vec![probe(DECIMALS_SELECTOR, result(CALL_DECIMALS_18))]
                }
                BlockchainContractRole::Quote => {
                    vec![probe(FACTORY_SELECTOR, result(CALL_FACTORY))]
                }
                BlockchainContractRole::Token => {
                    vec![probe(DECIMALS_SELECTOR, result(CALL_DECIMALS_6))]
                }
                BlockchainContractRole::Pool => vec![
                    probe(POOL_TOKEN0_SELECTOR, result(CALL_WETH)),
                    probe(POOL_TOKEN1_SELECTOR, result(CALL_USDC)),
                    probe(POOL_FEE_SELECTOR, result(CALL_FEE_500)),
                ],
                BlockchainContractRole::Implementation => Vec::new(),
            };
            BlockchainContractManifest {
                address: address.to_string(),
                role,
                runtime_code_hash: code_hash.clone(),
                proxy: None,
                probes,
            }
        };
        let deployment_manifest = BlockchainDeploymentManifest {
            version: "test-v1".to_string(),
            chain_id: chains::ARBITRUM.chain_id,
            chain_name: chains::ARBITRUM.name.to_string(),
            contracts: vec![
                contract(ROUTER, BlockchainContractRole::Router),
                contract(
                    "0x1F98431c8aD98523631AE4a59f267346ea31F984",
                    BlockchainContractRole::Factory,
                ),
                contract(WETH, BlockchainContractRole::WrappedNative),
                contract(
                    "0x61fFE014bA17989E743c5F6cB21bF9697530B21e",
                    BlockchainContractRole::Quote,
                ),
                contract(USDC, BlockchainContractRole::Token),
                contract(
                    "0xC6962004f452bE9203591991D15f6b388e09E8D0",
                    BlockchainContractRole::Pool,
                ),
            ],
            tokens: vec![
                BlockchainTokenManifest {
                    address: WETH.to_string(),
                    name: "Wrapped Ether".to_string(),
                    symbol: "WETH".to_string(),
                    decimals: 18,
                    asset_role: "both".to_string(),
                },
                BlockchainTokenManifest {
                    address: USDC.to_string(),
                    name: "USD Coin".to_string(),
                    symbol: "USDC".to_string(),
                    decimals: 6,
                    asset_role: "both".to_string(),
                },
            ],
            pools: vec![BlockchainPoolManifest {
                address: "0xC6962004f452bE9203591991D15f6b388e09E8D0".to_string(),
                token0: WETH.to_string(),
                token1: USDC.to_string(),
                fee: 500,
                factory: "0x1F98431c8aD98523631AE4a59f267346ea31F984".to_string(),
                quote_contract: "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".to_string(),
            }],
            call_edges: ["swap_sell", "swap_buy"]
                .into_iter()
                .map(|purpose| BlockchainCallEdgeManifest {
                    purpose: purpose.to_string(),
                    caller: ROUTER.to_string(),
                    target: "0xC6962004f452bE9203591991D15f6b388e09E8D0".to_string(),
                    call_type: "call".to_string(),
                })
                .collect(),
        };
        let manifest_digest =
            keccak256(serde_json::to_vec(&deployment_manifest).unwrap()).to_string();
        let verification = BlockchainVerificationConfig {
            authoritative: BlockchainProviderIdentity {
                provider_id: "authoritative".to_string(),
                operator_id: "operator-a".to_string(),
                failure_domain_ids: vec!["domain-a".to_string()],
            },
            verifiers: vec![
                BlockchainVerificationProviderConfig {
                    identity: BlockchainProviderIdentity {
                        provider_id: "verifier-a".to_string(),
                        operator_id: "operator-b".to_string(),
                        failure_domain_ids: vec!["domain-b".to_string()],
                    },
                    http_rpc_url: format!("{http_rpc_url}{verifier_separator}source=verifier-a"),
                },
                BlockchainVerificationProviderConfig {
                    identity: BlockchainProviderIdentity {
                        provider_id: "verifier-b".to_string(),
                        operator_id: "operator-c".to_string(),
                        failure_domain_ids: vec!["domain-c".to_string()],
                    },
                    http_rpc_url: format!("{http_rpc_url}{verifier_separator}source=verifier-b"),
                },
            ],
            chain_anchor: BlockchainChainAnchorConfig {
                chain_id: chains::ARBITRUM.chain_id,
                chain_name: chains::ARBITRUM.name.to_string(),
                checkpoint_height: 30_346_560,
                checkpoint_hash:
                    "0x1111111111111111111111111111111111111111111111111111111111111111".to_string(),
                checkpoint_timestamp: 1_761_888_800,
                max_head_skew_blocks: 3,
                max_head_age_secs: u64::MAX,
                max_future_drift_secs: u64::MAX,
            },
            manifest_version: "test-v1".to_string(),
            manifest_digest,
            deployment_manifest,
        };
        BlockchainExecutionClientConfig::builder()
            .client_id(AccountId::from("BLOCKCHAIN-001"))
            .chain(chains::ARBITRUM.clone())
            .wallet_address(WALLET.to_string())
            .http_rpc_url(http_rpc_url)
            .verification(verification)
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

    fn buy_test_config(http_rpc_url: String) -> BlockchainExecutionClientConfig {
        let max_amount = expected_buy_amount_in().to_string();
        let mut config = test_config(http_rpc_url);
        config.allowed_token_pairs = Some(vec![
            (WETH.to_string(), USDC.to_string()),
            (USDC.to_string(), WETH.to_string()),
        ]);
        config.quote_spend_limits = Some(vec![quote_spend_limit(USDC, WETH, 6, &max_amount)]);
        config
    }

    fn refresh_test_manifest_digest(config: &mut BlockchainExecutionClientConfig) {
        let verification = config.verification.as_mut().unwrap();
        verification.manifest_digest =
            keccak256(serde_json::to_vec(&verification.deployment_manifest).unwrap()).to_string();
    }

    fn quote_spend_limit(
        token_in: &str,
        token_out: &str,
        spend_token_decimals: u8,
        max_amount: &str,
    ) -> QuoteSpendLimit {
        QuoteSpendLimit::builder()
            .token_in(token_in.to_string())
            .token_out(token_out.to_string())
            .spend_token(token_in.to_string())
            .spend_token_decimals(spend_token_decimals)
            .max_amount(max_amount.to_string())
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
        let state = with_connect_capabilities(state);
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
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", BLOCK_CANONICAL)
            .with_parameter_response("eth_getBlockByNumber", "finalized", BLOCK_FINALIZED)
            .with_parameter_response("eth_getBlockByNumber", "0x1cf0d42", BLOCK_FINALIZED)
            .with_response("eth_maxPriorityFeePerGas", MAX_PRIORITY_FEE)
            .with_call_response(FACTORY_SELECTOR, CALL_FACTORY)
            .with_call_response(WETH9_SELECTOR, CALL_WETH)
            .with_call_response(GET_POOL_SELECTOR, CALL_POOL)
            .with_call_response(POOL_TOKEN0_SELECTOR, CALL_WETH)
            .with_call_response(POOL_TOKEN1_SELECTOR, CALL_USDC)
            .with_call_response(POOL_FEE_SELECTOR, CALL_FEE_500)
            .with_contract_call_response(WETH, DECIMALS_SELECTOR, CALL_DECIMALS_18)
            .with_contract_call_response(USDC, DECIMALS_SELECTOR, CALL_DECIMALS_6)
            .with_call_response(
                QUOTE_EXACT_INPUT_SELECTOR,
                &quote_response(expected_sell_quote_amount()),
            )
            .with_call_response(
                QUOTE_EXACT_OUTPUT_SELECTOR,
                &quote_response(expected_buy_amount_in()),
            )
    }

    fn ready_rpc_state() -> MockRpcState {
        with_connect_capabilities(execution_rpc_state())
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE)
            .with_call_response(ALLOWANCE_SELECTOR, CALL_ALLOWANCE)
    }

    fn with_connect_capabilities(state: MockRpcState) -> MockRpcState {
        state
            .with_response("eth_getStorageAt", STORAGE_ZERO)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_parameter_response(
                "debug_traceTransaction",
                &B256::ZERO.to_string(),
                TRACE_UNKNOWN_TRANSACTION,
            )
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
            .with_call_response_sequence(ALLOWANCE_SELECTOR, &[CALL_ZERO, CALL_ALLOWANCE_1000])
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

    async fn await_recorded_requests(state: &MockRpcState, method: &str, expected: usize) {
        wait_until_async(
            || async {
                state
                    .recorded_requests()
                    .iter()
                    .filter(|request| request["method"] == method)
                    .count()
                    >= expected
            },
            TEST_TIMEOUT,
        )
        .await;
    }

    /// The block number served by the `eth_getBlockByNumber` fixture; swap quotes pin their
    /// profiler state to it so the freshness check passes.
    const FIXTURE_BLOCK: u64 = 30_346_560;
    const FIXTURE_BLOCK_PARAM: &str = "0x1cf0d40";
    const FIXTURE_BLOCK_HASH: &str =
        "0x1111111111111111111111111111111111111111111111111111111111111111";
    /// The timestamp served by the `eth_getBlockByNumber` fixture.
    const FIXTURE_BLOCK_TIMESTAMP: u64 = 1_761_888_800;
    /// Synthetic full-range liquidity for the test pool profiler.
    const TEST_LIQUIDITY: u128 = 1_000_000_000_000_000_000_000;

    fn test_profiler(pool: &Pool, block_number: u64) -> PoolProfiler {
        test_profiler_at_block(pool, block_number, FIXTURE_BLOCK_HASH)
    }

    fn test_profiler_at_block(pool: &Pool, block_number: u64, block_hash: &str) -> PoolProfiler {
        test_profiler_with_range(
            pool,
            block_number,
            block_hash,
            U160::from(1u128 << 96),
            -887_220,
            887_220,
            TEST_LIQUIDITY,
        )
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
            FIXTURE_BLOCK_HASH,
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
        block_hash: &str,
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
                block_hash.to_string(),
                BLOCK_SCOPED_SNAPSHOT_INDEX,
                BLOCK_SCOPED_SNAPSHOT_INDEX,
            )
            .with_block_hash(Some(block_hash.to_string())),
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

    fn test_market_buy_order(instrument_id: InstrumentId) -> OrderAny {
        market_buy_order_with_id(instrument_id, "O-SWAP-BUY-001")
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
            .add_pool_profiler(test_profiler_at_block(
                &pool,
                FIXTURE_BLOCK,
                FIXTURE_BLOCK_HASH,
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
        swap_client_with_database_config(test_name, state, test_config).await
    }

    async fn swap_client_with_buy_database(
        test_name: &str,
        state: MockRpcState,
    ) -> Option<(
        sqlx::PgPool,
        String,
        BlockchainExecutionClient,
        MockRpcState,
        Rc<RefCell<Cache>>,
    )> {
        swap_client_with_database_config(test_name, state, buy_test_config).await
    }

    async fn swap_client_with_database_config<F>(
        test_name: &str,
        state: MockRpcState,
        config: F,
    ) -> Option<(
        sqlx::PgPool,
        String,
        BlockchainExecutionClient,
        MockRpcState,
        Rc<RefCell<Cache>>,
    )>
    where
        F: FnOnce(String) -> BlockchainExecutionClientConfig,
    {
        let (admin_pool, pg_config) = connect_test_postgres(test_name).await?;
        let schema = format!("{test_name}_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;

        let db_options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let db_options = db_options.options([("search_path", schema.clone())]);
        let database = connect_test_database(db_options).await.unwrap();
        let addr = start_mock_rpc_server(state.clone()).await;
        let (mut client, cache) = swap_client_with_cache(config(format!("http://{addr}")));
        client.cache.database = Some(database);
        // Mirror the connect-time migration: tests create the pre-submission table shape
        client
            .cache
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        protect_test_storage(&mut client, &schema).await;
        initialize_test_verification_ledger(&client).await;
        client.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
        client.core.set_connected();

        Some((admin_pool, schema, client, state, cache))
    }

    async fn initialize_test_verification_ledger(client: &BlockchainExecutionClient) {
        initialize_test_verification_ledger_with_headers(
            client,
            &[ExecutionVerifiedHeader {
                number: FIXTURE_BLOCK,
                hash: FIXTURE_BLOCK_HASH.to_string(),
                parent_hash: "0x0000000000000000000000000000000000000000000000000000000000000001"
                    .to_string(),
                timestamp: FIXTURE_BLOCK_TIMESTAMP,
                base_fee_per_gas: Some(100_000_000),
            }],
        )
        .await;
    }

    async fn initialize_test_verification_ledger_with_headers(
        client: &BlockchainExecutionClient,
        finalized_headers: &[ExecutionVerifiedHeader],
    ) {
        ensure_test_verification_ledger(client, finalized_headers, 7, 7)
            .await
            .unwrap();
    }

    async fn ensure_test_verification_ledger(
        client: &BlockchainExecutionClient,
        finalized_headers: &[ExecutionVerifiedHeader],
        next_canonical_nonce: u64,
        observed_canonical_nonce: u64,
    ) -> anyhow::Result<()> {
        let verification = client.config.verification.as_ref().unwrap();
        let provider_ids = vec![
            "authoritative".to_string(),
            "verifier-a".to_string(),
            "verifier-b".to_string(),
        ];
        let operator_ids = vec![
            "operator-a".to_string(),
            "operator-b".to_string(),
            "operator-c".to_string(),
        ];
        let failure_domain_ids = vec![
            "domain-a".to_string(),
            "domain-b".to_string(),
            "domain-c".to_string(),
        ];
        let decisions = [ExecutionVerificationDecision {
            read_class: "numbered_block",
            height_start: Some(FIXTURE_BLOCK),
            height_end: Some(FIXTURE_BLOCK),
            normalized_value_digest: B256::ZERO.to_string(),
        }];
        client
            .cache
            .database
            .as_ref()
            .unwrap()
            .ensure_execution_verification_schema(&ExecutionVerificationBootstrap {
                chain_id: 42_161,
                wallet_address: WALLET,
                manifest_version: &verification.manifest_version,
                manifest_digest: &verification.manifest_digest,
                checkpoint_number: FIXTURE_BLOCK,
                checkpoint_hash: FIXTURE_BLOCK_HASH,
                checkpoint_parent_hash:
                    "0x0000000000000000000000000000000000000000000000000000000000000001",
                checkpoint_timestamp: FIXTURE_BLOCK_TIMESTAMP,
                checkpoint_base_fee_per_gas: Some(100_000_000),
                finalized_headers,
                next_canonical_nonce,
                observed_canonical_nonce,
                provider_ids: &provider_ids,
                operator_ids: &operator_ids,
                failure_domain_ids: &failure_domain_ids,
                decisions: &decisions,
                migration: None,
            })
            .await
    }

    async fn initialize_test_verification_migration(
        client: &BlockchainExecutionClient,
        finalized_headers: &[ExecutionVerifiedHeader],
        next_canonical_nonce: u64,
        decisions: &[ExecutionVerificationDecision],
        migration: &ExecutionVerificationMigration,
    ) {
        let verification = client.config.verification.as_ref().unwrap();
        let provider_ids = vec![
            "authoritative".to_string(),
            "verifier-a".to_string(),
            "verifier-b".to_string(),
        ];
        let operator_ids = vec![
            "operator-a".to_string(),
            "operator-b".to_string(),
            "operator-c".to_string(),
        ];
        let failure_domain_ids = vec![
            "domain-a".to_string(),
            "domain-b".to_string(),
            "domain-c".to_string(),
        ];
        client
            .cache
            .database
            .as_ref()
            .unwrap()
            .ensure_execution_verification_schema(&ExecutionVerificationBootstrap {
                chain_id: 42_161,
                wallet_address: WALLET,
                manifest_version: &verification.manifest_version,
                manifest_digest: &verification.manifest_digest,
                checkpoint_number: FIXTURE_BLOCK,
                checkpoint_hash: FIXTURE_BLOCK_HASH,
                checkpoint_parent_hash:
                    "0x0000000000000000000000000000000000000000000000000000000000000001",
                checkpoint_timestamp: FIXTURE_BLOCK_TIMESTAMP,
                checkpoint_base_fee_per_gas: Some(100_000_000),
                finalized_headers,
                next_canonical_nonce,
                observed_canonical_nonce: next_canonical_nonce,
                provider_ids: &provider_ids,
                operator_ids: &operator_ids,
                failure_domain_ids: &failure_domain_ids,
                decisions,
                migration: Some(migration),
            })
            .await
            .unwrap();
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

    fn recovering_in_flight(client: &BlockchainExecutionClient) -> RecoveryTransaction {
        let slot = *client.in_flight.lock().unwrap();
        let Some(InFlightSlot::Recovering(recovery)) = slot else {
            panic!("expected a recovery transaction, was {slot:?}");
        };
        recovery
    }

    async fn execution_intent_markers(
        admin_pool: &sqlx::PgPool,
        schema: &str,
    ) -> Vec<(String, String, bool, bool)> {
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT purpose, status, terminal_emitted, active \
             FROM {schema}.execution_intent ORDER BY id"
        )))
        .fetch_all(admin_pool)
        .await
        .unwrap()
    }

    #[allow(unsafe_code)] // env-var mutation in tests; unique names avoid cross-test races
    fn payload_test_keys(
        active: [u8; 32],
        retired: Vec<[u8; 32]>,
        deployment_id: &str,
    ) -> PayloadKeySet {
        static NEXT_ENV_ID: AtomicU64 = AtomicU64::new(0);

        let env_id = NEXT_ENV_ID.fetch_add(1, Ordering::Relaxed);
        let active_env = format!("BLOCKCHAIN_TEST_PAYLOAD_KEY_{env_id}_ACTIVE");
        let retired_envs = retired
            .iter()
            .enumerate()
            .map(|(index, _)| format!("BLOCKCHAIN_TEST_PAYLOAD_KEY_{env_id}_RETIRED_{index}"))
            .collect::<Vec<_>>();
        // SAFETY: each invocation uses unique variable names and removes them before returning
        unsafe { std::env::set_var(&active_env, hex::encode(active)) };
        for (env, key) in retired_envs.iter().zip(&retired) {
            // SAFETY: the retired variable name is unique to this invocation
            unsafe { std::env::set_var(env, hex::encode(key)) };
        }

        let keys = PayloadKeySet::load(Some(&active_env), &retired_envs, Some(deployment_id))
            .unwrap()
            .unwrap();

        // SAFETY: no other test or thread uses these unique variable names
        unsafe { std::env::remove_var(active_env) };
        for env in retired_envs {
            // SAFETY: the retired variable name is unique to this invocation
            unsafe { std::env::remove_var(env) };
        }
        keys
    }

    #[allow(unsafe_code)] // env-var mutation in tests; unique names avoid cross-test races
    fn set_test_payload_key(
        client: &mut BlockchainExecutionClient,
        key: [u8; 32],
        deployment_id: &str,
    ) -> PayloadKeySet {
        static NEXT_ENV_ID: AtomicU64 = AtomicU64::new(0);

        let env_id = NEXT_ENV_ID.fetch_add(1, Ordering::Relaxed);
        let active_env = format!("BLOCKCHAIN_TEST_EXECUTION_KEY_{env_id}");
        // SAFETY: each invocation uses a unique variable name retained for reconnect tests
        unsafe { std::env::set_var(&active_env, hex::encode(key)) };
        client.config.payload_key_env = Some(active_env);
        client.config.payload_deployment_id = Some(deployment_id.to_string());
        client.load_payload_keys().unwrap().unwrap()
    }

    async fn protect_test_storage(client: &mut BlockchainExecutionClient, deployment_id: &str) {
        let keys = set_test_payload_key(client, [0xa5; 32], deployment_id);
        client
            .cache
            .database
            .as_ref()
            .unwrap()
            .ensure_execution_payload_storage(&keys)
            .await
            .unwrap();
        client.payload_keys = Some(Arc::new(keys));
    }

    async fn reserve_test_wrap_intent(database: &BlockchainCacheDatabase) -> ExecutionIntentRow {
        reserve_test_wrap_intent_for_wallet(database, WALLET).await
    }

    async fn reserve_test_wrap_intent_for_wallet(
        database: &BlockchainCacheDatabase,
        wallet: &str,
    ) -> ExecutionIntentRow {
        database
            .reserve_execution_intent(&ExecutionIntentInsert {
                chain_id: 42161,
                wallet_address: wallet.to_string(),
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
            .unwrap()
    }

    async fn persist_test_wrap_broadcast(
        database: &BlockchainCacheDatabase,
        keys: Option<&PayloadKeySet>,
    ) -> (ExecutionIntentRow, B256, Vec<u8>) {
        let intent = reserve_test_wrap_intent(database).await;
        database
            .assign_execution_intent_nonce(intent.id, 7)
            .await
            .unwrap();
        let intent = database.get_execution_intent(intent.id).await.unwrap();
        let transaction = build_eip1559_transaction(
            42161,
            7,
            78_000,
            130_000_000,
            10_000_000,
            WETH_ADDRESS,
            U256::from(1u64),
            Bytes::from(nautilus_core::hex::decode("d0e30db0").unwrap()),
        );
        let (tx_hash, raw_tx) = sign_eip1559_transaction(
            transaction,
            &PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        )
        .await
        .unwrap();
        persist_test_payload(database, keys, &intent, tx_hash, &raw_tx).await;
        database
            .record_execution_status(
                intent.id,
                &tx_hash.to_string(),
                TransactionStatus::Broadcast,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        (intent, tx_hash, raw_tx)
    }

    async fn reserve_test_swap_intent(database: &BlockchainCacheDatabase) -> ExecutionIntentRow {
        let pool = test_pool();
        let order = test_market_sell_order(pool.instrument_id);
        let calldata = expected_swap_calldata(expected_min_amount_out(50));

        database
            .reserve_execution_intent(&ExecutionIntentInsert {
                chain_id: 42161,
                wallet_address: WALLET.to_string(),
                purpose: "swap".to_string(),
                client_order_id: Some(order.client_order_id().to_string()),
                trader_id: Some(order.trader_id().to_string()),
                strategy_id: Some(order.strategy_id().to_string()),
                account_id: Some("BLOCKCHAIN-001".to_string()),
                instrument_id: Some(pool.instrument_id.to_string()),
                pool_address: Some(pool.address.to_string()),
                transaction_to: ROUTER_ADDRESS.to_string(),
                transaction_input: hex::encode_prefixed(&calldata),
                transaction_value: U256::ZERO.to_string(),
                amount_in: Some("1000000000000000".to_string()),
                created_block: FIXTURE_BLOCK,
            })
            .await
            .unwrap()
    }

    async fn persist_invalid_test_swap(
        database: &BlockchainCacheDatabase,
        keys: Option<&PayloadKeySet>,
    ) -> (ExecutionIntentRow, B256) {
        let intent = reserve_test_swap_intent(database).await;
        database
            .assign_execution_intent_nonce(intent.id, 7)
            .await
            .unwrap();
        let intent = database.get_execution_intent(intent.id).await.unwrap();
        let (tx_hash, raw_transaction) = expected_swap_tx(expected_min_amount_out(50)).await;
        let mut raw_transaction = hex::decode(raw_transaction.strip_prefix("0x").unwrap()).unwrap();
        raw_transaction.push(0xff);
        persist_test_payload(database, keys, &intent, tx_hash, &raw_transaction).await;
        database
            .record_execution_status(
                intent.id,
                &tx_hash.to_string(),
                TransactionStatus::Broadcast,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        (intent, tx_hash)
    }

    async fn persist_test_swap_broadcast(
        database: &BlockchainCacheDatabase,
        keys: Option<&PayloadKeySet>,
    ) -> (ExecutionIntentRow, B256, Vec<u8>) {
        let intent = reserve_test_swap_intent(database).await;
        database
            .assign_execution_intent_nonce(intent.id, 7)
            .await
            .unwrap();
        let intent = database.get_execution_intent(intent.id).await.unwrap();
        let (tx_hash, raw_transaction) = expected_swap_tx(expected_min_amount_out(50)).await;
        let raw_transaction = hex::decode(raw_transaction.strip_prefix("0x").unwrap()).unwrap();
        persist_test_payload(database, keys, &intent, tx_hash, &raw_transaction).await;
        database
            .record_execution_status(
                intent.id,
                &tx_hash.to_string(),
                TransactionStatus::Broadcast,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        (intent, tx_hash, raw_transaction)
    }

    async fn persist_test_payload(
        database: &BlockchainCacheDatabase,
        keys: Option<&PayloadKeySet>,
        intent: &ExecutionIntentRow,
        tx_hash: B256,
        raw_transaction: &[u8],
    ) {
        if let Some(keys) = keys {
            database.reserve_execution_payload_seal(keys).await.unwrap();
            let transaction_hash = tx_hash.to_string();
            let context =
                payload_context_identity(intent, &transaction_hash, 42_161, keys.deployment_id())
                    .unwrap();
            let envelope = keys.seal(raw_transaction, &context).unwrap();
            database
                .add_execution_transaction_envelope(intent.id, 42_161, &transaction_hash, &envelope)
                .await
                .unwrap();
        } else {
            database
                .add_execution_transaction_hash(
                    intent.id,
                    42_161,
                    &tx_hash.to_string(),
                    raw_transaction,
                )
                .await
                .unwrap();
        }
    }

    async fn later_reconnect(
        previous: BlockchainExecutionClient,
        http_rpc_url: String,
    ) -> anyhow::Error {
        let database = previous.cache.database.as_ref().unwrap().clone();
        let payload_keys = previous.payload_keys.clone();
        drop(previous);
        let mut next = test_client(http_rpc_url);
        next.cache.database = Some(database);
        next.payload_keys = payload_keys;
        next.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
        next.reconcile_unresolved_execution().await.unwrap_err()
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
        tokio::time::timeout(TEST_TIMEOUT, async {
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

    fn assert_swap_quarantined_without_terminal_event(events: &[OrderEventAny]) {
        assert_eq!(events.len(), 1, "was: {events:?}");
        assert!(matches!(&events[0], OrderEventAny::Submitted(_)));
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

    fn expected_sell_quote_amount() -> U256 {
        let profiler = test_profiler(&test_pool(), FIXTURE_BLOCK);
        let quote = profiler
            .swap_exact_in(U256::from(1_000_000_000_000_000u64), true, None)
            .unwrap();
        exact_output_amount(&quote, true).unwrap()
    }

    fn quote_response(amount: U256) -> String {
        let result = (amount, U160::from(1u128 << 96), 0u32, U256::from(50_000u64)).abi_encode();
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": hex::encode_prefixed(result),
        })
        .to_string()
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

    fn expected_buy_base_amount() -> U256 {
        U256::from(1_000_000_000_000_000u64)
    }

    fn expected_buy_amount_in() -> U256 {
        let profiler = test_profiler(&test_pool(), FIXTURE_BLOCK);
        profiler
            .swap_exact_out(expected_buy_base_amount(), false, None)
            .unwrap()
            .get_input_amount()
    }

    fn expected_buy_min_amount_out(slippage_bps: u32) -> U256 {
        derive_min_amount_out(expected_buy_base_amount(), slippage_bps).unwrap()
    }

    fn expected_buy_swap_calldata(min_amount_out: U256, amount_in: U256) -> Vec<u8> {
        UniswapV3SwapRouter::exactInputSingleCall {
            params: UniswapV3SwapRouter::ExactInputSingleParams {
                tokenIn: USDC_ADDRESS,
                tokenOut: WETH_ADDRESS,
                fee: U24::try_from(500u32).unwrap(),
                recipient: address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
                deadline: U256::from(FIXTURE_BLOCK_TIMESTAMP + 300),
                amountIn: amount_in,
                amountOutMinimum: min_amount_out,
                sqrtPriceLimitX96: U160::ZERO,
            },
        }
        .abi_encode()
    }

    async fn expected_buy_swap_tx(min_amount_out: U256, amount_in: U256) -> (B256, String) {
        let expected_tx = build_eip1559_transaction(
            42161,
            7,
            78_000,
            130_000_000,
            10_000_000,
            ROUTER_ADDRESS,
            U256::ZERO,
            Bytes::from(expected_buy_swap_calldata(min_amount_out, amount_in)),
        );
        sign_eip1559_transaction(
            expected_tx,
            &PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        )
        .await
        .map(|(hash, raw)| (hash, nautilus_core::hex::encode_prefixed(&raw)))
        .unwrap()
    }

    fn finalized_buy_swap_receipt(tx_hash: B256, amount_in: U256) -> String {
        finalized_buy_swap_receipt_with_base_out(tx_hash, amount_in, expected_buy_base_amount())
    }

    fn finalized_buy_swap_receipt_with_base_out(
        tx_hash: B256,
        amount_in: U256,
        base_out: U256,
    ) -> String {
        let data = (
            -I256::try_from(u128::try_from(base_out).unwrap()).unwrap(),
            I256::try_from(u128::try_from(amount_in).unwrap()).unwrap(),
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

    fn finalized_buy_swap_block(tx_hash: B256, min_amount_out: U256, amount_in: U256) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d41",
                "hash": "0x2222222222222222222222222222222222222222222222222222222222222222",
                "parentHash": FIXTURE_BLOCK_HASH,
                "timestamp": "0x69044a21",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": tx_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "chainId": "0xa4b1",
                    "type": "0x2",
                    "to": ROUTER,
                    "input": hex::encode_prefixed(expected_buy_swap_calldata(min_amount_out, amount_in)),
                    "value": "0x0",
                    "gas": "0x130b0",
                    "maxFeePerGas": "0x7bfa480",
                    "maxPriorityFeePerGas": "0x989680"
                }]
            }
        })
        .to_string()
    }

    fn finalized_buy_swap_rpc_state(
        tx_hash: B256,
        min_amount_out: U256,
        amount_in: U256,
    ) -> MockRpcState {
        let receipt = finalized_buy_swap_receipt(tx_hash, amount_in);
        let block = finalized_buy_swap_block(tx_hash, min_amount_out, amount_in);
        with_finalized_identity(
            signing_rpc_state()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
                .with_send_raw_transaction_echo()
                .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE)
                .with_call_response(ALLOWANCE_SELECTOR, CALL_ALLOWANCE),
            &block,
            &receipt,
        )
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

    fn finalized_swap_receipt_with_unrelated_swap(tx_hash: B256) -> String {
        let mut receipt: serde_json::Value =
            serde_json::from_str(&finalized_swap_receipt(tx_hash)).unwrap();
        let logs = receipt["result"]["logs"].as_array_mut().unwrap();
        let mut unrelated = logs[0].clone();
        unrelated["address"] = serde_json::json!(ROUTER);
        unrelated["logIndex"] = serde_json::json!("0x7");
        logs.push(unrelated);
        receipt.to_string()
    }

    fn finalized_swap_block(tx_hash: B256, min_amount_out: U256) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d41",
                "hash": "0x2222222222222222222222222222222222222222222222222222222222222222",
                "parentHash": FIXTURE_BLOCK_HASH,
                "timestamp": "0x69044a21",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": tx_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "chainId": "0xa4b1",
                    "type": "0x2",
                    "to": ROUTER,
                    "input": hex::encode_prefixed(expected_swap_calldata(min_amount_out)),
                    "value": "0x0",
                    "gas": "0x130b0",
                    "maxFeePerGas": "0x7bfa480",
                    "maxPriorityFeePerGas": "0x989680"
                }]
            }
        })
        .to_string()
    }

    fn finalized_swap_rpc_state(tx_hash: B256, min_amount_out: U256) -> MockRpcState {
        let receipt = finalized_swap_receipt(tx_hash);
        let block = finalized_swap_block(tx_hash, min_amount_out);
        with_finalized_identity(
            signing_rpc_state()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
                .with_send_raw_transaction_echo()
                .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE)
                .with_call_response(ALLOWANCE_SELECTOR, CALL_ALLOWANCE),
            &block,
            &receipt,
        )
    }

    fn with_finalized_identity(
        state: MockRpcState,
        block_response: &str,
        receipt_response: &str,
    ) -> MockRpcState {
        let block: serde_json::Value = serde_json::from_str(block_response).unwrap();
        let receipt: serde_json::Value = serde_json::from_str(receipt_response).unwrap();
        let transaction = block["result"]["transactions"][0].clone();
        let transaction_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": transaction,
        })
        .to_string();
        let success = receipt["result"]["status"] == "0x1";
        let mut trace = serde_json::json!({
            "type": "CALL",
            "from": transaction["from"],
            "to": transaction["to"],
            "value": transaction["value"],
            "gas": transaction["gas"],
            "gasUsed": receipt["result"]["gasUsed"],
            "input": transaction["input"],
            "output": "0x",
            "calls": [],
        });

        if !success {
            trace["error"] = serde_json::json!("execution reverted");
        }
        let trace_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": trace,
        })
        .to_string();
        state
            .with_response("eth_getTransactionByHash", &transaction_response)
            .with_response("debug_traceTransaction", &trace_response)
    }

    fn receipt_with_transaction_hash(receipt_response: &str, tx_hash: B256) -> String {
        let mut receipt: serde_json::Value = serde_json::from_str(receipt_response).unwrap();
        receipt["result"]["transactionHash"] = serde_json::json!(tx_hash.to_string());
        receipt.to_string()
    }

    fn replacement_head_block(tx_hash: B256) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d40",
                "hash": "0x1111111111111111111111111111111111111111111111111111111111111111",
                "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000001",
                "timestamp": "0x69044a20",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": "0x3333333333333333333333333333333333333333333333333333333333333333",
                    "from": "0x0000000000000000000000000000000000000001",
                    "nonce": "0x1",
                    "type": "0x0",
                    "to": WETH,
                    "input": "0x",
                    "value": "0x0",
                    "gas": "0x5208"
                }, {
                    "hash": tx_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "chainId": "0xa4b1",
                    "type": "0x2",
                    "to": WETH,
                    "input": "0xd0e30db0",
                    "value": "0x38d7ea4c68000",
                    "gas": "0x130b0",
                    "maxFeePerGas": "0x7bfa480",
                    "maxPriorityFeePerGas": "0x989680"
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
                "parentHash": FIXTURE_BLOCK_HASH,
                "timestamp": "0x69044a21",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": tx_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "chainId": "0xa4b1",
                    "type": "0x2",
                    "to": WETH,
                    "input": "0xd0e30db0",
                    "value": "0x38d7ea4c68000",
                    "gas": "0x130b0",
                    "maxFeePerGas": "0x7bfa480",
                    "maxPriorityFeePerGas": "0x989680"
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
                "parentHash": FIXTURE_BLOCK_HASH,
                "timestamp": "0x69044a21",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": tx_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "chainId": "0xa4b1",
                    "type": "0x2",
                    "to": WETH,
                    "input": hex::encode_prefixed(calldata),
                    "value": "0x0",
                    "gas": "0x130b0",
                    "maxFeePerGas": "0x7bfa480",
                    "maxPriorityFeePerGas": "0x989680"
                }]
            }
        })
        .to_string()
    }

    fn fixture_sell_plan() -> SwapPlan {
        let pool = test_pool();
        let order = test_market_sell_order(pool.instrument_id);
        let quote_token = pool.get_quote_token();
        SwapPlan {
            order,
            quote_currency: Currency::new_checked(
                &quote_token.symbol,
                quote_token.decimals,
                0,
                &quote_token.name,
                CurrencyType::Crypto,
            )
            .unwrap(),
            instrument_id: pool.instrument_id,
            pool_address: pool.address,
            router: ROUTER_ADDRESS,
            factory: UNISWAP_V3.dex.factory,
            weth: WETH_ADDRESS,
            token_in: WETH_ADDRESS,
            token_out: USDC_ADDRESS,
            fee: U24::try_from(500u32).unwrap(),
            amount_in: U256::from(1_000_000_000_000_000u64),
            min_amount_out: expected_min_amount_out(50),
            slippage_bps: 50,
            quote_spend_ceiling: None,
            profiler_position: Some(profiler_event_position()),
            pool,
        }
    }

    fn fixture_block_response(number: u64, hash: B256) -> String {
        let parent_hash = if number == FIXTURE_BLOCK {
            B256::from_str("0x0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap()
        } else {
            B256::from_str(FIXTURE_BLOCK_HASH).unwrap()
        };
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": format!("0x{number:x}"),
                "hash": hash.to_string(),
                "parentHash": parent_hash.to_string(),
                "timestamp": format!("0x{:x}", FIXTURE_BLOCK_TIMESTAMP + number - FIXTURE_BLOCK),
                "baseFeePerGas": "0x5f5e100",
                "transactions": []
            }
        })
        .to_string()
    }

    fn profiler_event_receipt(pool_address: Address) -> String {
        let transaction_hash = B256::from([0x33; 32]);
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "transactionHash": transaction_hash.to_string(),
                "blockHash": FIXTURE_BLOCK_HASH,
                "blockNumber": "0x1cf0d40",
                "transactionIndex": "0x2",
                "gasUsed": "0xc3c0",
                "effectiveGasPrice": "0x5f5e100",
                "status": "0x1",
                "logs": [{
                    "removed": false,
                    "logIndex": "0x6",
                    "transactionIndex": "0x2",
                    "transactionHash": transaction_hash.to_string(),
                    "blockHash": FIXTURE_BLOCK_HASH,
                    "blockNumber": "0x1cf0d40",
                    "address": pool_address.to_string(),
                    "data": "0x",
                    "topics": [test_pool().dex.swap_created_event.as_ref()]
                }]
            }
        })
        .to_string()
    }

    fn profiler_event_position() -> BlockPosition {
        BlockPosition::new(FIXTURE_BLOCK, B256::from([0x33; 32]).to_string(), 2, 6)
            .with_block_hash(Some(FIXTURE_BLOCK_HASH.to_string()))
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
    fn raw_amount_to_quantity_inverts_base_quantity() {
        let quantity = Quantity::from("0.001");
        let amount = quantity_to_raw_amount(quantity, 18).unwrap();

        assert_eq!(raw_amount_to_quantity(amount, 18).unwrap(), quantity);
    }

    #[rstest]
    fn raw_amount_to_quantity_rejects_positive_amount_truncated_to_zero() {
        let error = raw_amount_to_quantity(U256::from(99u64), 18).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("is below representable quantity precision"),
            "was: {error}"
        );
    }

    #[rstest]
    fn fill_price_from_quote_recovers_spent_quote_after_quantity_truncation() {
        let last_qty = raw_amount_to_quantity(U256::from(1_000_000_403_079_044u64), 18).unwrap();
        let quote_amount = U256::from(1_891_348u64);
        let quote = Currency::new_checked("USDC", 6, 0, "USD Coin", CurrencyType::Crypto).unwrap();
        let last_px = fill_price_from_quote(last_qty, quote_amount, quote).unwrap();

        assert_eq!(
            Money::from_decimal(last_qty.as_decimal() * last_px.as_decimal(), quote).unwrap(),
            Money::from_u256(quote_amount, quote).unwrap()
        );
    }

    #[rstest]
    fn swap_token_pair_is_directional() {
        assert_eq!(
            swap_token_pair(OrderSide::Sell, WETH_ADDRESS, USDC_ADDRESS).unwrap(),
            (WETH_ADDRESS, USDC_ADDRESS)
        );
        assert_eq!(
            swap_token_pair(OrderSide::Buy, WETH_ADDRESS, USDC_ADDRESS).unwrap(),
            (USDC_ADDRESS, WETH_ADDRESS)
        );
    }

    #[rstest]
    fn restore_swap_plan_buy_uses_quote_input() {
        let (client, cache) =
            swap_client_with_cache(buy_test_config("http://127.0.0.1:1".to_string()));
        let order = test_market_buy_order(test_pool().instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
        let amount_in = U256::from(2_345_678u64);
        let intent = ExecutionIntentRow {
            id: 7,
            schema_version: crate::execution::transaction::EXECUTION_SCHEMA_VERSION,
            chain_id: 42161,
            wallet_address: WALLET.to_string(),
            nonce: Some(7),
            purpose: "swap".to_string(),
            status: "finalized".to_string(),
            client_order_id: Some(order.client_order_id().to_string()),
            trader_id: Some(order.trader_id().to_string()),
            strategy_id: Some(order.strategy_id().to_string()),
            account_id: Some("BLOCKCHAIN-001".to_string()),
            instrument_id: Some(order.instrument_id().to_string()),
            pool_address: Some(test_pool().address.to_string()),
            transaction_to: ROUTER.to_string(),
            transaction_input: "0x".to_string(),
            transaction_value: "0".to_string(),
            amount_in: Some(amount_in.to_string()),
            created_block: FIXTURE_BLOCK,
            acknowledgement_emitted: true,
            fill_emitted: false,
            terminal_emitted: false,
            active: true,
        };

        let plan = client.restore_swap_plan(&intent).unwrap();

        assert_eq!(plan.token_in, USDC_ADDRESS);
        assert_eq!(plan.token_out, WETH_ADDRESS);
        assert_eq!(plan.amount_in, amount_in);
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
    fn replacement_scan_range_is_bounded_and_checked() {
        assert_eq!(
            replacement_scan_range(10, 10).unwrap(),
            RangeInclusive::new(10, 10)
        );
        assert_eq!(
            replacement_scan_range(10, 10 + MAX_REPLACEMENT_SCAN_BLOCKS).unwrap(),
            RangeInclusive::new(10, 10 + MAX_REPLACEMENT_SCAN_BLOCKS - 1)
        );
        assert!(
            replacement_scan_range(11, 10)
                .unwrap_err()
                .to_string()
                .contains("is behind execution creation block")
        );
        assert_eq!(
            replacement_scan_range(u64::MAX - 1, u64::MAX).unwrap(),
            RangeInclusive::new(u64::MAX - 1, u64::MAX)
        );
    }

    #[rstest]
    fn terminal_execution_event_ids_are_stable_and_kind_specific() {
        let transaction_hash = B256::from([0x42; 32]);

        let fill = execution_event_id(transaction_hash, b"fill");
        let fill_retry = execution_event_id(transaction_hash, b"fill");
        let reverted = execution_event_id(transaction_hash, b"reverted");

        assert_eq!(fill, fill_retry);
        assert_ne!(fill, reverted);
    }

    #[rstest]
    fn prepared_transaction_debug_redacts_raw_transaction() {
        let prepared = PreparedTransaction {
            intent_id: 1,
            created_block: 2,
            nonce: 3,
            tx_hash: B256::ZERO,
            raw_tx: vec![0xde, 0xad, 0xbe, 0xef],
            payload_lease: None,
        };

        let debug = format!("{prepared:?}");

        assert!(debug.contains("raw_tx: \"<redacted>\""));
        assert!(!debug.contains("[222, 173, 190, 239]"));
    }

    #[rstest]
    fn call_trace_rejects_unreviewed_internal_edge() {
        let signed = crate::execution::transaction::DecodedSignedTransaction {
            hash: B256::from([1; 32]),
            signer: Address::from_str(WALLET).unwrap(),
            chain_id: 42_161,
            nonce: 7,
            to: ROUTER_ADDRESS,
            value: U256::ZERO,
            input: Bytes::from(expected_swap_calldata(expected_min_amount_out(50))),
            gas_limit: 78_000,
            max_fee_per_gas: 130_000_000,
            max_priority_fee_per_gas: 10_000_000,
        };
        let input_digest = keccak256(&signed.input);
        let trace = VerifiedCallTrace {
            call_type: RpcCallType::Call,
            from: signed.signer,
            to: Some(signed.to),
            value: signed.value,
            input_selector: signed
                .input
                .get(..4)
                .map(|selector| selector.try_into().unwrap()),
            input_digest,
            success: true,
            calls: vec![VerifiedCallTrace {
                call_type: RpcCallType::Call,
                from: ROUTER_ADDRESS,
                to: Some(WETH_ADDRESS),
                value: U256::ZERO,
                input_selector: None,
                input_digest: B256::ZERO,
                success: true,
                calls: Vec::new(),
            }],
        };
        let manifest = &test_config("http://127.0.0.1:1".to_string())
            .verification
            .unwrap()
            .deployment_manifest;

        let error = validate_call_trace(&trace, &signed, true, "swap_sell", manifest).unwrap_err();

        assert!(
            error.to_string().contains("unreviewed call edge"),
            "was: {error}"
        );
    }

    #[rstest]
    fn call_trace_rejects_unlisted_precompile_target() {
        let manifest = test_config("http://127.0.0.1:1".to_string())
            .verification
            .unwrap()
            .deployment_manifest;
        let calls = [VerifiedCallTrace {
            call_type: RpcCallType::Call,
            from: ROUTER_ADDRESS,
            to: Some(address!("0000000000000000000000000000000000000007")),
            value: U256::ZERO,
            input_selector: None,
            input_digest: B256::ZERO,
            success: true,
            calls: Vec::new(),
        }];

        let error =
            validate_internal_calls(&calls, ROUTER_ADDRESS, "swap_sell", &manifest).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "Verified call trace contains an unreviewed call edge {ROUTER_ADDRESS} -> {} for swap_sell",
                address!("0000000000000000000000000000000000000007")
            )
        );
    }

    #[rstest]
    fn call_trace_requires_exact_call_type() {
        let manifest = test_config("http://127.0.0.1:1".to_string())
            .verification
            .unwrap()
            .deployment_manifest;
        let calls = [VerifiedCallTrace {
            call_type: RpcCallType::Staticcall,
            from: ROUTER_ADDRESS,
            to: Some(test_pool().address),
            value: U256::ZERO,
            input_selector: None,
            input_digest: B256::ZERO,
            success: true,
            calls: Vec::new(),
        }];

        let error =
            validate_internal_calls(&calls, ROUTER_ADDRESS, "swap_sell", &manifest).unwrap_err();

        assert!(
            error.to_string().contains("unreviewed staticcall edge"),
            "was: {error}"
        );
    }

    #[rstest]
    fn call_trace_requires_exact_caller() {
        let manifest = test_config("http://127.0.0.1:1".to_string())
            .verification
            .unwrap()
            .deployment_manifest;
        let calls = [VerifiedCallTrace {
            call_type: RpcCallType::Call,
            from: WETH_ADDRESS,
            to: Some(test_pool().address),
            value: U256::ZERO,
            input_selector: None,
            input_digest: B256::ZERO,
            success: true,
            calls: Vec::new(),
        }];

        let error =
            validate_internal_calls(&calls, ROUTER_ADDRESS, "swap_sell", &manifest).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Verified call trace child has an invalid caller context"
        );
    }

    #[rstest]
    fn call_trace_rejects_contract_creation() {
        let signed = crate::execution::transaction::DecodedSignedTransaction {
            hash: B256::from([1; 32]),
            signer: Address::from_str(WALLET).unwrap(),
            chain_id: 42_161,
            nonce: 7,
            to: ROUTER_ADDRESS,
            value: U256::ZERO,
            input: Bytes::from(expected_swap_calldata(expected_min_amount_out(50))),
            gas_limit: 78_000,
            max_fee_per_gas: 130_000_000,
            max_priority_fee_per_gas: 10_000_000,
        };
        let trace = VerifiedCallTrace {
            call_type: RpcCallType::Call,
            from: signed.signer,
            to: Some(signed.to),
            value: signed.value,
            input_selector: signed
                .input
                .get(..4)
                .map(|selector| selector.try_into().unwrap()),
            input_digest: keccak256(&signed.input),
            success: true,
            calls: vec![VerifiedCallTrace {
                call_type: RpcCallType::Create,
                from: ROUTER_ADDRESS,
                to: Some(address!("0000000000000000000000000000000000000007")),
                value: U256::ZERO,
                input_selector: None,
                input_digest: B256::ZERO,
                success: true,
                calls: Vec::new(),
            }],
        };
        let manifest = &test_config("http://127.0.0.1:1".to_string())
            .verification
            .unwrap()
            .deployment_manifest;

        let error = validate_call_trace(&trace, &signed, true, "swap_sell", manifest).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("forbidden state-changing operation"),
            "was: {error}"
        );
    }

    #[tokio::test]
    async fn live_arbitrum_numbered_swap_reads_are_available() {
        if std::env::var(LIVE_READ_SMOKE_ENV).as_deref() != Ok("1") {
            eprintln!("{LIVE_READ_SMOKE_ENV} is not 1; skipping live read smoke");
            return;
        }

        let rpc_url = std::env::var("ARBITRUM_RPC_HTTP_URL")
            .unwrap_or_else(|_| LIVE_READ_SMOKE_RPC.to_string());
        let rpc = Arc::new(BlockchainHttpRpcClient::new(rpc_url, None, None));
        let anchor = rpc.latest_block().await.unwrap();
        let pool = test_pool();
        let factory = UNISWAP_V3.dex.factory;
        let wallet = Address::from_str(WALLET).unwrap();
        let mut balance_call =
            nautilus_core::hex::decode(BALANCE_OF_SELECTOR.trim_start_matches("0x")).unwrap();
        balance_call.extend_from_slice(&[0; 12]);
        balance_call.extend_from_slice(wallet.as_slice());

        let code = rpc
            .get_code_at(&ROUTER_ADDRESS, anchor.number)
            .await
            .unwrap();
        let router_factory = rpc
            .call_at(
                None,
                &ROUTER_ADDRESS,
                U256::ZERO,
                &UniswapV3RouterState::factoryCall {}.abi_encode(),
                anchor.number,
            )
            .await
            .unwrap();
        let router_factory =
            UniswapV3RouterState::factoryCall::abi_decode_returns(&router_factory).unwrap();
        let router_weth = rpc
            .call_at(
                None,
                &ROUTER_ADDRESS,
                U256::ZERO,
                &UniswapV3RouterState::WETH9Call {}.abi_encode(),
                anchor.number,
            )
            .await
            .unwrap();
        let router_weth =
            UniswapV3RouterState::WETH9Call::abi_decode_returns(&router_weth).unwrap();
        let registered_pool_call = UniswapV3Factory::getPoolCall {
            tokenA: WETH_ADDRESS,
            tokenB: USDC_ADDRESS,
            fee: U24::try_from(500u32).unwrap(),
        }
        .abi_encode();
        let registered_pool = rpc
            .call_at(
                None,
                &factory,
                U256::ZERO,
                &registered_pool_call,
                anchor.number,
            )
            .await
            .unwrap();
        let registered_pool =
            UniswapV3Factory::getPoolCall::abi_decode_returns(&registered_pool).unwrap();
        let weth_decimals = rpc
            .call_at(
                None,
                &WETH_ADDRESS,
                U256::ZERO,
                &ERC20::decimalsCall {}.abi_encode(),
                anchor.number,
            )
            .await
            .unwrap();
        let weth_decimals = ERC20::decimalsCall::abi_decode_returns(&weth_decimals).unwrap();
        let usdc_decimals = rpc
            .call_at(
                None,
                &USDC_ADDRESS,
                U256::ZERO,
                &ERC20::decimalsCall {}.abi_encode(),
                anchor.number,
            )
            .await
            .unwrap();
        let usdc_decimals = ERC20::decimalsCall::abi_decode_returns(&usdc_decimals).unwrap();
        let allowance_call = ERC20::allowanceCall {
            owner: wallet,
            spender: ROUTER_ADDRESS,
        }
        .abi_encode();
        rpc.call_at(
            None,
            &WETH_ADDRESS,
            U256::ZERO,
            &allowance_call,
            anchor.number,
        )
        .await
        .unwrap();
        let balance_call = ERC20::balanceOfCall { account: wallet }.abi_encode();
        rpc.call_at(
            None,
            &WETH_ADDRESS,
            U256::ZERO,
            &balance_call,
            anchor.number,
        )
        .await
        .unwrap();
        let gas = rpc
            .estimate_gas_at(
                &wallet,
                &WETH_ADDRESS,
                U256::ZERO,
                &balance_call,
                anchor.number,
            )
            .await
            .unwrap();
        rpc.get_balance_with_timeout(
            &wallet,
            Some(anchor.number),
            Some(EXECUTION_RPC_TIMEOUT_SECS),
        )
        .await
        .unwrap();
        let canonical = rpc.block_by_number(anchor.number, false).await.unwrap();

        assert!(!code.is_empty());
        assert_eq!(router_factory, factory);
        assert_eq!(router_weth, WETH_ADDRESS);
        assert_eq!(registered_pool, pool.address);
        assert_eq!(weth_decimals, 18);
        assert_eq!(usdc_decimals, 6);
        assert!(gas > 0);
        assert_eq!(canonical.hash, anchor.hash);
    }

    #[tokio::test]
    async fn swap_quote_rejects_missing_ingestion_block_hash() {
        let (client, state) = client_with_mock_rpc(execution_rpc_state()).await;
        let plan = fixture_sell_plan();
        let position = BlockPosition::new(
            FIXTURE_BLOCK,
            FIXTURE_BLOCK_HASH.to_string(),
            BLOCK_SCOPED_SNAPSHOT_INDEX,
            BLOCK_SCOPED_SNAPSHOT_INDEX,
        );

        let error = validate_swap_quote(
            &position,
            &plan,
            100,
            &client.verification,
            &client
                .config
                .verification
                .as_ref()
                .unwrap()
                .deployment_manifest,
        )
        .await
        .unwrap_err();

        assert!(
            error.to_string().contains("no ingestion-time block hash"),
            "was: {error}"
        );
        assert!(state.recorded_requests().is_empty());
    }

    #[tokio::test]
    async fn swap_quote_rejects_replaced_ingestion_block() {
        let changed = fixture_block_response(FIXTURE_BLOCK, B256::from([0x44; 32]));
        let state = execution_rpc_state().with_parameter_response(
            "eth_getBlockByNumber",
            "0x1cf0d40",
            &changed,
        );
        let (client, _) = client_with_mock_rpc(state).await;
        let plan = fixture_sell_plan();
        let position = BlockPosition::new(
            FIXTURE_BLOCK,
            FIXTURE_BLOCK_HASH.to_string(),
            BLOCK_SCOPED_SNAPSHOT_INDEX,
            BLOCK_SCOPED_SNAPSHOT_INDEX,
        )
        .with_block_hash(Some(FIXTURE_BLOCK_HASH.to_string()));

        let error = validate_swap_quote(
            &position,
            &plan,
            100,
            &client.verification,
            &client
                .config
                .verification
                .as_ref()
                .unwrap()
                .deployment_manifest,
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("changed from"), "was: {error}");
    }

    #[tokio::test]
    async fn swap_quote_accepts_exact_canonical_event_watermark() {
        let pool = test_pool();
        let receipt = profiler_event_receipt(pool.address);
        let state = execution_rpc_state().with_response("eth_getTransactionReceipt", &receipt);
        let (client, _) = client_with_mock_rpc(state).await;
        let plan = fixture_sell_plan();

        let anchors = validate_swap_quote(
            &profiler_event_position(),
            &plan,
            100,
            &client.verification,
            &client
                .config
                .verification
                .as_ref()
                .unwrap()
                .deployment_manifest,
        )
        .await
        .unwrap();

        assert_eq!(anchors.watermark.number, profiler_event_position().number);
        assert_eq!(anchors.state.number, FIXTURE_BLOCK);
        assert_eq!(anchors.state.hash, B256::from([0x11; 32]));
        assert_eq!(anchors.state.timestamp, FIXTURE_BLOCK_TIMESTAMP);
    }

    #[tokio::test]
    async fn swap_quote_rejects_mismatched_receipt_position() {
        let pool = test_pool();
        let mut receipt: serde_json::Value =
            serde_json::from_str(&profiler_event_receipt(pool.address)).unwrap();
        receipt["result"]["transactionIndex"] = serde_json::json!("0x3");
        let state =
            execution_rpc_state().with_response("eth_getTransactionReceipt", &receipt.to_string());
        let (client, _) = client_with_mock_rpc(state).await;
        let plan = fixture_sell_plan();

        let error = validate_swap_quote(
            &profiler_event_position(),
            &plan,
            100,
            &client.verification,
            &client
                .config
                .verification
                .as_ref()
                .unwrap()
                .deployment_manifest,
        )
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Profiler receipt position does not match its ingestion watermark"
        );
    }

    #[tokio::test]
    async fn swap_quote_rejects_watermark_from_different_pool() {
        let receipt = profiler_event_receipt(ROUTER_ADDRESS);
        let state = execution_rpc_state().with_response("eth_getTransactionReceipt", &receipt);
        let (client, _) = client_with_mock_rpc(state).await;
        let plan = fixture_sell_plan();

        let error = validate_swap_quote(
            &profiler_event_position(),
            &plan,
            100,
            &client.verification,
            &client
                .config
                .verification
                .as_ref()
                .unwrap()
                .deployment_manifest,
        )
        .await
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("did not come from expected pool"),
            "was: {error}"
        );
    }

    #[rstest]
    fn verified_sell_quote_sets_signed_amounts() {
        let plan = fixture_sell_plan();
        let quote = UniswapV3Quote {
            amount: expected_sell_quote_amount(),
            sqrt_price_x96_after: U160::from(1u128 << 96),
            initialized_ticks_crossed: 0,
            gas_estimate: U256::from(50_000u64),
        };

        let (amount_in, min_amount_out) = verified_swap_amounts(&plan, quote).unwrap();

        assert_eq!(amount_in, U256::from(1_000_000_000_000_000u64));
        assert_eq!(min_amount_out, expected_min_amount_out(50));
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
    async fn submit_order_denies_buy_without_allowlisted_pair() {
        let (mut client, cache) =
            swap_client_with_cache(test_config("http://127.0.0.1:1".to_string()));
        let pool = test_pool();
        let order = test_market_buy_order(pool.instrument_id);
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
            denied
                .reason
                .as_str()
                .contains("not in the `allowed_token_pairs` allowlist"),
            "was: {}",
            denied.reason
        );
        assert!(
            denied.reason.as_str().contains(&USDC_ADDRESS.to_string()),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_buy_quote_denominated_quantity() {
        let (mut client, cache) =
            swap_client_with_cache(buy_test_config("http://127.0.0.1:1".to_string()));
        let pool = test_pool();
        let order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("S-001"))
            .instrument_id(pool.instrument_id)
            .client_order_id(ClientOrderId::from("O-SWAP-BUY-001"))
            .side(OrderSide::Buy)
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
    async fn submit_order_denies_buy_amount_above_max_order_amount() {
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.max_order_amount = Some(999_999_999_999_999);
        let (mut client, cache) = swap_client_with_cache(config);
        let order = test_market_buy_order(test_pool().instrument_id);
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
            denied
                .reason
                .as_str()
                .contains("exceeds the configured `max_order_amount`"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_buy_without_quote_spend_limit() {
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits = None;
        let (mut client, cache) = swap_client_with_cache(config);
        let order = test_market_buy_order(test_pool().instrument_id);
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
            denied
                .reason
                .as_str()
                .contains("No `quote_spend_limits` entry for BUY token pair"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_uses_pair_specific_quote_spend_limit() {
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits = Some(vec![quote_spend_limit(
            WETH,
            USDC,
            18,
            "1000000000000000000",
        )]);
        let (mut client, cache) = swap_client_with_cache(config);
        let order = test_market_buy_order(test_pool().instrument_id);
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
            denied
                .reason
                .as_str()
                .contains("No `quote_spend_limits` entry for BUY token pair"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_buy_with_quote_spend_precision_mismatch() {
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits.as_mut().unwrap()[0].spend_token_decimals = 18;
        let error = test_client_result(config, test_pool()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Quote spend limit decimals do not match the deployment manifest"),
            "was: {error}"
        );
    }

    #[tokio::test]
    async fn submit_order_denies_buy_with_zero_quote_spend_limit() {
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits.as_mut().unwrap()[0].max_amount = "0".to_string();
        let (mut client, cache) = swap_client_with_cache(config);
        let order = test_market_buy_order(test_pool().instrument_id);
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
            denied
                .reason
                .as_str()
                .contains("exceeds the configured `quote_spend_limits` maximum 0"),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_denies_buy_one_raw_unit_above_quote_spend_limit_before_readiness() {
        let amount_in = expected_buy_amount_in();
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits.as_mut().unwrap()[0].max_amount =
            (amount_in - U256::from(1u8)).to_string();
        let (mut client, cache) = swap_client_with_cache(config);
        let order = test_market_buy_order(test_pool().instrument_id);
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
            denied.reason.as_str().contains(&format!(
                "BUY quote amount {amount_in} exceeds the configured `quote_spend_limits`"
            )),
            "was: {}",
            denied.reason
        );
        assert!(!client.core.is_connected());
        assert!(!client.cache.has_database());
        assert!(client.signer.is_none());
        assert!(client.pending_tasks.is_empty());
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
    async fn submit_order_denies_sell_when_only_buy_pair_allowlisted() {
        let mut config = test_config("http://127.0.0.1:1".to_string());
        config.allowed_token_pairs = Some(vec![(USDC.to_string(), WETH.to_string())]);
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
        assert!(
            denied.reason.as_str().contains(&WETH_ADDRESS.to_string()),
            "was: {}",
            denied.reason
        );
    }

    #[tokio::test]
    async fn submit_order_sell_ignores_quote_spend_limits() {
        let mut config = test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits = Some(vec![quote_spend_limit(WETH, USDC, 18, "0")]);
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
                .contains("Blockchain execution client is not connected"),
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
        let nonce_state = sqlx::query_as::<_, (i64, i64)>(sqlx::AssertSqlSafe(format!(
            "SELECT next_canonical_nonce, revision FROM {schema}.execution_verification_nonce"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let decision_count = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_verification_decision \
             WHERE outcome = 'verified'"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let connect_decision_count = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_verification_decision \
                 WHERE decision_class = 'connect' AND outcome = 'verified'"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(nonce_state, (8, 1));
        assert_eq!(decision_count, 35);
        assert_eq!(connect_decision_count, 1);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_pins_every_pre_sign_state_read_to_swap_anchor() {
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_anchor_reads_test", swap_rpc_state().await)
                .await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);
        let requests = state.recorded_requests();
        let broadcast_index = requests
            .iter()
            .position(|request| request["method"] == "eth_sendRawTransaction")
            .unwrap();
        let pre_broadcast = &requests[..broadcast_index];
        let latest_reads: Vec<_> = pre_broadcast
            .iter()
            .filter(|request| {
                request["method"] == "eth_getBlockByNumber" && request["params"][0] == "latest"
            })
            .collect();
        assert_eq!(latest_reads.len(), 3);
        assert!(
            latest_reads
                .iter()
                .all(|request| request["params"] == serde_json::json!(["latest", false]))
        );

        for method in [
            "eth_getCode",
            "eth_call",
            "eth_estimateGas",
            "eth_getBalance",
        ] {
            let pinned: Vec<_> = pre_broadcast
                .iter()
                .filter(|request| request["method"] == method)
                .collect();
            assert!(!pinned.is_empty(), "method {method}");
            assert_eq!(pinned.len() % 3, 0, "method {method}: {pinned:?}");
            assert!(
                pinned
                    .iter()
                    .all(|request| request["params"][1] == FIXTURE_BLOCK_PARAM),
                "method {method}: {pinned:?}"
            );
        }

        let numbered_blocks: Vec<_> = pre_broadcast
            .iter()
            .filter(|request| {
                request["method"] == "eth_getBlockByNumber"
                    && request["params"][0] == FIXTURE_BLOCK_PARAM
            })
            .collect();
        assert!(!numbered_blocks.is_empty());
        assert_eq!(numbered_blocks.len() % 3, 0);
        assert!(
            numbered_blocks
                .iter()
                .all(|request| request["params"][1] == false)
        );

        let chain_ids: Vec<_> = pre_broadcast
            .iter()
            .filter(|request| request["method"] == "eth_chainId")
            .collect();
        assert_eq!(chain_ids.len(), 3);
        assert!(
            chain_ids
                .iter()
                .all(|request| request["params"] == serde_json::json!([]))
        );
        let nonces: Vec<_> = pre_broadcast
            .iter()
            .filter(|request| request["method"] == "eth_getTransactionCount")
            .collect();
        assert_eq!(nonces.len(), 6);
        assert_eq!(
            nonces
                .iter()
                .filter(|request| {
                    request["params"] == serde_json::json!([WALLET.to_ascii_lowercase(), "pending"])
                })
                .count(),
            3
        );
        assert_eq!(
            nonces
                .iter()
                .filter(|request| {
                    request["params"]
                        == serde_json::json!([WALLET.to_ascii_lowercase(), FIXTURE_BLOCK_PARAM])
                })
                .count(),
            3
        );
        let priority_fees: Vec<_> = pre_broadcast
            .iter()
            .filter(|request| request["method"] == "eth_maxPriorityFeePerGas")
            .collect();
        assert_eq!(priority_fees.len(), 3);
        assert!(
            priority_fees
                .iter()
                .all(|request| request["params"] == serde_json::json!([]))
        );

        let (_, expected_raw) = expected_swap_tx(expected_min_amount_out(50)).await;
        assert_eq!(
            requests[broadcast_index]["params"][0].as_str().unwrap(),
            expected_raw
        );
        let created_block: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT created_block FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(created_block, i64::try_from(FIXTURE_BLOCK).unwrap());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_changed_swap_anchor_before_signing() {
        let canonical = fixture_block_response(FIXTURE_BLOCK, B256::from([0x11; 32]));
        let changed = fixture_block_response(FIXTURE_BLOCK, B256::from([0x44; 32]));
        let state = swap_rpc_state().await.with_parameter_response_sequence(
            "eth_getBlockByNumber",
            FIXTURE_BLOCK_PARAM,
            &[
                &canonical, &canonical, &canonical, &canonical, &canonical, &canonical, &canonical,
                &canonical, &canonical, &canonical, &canonical, &canonical, &canonical, &canonical,
                &canonical, &changed, &changed, &changed,
            ],
        );
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_anchor_change_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1, "was: {events:?}");
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied
                .reason
                .as_str()
                .contains("pre-sign checkpoint reread verification disagreed"),
            "was: {}",
            denied.reason
        );
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("swap".into(), "recoverable".into(), false, false)]
        );
        let signed_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction_hash"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(signed_count, 0);
        assert!(
            state
                .recorded_requests()
                .iter()
                .all(|request| { request["method"] != "eth_sendRawTransaction" })
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_keeps_verified_swap_anchor_when_latest_advances() {
        let canonical = fixture_block_response(FIXTURE_BLOCK, B256::from([0x11; 32]));
        let newer = fixture_block_response(FIXTURE_BLOCK + 1, B256::from([0x44; 32]));
        let state = swap_rpc_state().await.with_parameter_response_sequence(
            "eth_getBlockByNumber",
            "latest",
            &[&canonical, &canonical, &canonical, &newer, &newer, &newer],
        );
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_advancing_head_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);
        let latest_reads = state
            .recorded_requests()
            .iter()
            .filter(|request| {
                request["method"] == "eth_getBlockByNumber" && request["params"][0] == "latest"
            })
            .count();
        assert_eq!(latest_reads, 3);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_changed_quote_watermark_before_signing() {
        let watermark_number = FIXTURE_BLOCK;
        let watermark_hash = B256::from([0x11; 32]);
        let canonical = fixture_block_response(watermark_number, watermark_hash);
        let changed = fixture_block_response(watermark_number, B256::from([0x66; 32]));
        let watermark_param = format!("0x{watermark_number:x}");
        let min_amount_out = expected_min_amount_out(50);
        let (tx_hash, _) = expected_swap_tx(min_amount_out).await;
        let head = finalized_swap_block(tx_hash, min_amount_out);
        let state = swap_rpc_state()
            .await
            .with_response("eth_getBlockByNumber", &head)
            .with_parameter_response_sequence(
                "eth_getBlockByNumber",
                &watermark_param,
                &[
                    &canonical, &canonical, &canonical, &canonical, &canonical, &canonical,
                    &canonical, &canonical, &canonical, &canonical, &canonical, &canonical,
                    &changed, &changed, &changed,
                ],
            );
        let Some((admin_pool, schema, mut client, state, cache)) =
            swap_client_with_database("execution_submit_watermark_change_test", state).await
        else {
            return;
        };
        let pool = test_pool();
        cache
            .borrow_mut()
            .add_pool_profiler(test_profiler_at_block(
                &pool,
                watermark_number,
                &watermark_hash.to_string(),
            ))
            .unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1, "was: {events:?}");
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied.reason.as_str().contains("changed before signing"),
            "was: {}",
            denied.reason
        );
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("swap".into(), "recoverable".into(), false, false)]
        );
        let signed_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction_hash"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(signed_count, 0);
        assert!(
            state
                .recorded_requests()
                .iter()
                .all(|request| { request["method"] != "eth_sendRawTransaction" })
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn changed_swap_anchor_retains_ownership_when_recovery_commit_fails() {
        let canonical = fixture_block_response(FIXTURE_BLOCK, B256::from([0x11; 32]));
        let changed = fixture_block_response(FIXTURE_BLOCK, B256::from([0x44; 32]));
        let state = swap_rpc_state().await.with_parameter_response_sequence(
            "eth_getBlockByNumber",
            FIXTURE_BLOCK_PARAM,
            &[
                &canonical, &canonical, &canonical, &canonical, &canonical, &canonical, &canonical,
                &canonical, &canonical, &canonical, &canonical, &canonical, &canonical, &canonical,
                &canonical, &changed, &changed, &changed,
            ],
        );
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_anchor_recovery_fail_test", state).await
        else {
            return;
        };
        install_recoverable_commit_rejection(&admin_pool, &schema).await;
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1, "was: {events:?}");
        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected OrderDenied, was {:?}", events[0]);
        };
        assert!(
            denied
                .reason
                .as_str()
                .contains("pre-sign checkpoint reread verification disagreed"),
            "was: {}",
            denied.reason
        );
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("swap".into(), "prepared".into(), false, true)]
        );
        let signed_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction_hash"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(signed_count, 0);
        assert!(
            state
                .recorded_requests()
                .iter()
                .all(|request| { request["method"] != "eth_sendRawTransaction" })
        );
        assert!(matches!(
            *client.in_flight.lock().unwrap(),
            Some(InFlightSlot::Preparing(TransactionPurpose::Swap))
        ));

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
        let payload_keys = client.payload_keys.clone();
        let restart_config = client.config.clone();
        drop(client);
        let (mut restarted, _) = swap_client_with_cache(restart_config);
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
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
            9
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_getBalance")
                .count(),
            6
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_emits_committed_swap_from_verified_inclusion_header() {
        let min_amount_out = expected_min_amount_out(50);
        let (expected_hash, _) = expected_swap_tx(min_amount_out).await;
        let state = finalized_swap_rpc_state(expected_hash, min_amount_out);
        let Some((admin_pool, schema, mut client, _, _)) =
            swap_client_with_database("execution_committed_fill_restart_test", state).await
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
        assert_swap_submitted_and_filled(&collect_order_events(&mut receiver));
        let ancestry_range: (i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT height_start, height_end FROM {schema}.execution_verification_decision \
             WHERE decision_class = 'finality' AND read_class = 'numbered_block' \
               AND height_end > height_start"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(
            ancestry_range,
            ((FIXTURE_BLOCK + 1) as i64, (FIXTURE_BLOCK + 2) as i64)
        );

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {schema}.execution_intent SET fill_emitted = FALSE, active = TRUE"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();
        let database = client.cache.database.as_ref().unwrap().clone();
        let payload_keys = client.payload_keys.clone();
        let restart_config = client.config.clone();
        drop(client);
        let (mut restarted, _) = swap_client_with_cache(restart_config);
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
        let mut restart_receiver = start_with_events(&mut restarted);

        restarted.reconcile_unresolved_execution().await.unwrap();

        let events = collect_order_events(&mut restart_receiver);
        let (fill_emitted, active): (bool, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT fill_emitted, active FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(events.len(), 1, "was: {events:?}");
        assert!(matches!(&events[0], OrderEventAny::Filled(_)));
        assert!(fill_emitted);
        assert!(!active);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn finalized_swap_ignores_unrelated_swap_logs() {
        let min_amount_out = expected_min_amount_out(50);
        let (expected_hash, _) = expected_swap_tx(min_amount_out).await;
        let receipt = finalized_swap_receipt_with_unrelated_swap(expected_hash);
        let state = finalized_swap_rpc_state(expected_hash, min_amount_out)
            .with_response("eth_getTransactionReceipt", &receipt);
        let Some((admin_pool, schema, mut client, _, _)) =
            swap_client_with_database("execution_submit_unrelated_log_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn prepare_swap_buy_accepts_quote_spend_exact_boundary() {
        let amount_in = expected_buy_amount_in();
        let min_amount_out = expected_buy_min_amount_out(50);
        let (expected_hash, _) = expected_buy_swap_tx(min_amount_out, amount_in).await;
        let state = finalized_buy_swap_rpc_state(expected_hash, min_amount_out, amount_in);
        let max_amount = amount_in.to_string();
        let Some((admin_pool, schema, client, _, cache)) = swap_client_with_database_config(
            "execution_prepare_buy_test",
            state,
            move |http_rpc_url| {
                let mut config = buy_test_config(http_rpc_url);
                config.quote_spend_limits.as_mut().unwrap()[0].max_amount = max_amount;
                config
            },
        )
        .await
        else {
            return;
        };
        let order = test_market_buy_order(test_pool().instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();

        let plan = client
            .prepare_swap(&submit_order_cmd(&order), &order)
            .unwrap();

        assert_eq!(plan.token_in, USDC_ADDRESS);
        assert_eq!(plan.token_out, WETH_ADDRESS);
        assert_eq!(plan.amount_in, amount_in);
        assert_eq!(plan.min_amount_out, min_amount_out);
        assert_ne!(plan.token_in, WETH_ADDRESS);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_broadcasts_buy_swap() {
        let amount_in = expected_buy_amount_in();
        let min_amount_out = expected_buy_min_amount_out(50);
        let (expected_hash, expected_raw) = expected_buy_swap_tx(min_amount_out, amount_in).await;
        let state = finalized_buy_swap_rpc_state(expected_hash, min_amount_out, amount_in);
        let Some((admin_pool, schema, mut client, state, cache)) =
            swap_client_with_buy_database("execution_submit_buy_success_test", state).await
        else {
            return;
        };
        let order = test_market_buy_order(test_pool().instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);
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
    async fn finalized_buy_swap_emits_fill_from_output_leg() {
        let amount_in = expected_buy_amount_in();
        let min_amount_out = expected_buy_min_amount_out(50);
        let (expected_hash, _) = expected_buy_swap_tx(min_amount_out, amount_in).await;
        let state = finalized_buy_swap_rpc_state(expected_hash, min_amount_out, amount_in);
        let Some((admin_pool, schema, mut client, state, cache)) =
            swap_client_with_buy_database("execution_submit_buy_fill_test", state).await
        else {
            return;
        };
        let order = test_market_buy_order(test_pool().instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
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

        let mut order_events = Vec::new();

        while let Ok(event) = receiver.try_recv() {
            if let ExecutionEvent::Order(event) = event {
                order_events.push(event);
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
        assert_eq!(fill.order_side, OrderSide::Buy);
        assert_eq!(fill.last_qty, Quantity::from("0.001"));
        assert_eq!(fill.currency.code.as_str(), "USDC");
        assert_eq!(fill.commission, Some(expected_commission));
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
        assert_eq!(
            Money::from_decimal(
                fill.last_qty.as_decimal() * fill.last_px.as_decimal(),
                fill.currency,
            )
            .unwrap(),
            Money::from_u256(amount_in, fill.currency).unwrap()
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
        let payload_keys = client.payload_keys.clone();
        let restart_config = client.config.clone();
        drop(client);
        let (mut restarted, _) = swap_client_with_cache(restart_config);
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
        let mut restart_receiver = start_with_events(&mut restarted);
        restarted.reconcile_unresolved_execution().await.unwrap();
        restarted.reconcile_unresolved_execution().await.unwrap();
        assert!(collect_order_events(&mut restart_receiver).is_empty());
        assert_eq!(
            state
                .recorded_requests()
                .iter()
                .filter(|request| request["method"] == "eth_sendRawTransaction")
                .count(),
            1
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn finalized_buy_swap_cancels_remainder_when_output_is_short() {
        let amount_in = expected_buy_amount_in();
        let min_amount_out = expected_buy_min_amount_out(50);
        let (expected_hash, _) = expected_buy_swap_tx(min_amount_out, amount_in).await;
        let short_base_out = min_amount_out;
        let mut state = finalized_buy_swap_rpc_state(expected_hash, min_amount_out, amount_in);
        state = state.with_response(
            "eth_getTransactionReceipt",
            &finalized_buy_swap_receipt_with_base_out(expected_hash, amount_in, short_base_out),
        );
        let Some((admin_pool, schema, mut client, _, cache)) =
            swap_client_with_buy_database("execution_submit_buy_short_fill_test", state).await
        else {
            return;
        };
        let order = test_market_buy_order(test_pool().instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
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

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 3, "was: {events:?}");
        assert!(matches!(&events[0], OrderEventAny::Submitted(_)));
        let OrderEventAny::Filled(fill) = &events[1] else {
            panic!("expected OrderFilled, was {:?}", events[1]);
        };
        let expected_qty = raw_amount_to_quantity(short_base_out, 18).unwrap();
        assert_eq!(fill.order_side, OrderSide::Buy);
        assert_eq!(fill.last_qty, expected_qty);
        assert!(fill.last_qty < order.quantity());
        let OrderEventAny::Canceled(canceled) = &events[2] else {
            panic!("expected OrderCanceled, was {:?}", events[2]);
        };
        assert_eq!(canceled.client_order_id, order.client_order_id());
        assert_eq!(
            canceled.venue_order_id.as_ref().map(VenueOrderId::as_str),
            Some(expected_hash.to_string().as_str())
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn finalized_buy_swap_reports_full_output_when_above_order_quantity() {
        let amount_in = expected_buy_amount_in();
        let min_amount_out = expected_buy_min_amount_out(50);
        let (expected_hash, _) = expected_buy_swap_tx(min_amount_out, amount_in).await;
        let overshoot_base_out = expected_buy_base_amount() * U256::from(2) + U256::from(44);
        let mut state = finalized_buy_swap_rpc_state(expected_hash, min_amount_out, amount_in);
        state = state.with_response(
            "eth_getTransactionReceipt",
            &finalized_buy_swap_receipt_with_base_out(expected_hash, amount_in, overshoot_base_out),
        );
        let Some((admin_pool, schema, mut client, _, cache)) =
            swap_client_with_buy_database("execution_submit_buy_overshoot_fill_test", state).await
        else {
            return;
        };
        let order = test_market_buy_order(test_pool().instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
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

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 2, "was: {events:?}");
        assert!(matches!(&events[0], OrderEventAny::Submitted(_)));
        let OrderEventAny::Filled(fill) = &events[1] else {
            panic!("expected OrderFilled, was {:?}", events[1]);
        };
        let expected_qty = raw_amount_to_quantity(overshoot_base_out, 18).unwrap();
        assert_eq!(fill.order_side, OrderSide::Buy);
        assert_eq!(fill.last_qty, expected_qty);
        assert!(fill.last_qty > order.quantity());
        assert_eq!(
            Money::from_decimal(
                fill.last_qty.as_decimal() * fill.last_px.as_decimal(),
                fill.currency,
            )
            .unwrap(),
            Money::from_u256(amount_in, fill.currency).unwrap()
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn finalized_buy_swap_quarantines_sell_oriented_log() {
        let amount_in = expected_buy_amount_in();
        let min_amount_out = expected_buy_min_amount_out(50);
        let (expected_hash, _) = expected_buy_swap_tx(min_amount_out, amount_in).await;
        let state = finalized_buy_swap_rpc_state(expected_hash, min_amount_out, amount_in)
            .with_response(
                "eth_getTransactionReceipt",
                &finalized_swap_receipt(expected_hash),
            );
        let Some((admin_pool, schema, mut client, _, cache)) =
            swap_client_with_buy_database("execution_submit_buy_sell_log_test", state).await
        else {
            return;
        };
        let order = test_market_buy_order(test_pool().instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
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
        assert_swap_quarantined_without_terminal_event(&events);
        assert!(
            error
                .to_string()
                .contains("does not match the persisted amount")
                || error.to_string().contains("is not a BUY output"),
            "was: {error}"
        );
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_applies_buy_slippage_to_base_output() {
        let amount_in = expected_buy_amount_in();
        let min_amount_out = expected_buy_min_amount_out(200);
        let (expected_hash, expected_raw) = expected_buy_swap_tx(min_amount_out, amount_in).await;
        let state = finalized_buy_swap_rpc_state(expected_hash, min_amount_out, amount_in);
        let Some((admin_pool, schema, mut client, state, cache)) =
            swap_client_with_buy_database("execution_submit_buy_slippage_test", state).await
        else {
            return;
        };
        let order = test_market_buy_order(test_pool().instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
        let mut cmd = submit_order_cmd(&order);
        cmd.params = Some(serde_json::from_str(r#"{"slippage_bps": 200}"#).unwrap());
        let mut receiver = start_with_events(&mut client);

        client.submit_order(cmd).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);
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
    async fn submit_order_denies_buy_on_insufficient_quote_balance() {
        let amount_in = expected_buy_amount_in();
        let min_amount_out = expected_buy_min_amount_out(50);
        let (expected_hash, _) = expected_buy_swap_tx(min_amount_out, amount_in).await;
        let state = finalized_buy_swap_rpc_state(expected_hash, min_amount_out, amount_in)
            .with_call_response(BALANCE_OF_SELECTOR, CALL_ZERO);
        let Some((admin_pool, schema, mut client, _, cache)) =
            swap_client_with_buy_database("execution_submit_buy_balance_test", state).await
        else {
            return;
        };
        let order = test_market_buy_order(test_pool().instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
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
    async fn finalized_swap_without_log_stays_quarantined() {
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

        assert_swap_quarantined_without_terminal_event(&events);
        assert_eq!(status, "broadcast");
        assert!(!terminal_emitted);
        assert!(active);
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn finalized_swap_refresh_failure_stays_owned_for_reconciliation() {
        let min_amount_out = expected_min_amount_out(50);
        let (expected_hash, _) = expected_swap_tx(min_amount_out).await;
        let state = finalized_swap_rpc_state(expected_hash, min_amount_out).with_response_sequence(
            "eth_getBalance",
            &[
                GET_BALANCE,
                GET_BALANCE,
                GET_BALANCE,
                RPC_METHOD_NOT_FOUND,
                RPC_METHOD_NOT_FOUND,
                RPC_METHOD_NOT_FOUND,
            ],
        );
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
            error
                .to_string()
                .contains("finalized native balance verification is locally invalid"),
            "was: {error}"
        );
        assert_eq!(events.len(), 1, "was: {events:?}");
        assert!(matches!(&events[0], OrderEventAny::Submitted(_)));
        assert_eq!(status, "broadcast");
        assert!(!fill_emitted);
        assert!(active);
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_router_factory_mismatch_before_signing() {
        let state = swap_rpc_state()
            .await
            .with_call_response(FACTORY_SELECTOR, CALL_ZERO);
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_factory_test", state).await
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
                .contains("swap deployment manifest verification disagreed"),
            "was: {}",
            denied.reason
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_getTransactionCount")
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_router_weth_mismatch_before_signing() {
        let state = swap_rpc_state()
            .await
            .with_call_response(WETH9_SELECTOR, CALL_ZERO);
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_weth_test", state).await
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
                .contains("swap deployment manifest verification disagreed"),
            "was: {}",
            denied.reason
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_getTransactionCount")
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_factory_pool_mismatch_before_signing() {
        let state = swap_rpc_state()
            .await
            .with_call_response(GET_POOL_SELECTOR, CALL_ZERO);
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_pool_identity_test", state).await
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
                .contains("swap deployment manifest verification disagreed"),
            "was: {}",
            denied.reason
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_getTransactionCount")
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_denies_cached_token_decimal_mismatch_before_signing() {
        let state = swap_rpc_state().await.with_contract_call_response(
            WETH,
            DECIMALS_SELECTOR,
            CALL_DECIMALS_6,
        );
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_decimals_test", state).await
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
                .contains("swap deployment manifest verification disagreed"),
            "was: {}",
            denied.reason
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_getTransactionCount")
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

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
        let expected_min_out = expected_min_amount_out(50);
        let (expected_hash, _) = expected_swap_tx(expected_min_out).await;
        let block = finalized_swap_block(expected_hash, expected_min_out);
        let receipt = receipt_with_transaction_hash(RECEIPT_REVERTED, expected_hash);
        let state = with_finalized_identity(
            swap_rpc_state()
                .await
                .with_response("eth_getTransactionReceipt", &receipt),
            &block,
            &receipt,
        );
        let Some((admin_pool, schema, mut client, _state, _)) =
            swap_client_with_database("execution_submit_reverted_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);
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
    async fn submit_order_persists_authenticated_envelope_before_broadcast() {
        let state = swap_rpc_state().await;
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("protected_submit_test", state).await
        else {
            return;
        };
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);
        let representations: Vec<(bool, bool)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT raw_transaction IS NULL, sealed_transaction IS NOT NULL \
             FROM {schema}.execution_transaction_hash WHERE payload_expected"
        )))
        .fetch_all(&admin_pool)
        .await
        .unwrap();
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        assert_eq!(representations, vec![(true, true)]);
        assert_eq!(broadcasts, 1);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn protected_persistence_failure_prevents_broadcast_and_acknowledgment() {
        let state = swap_rpc_state().await;
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("protected_persist_failure_test", state).await
        else {
            return;
        };

        for statement in [
            format!(
                "CREATE FUNCTION {schema}.reject_protected_payload() RETURNS trigger \
                 LANGUAGE plpgsql AS 'BEGIN RAISE EXCEPTION ''test payload rejection''; END'"
            ),
            format!(
                "CREATE TRIGGER reject_protected_payload BEFORE INSERT ON \
                 {schema}.execution_transaction_hash FOR EACH ROW \
                 EXECUTE FUNCTION {schema}.reject_protected_payload()"
            ),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(&admin_pool)
                .await
                .unwrap();
        }
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert!(
            events
                .iter()
                .all(|event| !matches!(event, OrderEventAny::Submitted(_)))
        );
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        let payload_rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction_hash"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(broadcasts, 0);
        assert_eq!(payload_rows, 0);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_reservation_failure_denies_without_broadcast_and_releases_slot() {
        let Some((admin_pool, schema, mut client, state, cache)) =
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
        let pool = test_pool();
        let first = test_market_sell_order(pool.instrument_id);
        let second = market_sell_order_with_id(pool.instrument_id, "O-SWAP-002");
        cache
            .borrow_mut()
            .add_order(second.clone(), None, None, false)
            .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&first)).unwrap();
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
                .contains("Execution intent reservation failed before commit"),
            "was: {}",
            denied.reason
        );
        client.submit_order(submit_order_cmd(&second)).unwrap();
        await_pending_tasks(&client).await;
        let retry_events = collect_order_events(&mut receiver);
        assert_eq!(retry_events.len(), 1);
        let OrderEventAny::Denied(retry_denied) = &retry_events[0] else {
            panic!("expected OrderDenied, was {:?}", retry_events[0]);
        };
        assert_eq!(
            retry_denied.reason.as_str(),
            "Execution intent reservation failed before commit"
        );
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        assert_eq!(broadcasts, 0);
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_reservation_commit_failure_keeps_preparing_slot() {
        let Some((admin_pool, schema, mut client, state, cache)) = swap_client_with_database(
            "execution_submit_reservation_commit_fail_test",
            swap_rpc_state().await,
        )
        .await
        else {
            return;
        };
        install_reservation_commit_rejection(&admin_pool, &schema).await;
        let pool = test_pool();
        let first = test_market_sell_order(pool.instrument_id);
        let second = market_sell_order_with_id(pool.instrument_id, "O-SWAP-002");
        cache
            .borrow_mut()
            .add_order(second.clone(), None, None, false)
            .unwrap();
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&first)).unwrap();
        await_pending_tasks(&client).await;
        client.submit_order(submit_order_cmd(&second)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        let submitted = events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Submitted(_)))
            .count();
        let denied_commit = events
            .iter()
            .filter(|event| {
                matches!(event, OrderEventAny::Denied(denied) if denied.reason.as_str() == "Execution intent reservation commit outcome is unknown; reconciliation is required")
            })
            .count();
        let denied_in_flight = events
            .iter()
            .filter(|event| {
                matches!(event, OrderEventAny::Denied(denied) if denied.reason.as_str().contains("at most one transaction can be in flight"))
            })
            .count();
        let requests = state.recorded_requests();
        let intent_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let signed_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction_hash"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert_eq!(submitted, 0, "was: {events:?}");
        assert_eq!(denied_commit, 1, "was: {events:?}");
        assert_eq!(denied_in_flight, 1, "was: {events:?}");
        assert!(matches!(
            *client.in_flight.lock().unwrap(),
            Some(InFlightSlot::Preparing(TransactionPurpose::Swap))
        ));
        assert_eq!(intent_count, 0);
        assert_eq!(signed_count, 0);
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_getTransactionCount")
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn protected_payload_migration_failure_keeps_plaintext_and_blocks_ready() {
        let Some((admin_pool, schema, client, _)) = execution_client_with_unprotected_database(
            "protected_payload_migration_failure",
            execution_rpc_state(),
        )
        .await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap();
        let (intent, _) = persist_invalid_test_swap(database, None).await;
        let keys = payload_test_keys([0x31; 32], vec![], "migration-failure");
        let error = database
            .ensure_execution_payload_storage(&keys)
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("failed to authenticate execution payload")
        );
        let row = database
            .get_execution_transaction_hashes(intent.id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        let operation: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT operation FROM {schema}.execution_payload_state"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(operation, "migrate");
        assert!(row.raw_transaction.is_some());
        assert!(row.sealed_transaction.is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn protected_payload_migration_restart_restore_rewrap_and_rollback() {
        let Some((admin_pool, pg_config)) =
            connect_test_postgres("protected payload lifecycle").await
        else {
            return;
        };
        let schema = format!("protected_payload_lifecycle_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;
        let options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let options = options.options([("search_path", schema.clone())]);
        let database = connect_test_database(options.clone()).await.unwrap();
        database
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        let intent = reserve_test_wrap_intent(&database).await;
        database
            .assign_execution_intent_nonce(intent.id, 7)
            .await
            .unwrap();
        let transaction = build_eip1559_transaction(
            42161,
            7,
            78_000,
            130_000_000,
            10_000_000,
            WETH_ADDRESS,
            U256::from(1_u64),
            Bytes::from(hex::decode("d0e30db0").unwrap()),
        );
        let (transaction_hash, raw_transaction) = sign_eip1559_transaction(
            transaction,
            &PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        )
        .await
        .unwrap();
        database
            .add_execution_transaction_hash(
                intent.id,
                42161,
                &transaction_hash.to_string(),
                &raw_transaction,
            )
            .await
            .unwrap();
        let keys = payload_test_keys([0x41; 32], vec![], "restore-a");

        database
            .ensure_execution_payload_storage(&keys)
            .await
            .unwrap();
        let operation: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT operation FROM {schema}.execution_payload_state"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(operation, "ready");
        drop(database);

        let restarted = connect_test_database(options).await.unwrap();
        restarted
            .ensure_execution_payload_storage(&keys)
            .await
            .unwrap();
        let row = restarted
            .get_execution_transaction_hashes(intent.id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert!(row.raw_transaction.is_none());
        let sealed_transaction = row.sealed_transaction.clone().unwrap();
        let stored_intent = restarted.get_execution_intent(intent.id).await.unwrap();
        let context = payload_context(&stored_intent, &row, keys.deployment_id()).unwrap();
        let alternate_envelope = keys.seal(&raw_transaction, &context).unwrap();
        assert_ne!(alternate_envelope, sealed_transaction);
        let repeated = restarted
            .add_execution_transaction_envelope(
                intent.id,
                42161,
                &transaction_hash.to_string(),
                &alternate_envelope,
            )
            .await
            .unwrap();
        let check = restarted
            .check_execution_payload_storage(Some(&keys), None, 1)
            .await
            .unwrap();
        assert_eq!(
            repeated.sealed_transaction,
            Some(sealed_transaction.clone())
        );
        assert_eq!(check.plaintext_rows, 0);
        assert_eq!(check.original_rows, 1);
        assert_eq!(check.replacement_rows, 0);
        assert_eq!(check.authenticated_rows, 1);
        assert!(!check.read_roles.is_empty());

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {schema}.execution_payload_key_state SET seals = 4294967295"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();
        let exhausted = restarted
            .reserve_execution_payload_seal(&keys)
            .await
            .unwrap_err();
        assert!(exhausted.to_string().contains("seal limit"));
        let seals: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT seals FROM {schema}.execution_payload_key_state"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(seals, 4_294_967_295);
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {schema}.execution_payload_key_state SET seals = 1"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let missing_key = restarted
            .check_execution_payload_storage(None, None, 1)
            .await
            .unwrap_err();
        assert!(
            missing_key
                .to_string()
                .contains("no payload key is configured")
        );
        let restored_elsewhere = payload_test_keys([0x41; 32], vec![], "restore-b");
        let wrong_context = restarted
            .ensure_execution_payload_storage(&restored_elsewhere)
            .await
            .unwrap_err();
        assert!(wrong_context.to_string().contains("deployment ID"));

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {schema}.execution_transaction_hash \
             SET sealed_transaction = set_byte(sealed_transaction, octet_length(sealed_transaction) - 1, \
                 get_byte(sealed_transaction, octet_length(sealed_transaction) - 1) # 1)"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();
        assert!(
            restarted
                .check_execution_payload_storage(Some(&keys), None, 1)
                .await
                .is_err()
        );
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {schema}.execution_transaction_hash SET sealed_transaction = $1"
        )))
        .bind(&sealed_transaction)
        .execute(&admin_pool)
        .await
        .unwrap();

        for statement in [
            format!(
                "ALTER TABLE {schema}.execution_transaction_hash \
                 DROP CONSTRAINT execution_transaction_payload_protected_check"
            ),
            format!("UPDATE {schema}.execution_transaction_hash SET sealed_transaction = NULL"),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(&admin_pool)
                .await
                .unwrap();
        }
        assert!(
            restarted
                .check_execution_payload_storage(Some(&keys), None, 1)
                .await
                .is_err()
        );
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {schema}.execution_transaction_hash SET sealed_transaction = $1"
        )))
        .bind(&sealed_transaction)
        .execute(&admin_pool)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {schema}.execution_transaction_hash \
             ADD CONSTRAINT execution_transaction_payload_protected_check CHECK ( \
                 (payload_expected AND raw_transaction IS NULL AND sealed_transaction IS NOT NULL) \
                 OR (NOT payload_expected AND raw_transaction IS NULL AND sealed_transaction IS NULL) \
             )"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let rotated = payload_test_keys([0x52; 32], vec![[0x41; 32]], "restore-a");
        restarted
            .rewrap_execution_payload_storage(&rotated, 1)
            .await
            .unwrap();
        let rotated_check = restarted
            .check_execution_payload_storage(Some(&rotated), None, 1)
            .await
            .unwrap();
        assert_eq!(rotated_check.authenticated_rows, 1);
        assert_eq!(rotated_check.key_ids.len(), 1);

        restarted
            .rollback_execution_payload_storage(&rotated, 1)
            .await
            .unwrap();
        let rolled_back = restarted
            .get_execution_transaction_hashes(intent.id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(
            rolled_back.raw_transaction.as_deref(),
            Some(raw_transaction.as_slice())
        );
        assert!(rolled_back.sealed_transaction.is_none());
        let legacy_check = restarted
            .check_execution_payload_storage(None, None, 1)
            .await
            .unwrap();
        assert!(!legacy_check.protected);
        assert_eq!(legacy_check.plaintext_rows, 1);
        assert_eq!(legacy_check.authenticated_rows, 1);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn protected_payload_storage_authenticates_multiple_execution_identities() {
        let Some((admin_pool, pg_config)) =
            connect_test_postgres("protected payload multiple identities").await
        else {
            return;
        };
        let schema = format!("protected_payload_identities_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;
        let options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let database = connect_test_database(options.options([("search_path", schema.clone())]))
            .await
            .unwrap();
        database
            .ensure_execution_transaction_schema()
            .await
            .unwrap();

        let private_keys = [
            TEST_PRIVATE_KEY,
            "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
        ];

        for (index, private_key) in private_keys.into_iter().enumerate() {
            let signer = PrivateKeySigner::from_str(private_key).unwrap();
            let nonce = 7 + u64::try_from(index).unwrap();
            let intent =
                reserve_test_wrap_intent_for_wallet(&database, &signer.address().to_string()).await;
            database
                .assign_execution_intent_nonce(intent.id, nonce)
                .await
                .unwrap();
            let transaction = build_eip1559_transaction(
                42161,
                nonce,
                78_000,
                130_000_000,
                10_000_000,
                WETH_ADDRESS,
                U256::from(1_u64),
                Bytes::from(hex::decode("d0e30db0").unwrap()),
            );
            let (transaction_hash, raw_transaction) =
                sign_eip1559_transaction(transaction, &signer)
                    .await
                    .unwrap();
            database
                .add_execution_transaction_hash(
                    intent.id,
                    42161,
                    &transaction_hash.to_string(),
                    &raw_transaction,
                )
                .await
                .unwrap();
        }

        let keys = payload_test_keys([0x61; 32], vec![], "multiple-identities");
        database
            .ensure_execution_payload_storage(&keys)
            .await
            .unwrap();
        let lease = database
            .require_execution_payload_storage(
                &keys,
                PayloadPolicy {
                    chain_id: 42161,
                    signer: Address::from_str(WALLET).unwrap(),
                    gas_limit: 1_000_000,
                    max_fee_per_gas: 1_000_000_000,
                },
                1,
            )
            .await
            .unwrap();
        drop(lease);
        let check = database
            .check_execution_payload_storage(Some(&keys), None, 1)
            .await
            .unwrap();

        assert!(check.protected);
        assert_eq!(check.plaintext_rows, 0);
        assert_eq!(check.original_rows, 2);
        assert_eq!(check.replacement_rows, 0);
        assert_eq!(check.authenticated_rows, 2);
        assert_eq!(check.key_ids.len(), 1);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[rstest]
    #[case::finalized(TransactionStatus::Finalized)]
    #[case::reverted(TransactionStatus::Reverted)]
    #[tokio::test]
    async fn protected_payload_storage_ignores_current_policy_for_released_terminal_history(
        #[case] status: TransactionStatus,
    ) {
        let Some((admin_pool, pg_config)) =
            connect_test_postgres("protected payload terminal history").await
        else {
            return;
        };
        let schema = format!(
            "protected_payload_terminal_{}_{}",
            status.as_str(),
            std::process::id()
        );
        setup_execution_schema(&admin_pool, &schema).await;
        let options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let database = connect_test_database(options.options([("search_path", schema.clone())]))
            .await
            .unwrap();
        database
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        let (intent, transaction_hash, _) = persist_test_wrap_broadcast(&database, None).await;
        database
            .record_execution_status(
                intent.id,
                &transaction_hash.to_string(),
                status,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        database
            .mark_execution_event_emitted(intent.id, "terminal")
            .await
            .unwrap();
        let keys = payload_test_keys([0x62; 32], vec![], "terminal-history");
        database
            .ensure_execution_payload_storage(&keys)
            .await
            .unwrap();

        let lease = database
            .require_execution_payload_storage(
                &keys,
                PayloadPolicy {
                    chain_id: 42161,
                    signer: Address::from_str(WALLET).unwrap(),
                    gas_limit: 1,
                    max_fee_per_gas: 1,
                },
                1,
            )
            .await
            .unwrap();
        drop(lease);
        let check = database
            .check_execution_payload_storage(Some(&keys), None, 1)
            .await
            .unwrap();
        let active_intent = reserve_test_wrap_intent(&database).await;
        database
            .assign_execution_intent_nonce(active_intent.id, 8)
            .await
            .unwrap();
        let active_intent = database
            .get_execution_intent(active_intent.id)
            .await
            .unwrap();
        let active_transaction = build_eip1559_transaction(
            42161,
            8,
            78_000,
            130_000_000,
            10_000_000,
            WETH_ADDRESS,
            U256::from(1_u64),
            Bytes::from(hex::decode("d0e30db0").unwrap()),
        );
        let (active_hash, active_raw_transaction) = sign_eip1559_transaction(
            active_transaction,
            &PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        )
        .await
        .unwrap();
        persist_test_payload(
            &database,
            Some(&keys),
            &active_intent,
            active_hash,
            &active_raw_transaction,
        )
        .await;
        let active_error = match database
            .require_execution_payload_storage(
                &keys,
                PayloadPolicy {
                    chain_id: 42161,
                    signer: Address::from_str(WALLET).unwrap(),
                    gas_limit: 1,
                    max_fee_per_gas: 1,
                },
                1,
            )
            .await
        {
            Ok(_) => panic!("active execution payload unexpectedly passed current policy"),
            Err(e) => e,
        };
        let active_error = format!("{active_error:#}");

        assert!(check.protected);
        assert_eq!(check.authenticated_rows, 1);
        assert!(
            active_error.contains(&format!(
                "execution intent {} transaction {active_hash} violates current execution policy",
                active_intent.id
            )),
            "was: {active_error}"
        );
        assert!(
            active_error.contains("gas limit 78000 exceeds configured ceiling 1"),
            "was: {active_error}"
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[allow(unsafe_code)] // env-var mutation uses the test client's dedicated signer variable
    #[tokio::test]
    async fn rollback_blocks_execution_until_protection_and_full_check_succeed() {
        let state = ready_rpc_state();
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("payload_rollback_reactivation", state).await
        else {
            return;
        };
        // SAFETY: this test binary owns the dedicated test signer variable
        unsafe { std::env::set_var("BLOCKCHAIN_TEST_PRIVATE_KEY", TEST_PRIVATE_KEY) };
        client.disconnect().await.unwrap();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        client.start().unwrap();

        client.rollback_payload_storage(1).await.unwrap();
        let unprotected = client.check_payload_storage(1).await.unwrap();
        let error = client.connect().await.unwrap_err();

        assert!(!unprotected.protected);
        assert_eq!(unprotected.plaintext_rows, 0);
        assert_eq!(unprotected.authenticated_rows, 0);
        assert!(
            error
                .to_string()
                .contains("Postgres execution requires protected payload storage"),
            "was: {error}"
        );
        assert!(client.signer.is_none());
        assert!(state.recorded_requests().is_empty());

        client.protect_payload_storage().await.unwrap();
        let protected = client.check_payload_storage(1).await.unwrap();
        assert!(protected.protected);
        assert_eq!(protected.plaintext_rows, 0);
        assert_eq!(protected.authenticated_rows, 0);

        client.connect().await.unwrap();

        assert!(client.is_connected());
        assert!(client.transaction_executor().is_ok());
        assert!(!state.recorded_requests().is_empty());

        drop(client);
        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[rstest]
    #[case::active_key(true)]
    #[case::deployment_identity(false)]
    #[tokio::test]
    async fn postgres_connect_rejects_missing_payload_identity_before_rpc(
        #[case] remove_active_key: bool,
    ) {
        let test_name = if remove_active_key {
            "payload_connect_missing_active_key"
        } else {
            "payload_connect_missing_deployment"
        };
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database(test_name, ready_rpc_state()).await
        else {
            return;
        };
        client.disconnect().await.unwrap();
        client.payload_keys = None;
        if remove_active_key {
            client.config.payload_key_env = None;
            client.config.payload_deployment_id = None;
        } else {
            client.config.payload_deployment_id = None;
        }

        let error = client.connect().await.unwrap_err();

        assert!(
            error.to_string().contains(if remove_active_key {
                "Postgres execution requires an active payload key and deployment identity"
            } else {
                "Payload deployment ID is required"
            }),
            "was: {error}"
        );
        assert!(client.signer.is_none());
        assert!(client.payload_keys.is_none());
        assert!(state.recorded_requests().is_empty());

        drop(client);
        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn postgres_connect_rejects_missing_envelope_key_before_rpc() {
        let Some((admin_pool, schema, mut client, state)) = execution_client_with_database(
            "payload_connect_missing_envelope_key",
            ready_rpc_state(),
        )
        .await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap().clone();
        persist_test_wrap_broadcast(&database, client.payload_keys.as_deref()).await;
        client.disconnect().await.unwrap();
        client.payload_keys = None;
        let keys = set_test_payload_key(&mut client, [0xb6; 32], &schema);
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {schema}.execution_payload_state SET active_key_id = $1 \
             WHERE component = 'signed_transactions'"
        )))
        .bind(keys.active_key_id().as_slice())
        .execute(&admin_pool)
        .await
        .unwrap();

        let error = client.connect().await.unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Stored execution payload requires unavailable key"),
            "was: {error}"
        );
        assert!(client.signer.is_none());
        assert!(client.payload_keys.is_none());
        assert!(state.recorded_requests().is_empty());

        drop(client);
        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn payload_action_lease_blocks_rewrap_transition_until_release() {
        let Some((admin_pool, schema, client, _)) = execution_client_with_unprotected_database(
            "payload_action_lease",
            execution_rpc_state(),
        )
        .await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap();
        let keys = payload_test_keys([0x71; 32], vec![], "action-lease");
        database
            .ensure_execution_payload_storage(&keys)
            .await
            .unwrap();
        let lease = database
            .acquire_execution_payload_lease(&keys)
            .await
            .unwrap();
        let rotated = payload_test_keys([0x72; 32], vec![[0x71; 32]], "action-lease");
        let mut operation = Box::pin(database.rewrap_execution_payload_storage(&rotated, 1));

        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut operation)
                .await
                .is_err()
        );
        let state: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT operation FROM {schema}.execution_payload_state"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(state, "ready");

        drop(lease);
        tokio::time::timeout(Duration::from_secs(2), operation)
            .await
            .unwrap()
            .unwrap();
        let state: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT operation FROM {schema}.execution_payload_state"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(state, "ready");

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn payload_infrastructure_checks_are_scoped_to_the_execution_schema() {
        let Some((admin_pool, pg_config)) = connect_test_postgres("payload schema isolation").await
        else {
            return;
        };
        let schema_a = format!("payload_schema_a_{}", std::process::id());
        let schema_b = format!("payload_schema_b_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema_a).await;
        setup_execution_schema(&admin_pool, &schema_b).await;
        let options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let database_a =
            connect_test_database(options.clone().options([("search_path", schema_a.clone())]))
                .await
                .unwrap();
        let database_b =
            connect_test_database(options.options([("search_path", schema_b.clone())]))
                .await
                .unwrap();
        database_a
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        database_b
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        let keys_a = payload_test_keys([0x91; 32], vec![], "schema-a");
        let keys_b = payload_test_keys([0x92; 32], vec![], "schema-b");

        database_a
            .ensure_execution_payload_storage(&keys_a)
            .await
            .unwrap();
        database_b
            .ensure_execution_payload_storage(&keys_b)
            .await
            .unwrap();

        for statement in [
            format!(
                "DROP TRIGGER execution_transaction_payload_fence ON \
                 {schema_a}.execution_transaction_hash"
            ),
            format!(
                "ALTER TABLE {schema_a}.execution_transaction_hash \
                 DROP CONSTRAINT execution_transaction_payload_protected_check"
            ),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(&admin_pool)
                .await
                .unwrap();
        }

        let error = database_a
            .ensure_execution_payload_storage(&keys_a)
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("without its write fence or constraint"),
            "was: {error}"
        );

        drop_execution_schema(&admin_pool, &schema_a).await;
        drop_execution_schema(&admin_pool, &schema_b).await;
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
        let database = connect_test_database(db_options).await.unwrap();
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
            .err()
            .unwrap();
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
        let database = connect_test_database(db_options).await.unwrap();
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
        let database = connect_test_database(db_options).await.unwrap();

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
                FIXTURE_BLOCK_HASH,
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
        let min_amount_out = expected_min_amount_out(50);
        let (tx_hash, _) = expected_swap_tx(min_amount_out).await;
        let head = finalized_swap_block(tx_hash, min_amount_out);
        let state = swap_rpc_state()
            .await
            .with_response("eth_getBlockByNumber", &head)
            .with_parameter_response("eth_getBlockByNumber", FIXTURE_BLOCK_PARAM, BLOCK_BY_NUMBER);
        let Some((admin_pool, schema, mut client, state, cache)) =
            swap_client_with_database_config(
                "execution_submit_fresh_boundary_test",
                state,
                |http_rpc_url| {
                    let mut config = test_config(http_rpc_url);
                    config.max_quote_age_blocks = Some(1);
                    config
                },
            )
            .await
        else {
            return;
        };
        let pool = test_pool();
        cache
            .borrow_mut()
            .add_pool_profiler(test_profiler_at_block(
                &pool,
                FIXTURE_BLOCK,
                FIXTURE_BLOCK_HASH,
            ))
            .unwrap();
        let order = test_market_sell_order(pool.instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_eq!(events.len(), 1, "was: {events:?}");
        assert!(matches!(&events[0], OrderEventAny::Submitted(_)));
        assert_eq!(
            state
                .recorded_requests()
                .iter()
                .filter(|request| request["method"] == "eth_sendRawTransaction")
                .count(),
            1
        );

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
        let database = connect_test_database(db_options).await.unwrap();

        let state = swap_rpc_state()
            .await
            .with_call_response(POOL_TOKEN0_SELECTOR, CALL_USDC)
            .with_call_response(POOL_TOKEN1_SELECTOR, CALL_WETH)
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
        let mut config = test_config(format!("http://{addr}"));
        let result = |response: &str| {
            serde_json::from_str::<serde_json::Value>(response).unwrap()["result"]
                .as_str()
                .unwrap()
                .to_string()
        };
        {
            let verification = config.verification.as_mut().unwrap();
            let pool_identity = &mut verification.deployment_manifest.pools[0];
            pool_identity.token0 = USDC.to_string();
            pool_identity.token1 = WETH.to_string();
            let pool_contract = verification
                .deployment_manifest
                .contracts
                .iter_mut()
                .find(|contract| contract.role == BlockchainContractRole::Pool)
                .unwrap();

            for probe in &mut pool_contract.probes {
                if probe.call_data.starts_with(POOL_TOKEN0_SELECTOR) {
                    probe.expected_output = result(CALL_USDC);
                } else if probe.call_data.starts_with(POOL_TOKEN1_SELECTOR) {
                    probe.expected_output = result(CALL_WETH);
                }
            }
        }
        refresh_test_manifest_digest(&mut config);
        let mut client = BlockchainExecutionClient::new(core, config).unwrap();
        client.cache.database = Some(database);
        client
            .cache
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        protect_test_storage(&mut client, &schema).await;
        initialize_test_verification_ledger(&client).await;
        client.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
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
        assert_ne!(
            expected_min_out,
            expected_min_amount_out(50),
            "the profiler quote must remain distinct from the independent quote fixture"
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

        let (_, expected_raw) = expected_swap_tx(expected_min_amount_out(50)).await;
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
            denied
                .reason
                .as_str()
                .contains("pre-sign chain ID verification disagreed"),
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
        let receipt_release = Arc::new(tokio::sync::Semaphore::new(0));
        let state = swap_rpc_state()
            .await
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_response_release("eth_getTransactionReceipt", Arc::clone(&receipt_release));
        let Some((admin_pool, schema, mut client, state, _)) =
            swap_client_with_database("execution_submit_inclusion_timeout_test", state).await
        else {
            return;
        };
        client.transaction_limits.receipt_timeout_secs = 1;
        let order = test_market_sell_order(test_pool().instrument_id);
        let mut receiver = start_with_events(&mut client);

        client.submit_order(submit_order_cmd(&order)).unwrap();
        await_recorded_requests(&state, "eth_getTransactionReceipt", 3).await;
        tokio::time::timeout(Duration::from_secs(3), await_pending_tasks(&client))
            .await
            .unwrap();
        receipt_release.add_permits(3);

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
        assert_eq!(transitions, ["prepared", "signed", "broadcast", "dropped"]);
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn submit_order_single_in_flight_rejects_concurrent_swap() {
        let broadcast_release = Arc::new(tokio::sync::Semaphore::new(0));
        let state = swap_rpc_state()
            .await
            .with_response_release("eth_sendRawTransaction", Arc::clone(&broadcast_release));
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
        await_recorded_requests(&state, "eth_sendRawTransaction", 1).await;
        client.submit_order(submit_order_cmd(&second)).unwrap();
        let event = tokio::time::timeout(TEST_TIMEOUT, receiver.recv())
            .await
            .unwrap()
            .unwrap();
        let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event else {
            panic!("expected OrderDenied, was {event:?}");
        };
        assert_eq!(denied.client_order_id, second.client_order_id());
        assert!(
            denied
                .reason
                .as_str()
                .contains("at most one transaction can be in flight"),
            "was: {}",
            denied.reason
        );
        broadcast_release.add_permits(1);
        await_pending_tasks(&client).await;

        let events = collect_order_events(&mut receiver);
        assert_swap_submitted_and_filled(&events);

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
        let (client, state) = client_with_mock_rpc(ready_rpc_state()).await;
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

        let requests = state.recorded_requests();
        for method in ["eth_getCode", "eth_call", "eth_getBalance"] {
            let matching: Vec<_> = requests
                .iter()
                .filter(|request| request["method"] == method)
                .collect();
            assert!(!matching.is_empty(), "method {method}");
            assert!(
                matching
                    .iter()
                    .all(|request| request["params"][1] == "latest"),
                "method {method}: {matching:?}"
            );
        }
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
    fn new_parses_pair_specific_quote_spend_limits_with_distinct_precisions() {
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits = Some(vec![
            quote_spend_limit(WETH, USDC, 18, &U256::MAX.to_string()),
            quote_spend_limit(USDC, WETH, 6, "1000000000"),
        ]);

        let client = test_client_from_config(config, test_pool());
        let sell_ceiling = client
            .transaction_limits
            .quote_spend_limits
            .get(&(WETH_ADDRESS, USDC_ADDRESS))
            .unwrap();
        let buy_ceiling = client
            .transaction_limits
            .quote_spend_limits
            .get(&(USDC_ADDRESS, WETH_ADDRESS))
            .unwrap();

        assert_eq!(sell_ceiling.spend_token, WETH_ADDRESS);
        assert_eq!(sell_ceiling.spend_token_decimals, 18);
        assert_eq!(sell_ceiling.max_amount, U256::MAX);
        assert_eq!(buy_ceiling.spend_token, USDC_ADDRESS);
        assert_eq!(buy_ceiling.spend_token_decimals, 6);
        assert_eq!(buy_ceiling.max_amount, U256::from(1_000_000_000u64));
    }

    #[rstest]
    fn new_rejects_quote_spend_limit_token_pair_mismatch() {
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits.as_mut().unwrap()[0].spend_token = WETH.to_string();

        let error = test_client_result(config, test_pool()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("`spend_token` must match `token_in`"),
            "was: {error}"
        );
    }

    #[rstest]
    fn new_rejects_quote_spend_limit_pair_outside_allowlist() {
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits = Some(vec![quote_spend_limit(
            USDC,
            "0x1111111111111111111111111111111111111111",
            6,
            "1000000000",
        )]);

        let error = test_client_result(config, test_pool()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("is not in the `allowed_token_pairs` allowlist"),
            "was: {error}"
        );
    }

    #[rstest]
    fn new_rejects_duplicate_quote_spend_pairs() {
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits = Some(vec![
            quote_spend_limit(USDC, WETH, 6, "1000000000"),
            quote_spend_limit(USDC, WETH, 6, "2000000000"),
        ]);

        let error = test_client_result(config, test_pool()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Duplicate quote spend limit for token pair"),
            "was: {error}"
        );
    }

    #[rstest]
    #[case::empty("")]
    #[case::signed("-1")]
    #[case::fractional("1.5")]
    #[case::hexadecimal("0x10")]
    #[case::overflow(
        "115792089237316195423570985008687907853269984665640564039457584007913129639936"
    )]
    fn new_rejects_invalid_quote_spend_max_amount(#[case] max_amount: &str) {
        let mut config = buy_test_config("http://127.0.0.1:1".to_string());
        config.quote_spend_limits.as_mut().unwrap()[0].max_amount = max_amount.to_string();

        let error = test_client_result(config, test_pool()).unwrap_err();

        assert!(
            error.to_string().contains("Quote spend limit `max_amount`"),
            "was: {error}"
        );
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
        let (mut client, state) = client_with_mock_rpc(ready_rpc_state()).await;

        let error = client
            .approve(
                WETH_ADDRESS,
                U256::from(1_000u64),
                address!("68b3465833fb72A70ecDF485E0e4C7bD8665Fc45"),
            )
            .await
            .err()
            .unwrap();

        assert!(
            error
                .to_string()
                .contains("not in the configured `router_addresses` allowlist"),
            "was: {error}"
        );
        assert!(state.recorded_requests().is_empty());
    }

    #[tokio::test]
    async fn approve_rejects_token_outside_input_allowlist() {
        let (mut client, state) = client_with_mock_rpc(ready_rpc_state()).await;

        let error = client
            .approve(USDC_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("is not an input token in the configured `allowed_token_pairs`"),
            "was: {error}"
        );
        assert!(state.recorded_requests().is_empty());
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
            error
                .to_string()
                .contains("pre-sign deployment manifest verification disagreed"),
            "was: {error}"
        );
        assert!(client.in_flight.lock().unwrap().is_none());
        let requests = state.recorded_requests();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_getCode")
                .count(),
            3
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

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

        assert!(
            error
                .to_string()
                .contains("pre-sign wrapped token probe verification is unavailable"),
            "was: {error}"
        );
        assert!(client.in_flight.lock().unwrap().is_none());
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn wrap_rejects_included_transaction_without_balance_delta() {
        let state = broadcast_rpc_state().with_response_sequence("eth_call", &[CALL_BALANCE; 9]);
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
        let in_flight = awaiting_in_flight(&client);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "broadcast".into(), false, true)]
        );
        let nonce_state = sqlx::query_as::<_, (i64, i64)>(sqlx::AssertSqlSafe(format!(
            "SELECT next_canonical_nonce, revision FROM {schema}.execution_verification_nonce"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(nonce_state, (7, 0));
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();
        assert_eq!(broadcasts, 1);

        let expected_hash = expected_wrap_tx_hash(U256::from(1_000_000_000_000_000u64)).await;
        let block = finalized_wrap_block(expected_hash);
        let receipt = receipt_with_transaction_hash(RECEIPT_SUCCESS, expected_hash);
        let restart_state = with_finalized_identity(
            execution_rpc_state()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
                .with_response_sequence("eth_call", &[CALL_BALANCE; 6]),
            &block,
            &receipt,
        );
        let addr = start_mock_rpc_server(restart_state).await;
        let error = later_reconnect(client, format!("http://{addr}")).await;
        assert!(
            error.to_string().contains("did not increase"),
            "was: {error}"
        );
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "broadcast".into(), false, true)]
        );
        let nonce_state = sqlx::query_as::<_, (i64, i64)>(sqlx::AssertSqlSafe(format!(
            "SELECT next_canonical_nonce, revision FROM {schema}.execution_verification_nonce"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(nonce_state, (7, 0));

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn wrap_reports_inclusion_when_postcondition_read_fails() {
        let state = broadcast_rpc_state().with_response_sequence(
            "eth_call",
            &[
                CALL_BALANCE,
                CALL_BALANCE,
                CALL_BALANCE,
                CALL_BALANCE,
                CALL_BALANCE,
                CALL_BALANCE,
                RPC_METHOD_NOT_FOUND,
                RPC_METHOD_NOT_FOUND,
                RPC_METHOD_NOT_FOUND,
            ],
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
            message.contains("failed to verify WETH balance after included transaction 0x"),
            "was: {message}"
        );
        assert!(message.contains("at block 30346561"), "was: {message}");
        let in_flight = awaiting_in_flight(&client);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "broadcast".into(), false, true)]
        );

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
        let approval_calls = requests
            .iter()
            .filter(|request| {
                request["method"] == "eth_call"
                    && request["params"][0]["data"]
                        .as_str()
                        .is_some_and(|data| data.starts_with("0x095ea7b3"))
            })
            .collect::<Vec<_>>();
        assert_eq!(approval_calls.len(), 3);
        for request in approval_calls {
            assert_eq!(
                request["params"][0]["from"]
                    .as_str()
                    .unwrap()
                    .parse::<Address>()
                    .unwrap(),
                WALLET.parse::<Address>().unwrap()
            );
            assert_eq!(request["params"][1], FIXTURE_BLOCK_PARAM);
        }
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_getTransactionCount")
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_rejects_router_with_wrong_factory_before_signing() {
        let state = ready_rpc_state().with_call_response(FACTORY_SELECTOR, CALL_ZERO);
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_approve_factory_test", state).await
        else {
            return;
        };

        let error = client
            .approve(WETH_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("pre-sign deployment manifest verification disagreed"),
            "was: {error}"
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_getTransactionCount")
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_rejects_router_with_wrong_weth_before_signing() {
        let state = ready_rpc_state().with_call_response(WETH9_SELECTOR, CALL_ZERO);
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_approve_weth_test", state).await
        else {
            return;
        };

        let error = client
            .approve(WETH_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("pre-sign deployment manifest verification disagreed"),
            "was: {error}"
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_getTransactionCount")
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_rejects_nonzero_to_nonzero_transition_before_signing() {
        let state = ready_rpc_state();
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_approve_nonzero_test", state).await
        else {
            return;
        };

        let error = client
            .approve(WETH_ADDRESS, U256::from(1_000u64), ROUTER_ADDRESS)
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("approve zero before setting"),
            "was: {error}"
        );
        let requests = state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_getTransactionCount")
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_zero_revokes_under_unlimited_policy() {
        let state = broadcast_rpc_state()
            .with_response("eth_call", CALL_BOOL_TRUE)
            .with_call_response_sequence(
                ALLOWANCE_SELECTOR,
                &[
                    CALL_ALLOWANCE,
                    CALL_ALLOWANCE,
                    CALL_ALLOWANCE,
                    CALL_ZERO,
                    CALL_ZERO,
                    CALL_ZERO,
                ],
            );
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_approve_revoke_test", state).await
        else {
            return;
        };
        client.config.unlimited_approval = true;

        let tx_hash = client
            .approve(WETH_ADDRESS, U256::ZERO, ROUTER_ADDRESS)
            .await
            .unwrap();

        assert_eq!(tx_hash, expected_approve_tx_hash(U256::ZERO).await);
        let approve_data = state
            .recorded_requests()
            .into_iter()
            .find_map(|request| {
                (request["method"] == "eth_estimateGas")
                    .then(|| request["params"][0]["data"].as_str().map(str::to_owned))
                    .flatten()
            })
            .unwrap();
        assert!(approve_data.starts_with("0x095ea7b3"));
        assert!(approve_data.ends_with(&"0".repeat(64)));
        assert_eq!(
            state
                .recorded_requests()
                .iter()
                .filter(|request| {
                    request["method"] == "eth_call"
                        && request["params"][0]["data"]
                            .as_str()
                            .is_some_and(|data| data.starts_with(FACTORY_SELECTOR))
                })
                .count(),
            12
        );
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("approve".into(), "finalized".into(), true, false)]
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_accepts_empty_return_with_sufficient_allowance() {
        let state = broadcast_rpc_state()
            .with_response("eth_call", CALL_EMPTY)
            .with_call_response_sequence(
                ALLOWANCE_SELECTOR,
                &[
                    CALL_ZERO,
                    CALL_ZERO,
                    CALL_ZERO,
                    CALL_ALLOWANCE_1000,
                    CALL_ALLOWANCE_1000,
                    CALL_ALLOWANCE_1000,
                ],
            );
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
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("approve".into(), "finalized".into(), true, false)]
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_rejects_empty_return_with_insufficient_allowance() {
        let state = broadcast_rpc_state()
            .with_response("eth_call", CALL_EMPTY)
            .with_call_response_sequence(ALLOWANCE_SELECTOR, &[CALL_ZERO; 6]);
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
            error
                .to_string()
                .contains("does not equal the requested amount"),
            "was: {error}"
        );
        let in_flight = awaiting_in_flight(&client);
        assert_eq!(in_flight.purpose, TransactionPurpose::Approve);
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("approve".into(), "broadcast".into(), false, true)]
        );
        let nonce_state = sqlx::query_as::<_, (i64, i64)>(sqlx::AssertSqlSafe(format!(
            "SELECT next_canonical_nonce, revision FROM {schema}.execution_verification_nonce"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(nonce_state, (7, 0));

        let expected_hash = expected_approve_tx_hash(U256::from(1_000u64)).await;
        let block = finalized_approve_block(expected_hash, U256::from(1_000u64));
        let receipt = receipt_with_transaction_hash(RECEIPT_SUCCESS, expected_hash);
        let restart_state = with_finalized_identity(
            execution_rpc_state()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
                .with_call_response(ALLOWANCE_SELECTOR, CALL_ZERO),
            &block,
            &receipt,
        );
        let addr = start_mock_rpc_server(restart_state).await;
        let error = later_reconnect(client, format!("http://{addr}")).await;
        assert!(
            error
                .to_string()
                .contains("does not equal the requested amount"),
            "was: {error}"
        );
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("approve".into(), "broadcast".into(), false, true)]
        );
        let nonce_state = sqlx::query_as::<_, (i64, i64)>(sqlx::AssertSqlSafe(format!(
            "SELECT next_canonical_nonce, revision FROM {schema}.execution_verification_nonce"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(nonce_state, (7, 0));

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn approve_reports_inclusion_when_postcondition_read_fails() {
        let state = broadcast_rpc_state()
            .with_response("eth_call", CALL_BOOL_TRUE)
            .with_call_response_sequence(
                ALLOWANCE_SELECTOR,
                &[
                    CALL_ZERO,
                    CALL_ZERO,
                    CALL_ZERO,
                    RPC_METHOD_NOT_FOUND,
                    RPC_METHOD_NOT_FOUND,
                    RPC_METHOD_NOT_FOUND,
                ],
            );
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
            message.contains("failed to verify router allowance after included transaction 0x"),
            "was: {message}"
        );
        assert!(message.contains("at block 30346561"), "was: {message}");
        let in_flight = awaiting_in_flight(&client);
        assert_eq!(in_flight.purpose, TransactionPurpose::Approve);
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("approve".into(), "broadcast".into(), false, true)]
        );

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
            error
                .to_string()
                .contains("Blockchain chain ID verification disagreed"),
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
    async fn connect_rejects_missing_trace_capability_before_signer_load() {
        let state = ready_rpc_state().with_parameter_response(
            "debug_traceTransaction",
            &B256::ZERO.to_string(),
            RPC_METHOD_NOT_FOUND,
        );
        let addr = start_mock_rpc_server(state.clone()).await;
        let config = test_config_with_signer_env(
            format!("http://{addr}"),
            "BLOCKCHAIN_TEST_TRACE_CAPABILITY",
        );
        let mut client = test_client_from_config(config, test_pool());
        // SAFETY: this variable name is unique to this test across the test binary
        unsafe { std::env::set_var("BLOCKCHAIN_TEST_TRACE_CAPABILITY", TEST_PRIVATE_KEY) };

        let error = client.connect().await.unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Blockchain call trace capability verification is locally invalid"),
            "was: {error}"
        );
        assert!(client.signer.is_none());
        assert!(
            state
                .recorded_requests()
                .iter()
                .all(|request| request["method"] != "eth_getBalance")
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

    #[allow(unsafe_code)] // env-var mutation in tests; unique var names avoid cross-test races
    #[tokio::test]
    async fn reconnect_resumes_at_the_durable_finalized_tip() {
        let Some((admin_pool, pg_config)) =
            connect_test_postgres("verification ledger resume").await
        else {
            return;
        };
        let schema = format!("verification_ledger_resume_{}", std::process::id());
        setup_execution_schema(&admin_pool, &schema).await;
        let options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let options = options.options([("search_path", schema.clone())]);
        let database = connect_test_database(options).await.unwrap();
        database
            .ensure_execution_transaction_schema()
            .await
            .unwrap();

        let state = ready_rpc_state();
        let addr = start_mock_rpc_server(state.clone()).await;
        let config = test_config_with_signer_env(
            format!("http://{addr}"),
            "BLOCKCHAIN_TEST_VERIFICATION_RESUME",
        );
        let mut client = test_client_from_config(config, test_pool());
        client.cache.database = Some(database);
        protect_test_storage(&mut client, &schema).await;
        let finalized_headers = [
            ExecutionVerifiedHeader {
                number: FIXTURE_BLOCK,
                hash: FIXTURE_BLOCK_HASH.to_string(),
                parent_hash: "0x0000000000000000000000000000000000000000000000000000000000000001"
                    .to_string(),
                timestamp: FIXTURE_BLOCK_TIMESTAMP,
                base_fee_per_gas: Some(100_000_000),
            },
            ExecutionVerifiedHeader {
                number: FIXTURE_BLOCK + 1,
                hash: B256::from([0x22; 32]).to_string(),
                parent_hash: FIXTURE_BLOCK_HASH.to_string(),
                timestamp: FIXTURE_BLOCK_TIMESTAMP + 1,
                base_fee_per_gas: Some(100_000_000),
            },
            ExecutionVerifiedHeader {
                number: FIXTURE_BLOCK + 2,
                hash: B256::from([0x33; 32]).to_string(),
                parent_hash: B256::from([0x22; 32]).to_string(),
                timestamp: FIXTURE_BLOCK_TIMESTAMP + 2,
                base_fee_per_gas: Some(100_000_000),
            },
        ];
        initialize_test_verification_ledger_with_headers(&client, &finalized_headers).await;
        let verification = client.config.verification.as_ref().unwrap();
        let position = client
            .cache
            .database
            .as_ref()
            .unwrap()
            .load_execution_verification_position(
                42_161,
                WALLET,
                &verification.manifest_version,
                &verification.manifest_digest,
            )
            .await
            .unwrap()
            .unwrap();
        let resume = client
            .cache
            .database
            .as_ref()
            .unwrap()
            .load_execution_verification_resume(
                42_161,
                WALLET,
                &verification.manifest_version,
                &verification.manifest_digest,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(position.next_canonical_nonce, 7);
        assert_eq!(position.revision, 0);
        assert_eq!(position.finalized_tip, finalized_headers[2]);
        assert_eq!(resume.next_canonical_nonce, 7);
        assert_eq!(resume.revision, 0);
        assert_eq!(resume.finalized_headers, finalized_headers);

        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        client.start().unwrap();
        // SAFETY: this variable name is unique to this test across the test binary
        unsafe { std::env::set_var("BLOCKCHAIN_TEST_VERIFICATION_RESUME", TEST_PRIVATE_KEY) };

        client.connect().await.unwrap();

        let skipped_height = format!("0x{:x}", FIXTURE_BLOCK + 1);
        assert!(client.is_connected());
        assert!(state.recorded_requests().iter().all(|request| {
            request["method"] != "eth_getBlockByNumber"
                || request["params"][0].as_str() != Some(&skipped_height)
        }));
        let header_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_verified_finalized_header"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(header_count, 3);

        drop(client);
        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn disconnect_revokes_signer_and_blocks_execution() {
        let (mut client, state) = client_with_mock_rpc(ready_rpc_state()).await;
        client.core.set_connected();
        client.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

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
        client
            .pending_tasks
            .spawn(async {})
            .expect("stale task spawn");

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
        await_recorded_requests(&state, "eth_getBlockByNumber", 1).await;
        assert!(client.in_flight.lock().unwrap().is_some());

        client.disconnect().await.unwrap();
        let ready_addr = start_mock_rpc_server(ready_rpc_state()).await;
        let ready = test_client(format!("http://{ready_addr}"));
        client.http_rpc_client = ready.http_rpc_client;
        client.verification = ready.verification;
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
        let database = connect_test_database(db_options).await.unwrap();

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
        protect_test_storage(&mut client, &schema).await;
        initialize_test_verification_ledger(&client).await;
        client.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
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
    async fn reservation_failure_releases_preparing_slot() {
        let state = signing_rpc_state();
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_reservation_fail_test", state).await
        else {
            return;
        };
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP TABLE {schema}.execution_intent CASCADE"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let retry_error = client
            .wrap(U256::from(2_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let broadcasts = state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .count();

        assert_eq!(
            error.to_string(),
            "Execution intent reservation failed before commit"
        );
        assert!(reservation_failure_proven_not_committed(&error));
        assert_eq!(
            retry_error.to_string(),
            "Execution intent reservation failed before commit"
        );
        assert_eq!(broadcasts, 0);
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn reservation_commit_failure_keeps_preparing_slot() {
        let state = signing_rpc_state();
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_reservation_commit_fail_test", state).await
        else {
            return;
        };
        install_reservation_commit_rejection(&admin_pool, &schema).await;

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let slot = *client.in_flight.lock().unwrap();
        let second_error = client
            .wrap(U256::from(2_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let requests = state.recorded_requests();
        let intent_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let signed_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction_hash"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert_eq!(
            error.to_string(),
            "Execution intent reservation commit outcome is unknown; reconciliation is required"
        );
        assert!(!reservation_failure_proven_not_committed(&error));
        assert!(matches!(
            slot,
            Some(InFlightSlot::Preparing(TransactionPurpose::Wrap))
        ));
        assert!(
            second_error.to_string().contains("being prepared"),
            "was: {second_error}"
        );
        assert_eq!(intent_count, 0);
        assert_eq!(signed_count, 0);
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_getTransactionCount")
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

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
            () = await_recorded_requests(&state, "eth_sendRawTransaction", 1) => {}
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
            () = await_recorded_requests(&state, "eth_getTransactionReceipt", 3) => {}
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
            3
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn rejected_broadcast_stays_dropped_and_occupied() {
        let state = signing_rpc_state()
            .with_response("eth_sendRawTransaction", SEND_RAW_TRANSACTION_REJECTED)
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL);
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
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_sleep("eth_sendRawTransaction", Duration::from_secs(1));
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_rejected_update_test", state).await
        else {
            return;
        };

        let mut wrap = Box::pin(client.wrap(U256::from(1_000_000_000_000_000u64)));
        tokio::select! {
            result = &mut wrap => panic!("broadcast completed before database failure: {result:?}"),
            () = await_recorded_requests(&state, "eth_sendRawTransaction", 1) => {}
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
    async fn finalized_receipt_keeps_slot_when_status_update_fails() {
        let expected_hash = expected_wrap_tx_hash(U256::from(1_000_000_000_000_000u64)).await;
        let block = finalized_wrap_block(expected_hash);
        let receipt = receipt_with_transaction_hash(RECEIPT_SUCCESS, expected_hash);
        let state = with_finalized_identity(
            signing_rpc_state()
                .with_send_raw_transaction_echo()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
                .with_call_response_sequence(
                    BALANCE_OF_SELECTOR,
                    &[
                        CALL_BALANCE,
                        CALL_BALANCE,
                        CALL_BALANCE,
                        CALL_BALANCE,
                        CALL_BALANCE,
                        CALL_BALANCE,
                        CALL_BALANCE_AFTER_WRAP,
                        CALL_BALANCE_AFTER_WRAP,
                        CALL_BALANCE_AFTER_WRAP,
                    ],
                ),
            &block,
            &receipt,
        );
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("execution_finalized_update_test", state).await
        else {
            return;
        };

        for statement in [
            format!(
                "CREATE FUNCTION {schema}.reject_finalized_status() RETURNS trigger \
                 LANGUAGE plpgsql AS 'BEGIN IF NEW.status = ''finalized'' THEN \
                 RAISE EXCEPTION ''test finalized status rejection''; END IF; \
                 RETURN NEW; END'"
            ),
            format!(
                "CREATE TRIGGER reject_finalized_status BEFORE UPDATE ON \
                 {schema}.execution_transaction_hash FOR EACH ROW \
                 EXECUTE FUNCTION {schema}.reject_finalized_status()"
            ),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(&admin_pool)
                .await
                .unwrap();
        }

        let error = client
            .wrap(U256::from(1_000_000_000_000_000u64))
            .await
            .unwrap_err();
        let in_flight = awaiting_in_flight(&client);
        let requests = state.recorded_requests();

        assert!(
            error
                .to_string()
                .contains("test finalized status rejection"),
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
            3
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn broadcast_timeout_marks_dropped_and_keeps_ownership() {
        let state = signing_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_sleep(
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
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        let block = finalized_wrap_block(expected_hash);
        let receipt = receipt_with_transaction_hash(RECEIPT_SUCCESS, expected_hash);
        let restart_state = with_finalized_identity(
            execution_rpc_state()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
                .with_response_sequence(
                    "eth_call",
                    &[
                        CALL_BALANCE,
                        CALL_BALANCE,
                        CALL_BALANCE,
                        CALL_BALANCE_AFTER_WRAP,
                        CALL_BALANCE_AFTER_WRAP,
                        CALL_BALANCE_AFTER_WRAP,
                    ],
                ),
            &block,
            &receipt,
        );
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

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
            6
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
            6
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

    fn migration_test_intent(id: i64) -> ExecutionIntentRow {
        ExecutionIntentRow {
            id,
            schema_version: crate::execution::transaction::EXECUTION_SCHEMA_VERSION,
            chain_id: 42_161,
            wallet_address: WALLET.to_string(),
            nonce: None,
            purpose: "wrap".to_string(),
            status: "prepared".to_string(),
            client_order_id: None,
            trader_id: None,
            strategy_id: None,
            account_id: None,
            instrument_id: None,
            pool_address: None,
            transaction_to: WETH.to_string(),
            transaction_input: "0xd0e30db0".to_string(),
            transaction_value: "1".to_string(),
            amount_in: None,
            created_block: FIXTURE_BLOCK,
            acknowledgement_emitted: false,
            fill_emitted: false,
            terminal_emitted: false,
            active: true,
        }
    }

    fn migration_nonce_verification() -> Verified<u64> {
        Verified {
            value: 7,
            read: crate::rpc::verification::VerificationRead::TransactionCount,
            provider_ids: [
                "authoritative".to_string(),
                "verifier-a".to_string(),
                "verifier-b".to_string(),
            ],
            normalized_value_digest: keccak256(7_u64.to_be_bytes()),
        }
    }

    fn migration_finalized_header() -> VerifiedBlockHeader {
        VerifiedBlockHeader {
            number: FIXTURE_BLOCK,
            hash: B256::from_str(FIXTURE_BLOCK_HASH).unwrap(),
            parent_hash: B256::from_str(
                "0x0000000000000000000000000000000000000000000000000000000000000001",
            )
            .unwrap(),
            timestamp: FIXTURE_BLOCK_TIMESTAMP,
            base_fee_per_gas: Some(100_000_000),
        }
    }

    #[tokio::test]
    async fn verification_migration_recovers_prepared_unassigned_intent() {
        let client = test_client("http://127.0.0.1:1".to_string());
        let snapshot = ExecutionVerificationMigrationSnapshot {
            intents: vec![migration_test_intent(1)],
            hashes: Vec::new(),
        };
        let finalized = migration_finalized_header();
        let migration = client
            .build_execution_verification_migration(
                snapshot,
                finalized,
                &[finalized],
                &migration_nonce_verification(),
            )
            .await
            .unwrap();

        assert_eq!(migration.records.len(), 1);
        let record = &migration.records[0];
        assert_eq!(record.intent_id, 1);
        assert_eq!(record.nonce, None);
        assert_eq!(record.transaction_hash, None);
        assert!(record.recover_prepared);
        assert_eq!(record.decisions.len(), 1);
    }

    #[tokio::test]
    async fn verification_migration_rejects_inconsistent_released_history() {
        let client = test_client("http://127.0.0.1:1".to_string());
        let finalized = migration_finalized_header();
        let nonce_verification = migration_nonce_verification();
        let mut missing_marker = migration_test_intent(1);
        missing_marker.active = false;
        missing_marker.status = "finalized".to_string();
        missing_marker.nonce = Some(6);
        let error = client
            .build_execution_verification_migration(
                ExecutionVerificationMigrationSnapshot {
                    intents: vec![missing_marker],
                    hashes: Vec::new(),
                },
                finalized,
                &[finalized],
                &nonce_verification,
            )
            .await
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("has no durable event marker"),
            "was: {error}"
        );

        let mut first = migration_test_intent(1);
        first.active = false;
        first.status = "recoverable".to_string();
        first.nonce = Some(6);
        let mut second = first.clone();
        second.id = 2;
        let error = client
            .build_execution_verification_migration(
                ExecutionVerificationMigrationSnapshot {
                    intents: vec![first, second],
                    hashes: Vec::new(),
                },
                finalized,
                &[finalized],
                &nonce_verification,
            )
            .await
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("duplicate signer nonce ownership"),
            "was: {error}"
        );
    }

    #[tokio::test]
    async fn verification_migration_reconstructs_consumed_active_intent() {
        let expected_hash = expected_wrap_tx_hash(U256::from(1u64)).await;
        let mut block_value: serde_json::Value =
            serde_json::from_str(&finalized_wrap_block(expected_hash)).unwrap();
        block_value["result"]["transactions"][0]["value"] = serde_json::json!("0x1");
        let block = block_value.to_string();
        let receipt = receipt_with_transaction_hash(RECEIPT_SUCCESS, expected_hash);
        let state = with_finalized_identity(
            execution_rpc_state()
                .with_response("eth_getTransactionCount", TRANSACTION_COUNT_NEXT)
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block),
            &block,
            &receipt,
        );
        let Some((admin_pool, pg_config)) =
            connect_test_postgres("verification_migration_reconstructs_consumed_active_intent")
                .await
        else {
            return;
        };
        let schema = format!(
            "verification_migration_reconstructs_consumed_active_intent_{}",
            std::process::id()
        );
        setup_execution_schema(&admin_pool, &schema).await;
        let options: sqlx::postgres::PgConnectOptions = pg_config.into();
        let database = connect_test_database(options.options([("search_path", schema.clone())]))
            .await
            .unwrap();
        let addr = start_mock_rpc_server(state).await;
        let (mut client, _) = swap_client_with_cache(test_config(format!("http://{addr}")));
        client.cache.database = Some(database.clone());
        client
            .cache
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        let (intent, persisted_hash, _) = persist_test_wrap_broadcast(&database, None).await;
        protect_test_storage(&mut client, &schema).await;
        assert_eq!(persisted_hash, expected_hash);

        let snapshot = database
            .load_execution_verification_migration_snapshot(42_161, WALLET)
            .await
            .unwrap();
        let nonce_verification = required_verification(
            client
                .verification
                .verify_transaction_count(&Address::from_str(WALLET).unwrap(), FIXTURE_BLOCK + 1)
                .await,
            "test migration nonce",
        )
        .unwrap();
        let headers = [
            VerifiedBlockHeader {
                number: FIXTURE_BLOCK,
                hash: B256::from_str(FIXTURE_BLOCK_HASH).unwrap(),
                parent_hash: B256::from_str(
                    "0x0000000000000000000000000000000000000000000000000000000000000001",
                )
                .unwrap(),
                timestamp: FIXTURE_BLOCK_TIMESTAMP,
                base_fee_per_gas: Some(100_000_000),
            },
            VerifiedBlockHeader {
                number: FIXTURE_BLOCK + 1,
                hash: B256::from([0x22; 32]),
                parent_hash: B256::from_str(FIXTURE_BLOCK_HASH).unwrap(),
                timestamp: FIXTURE_BLOCK_TIMESTAMP + 1,
                base_fee_per_gas: Some(100_000_000),
            },
        ];
        let migration = client
            .build_execution_verification_migration(
                snapshot,
                headers[1],
                &headers,
                &nonce_verification,
            )
            .await
            .unwrap();
        let finalized_headers = headers
            .iter()
            .map(|header| ExecutionVerifiedHeader {
                number: header.number,
                hash: header.hash.to_string(),
                parent_hash: header.parent_hash.to_string(),
                timestamp: header.timestamp,
                base_fee_per_gas: header.base_fee_per_gas,
            })
            .collect::<Vec<_>>();
        let decisions = [verification_decision(
            &nonce_verification,
            Some(FIXTURE_BLOCK + 1),
            Some(FIXTURE_BLOCK + 1),
        )];

        initialize_test_verification_migration(
            &client,
            &finalized_headers,
            8,
            &decisions,
            &migration,
        )
        .await;

        let migrated = database.get_execution_intent(intent.id).await.unwrap();
        let resume = database
            .load_execution_verification_resume(
                42_161,
                WALLET,
                &client
                    .config
                    .verification
                    .as_ref()
                    .unwrap()
                    .manifest_version,
                &client.config.verification.as_ref().unwrap().manifest_digest,
            )
            .await
            .unwrap()
            .unwrap();
        let evidence: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_verification_decision \
             WHERE intent_id = $1 AND decision_class = 'migration'"
        )))
        .bind(intent.id)
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert_eq!(migrated.status, "finalized");
        assert!(migrated.active);
        assert_eq!(resume.next_canonical_nonce, 8);
        assert!(evidence >= 5);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn verification_resume_accepts_one_consumed_owned_nonce() {
        let Some((admin_pool, schema, client, _)) = execution_client_with_database(
            "verification_resume_owned_nonce_test",
            ready_rpc_state(),
        )
        .await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap();
        persist_test_wrap_broadcast(database, client.payload_keys.as_deref()).await;
        let resume = database
            .load_execution_verification_resume(
                42_161,
                WALLET,
                &client
                    .config
                    .verification
                    .as_ref()
                    .unwrap()
                    .manifest_version,
                &client.config.verification.as_ref().unwrap().manifest_digest,
            )
            .await
            .unwrap()
            .unwrap();

        ensure_test_verification_ledger(&client, &resume.finalized_headers, 7, 8)
            .await
            .unwrap();

        let nonce_state = sqlx::query_as::<_, (i64, i64)>(sqlx::AssertSqlSafe(format!(
            "SELECT next_canonical_nonce, revision FROM {schema}.execution_verification_nonce"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(nonce_state, (7, 0));

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn verification_resume_rejects_unowned_nonce_advance() {
        for (test_name, mutation, observed_canonical_nonce) in [
            ("verification_resume_no_active_test", "no_active", 8),
            ("verification_resume_excess_test", "unchanged", 9),
            ("verification_resume_signed_test", "signed", 8),
            ("verification_resume_prepared_test", "prepared", 8),
            (
                "verification_resume_missing_payload_test",
                "missing_payload",
                8,
            ),
        ] {
            let Some((admin_pool, schema, client, _)) =
                execution_client_with_database(test_name, ready_rpc_state()).await
            else {
                return;
            };
            let database = client.cache.database.as_ref().unwrap();
            let (intent, _, _) =
                persist_test_wrap_broadcast(database, client.payload_keys.as_deref()).await;

            match mutation {
                "no_active" => {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "UPDATE {schema}.execution_intent SET active = FALSE WHERE id = $1"
                    )))
                    .bind(intent.id)
                    .execute(&admin_pool)
                    .await
                    .unwrap();
                }
                "signed" => {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "UPDATE {schema}.execution_intent SET status = 'signed' WHERE id = $1"
                    )))
                    .bind(intent.id)
                    .execute(&admin_pool)
                    .await
                    .unwrap();
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "UPDATE {schema}.execution_transaction_hash SET status = 'signed' \
                         WHERE intent_id = $1"
                    )))
                    .bind(intent.id)
                    .execute(&admin_pool)
                    .await
                    .unwrap();
                }
                "prepared" => {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "UPDATE {schema}.execution_intent \
                         SET nonce = NULL, status = 'prepared' WHERE id = $1"
                    )))
                    .bind(intent.id)
                    .execute(&admin_pool)
                    .await
                    .unwrap();
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "UPDATE {schema}.execution_transaction_hash SET current = FALSE \
                         WHERE intent_id = $1"
                    )))
                    .bind(intent.id)
                    .execute(&admin_pool)
                    .await
                    .unwrap();
                }
                "missing_payload" => {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "UPDATE {schema}.execution_transaction_hash \
                         SET payload_expected = FALSE, sealed_transaction = NULL \
                         WHERE intent_id = $1"
                    )))
                    .bind(intent.id)
                    .execute(&admin_pool)
                    .await
                    .unwrap();
                }
                "unchanged" => {}
                _ => unreachable!(),
            }
            let resume = database
                .load_execution_verification_resume(
                    42_161,
                    WALLET,
                    &client
                        .config
                        .verification
                        .as_ref()
                        .unwrap()
                        .manifest_version,
                    &client.config.verification.as_ref().unwrap().manifest_digest,
                )
                .await
                .unwrap()
                .unwrap();

            let error = ensure_test_verification_ledger(
                &client,
                &resume.finalized_headers,
                7,
                observed_canonical_nonce,
            )
            .await
            .unwrap_err();

            assert!(
                error
                    .to_string()
                    .contains(if observed_canonical_nonce == 9 {
                        "outside the owned recovery range"
                    } else {
                        "without"
                    }),
                "{mutation}: {error}"
            );
            let nonce_state = sqlx::query_as::<_, (i64, i64)>(sqlx::AssertSqlSafe(format!(
                "SELECT next_canonical_nonce, revision FROM {schema}.execution_verification_nonce"
            )))
            .fetch_one(&admin_pool)
            .await
            .unwrap();
            assert_eq!(nonce_state, (7, 0), "{mutation}");

            drop_execution_schema(&admin_pool, &schema).await;
        }
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
        let intent = reserve_test_wrap_intent(database).await;
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
    async fn protected_restart_authenticates_envelope_before_recovery() {
        let initial_state = broadcast_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE);
        let Some((admin_pool, schema, mut first_client, _)) =
            execution_client_with_database("protected_restart_test", initial_state).await
        else {
            return;
        };
        let database = first_client.cache.database.as_ref().unwrap().clone();
        let value = U256::from(1_000_000_000_000_000_u64);
        let expected_hash = expected_wrap_tx_hash(value).await;

        let error = first_client.wrap(value).await.unwrap_err();

        assert!(error.to_string().contains("Timed out awaiting finality"));
        let intent = database
            .get_active_execution_intent(42161, WALLET)
            .await
            .unwrap()
            .unwrap();
        let payload = database
            .get_execution_transaction_hashes(intent.id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert!(payload.raw_transaction.is_none());
        assert!(payload.sealed_transaction.is_some());
        let keys = first_client.payload_keys.take().unwrap();
        drop(first_client);

        let block = finalized_wrap_block(expected_hash);
        let receipt = receipt_with_transaction_hash(RECEIPT_SUCCESS, expected_hash);
        let restart_state = with_finalized_identity(
            execution_rpc_state()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
                .with_response_sequence(
                    "eth_call",
                    &[
                        CALL_BALANCE,
                        CALL_BALANCE,
                        CALL_BALANCE,
                        CALL_BALANCE_AFTER_WRAP,
                        CALL_BALANCE_AFTER_WRAP,
                        CALL_BALANCE_AFTER_WRAP,
                    ],
                ),
            &block,
            &receipt,
        );
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = Some(keys);
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

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
                .filter(|request| request["method"] == "eth_sendRawTransaction")
                .count(),
            0
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn active_intent_reconciliation_waits_for_reservation_fence() {
        let Some((admin_pool, schema, client, _)) = execution_client_with_database(
            "execution_active_intent_reservation_fence_test",
            ready_rpc_state(),
        )
        .await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap().clone();
        let lock = PgAdvisoryLock::new(format!(
            "nautilus:blockchain:execution:42161:{}",
            WALLET.to_ascii_lowercase()
        ));
        let PgAdvisoryLockKey::BigInt(lock_key) = lock.key() else {
            unreachable!("string advisory locks use the 64-bit key space");
        };
        let mut reservation = admin_pool.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(*lock_key)
            .execute(&mut *reservation)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {schema}.execution_intent (\
                schema_version, chain_id, wallet_address, purpose, status, transaction_to, \
                transaction_input, transaction_value, created_block\
             ) VALUES (2, $1, $2, 'wrap', 'prepared', $3, '0xd0e30db0', '1', $4)"
        )))
        .bind(42161_i32)
        .bind(WALLET)
        .bind(WETH_ADDRESS.to_string())
        .bind(i64::try_from(FIXTURE_BLOCK).unwrap())
        .execute(&mut *reservation)
        .await
        .unwrap();

        let mut reconciliation = Box::pin(database.get_active_execution_intent(42161, WALLET));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut reconciliation)
                .await
                .is_err(),
            "reconciliation did not wait for the reservation fence"
        );

        reservation.commit().await.unwrap();
        let intent = tokio::time::timeout(Duration::from_secs(2), reconciliation)
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        assert_eq!(intent.chain_id, 42161);
        assert_eq!(intent.wallet_address, WALLET);
        assert_eq!(intent.purpose, "wrap");
        assert_eq!(intent.status, "prepared");
        assert!(intent.active);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_keeps_unbroadcast_signed_intent_reserved() {
        let Some((admin_pool, schema, client, state)) =
            execution_client_with_database("execution_signed_restart_test", ready_rpc_state())
                .await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap();
        let intent = reserve_test_wrap_intent(database).await;
        let transaction = build_eip1559_transaction(
            42161,
            7,
            78_000,
            130_000_000,
            10_000_000,
            WETH_ADDRESS,
            U256::from(1u64),
            Bytes::from(nautilus_core::hex::decode("d0e30db0").unwrap()),
        );
        let (tx_hash, raw_transaction) = sign_eip1559_transaction(
            transaction,
            &PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        )
        .await
        .unwrap();
        database
            .assign_execution_intent_nonce(intent.id, 7)
            .await
            .unwrap();
        let intent = database.get_execution_intent(intent.id).await.unwrap();
        persist_test_payload(
            database,
            client.payload_keys.as_deref(),
            &intent,
            tx_hash,
            &raw_transaction,
        )
        .await;

        let error = client.reconcile_unresolved_execution().await.unwrap_err();

        assert!(
            error
                .to_string()
                .contains("was not authorized for broadcast"),
            "was: {error}"
        );
        let recovery = recovering_in_flight(&client);
        assert_eq!(recovery.intent_id, intent.id);
        assert_eq!(recovery.nonce, 7);
        assert_eq!(recovery.purpose, TransactionPurpose::Wrap);
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "signed".into(), false, true)]
        );
        assert!(
            state
                .recorded_requests()
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_quarantines_legacy_recoverable_signed_intent() {
        let Some((admin_pool, schema, client, state)) = execution_client_with_database(
            "execution_legacy_recoverable_restart_test",
            ready_rpc_state(),
        )
        .await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap();
        let intent = reserve_test_wrap_intent(database).await;
        let tx_hash = B256::from([0x55; 32]);
        database
            .assign_execution_intent_nonce(intent.id, 7)
            .await
            .unwrap();
        let intent = database.get_execution_intent(intent.id).await.unwrap();
        persist_test_payload(
            database,
            client.payload_keys.as_deref(),
            &intent,
            tx_hash,
            &[0x01, 0x02, 0x03],
        )
        .await;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {schema}.execution_intent SET status = 'recoverable', active = FALSE WHERE id = {}",
            intent.id
        )))
        .execute(&admin_pool)
        .await
        .unwrap();

        let error = client.reconcile_unresolved_execution().await.unwrap_err();

        assert!(
            error
                .to_string()
                .contains("retains signed transaction bytes"),
            "was: {error}"
        );
        assert!(client.in_flight.lock().unwrap().is_none());
        assert!(
            state
                .recorded_requests()
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[rstest]
    #[case::current("execution_invalid_signed_restart_test", false, "broadcast")]
    #[case::historical("execution_invalid_historical_restart_test", true, "replaced")]
    #[tokio::test]
    async fn restart_rejects_invalid_signed_bytes_before_recovery_effects(
        #[case] test_name: &str,
        #[case] historical: bool,
        #[case] expected_status: &str,
    ) {
        let Some((admin_pool, schema, first_client, state, _)) =
            swap_client_with_database(test_name, ready_rpc_state()).await
        else {
            return;
        };
        let database = first_client.cache.database.as_ref().unwrap().clone();
        let (intent, _) =
            persist_invalid_test_swap(&database, first_client.payload_keys.as_deref()).await;

        if historical {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {schema}.execution_transaction_hash \
                 SET current = FALSE, status = 'replaced' WHERE intent_id = $1"
            )))
            .bind(intent.id)
            .execute(&admin_pool)
            .await
            .unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO {schema}.execution_transaction_hash (\
                     intent_id, chain_id, transaction_hash, payload_expected, status, current\
                 ) VALUES ($1, 42161, $2, FALSE, 'replaced', TRUE)"
            )))
            .bind(intent.id)
            .bind(B256::from([0x44; 32]).to_string())
            .execute(&admin_pool)
            .await
            .unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {schema}.execution_intent SET status = 'replaced' WHERE id = $1"
            )))
            .bind(intent.id)
            .execute(&admin_pool)
            .await
            .unwrap();
        }
        let transitions_before: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction_transition"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        let config = first_client.config.clone();
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        let (mut restarted, _) = swap_client_with_cache(config);
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
        let mut receiver = start_with_events(&mut restarted);

        let error = restarted
            .reconcile_unresolved_execution()
            .await
            .unwrap_err();

        let recovery = recovering_in_flight(&restarted);
        let (status, acknowledgement_emitted, active): (String, bool, bool) =
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT status, acknowledgement_emitted, active FROM {schema}.execution_intent"
            )))
            .fetch_one(&admin_pool)
            .await
            .unwrap();
        let transitions_after: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction_transition"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert!(
            error
                .to_string()
                .contains("is not a complete EIP-2718 envelope"),
            "was: {error}"
        );
        assert_eq!(recovery.intent_id, intent.id);
        assert_eq!(recovery.nonce, 7);
        assert_eq!(recovery.purpose, TransactionPurpose::Swap);
        assert_eq!(status, expected_status);
        assert!(!acknowledgement_emitted);
        assert!(active);
        assert_eq!(transitions_after, transitions_before);
        assert!(collect_order_events(&mut receiver).is_empty());
        assert!(
            state
                .recorded_requests()
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_rebroadcasts_only_durably_authorized_bytes() {
        let Some((admin_pool, schema, first_client, _)) =
            execution_client_with_database("execution_broadcast_restart_test", ready_rpc_state())
                .await
        else {
            return;
        };
        let database = first_client.cache.database.as_ref().unwrap().clone();
        let (_, _, raw_tx) =
            persist_test_wrap_broadcast(&database, first_client.payload_keys.as_deref()).await;
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_response("eth_call", CALL_EMPTY)
            .with_send_raw_transaction_echo();
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

        restarted.reconcile_unresolved_execution().await.unwrap();

        let broadcasts = restart_state
            .recorded_requests()
            .into_iter()
            .filter(|request| request["method"] == "eth_sendRawTransaction")
            .collect::<Vec<_>>();
        assert_eq!(broadcasts.len(), 1);
        assert_eq!(broadcasts[0]["params"][0], hex::encode_prefixed(&raw_tx));
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "dropped".into(), false, true)]
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_suppresses_rebroadcast_when_canonical_nonce_advanced() {
        let Some((admin_pool, schema, first_client, _)) = execution_client_with_database(
            "execution_rebroadcast_nonce_advanced_test",
            ready_rpc_state(),
        )
        .await
        else {
            return;
        };
        let database = first_client.cache.database.as_ref().unwrap().clone();
        let (intent, _, _) =
            persist_test_wrap_broadcast(&database, first_client.payload_keys.as_deref()).await;
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        let mut empty_block: serde_json::Value =
            serde_json::from_str(&replacement_head_block(B256::from([0x44; 32]))).unwrap();
        empty_block["result"]["transactions"] = serde_json::json!([]);
        let empty_block = empty_block.to_string();
        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT_NEXT)
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_response("eth_call", CALL_EMPTY)
            .with_parameter_response("eth_getBlockByNumber", "0x1cf0d40", &empty_block)
            .with_send_raw_transaction_echo();
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

        let error = restarted
            .reconcile_unresolved_execution()
            .await
            .unwrap_err();

        let requests = restart_state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );
        assert!(requests.iter().all(|request| {
            request["method"] != "eth_call" || request["params"][0]["data"] != "0xd0e30db0"
        }));
        let decision_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_verification_decision \
             WHERE intent_id = $1 AND decision_class = 'rebroadcast'"
        )))
        .bind(intent.id)
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert!(
            error
                .to_string()
                .contains("without an authenticated signer transaction"),
            "was: {error}"
        );
        assert_eq!(decision_count, 6);
        let replacement_decision_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_verification_decision \
                 WHERE intent_id = $1 AND decision_class = 'replacement_scan'"
        )))
        .bind(intent.id)
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(replacement_decision_count, 2);
        let nonce_state = sqlx::query_as::<_, (i64, i64)>(sqlx::AssertSqlSafe(format!(
            "SELECT next_canonical_nonce, revision FROM {schema}.execution_verification_nonce"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(nonce_state, (7, 0));

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_suppresses_rebroadcast_when_receipt_exists() {
        let Some((admin_pool, schema, first_client, _)) = execution_client_with_database(
            "execution_rebroadcast_receipt_present_test",
            ready_rpc_state(),
        )
        .await
        else {
            return;
        };
        let database = first_client.cache.database.as_ref().unwrap().clone();
        let (intent, _, _) =
            persist_test_wrap_broadcast(&database, first_client.payload_keys.as_deref()).await;
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS)
            .with_response("eth_call", CALL_EMPTY)
            .with_send_raw_transaction_echo();
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

        let error = restarted
            .reconcile_unresolved_execution()
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("finalized transaction verification is locally invalid"),
            "was: {error}"
        );
        let requests = restart_state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );
        assert!(requests.iter().all(|request| {
            request["method"] != "eth_call" || request["params"][0]["data"] != "0xd0e30db0"
        }));
        let decision_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_verification_decision \
             WHERE intent_id = $1 AND decision_class = 'rebroadcast'"
        )))
        .bind(intent.id)
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(decision_count, 6);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[rstest]
    #[case("execution_rebroadcast_false_test", CALL_ZERO)]
    #[case("execution_rebroadcast_revert_test", CALL_REVERTED)]
    #[tokio::test]
    async fn restart_suppresses_rebroadcast_when_simulation_denies(
        #[case] test_name: &str,
        #[case] simulation_response: &str,
    ) {
        let Some((admin_pool, schema, first_client, _)) =
            execution_client_with_database(test_name, ready_rpc_state()).await
        else {
            return;
        };
        let database = first_client.cache.database.as_ref().unwrap().clone();
        let (intent, _, _) =
            persist_test_wrap_broadcast(&database, first_client.payload_keys.as_deref()).await;
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_response("eth_call", simulation_response)
            .with_send_raw_transaction_echo();
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

        restarted.reconcile_unresolved_execution().await.unwrap();

        let requests = restart_state.recorded_requests();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );
        let simulation_calls = requests
            .iter()
            .filter(|request| {
                request["method"] == "eth_call"
                    && request["params"][0]["data"] == "0xd0e30db0"
                    && request["params"][1] == FIXTURE_BLOCK_PARAM
            })
            .count();
        assert_eq!(simulation_calls, 3);
        let decision_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_verification_decision \
             WHERE intent_id = $1 AND decision_class = 'rebroadcast'"
        )))
        .bind(intent.id)
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(decision_count, 7);

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
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        // The finalized transaction carries no value, so it cannot be the persisted wrap
        let mismatched_block = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d41",
                "hash": "0x2222222222222222222222222222222222222222222222222222222222222222",
                "parentHash": FIXTURE_BLOCK_HASH,
                "timestamp": "0x69044a21",
                "baseFeePerGas": "0x5f5e100",
                "transactions": [{
                    "hash": expected_hash.to_string(),
                    "from": WALLET,
                    "nonce": "0x7",
                    "chainId": "0xa4b1",
                    "type": "0x2",
                    "to": WETH,
                    "input": "0xd0e30db0",
                    "value": "0x0",
                    "gas": "0x130b0",
                    "maxFeePerGas": "0x7bfa480",
                    "maxPriorityFeePerGas": "0x989680"
                }]
            }
        })
        .to_string();
        let receipt = receipt_with_transaction_hash(RECEIPT_SUCCESS, expected_hash);
        let restart_state = with_finalized_identity(
            execution_rpc_state()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &mismatched_block),
            &mismatched_block,
            &receipt,
        );
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

        let error = restarted
            .reconcile_unresolved_execution()
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("finalized transaction identity mismatch"),
            "was: {error}"
        );
        let in_flight = awaiting_in_flight(&restarted);
        assert_eq!(in_flight.nonce, 7);
        assert_eq!(in_flight.purpose, TransactionPurpose::Wrap);
        assert_eq!(in_flight.tx_hash, expected_hash);
        let requests = restart_state.recorded_requests();
        assert!(
            requests.iter().all(|request| {
                request["method"] != "eth_call"
                    || !request["params"][0]["data"]
                        .as_str()
                        .is_some_and(|data| data.starts_with(BALANCE_OF_SELECTOR))
            }),
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
        assert_eq!(status, "dropped");
        assert!(active);

        let database = restarted.cache.database.as_ref().unwrap().clone();
        let payload_keys = restarted.payload_keys.clone();
        drop(restarted);
        let mut second = test_client(format!("http://{addr}"));
        second.cache.database = Some(database);
        second.payload_keys = payload_keys;
        second.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
        let error = second.reconcile_unresolved_execution().await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("finalized transaction identity mismatch"),
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
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        // Call identity matches, but the wrapped balance does not increase
        let failing_wrap_state = || {
            let block = finalized_wrap_block(expected_hash);
            let receipt = receipt_with_transaction_hash(RECEIPT_SUCCESS, expected_hash);
            with_finalized_identity(
                execution_rpc_state()
                    .with_response("eth_getTransactionReceipt", &receipt)
                    .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
                    .with_response_sequence("eth_call", &[CALL_BALANCE; 6]),
                &block,
                &receipt,
            )
        };
        let restart_state = failing_wrap_state();
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

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
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "dropped".into(), false, true)]
        );

        let addr = start_mock_rpc_server(failing_wrap_state()).await;
        let error = later_reconnect(restarted, format!("http://{addr}")).await;
        assert!(
            error.to_string().contains("did not increase by"),
            "was: {error}"
        );
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "dropped".into(), false, true)]
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_wrap_revert_marks_terminal_and_releases() {
        let initial_state = broadcast_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE);
        let Some((admin_pool, schema, mut first_client, _)) =
            execution_client_with_database("execution_restart_wrap_revert_test", initial_state)
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
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        let block = finalized_wrap_block(expected_hash);
        let receipt = receipt_with_transaction_hash(RECEIPT_REVERTED, expected_hash);
        let restart_state = with_finalized_identity(
            execution_rpc_state()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block),
            &block,
            &receipt,
        );
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

        restarted.reconcile_unresolved_execution().await.unwrap();

        assert!(restarted.in_flight.lock().unwrap().is_none());
        assert!(
            restart_state
                .recorded_requests()
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "reverted".into(), true, false)]
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_quarantines_unretained_same_nonce_wrap_replacement() {
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
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        // Matching call fields do not authenticate an unknown same-nonce payload.
        let replacement_hash = B256::from([0x44; 32]);
        let restart_state = execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT_NEXT)
            .with_response_sequence("eth_call", &[CALL_BALANCE, CALL_BALANCE_AFTER_WRAP])
            .with_response_sequence(
                "eth_getTransactionReceipt",
                &[RECEIPT_NULL, RECEIPT_NULL, RECEIPT_NULL],
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
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
        restarted.transaction_limits.receipt_timeout_secs = 2;

        let error = restarted
            .reconcile_unresolved_execution()
            .await
            .unwrap_err();

        let hashes: Vec<(String, String, bool)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT transaction_hash, status, current FROM \
                 {schema}.execution_transaction_hash ORDER BY id"
        )))
        .fetch_all(&admin_pool)
        .await
        .unwrap();
        let requests = restart_state.recorded_requests();

        assert!(
            error
                .to_string()
                .contains("has no authenticated retained payload"),
            "was: {error}"
        );
        assert_eq!(
            hashes,
            [(original_hash.to_string(), "dropped".to_string(), true)]
        );
        assert!(restarted.in_flight.lock().unwrap().is_some());
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "dropped".into(), false, true)]
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "eth_getTransactionReceipt")
                .count(),
            3
        );
        assert!(requests.iter().all(|request| {
            request["method"] != "eth_call" || request["params"][0]["data"] != "0xd0e30db0"
        }));
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn replacement_scan_accepts_only_an_authenticated_retained_payload() {
        let expected_hash = expected_wrap_tx_hash(U256::from(1_u64)).await;
        let mut replacement_block: serde_json::Value =
            serde_json::from_str(&replacement_head_block(expected_hash)).unwrap();
        replacement_block["result"]["transactions"][1]["value"] = serde_json::json!("0x1");
        let replacement_block = replacement_block.to_string();
        let state = execution_rpc_state().with_parameter_response(
            "eth_getBlockByNumber",
            "0x1cf0d40",
            &replacement_block,
        );
        let Some((admin_pool, schema, client, _)) =
            execution_client_with_database("replacement_scan_authenticated", state).await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap();
        let (intent, persisted_hash, persisted_raw) =
            persist_test_wrap_broadcast(database, client.payload_keys.as_deref()).await;
        assert_eq!(persisted_hash, expected_hash);
        let authenticated_payloads = HashMap::from([(expected_hash, persisted_raw.clone())]);
        let head = VerifiedBlockHeader {
            number: FIXTURE_BLOCK,
            hash: B256::from_str(FIXTURE_BLOCK_HASH).unwrap(),
            parent_hash: B256::from_str(
                "0x0000000000000000000000000000000000000000000000000000000000000001",
            )
            .unwrap(),
            timestamp: FIXTURE_BLOCK_TIMESTAMP,
            base_fee_per_gas: Some(100_000_000),
        };

        let matched = client
            .transaction_executor()
            .unwrap()
            .scan_canonical_replacement(&intent, 7, head, &authenticated_payloads)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(matched, (expected_hash, persisted_raw));
        let cursor: (i64, String) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT finalized_cursor_number, finalized_cursor_hash \
             FROM {schema}.execution_replacement_scan WHERE intent_id = $1"
        )))
        .bind(intent.id)
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(
            cursor,
            (FIXTURE_BLOCK as i64, FIXTURE_BLOCK_HASH.to_string())
        );
        let evidence_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_verification_decision \
             WHERE intent_id = $1 AND decision_class = 'replacement_scan'"
        )))
        .bind(intent.id)
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(evidence_count, 2);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn restart_reconciles_finalized_approve_after_validation() {
        let initial_state = broadcast_rpc_state()
            .with_response("eth_getTransactionReceipt", RECEIPT_NULL)
            .with_response("eth_call", CALL_BOOL_TRUE)
            .with_call_response_sequence(ALLOWANCE_SELECTOR, &[CALL_ZERO; 3])
            .with_call_response(ALLOWANCE_SELECTOR, CALL_ZERO);
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
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        let block = finalized_approve_block(expected_hash, U256::from(1_000u64));
        let receipt = receipt_with_transaction_hash(RECEIPT_SUCCESS, expected_hash);
        let restart_state = with_finalized_identity(
            execution_rpc_state()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
                .with_call_response(ALLOWANCE_SELECTOR, CALL_ALLOWANCE_1000),
            &block,
            &receipt,
        );
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

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
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("approve".into(), "finalized".into(), true, false)]
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| {
                    request["method"] == "eth_call"
                        && request["params"][0]["data"]
                            .as_str()
                            .is_some_and(|data| data.starts_with(ALLOWANCE_SELECTOR))
                })
                .count(),
            3
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
            .with_response("eth_call", CALL_BOOL_TRUE)
            .with_call_response_sequence(ALLOWANCE_SELECTOR, &[CALL_ZERO; 3])
            .with_call_response(ALLOWANCE_SELECTOR, CALL_ZERO);
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
        let payload_keys = first_client.payload_keys.clone();
        drop(first_client);

        // Call identity matches, but the allowance does not equal the approved amount
        let block = finalized_approve_block(expected_hash, U256::from(1_000u64));
        let receipt = receipt_with_transaction_hash(RECEIPT_SUCCESS, expected_hash);
        let restart_state = with_finalized_identity(
            execution_rpc_state()
                .with_response("eth_getTransactionReceipt", &receipt)
                .with_parameter_response("eth_getBlockByNumber", "0x1cf0d41", &block)
                .with_call_response(ALLOWANCE_SELECTOR, CALL_ZERO),
            &block,
            &receipt,
        );
        let addr = start_mock_rpc_server(restart_state.clone()).await;
        let mut restarted = test_client(format!("http://{addr}"));
        restarted.cache.database = Some(database);
        restarted.payload_keys = payload_keys;
        restarted.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));

        let error = restarted
            .reconcile_unresolved_execution()
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not equal the requested amount"),
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
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("approve".into(), "dropped".into(), false, true)]
        );

        let error = later_reconnect(restarted, format!("http://{addr}")).await;
        assert!(
            error
                .to_string()
                .contains("does not equal the requested amount"),
            "was: {error}"
        );
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("approve".into(), "dropped".into(), false, true)]
        );

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn disappearing_unfinalized_receipt_drops_without_committing_inclusion() {
        let state = execution_rpc_state()
            .with_response("eth_getTransactionCount", TRANSACTION_COUNT)
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response("eth_call", CALL_BALANCE)
            .with_response_sequence(
                "eth_getTransactionReceipt",
                &[
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                    RECEIPT_NULL,
                    RECEIPT_NULL,
                    RECEIPT_NULL,
                ],
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
        assert_eq!(transitions, ["prepared", "signed", "broadcast", "dropped"]);
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn changed_unfinalized_block_drops_without_committing_inclusion() {
        let changed_block = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1cf0d41",
                "hash": "0x4444444444444444444444444444444444444444444444444444444444444444",
                "parentHash": FIXTURE_BLOCK_HASH,
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
                &[
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                ],
            )
            .with_parameter_response_sequence(
                "eth_getBlockByNumber",
                "0x1cf0d41",
                &[
                    BLOCK_CANONICAL,
                    BLOCK_CANONICAL,
                    BLOCK_CANONICAL,
                    &changed_block,
                    &changed_block,
                    &changed_block,
                ],
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
        assert_eq!(transitions, ["prepared", "signed", "broadcast", "dropped"]);
        assert!(client.in_flight.lock().unwrap().is_some());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn pre_sign_pending_nonce_drift_blocks_wrap_before_signature() {
        let state = execution_rpc_state()
            .with_response_sequence(
                "eth_getTransactionCount",
                &[
                    TRANSACTION_COUNT,
                    TRANSACTION_COUNT,
                    TRANSACTION_COUNT,
                    TRANSACTION_COUNT_NEXT,
                    TRANSACTION_COUNT_NEXT,
                    TRANSACTION_COUNT_NEXT,
                ],
            )
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_call_response(BALANCE_OF_SELECTOR, CALL_BALANCE)
            .with_send_raw_transaction_echo();
        let Some((admin_pool, schema, mut client, state)) =
            execution_client_with_database("pre_sign_pending_nonce_drift", state).await
        else {
            return;
        };

        let error = client
            .wrap(U256::from(1_000_000_000_000_000_u64))
            .await
            .unwrap_err();
        let signed_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {schema}.execution_transaction_hash"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert!(
            error
                .to_string()
                .contains("Pending nonce does not match the verified canonical nonce"),
            "was: {error}"
        );
        assert_eq!(signed_count, 0);
        assert!(
            state
                .recorded_requests()
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );
        assert!(client.in_flight.lock().unwrap().is_none());

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn unknown_same_nonce_swap_replacement_emits_no_rejection() {
        let replacement_hash = B256::from([0x44; 32]);
        let replacement_block = replacement_head_block(replacement_hash);
        let state = execution_rpc_state()
            .with_parameter_response("eth_getBlockByNumber", "0x1cf0d40", &replacement_block)
            .with_send_raw_transaction_echo();
        let Some((admin_pool, schema, mut client, _, _)) =
            swap_client_with_database("unknown_same_nonce_swap_replacement", state).await
        else {
            return;
        };
        let mut receiver = start_with_events(&mut client);
        let database = client.cache.database.as_ref().unwrap();
        let (intent, original_hash, original_payload) =
            persist_test_swap_broadcast(database, client.payload_keys.as_deref()).await;
        let authenticated_payloads = HashMap::from([(original_hash, original_payload)]);
        let head = VerifiedBlockHeader {
            number: FIXTURE_BLOCK,
            hash: B256::from_str(FIXTURE_BLOCK_HASH).unwrap(),
            parent_hash: B256::from_str(
                "0x0000000000000000000000000000000000000000000000000000000000000001",
            )
            .unwrap(),
            timestamp: FIXTURE_BLOCK_TIMESTAMP,
            base_fee_per_gas: Some(100_000_000),
        };

        let error = client
            .transaction_executor()
            .unwrap()
            .scan_canonical_replacement(&intent, 7, head, &authenticated_payloads)
            .await
            .unwrap_err();
        let events = collect_order_events(&mut receiver);
        let (status, active): (String, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT status, active FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert!(
            error
                .to_string()
                .contains("has no authenticated retained payload"),
            "was: {error}"
        );
        assert!(events.is_empty(), "was: {events:?}");
        assert_eq!(status, "broadcast");
        assert!(active);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn replacement_scan_rejects_rpc_fields_that_conflict_with_the_payload() {
        let expected_hash = expected_wrap_tx_hash(U256::from(1_u64)).await;
        let mut replacement_block: serde_json::Value =
            serde_json::from_str(&replacement_head_block(expected_hash)).unwrap();
        replacement_block["result"]["transactions"][1]["value"] = serde_json::json!("0x2");
        let replacement_block = replacement_block.to_string();
        let state = execution_rpc_state().with_parameter_response(
            "eth_getBlockByNumber",
            "0x1cf0d40",
            &replacement_block,
        );
        let Some((admin_pool, schema, client, _)) =
            execution_client_with_database("replacement_scan_payload_mismatch", state).await
        else {
            return;
        };
        let database = client.cache.database.as_ref().unwrap();
        let (intent, persisted_hash, persisted_payload) =
            persist_test_wrap_broadcast(database, client.payload_keys.as_deref()).await;
        assert_eq!(persisted_hash, expected_hash);
        let authenticated_payloads = HashMap::from([(persisted_hash, persisted_payload)]);
        let head = VerifiedBlockHeader {
            number: FIXTURE_BLOCK,
            hash: B256::from_str(FIXTURE_BLOCK_HASH).unwrap(),
            parent_hash: B256::from_str(
                "0x0000000000000000000000000000000000000000000000000000000000000001",
            )
            .unwrap(),
            timestamp: FIXTURE_BLOCK_TIMESTAMP,
            base_fee_per_gas: Some(100_000_000),
        };

        let error = client
            .transaction_executor()
            .unwrap()
            .scan_canonical_replacement(&intent, 7, head, &authenticated_payloads)
            .await
            .unwrap_err();
        let (status, active): (String, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT status, active FROM {schema}.execution_intent"
        )))
        .fetch_one(&admin_pool)
        .await
        .unwrap();

        assert!(
            error
                .to_string()
                .contains("failed authenticated payload validation"),
            "was: {error}"
        );
        assert_eq!(status, "broadcast");
        assert!(active);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn execution_transaction_constraints_reject_conflicting_identity() {
        const TRANSACTION_HASH: &str = "0xduplicate-transaction-hash";
        const OTHER_WALLET: &str = "0x0000000000000000000000000000000000000001";
        let Some((admin_pool, schema, client, _)) = execution_client_with_unprotected_database(
            "execution_duplicate_record_test",
            ready_rpc_state(),
        )
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
        assert_eq!(
            signer_conflict.to_string(),
            "Execution intent reservation failed before commit"
        );
        assert!(
            signer_conflict.chain().any(|cause| cause
                .to_string()
                .contains("execution_intent_active_signer_key")),
            "was: {signer_conflict:#}"
        );
        assert_eq!(count, 2);

        drop_execution_schema(&admin_pool, &schema).await;
    }

    #[tokio::test]
    async fn execution_status_transitions_are_idempotent() {
        const TRANSACTION_HASH: &str =
            "0x5555555555555555555555555555555555555555555555555555555555555555";
        let Some((admin_pool, schema, client, _)) = execution_client_with_unprotected_database(
            "execution_transition_test",
            ready_rpc_state(),
        )
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
        assert!(active);
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
            .with_parameter_response_sequence(
                "eth_getBlockByNumber",
                "latest",
                &[
                    BLOCK_BY_NUMBER,
                    BLOCK_BY_NUMBER,
                    BLOCK_BY_NUMBER,
                    BLOCK_FINALIZED,
                    BLOCK_FINALIZED,
                    BLOCK_FINALIZED,
                ],
            )
            .with_response_sequence(
                "eth_call",
                &[
                    CALL_BALANCE,
                    CALL_BALANCE,
                    CALL_BALANCE,
                    CALL_BALANCE,
                    CALL_BALANCE,
                    CALL_BALANCE,
                    CALL_BALANCE_AFTER_WRAP,
                    CALL_BALANCE_AFTER_WRAP,
                    CALL_BALANCE_AFTER_WRAP,
                    CALL_BOOL_TRUE,
                    CALL_BOOL_TRUE,
                    CALL_BOOL_TRUE,
                ],
            )
            .with_call_response_sequence(
                ALLOWANCE_SELECTOR,
                &[
                    CALL_ZERO,
                    CALL_ZERO,
                    CALL_ZERO,
                    CALL_ALLOWANCE_MAX,
                    CALL_ALLOWANCE_MAX,
                    CALL_ALLOWANCE_MAX,
                ],
            )
            .with_response_sequence(
                "eth_getTransactionCount",
                &[
                    TRANSACTION_COUNT,
                    TRANSACTION_COUNT,
                    TRANSACTION_COUNT,
                    TRANSACTION_COUNT,
                    TRANSACTION_COUNT,
                    TRANSACTION_COUNT,
                    TRANSACTION_COUNT_NEXT,
                    TRANSACTION_COUNT_NEXT,
                    TRANSACTION_COUNT_NEXT,
                    TRANSACTION_COUNT_NEXT,
                    TRANSACTION_COUNT_NEXT,
                    TRANSACTION_COUNT_NEXT,
                ],
            )
            .with_response("eth_estimateGas", ESTIMATE_GAS)
            .with_response_sequence(
                "eth_getTransactionReceipt",
                &[
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                    RECEIPT_SUCCESS,
                ],
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
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "finalized".into(), true, false)]
        );

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
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![
                ("wrap".into(), "finalized".into(), true, false),
                ("approve".into(), "finalized".into(), true, false),
            ]
        );

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
        let broadcast_indexes = requests
            .iter()
            .enumerate()
            .filter_map(|(index, request)| {
                (request["method"] == "eth_sendRawTransaction").then_some(index)
            })
            .collect::<Vec<_>>();

        for (index, expected_block) in broadcast_indexes
            .into_iter()
            .zip([FIXTURE_BLOCK_PARAM, "0x1cf0d42"])
        {
            let nonce_fence = &requests[index - 6..index];
            assert!(nonce_fence[..3].iter().all(|request| {
                request["method"] == "eth_getTransactionCount"
                    && request["params"][1] == expected_block
            }));
            assert!(nonce_fence[3..].iter().all(|request| {
                request["method"] == "eth_getTransactionCount" && request["params"][1] == "pending"
            }));
        }
        let receipt_polls = requests
            .iter()
            .filter(|request| request["method"] == "eth_getTransactionReceipt")
            .count();
        assert_eq!(receipt_polls, 6);
        let allowance_calls: Vec<_> = requests
            .iter()
            .filter(|request| {
                request["method"] == "eth_call"
                    && request["params"][0]["data"]
                        .as_str()
                        .is_some_and(|data| data.starts_with(ALLOWANCE_SELECTOR))
            })
            .collect();
        assert_eq!(allowance_calls.len(), 6);
        assert!(
            allowance_calls[..3]
                .iter()
                .all(|request| request["params"][1] == "0x1cf0d42")
        );
        assert!(
            allowance_calls[3..]
                .iter()
                .all(|request| request["params"][1] == "0x1cf0d41")
        );

        // Unlimited approval policy encoded U256::MAX in the approve calldata
        let estimates: Vec<_> = requests
            .iter()
            .filter(|request| request["method"] == "eth_estimateGas")
            .collect();
        assert_eq!(estimates.len(), 6);
        assert!(estimates[..3].iter().all(|request| {
            request["params"].as_array().unwrap().len() == 2
                && request["params"][1] == FIXTURE_BLOCK_PARAM
        }));
        assert!(estimates[3..].iter().all(|request| {
            request["params"].as_array().unwrap().len() == 2 && request["params"][1] == "0x1cf0d42"
        }));
        let approve_data = estimates[3]["params"][0]["data"].as_str().unwrap();
        assert!(
            approve_data.starts_with("0x095ea7b3"),
            "was: {approve_data}"
        );
        assert!(
            approve_data.ends_with(&"f".repeat(64)),
            "was: {approve_data}"
        );

        for selector in [FACTORY_SELECTOR, WETH9_SELECTOR] {
            let calls: Vec<_> = requests
                .iter()
                .filter(|request| {
                    request["method"] == "eth_call"
                        && request["params"][0]["data"]
                            .as_str()
                            .is_some_and(|data| data.starts_with(selector))
                })
                .collect();
            assert!(!calls.is_empty(), "selector {selector}");
            assert!(
                calls.iter().all(|request| request["params"][1]
                    .as_str()
                    .is_some_and(|block| block.starts_with("0x"))),
                "selector {selector}: {calls:?}"
            );
        }
        let latest_blocks = requests
            .iter()
            .filter(|request| {
                request["method"] == "eth_getBlockByNumber"
                    && request["params"] == serde_json::json!(["latest", false])
            })
            .count();
        assert_eq!(latest_blocks, 6);

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
        assert_eq!(
            execution_intent_markers(&admin_pool, &schema).await,
            vec![("wrap".into(), "reverted".into(), true, false)]
        );

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

    fn market_buy_order_with_id(instrument_id: InstrumentId, client_order_id: &str) -> OrderAny {
        OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("S-001"))
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from(client_order_id))
            .side(OrderSide::Buy)
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
            Some(OrderSide::Sell),
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
        let (admin_pool, schema, mut client, state) =
            execution_client_with_unprotected_database(test_name, state).await?;
        protect_test_storage(&mut client, &schema).await;
        Some((admin_pool, schema, client, state))
    }

    async fn execution_client_with_unprotected_database(
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
        let database = connect_test_database(db_options).await.unwrap();
        let addr = start_mock_rpc_server(state.clone()).await;
        let mut client = test_client(format!("http://{addr}"));
        client.cache.database = Some(database);
        // Mirror the connect-time migration: tests create the pre-submission table shape
        client
            .cache
            .ensure_execution_transaction_schema()
            .await
            .unwrap();
        initialize_test_verification_ledger(&client).await;
        client.signer = Some(Arc::new(
            PrivateKeySigner::from_str(TEST_PRIVATE_KEY).unwrap(),
        ));
        client.core.set_connected();

        Some((admin_pool, schema, client, state))
    }

    async fn install_reservation_commit_rejection(admin_pool: &sqlx::PgPool, schema: &str) {
        for statement in [
            format!(
                "CREATE FUNCTION {schema}.reject_execution_reservation_commit() RETURNS trigger \
                 LANGUAGE plpgsql AS 'BEGIN RAISE EXCEPTION ''test reservation commit rejection''; \
                 RETURN NEW; END'"
            ),
            format!(
                "CREATE CONSTRAINT TRIGGER reject_execution_reservation_commit AFTER INSERT ON \
                 {schema}.execution_transaction_transition DEFERRABLE INITIALLY DEFERRED \
                 FOR EACH ROW EXECUTE FUNCTION {schema}.reject_execution_reservation_commit()"
            ),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(admin_pool)
                .await
                .unwrap();
        }
    }

    async fn install_recoverable_commit_rejection(admin_pool: &sqlx::PgPool, schema: &str) {
        for statement in [
            format!(
                "CREATE FUNCTION {schema}.reject_recoverable_commit() RETURNS trigger \
                 LANGUAGE plpgsql AS 'BEGIN IF NEW.transition_key = ''recoverable'' THEN \
                 RAISE EXCEPTION ''test recoverable commit rejection''; END IF; \
                 RETURN NEW; END'"
            ),
            format!(
                "CREATE CONSTRAINT TRIGGER reject_recoverable_commit AFTER INSERT ON \
                 {schema}.execution_transaction_transition DEFERRABLE INITIALLY DEFERRED \
                 FOR EACH ROW EXECUTE FUNCTION {schema}.reject_recoverable_commit()"
            ),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(admin_pool)
                .await
                .unwrap();
        }
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
