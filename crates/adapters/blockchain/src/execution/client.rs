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
    collections::{BTreeMap, HashMap, HashSet},
    future::Future,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use alloy::primitives::{Address, U256};
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    factories::OrderEventFactory,
    live::get_runtime,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
    msgbus::{self, MessagingSwitchboard},
};
use nautilus_core::{UnixNanos, time::nanos_since_unix_epoch};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::AccountAny,
    defi::{Pool, PoolIdentifier, SharedChain, validation::validate_address},
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, Venue, VenueOrderId,
    },
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{
        AccountBalance, Currency, MarginBalance, Money, Price, Quantity, fixed::FIXED_PRECISION,
    },
};
use rust_decimal::Decimal;
use tokio::{runtime::RuntimeFlavor, sync::Mutex as AsyncMutex, task::block_in_place};

use crate::{
    config::BlockchainExecutionClientConfig,
    contracts::erc20::Erc20Contract,
    execution::{
        amm::{AmmFill, AmmProtocolAdapter, pancakeswap_v2::PancakeSwapV2Adapter},
        journal::{
            DuplicateSubmitDisposition, JournalEvent, JournalEventStatus, JournalIntentKind,
            OrderIdempotencyKey, append_event_jsonl, classify_duplicate_submit, load_events_jsonl,
            replay_events,
        },
        metadata_store::{InMemoryMetadataStore, MetadataStore},
        signer::{
            RemoteSignerClient, RemoteSignerClientConfig, SignRequest, SignedTx,
            assert_rpc_tx_hash_matches_computed,
        },
        wallet::{WalletTracker, WalletTrackerConfig},
    },
    rpc::http::BlockchainHttpRpcClient,
};

/// Signer boundary used by [`BlockchainExecutionClient`] execution paths.
#[async_trait(?Send)]
pub trait ExecutionTxSigner: std::fmt::Debug {
    async fn sign_evm_tx(&self, request: SignRequest) -> anyhow::Result<SignedTx>;
}

#[async_trait(?Send)]
impl ExecutionTxSigner for RemoteSignerClient {
    async fn sign_evm_tx(&self, request: SignRequest) -> anyhow::Result<SignedTx> {
        <RemoteSignerClient>::sign_evm_tx(self, request)
            .await
            .map_err(anyhow::Error::from)
    }
}

#[derive(Debug, Clone)]
struct ExecutionRuntimeConfig {
    router_address: Address,
    unsupported_tokens: HashSet<Address>,
    default_slippage_bps: u32,
    default_deadline_secs: u64,
    confirmations_required: u64,
    receipt_max_polls: u32,
    receipt_poll_interval: Duration,
    max_inflight_txs_per_wallet: u32,
    require_preapproved_allowance: bool,
    max_fee_per_gas: u64,
    max_priority_fee_per_gas: u64,
    journal_path: Option<PathBuf>,
    signer_wallet_address: Address,
}

#[derive(Debug, Default)]
struct ExecutionRuntimeState {
    journal_events: Vec<JournalEvent>,
    journal_replay: BTreeMap<OrderIdempotencyKey, crate::execution::journal::JournalOrderState>,
    next_sequence: u64,
    order_reports: BTreeMap<ClientOrderId, OrderStatusReport>,
    fill_reports: Vec<FillReport>,
    external_orders: HashMap<ClientOrderId, VenueOrderId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendRawTxErrorKind {
    AlreadyKnown,
    AmbiguousBroadcast,
    Retryable,
    NonceTooLow,
    NonRetryable,
}

enum ConfirmationOutcome {
    Confirmed(nautilus_model::defi::TransactionReceipt),
    ReorgDetected,
    ConfirmationTimeout,
}

enum ReceiptPollOutcome {
    Found(nautilus_model::defi::TransactionReceipt),
    ReceiptTimeout,
}

enum SwapExecutionOutcome {
    Filled {
        venue_order_id: VenueOrderId,
        fill_reports: Vec<FillReport>,
        nonce: u64,
    },
    Pending {
        venue_order_id: VenueOrderId,
    },
}

impl ExecutionRuntimeState {
    fn append_event(
        &mut self,
        journal_path: Option<&PathBuf>,
        event: JournalEvent,
    ) -> anyhow::Result<()> {
        if let Some(path) = journal_path {
            append_event_jsonl(path, &event)?;
        }

        self.journal_events.push(event);
        self.journal_replay = replay_events(&self.journal_events);
        self.next_sequence = self
            .journal_events
            .iter()
            .map(|entry| entry.sequence)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Ok(())
    }
}

/// Execution client for blockchain interactions including balance tracking and order execution.
#[derive(Debug)]
pub struct BlockchainExecutionClient {
    /// Core execution client providing base functionality.
    core: ExecutionClientCore,
    /// Metadata store for token and pool details required during execution.
    metadata_store: Mutex<Box<dyn MetadataStore>>,
    /// The blockchain network configuration.
    chain: SharedChain,
    /// Parsed wallet address for execution and identity checks.
    wallet_address: Address,
    /// Tracks deterministic wallet snapshots and allowance state.
    wallet_tracker: AsyncMutex<WalletTracker>,
    /// Whether connect should refresh wallet state immediately.
    wallet_refresh_on_connect: bool,
    /// Contract interface for ERC-20 token interactions.
    erc20_contract: Erc20Contract,
    /// HTTP RPC client for blockchain queries.
    http_rpc_client: Arc<BlockchainHttpRpcClient>,
    /// Optional runtime execution configuration.
    execution_runtime: Option<ExecutionRuntimeConfig>,
    /// Optional signer implementation used for swap intents.
    tx_signer: Option<Arc<dyn ExecutionTxSigner>>,
    /// Journal/replay state and generated reports for reconciliation APIs.
    execution_state: AsyncMutex<ExecutionRuntimeState>,
    /// Enforces MVP single-inflight submission semantics across threads.
    submit_gate: Mutex<()>,
}

impl BlockchainExecutionClient {
    /// Creates a new [`BlockchainExecutionClient`] instance for the specified configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the wallet address or any token address in the config is invalid.
    pub fn new(
        core_client: ExecutionClientCore,
        config: BlockchainExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        Self::with_metadata_store(core_client, config, Box::new(InMemoryMetadataStore::new()))
    }

    /// Creates a new [`BlockchainExecutionClient`] instance with a caller-supplied metadata store.
    ///
    /// # Errors
    ///
    /// Returns an error if the wallet address or any token address in the config is invalid.
    pub fn with_metadata_store(
        core_client: ExecutionClientCore,
        config: BlockchainExecutionClientConfig,
        metadata_store: Box<dyn MetadataStore>,
    ) -> anyhow::Result<Self> {
        Self::with_metadata_store_internal(core_client, config, metadata_store, None)
    }

    /// Creates a client with a caller-supplied metadata store and signer implementation.
    ///
    /// Intended for deterministic integration tests that need signer injection.
    pub fn with_metadata_store_and_signer(
        core_client: ExecutionClientCore,
        config: BlockchainExecutionClientConfig,
        metadata_store: Box<dyn MetadataStore>,
        signer: Arc<dyn ExecutionTxSigner>,
    ) -> anyhow::Result<Self> {
        Self::with_metadata_store_internal(core_client, config, metadata_store, Some(signer))
    }

    fn with_metadata_store_internal(
        core_client: ExecutionClientCore,
        config: BlockchainExecutionClientConfig,
        metadata_store: Box<dyn MetadataStore>,
        signer_override: Option<Arc<dyn ExecutionTxSigner>>,
    ) -> anyhow::Result<Self> {
        let chain = Arc::new(config.chain.clone());
        let http_rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            config.http_rpc_url.clone(),
            config.rpc_requests_per_second,
        ));
        let wallet_address = validate_address(config.wallet_address.as_str())?;
        let erc20_contract = Erc20Contract::new(http_rpc_client.clone(), true);

        // Initialize token universe so wallet snapshots are deterministic and bounded.
        let mut token_universe = HashSet::new();
        if let Some(specified_tokens) = &config.tokens {
            for token in specified_tokens {
                let token_address = validate_address(token.as_str())?;
                token_universe.insert(token_address);
            }
        }

        for pool in metadata_store.all_pools() {
            token_universe.insert(pool.token0.address);
            token_universe.insert(pool.token1.address);
        }

        if let Some(wnative_address) = &config.wallet_wnative_address {
            let parsed = validate_address(wnative_address.as_str())?;
            token_universe.insert(parsed);
        }

        for token in &config.wallet_extra_tokens {
            let token_address = validate_address(token.as_str())?;
            token_universe.insert(token_address);
        }

        let allowance_spenders: Vec<Address> = config
            .wallet_allowance_spenders
            .iter()
            .map(|address| validate_address(address.as_str()))
            .collect::<anyhow::Result<Vec<_>>>()?;

        let max_batch_size = usize::try_from(config.multicall_max_batch_size)
            .unwrap_or(64)
            .max(1);
        let min_batch_size = usize::try_from(config.multicall_min_batch_size)
            .unwrap_or(4)
            .max(1)
            .min(max_batch_size);
        let max_tokens_per_refresh = usize::try_from(config.wallet_max_tokens_per_refresh)
            .unwrap_or(256)
            .max(1);

        let wallet_tracker_config = WalletTrackerConfig {
            allowance_spenders,
            snapshot_ttl: Duration::from_secs(u64::from(config.wallet_snapshot_ttl_secs.max(1))),
            max_tokens_per_refresh,
            multicall_max_batch_size: max_batch_size,
            multicall_min_batch_size: min_batch_size,
        };
        let wallet_tracker = WalletTracker::new(
            chain.clone(),
            wallet_address,
            token_universe,
            wallet_tracker_config,
        );

        let (execution_runtime, tx_signer) =
            Self::build_execution_runtime(&config, wallet_address, signer_override)?;
        let execution_state = Self::load_execution_state(execution_runtime.as_ref())?;

        Ok(Self {
            core: core_client,
            chain,
            metadata_store: Mutex::new(metadata_store),
            wallet_address,
            wallet_tracker: AsyncMutex::new(wallet_tracker),
            wallet_refresh_on_connect: config.wallet_refresh_on_connect,
            erc20_contract,
            http_rpc_client,
            execution_runtime,
            tx_signer,
            execution_state: AsyncMutex::new(execution_state),
            submit_gate: Mutex::new(()),
        })
    }

    fn build_execution_runtime(
        config: &BlockchainExecutionClientConfig,
        wallet_address: Address,
        signer_override: Option<Arc<dyn ExecutionTxSigner>>,
    ) -> anyhow::Result<(
        Option<ExecutionRuntimeConfig>,
        Option<Arc<dyn ExecutionTxSigner>>,
    )> {
        let signer: Option<Arc<dyn ExecutionTxSigner>> =
            if let Some(override_signer) = signer_override {
                Some(override_signer)
            } else if let Some(endpoint) = &config.signer_endpoint {
                let signer_wallet_address = match &config.signer_wallet_address {
                    Some(address) => validate_address(address.as_str())?,
                    None => wallet_address,
                };

                let mut signer_config =
                    RemoteSignerClientConfig::new(endpoint.clone(), signer_wallet_address);
                signer_config.signer_route = config.signer_route.clone();
                signer_config.signer_timeout_ms = config.signer_timeout_ms;
                signer_config.signer_require_tls = config.signer_require_tls;

                Some(Arc::new(RemoteSignerClient::new(signer_config)?))
            } else {
                None
            };

        if signer.is_none() {
            return Ok((None, None));
        }

        if config.execution_default_slippage_bps > 10_000 {
            anyhow::bail!(
                "execution_default_slippage_bps must be <= 10000, was {}",
                config.execution_default_slippage_bps
            );
        }
        if config.execution_receipt_max_polls == 0 {
            anyhow::bail!("execution_receipt_max_polls must be greater than zero");
        }
        if config.execution_receipt_poll_interval_ms == 0 {
            anyhow::bail!("execution_receipt_poll_interval_ms must be greater than zero");
        }
        if config.execution_max_inflight_txs_per_wallet != 1 {
            anyhow::bail!(
                "MVP execution currently enforces max_inflight_txs_per_wallet=1, received {}",
                config.execution_max_inflight_txs_per_wallet
            );
        }
        if config.execution_max_fee_per_gas == 0 || config.execution_max_priority_fee_per_gas == 0 {
            anyhow::bail!(
                "execution_max_fee_per_gas and execution_max_priority_fee_per_gas must be positive"
            );
        }

        let router_address = config
            .execution_router_address
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "execution_router_address is required when signer-based execution is enabled"
                )
            })
            .and_then(|value| validate_address(value.as_str()))?;

        let unsupported_tokens = config
            .execution_unsupported_token_addresses
            .iter()
            .map(|value| validate_address(value.as_str()))
            .collect::<anyhow::Result<HashSet<_>>>()?;

        let signer_wallet_address = match &config.signer_wallet_address {
            Some(value) => validate_address(value.as_str())?,
            None => wallet_address,
        };
        if signer_wallet_address != wallet_address {
            anyhow::bail!(
                "signer_wallet_address must match wallet_address for PR12b: signer={} wallet={}",
                signer_wallet_address,
                wallet_address
            );
        }

        let runtime = ExecutionRuntimeConfig {
            router_address,
            unsupported_tokens,
            default_slippage_bps: config.execution_default_slippage_bps,
            default_deadline_secs: config.execution_default_deadline_secs,
            confirmations_required: config.execution_confirmations_required.max(1),
            receipt_max_polls: config.execution_receipt_max_polls,
            receipt_poll_interval: Duration::from_millis(config.execution_receipt_poll_interval_ms),
            max_inflight_txs_per_wallet: config.execution_max_inflight_txs_per_wallet,
            require_preapproved_allowance: config.execution_require_preapproved_allowance,
            max_fee_per_gas: config.execution_max_fee_per_gas,
            max_priority_fee_per_gas: config.execution_max_priority_fee_per_gas,
            journal_path: config.execution_journal_path.clone().map(PathBuf::from),
            signer_wallet_address,
        };

        Ok((Some(runtime), signer))
    }

    fn load_execution_state(
        runtime: Option<&ExecutionRuntimeConfig>,
    ) -> anyhow::Result<ExecutionRuntimeState> {
        let mut state = ExecutionRuntimeState {
            next_sequence: 1,
            ..ExecutionRuntimeState::default()
        };

        if let Some(runtime) = runtime
            && let Some(path) = &runtime.journal_path
        {
            let events = load_events_jsonl(path)?;
            state.next_sequence = events
                .iter()
                .map(|event| event.sequence)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            state.journal_replay = replay_events(&events);
            state.journal_events = events;
        }

        Ok(state)
    }

    #[must_use]
    pub fn execution_journal_path(&self) -> Option<&PathBuf> {
        self.execution_runtime
            .as_ref()
            .and_then(|runtime| runtime.journal_path.as_ref())
    }

    async fn refresh_wallet_snapshot(&self, force: bool) -> anyhow::Result<Vec<AccountBalance>> {
        let mut tracker = self.wallet_tracker.lock().await;
        if force || tracker.needs_refresh() {
            let summary = tracker
                .refresh(self.http_rpc_client.as_ref(), &self.erc20_contract)
                .await?;
            log::info!(
                "Wallet snapshot refreshed on {}: tokens={}, spenders={}",
                self.chain.name,
                summary.token_count,
                summary.spender_count
            );
        }
        tracker.account_balances()
    }

    fn refresh_wallet_snapshot_blocking(&self, force: bool) -> anyhow::Result<Vec<AccountBalance>> {
        self.block_on_runtime(self.refresh_wallet_snapshot(force))
    }

    fn block_on_runtime<T>(
        &self,
        fut: impl Future<Output = anyhow::Result<T>>,
    ) -> anyhow::Result<T> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => match handle.runtime_flavor() {
                RuntimeFlavor::CurrentThread => {
                    anyhow::bail!("blocking execution path cannot run on a current-thread runtime")
                }
                _ => block_in_place(|| handle.block_on(fut)),
            },
            Err(_) => get_runtime().block_on(fut),
        }
    }

    fn require_execution_runtime(
        &self,
    ) -> anyhow::Result<(&ExecutionRuntimeConfig, &Arc<dyn ExecutionTxSigner>)> {
        let runtime = self.execution_runtime.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "swap execution is not configured: signer endpoint/signer injection is required"
            )
        })?;

        let signer = self.tx_signer.as_ref().ok_or_else(|| {
            anyhow::anyhow!("swap execution is not configured: missing signer instance")
        })?;

        Ok((runtime, signer))
    }

    async fn append_journal_event(
        &self,
        idempotency_key: OrderIdempotencyKey,
        intent_kind: JournalIntentKind,
        intent_hash: String,
        tx_hash: Option<String>,
        raw_tx_hash: Option<String>,
        reserved_nonce: Option<u64>,
        status: JournalEventStatus,
    ) -> anyhow::Result<()> {
        let runtime = self.execution_runtime.as_ref();
        let mut state = self.execution_state.lock().await;
        let sequence = state.next_sequence;
        let event = JournalEvent {
            sequence,
            ts_event_ns: nanos_since_unix_epoch(),
            idempotency_key,
            intent_kind,
            intent_hash,
            tx_hash,
            raw_tx_hash,
            reserved_nonce,
            status,
        };
        state.append_event(runtime.and_then(|cfg| cfg.journal_path.as_ref()), event)
    }

    async fn submit_order_async(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let (runtime, signer) = self.require_execution_runtime()?;

        if runtime.max_inflight_txs_per_wallet != 1 {
            anyhow::bail!(
                "unsupported max_inflight_txs_per_wallet={} (MVP requires 1)",
                runtime.max_inflight_txs_per_wallet
            );
        }

        if cmd.instrument_id.venue != self.core.venue {
            anyhow::bail!(
                "submit_order rejected: instrument venue {} does not match client venue {}",
                cmd.instrument_id.venue,
                self.core.venue
            );
        }
        if cmd.order_init.order_type != nautilus_model::enums::OrderType::Market {
            anyhow::bail!(
                "submit_order rejected: only market orders are supported in PR12b vertical slice"
            );
        }
        if !runtime.require_preapproved_allowance {
            anyhow::bail!(
                "submit_order rejected: automatic approve flow is not enabled in PR12b; require_preapproved_allowance must be true"
            );
        }

        let idempotency_key = OrderIdempotencyKey::new(
            self.core.venue.to_string(),
            self.wallet_address,
            cmd.client_order_id.to_string(),
        );
        let intent_hash = format!(
            "swap:{}:{}:{}:{}",
            idempotency_key.stable_key(),
            cmd.instrument_id,
            cmd.order_init.order_side,
            cmd.order_init.quantity
        );

        {
            let state = self.execution_state.lock().await;
            match classify_duplicate_submit(state.journal_replay.get(&idempotency_key)) {
                DuplicateSubmitDisposition::New => {}
                DuplicateSubmitDisposition::NoOpInFlight => {
                    log::info!(
                        "submit_order duplicate no-op (in-flight) client_order_id={}",
                        cmd.client_order_id
                    );
                    return Ok(());
                }
                DuplicateSubmitDisposition::RejectTerminal => {
                    anyhow::bail!(
                        "submit_order duplicate rejected: order {} is already terminal",
                        cmd.client_order_id
                    );
                }
            }
        }

        self.append_journal_event(
            idempotency_key.clone(),
            JournalIntentKind::Swap,
            intent_hash.clone(),
            None,
            None,
            None,
            JournalEventStatus::Submitted,
        )
        .await?;

        let outcome = self
            .execute_swap_vertical_slice(cmd, runtime, signer.as_ref(), intent_hash.as_str())
            .await;

        match outcome {
            Ok(SwapExecutionOutcome::Filled {
                venue_order_id,
                fill_reports,
                nonce,
            }) => {
                self.append_journal_event(
                    idempotency_key.clone(),
                    JournalIntentKind::Swap,
                    intent_hash.clone(),
                    Some(venue_order_id.as_str().to_string()),
                    Some(venue_order_id.as_str().to_string()),
                    Some(nonce),
                    JournalEventStatus::Filled,
                )
                .await?;

                let ts_event = UnixNanos::from(nanos_since_unix_epoch());
                let order_report = build_order_report(
                    self.core.account_id,
                    cmd,
                    venue_order_id,
                    OrderStatus::Filled,
                    cmd.order_init.quantity,
                    ts_event,
                );
                let mut state = self.execution_state.lock().await;
                state
                    .order_reports
                    .insert(cmd.client_order_id, order_report);
                state.fill_reports.extend(fill_reports);
                Ok(())
            }
            Ok(SwapExecutionOutcome::Pending { venue_order_id }) => {
                let ts_event = UnixNanos::from(nanos_since_unix_epoch());
                let order_report = build_order_report(
                    self.core.account_id,
                    cmd,
                    venue_order_id,
                    OrderStatus::Accepted,
                    Quantity::zero(cmd.order_init.quantity.precision),
                    ts_event,
                );
                let mut state = self.execution_state.lock().await;
                state
                    .order_reports
                    .insert(cmd.client_order_id, order_report);
                Ok(())
            }
            Err(e) => {
                let rejection_venue_order_id =
                    VenueOrderId::new(format!("reject-{}", cmd.client_order_id.as_str()));
                let ts_event = UnixNanos::from(nanos_since_unix_epoch());
                let order_report = build_order_report(
                    self.core.account_id,
                    cmd,
                    rejection_venue_order_id,
                    OrderStatus::Rejected,
                    Quantity::zero(cmd.order_init.quantity.precision),
                    ts_event,
                );

                self.append_journal_event(
                    idempotency_key,
                    JournalIntentKind::Swap,
                    intent_hash,
                    None,
                    None,
                    None,
                    JournalEventStatus::Rejected,
                )
                .await?;

                let mut state = self.execution_state.lock().await;
                state
                    .order_reports
                    .insert(cmd.client_order_id, order_report);
                Err(e)
            }
        }
    }

    async fn execute_swap_vertical_slice(
        &self,
        cmd: &SubmitOrder,
        runtime: &ExecutionRuntimeConfig,
        signer: &dyn ExecutionTxSigner,
        intent_hash: &str,
    ) -> anyhow::Result<SwapExecutionOutcome> {
        let pool_identifier = PoolIdentifier::new_checked(cmd.instrument_id.symbol.as_str())?;
        let pool = {
            let metadata_store = self.metadata_store.lock().map_err(|e| {
                anyhow::anyhow!("failed to lock metadata store for submit_order: {e}")
            })?;
            metadata_store
                .get_pool(&pool_identifier)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "submit_order rejected: pool {} not present in metadata store",
                        pool_identifier
                    )
                })?
        };

        let adapter = self.build_amm_adapter(runtime, pool.as_ref())?;
        let (path, token_in, token_out) = path_for_side(pool.as_ref(), cmd.order_init.order_side)?;

        let deadline = unix_deadline(runtime.default_deadline_secs)?;
        let deadline_u256 = U256::from(
            u64::try_from(deadline)
                .map_err(|_| anyhow::anyhow!("deadline does not fit into u64: {deadline}"))?,
        );
        let (swap_call, amount_in_for_signer) = match cmd.order_init.order_side {
            OrderSide::Sell => {
                let amount_in =
                    quantity_to_token_amount(cmd.order_init.quantity, token_in.decimals)?;
                if amount_in == U256::ZERO {
                    anyhow::bail!("submit_order rejected: quantity converts to zero token amount");
                }

                let quote_amounts = adapter.quote_exact_in(amount_in, path.clone()).await?;
                if quote_amounts.len() != path.len() {
                    anyhow::bail!(
                        "quote path length mismatch expected={} actual={}",
                        path.len(),
                        quote_amounts.len()
                    );
                }
                let quoted_out = *quote_amounts.last().ok_or_else(|| {
                    anyhow::anyhow!("quote response missing terminal output amount")
                })?;
                let amount_out_min =
                    apply_slippage_floor(quoted_out, runtime.default_slippage_bps)?;

                let swap_call = adapter.build_swap_exact_in_tx(
                    amount_in,
                    amount_out_min,
                    path.clone(),
                    self.wallet_address,
                    deadline_u256,
                )?;
                (swap_call, amount_in)
            }
            OrderSide::Buy => {
                let amount_out =
                    quantity_to_token_amount(cmd.order_init.quantity, token_out.decimals)?;
                if amount_out == U256::ZERO {
                    anyhow::bail!("submit_order rejected: quantity converts to zero token amount");
                }

                let quote_amounts = adapter.quote_exact_out(amount_out, path.clone()).await?;
                if quote_amounts.len() != path.len() {
                    anyhow::bail!(
                        "quote path length mismatch expected={} actual={}",
                        path.len(),
                        quote_amounts.len()
                    );
                }
                let quoted_in = *quote_amounts.first().ok_or_else(|| {
                    anyhow::anyhow!("quote response missing terminal input amount")
                })?;
                let amount_in_max =
                    apply_slippage_ceiling(quoted_in, runtime.default_slippage_bps)?;

                let swap_call = adapter.build_swap_exact_out_tx(
                    amount_out,
                    amount_in_max,
                    path.clone(),
                    self.wallet_address,
                    deadline_u256,
                )?;
                (swap_call, amount_in_max)
            }
            _ => anyhow::bail!(
                "submit_order rejected: unsupported order side {}",
                cmd.order_init.order_side
            ),
        };
        let expected_notional =
            token_amount_to_decimal(amount_in_for_signer, token_in.decimals)?.normalize();

        let estimate_call = serde_json::json!({
            "from": self.wallet_address,
            "to": swap_call.to,
            "data": format!("0x{}", hex::encode(&swap_call.data)),
            "value": format!("0x{:x}", swap_call.value),
        });
        let gas_estimate = self
            .http_rpc_client
            .estimate_gas(estimate_call, Some("latest"))
            .await?;
        let gas_limit = u64::try_from(gas_estimate)
            .map_err(|_| anyhow::anyhow!("estimated gas does not fit in u64: {gas_estimate}"))?;

        let nonce = self
            .http_rpc_client
            .get_transaction_count(&self.wallet_address, Some("pending"))
            .await?;

        let sign_request = SignRequest {
            chain_id: u64::from(self.chain.chain_id),
            nonce,
            to: swap_call.to,
            data: format!("0x{}", hex::encode(&swap_call.data)),
            value: format!("0x{:x}", swap_call.value),
            gas: gas_limit,
            max_fee_per_gas: Some(runtime.max_fee_per_gas),
            max_priority_fee_per_gas: Some(runtime.max_priority_fee_per_gas),
            gas_price: None,
            deadline,
            expected_notional: expected_notional.to_string(),
            expected_selector: selector_hex(&swap_call.data)?,
        };

        let signed = signer.sign_evm_tx(sign_request).await?;
        let rpc_tx_hash = send_raw_transaction_with_policy(
            self.http_rpc_client.as_ref(),
            signed.raw_tx_hex.as_str(),
            signed.tx_hash.as_str(),
            2,
        )
        .await?;
        assert_rpc_tx_hash_matches_computed(&rpc_tx_hash, &signed.tx_hash)?;

        self.append_journal_event(
            OrderIdempotencyKey::new(
                self.core.venue.to_string(),
                self.wallet_address,
                cmd.client_order_id.to_string(),
            ),
            JournalIntentKind::Swap,
            intent_hash.to_string(),
            Some(signed.tx_hash.clone()),
            Some(signed.tx_hash.clone()),
            Some(nonce),
            JournalEventStatus::Accepted,
        )
        .await?;

        let receipt = match poll_for_receipt(
            self.http_rpc_client.as_ref(),
            signed.tx_hash.as_str(),
            runtime.receipt_max_polls,
            runtime.receipt_poll_interval,
        )
        .await?
        {
            ReceiptPollOutcome::Found(receipt) => receipt,
            ReceiptPollOutcome::ReceiptTimeout => {
                log::warn!(
                    "EXEC_ERR[RECEIPT_TIMEOUT] receipt not found within polling budget tx_hash={} polls={}",
                    signed.tx_hash,
                    runtime.receipt_max_polls
                );
                return Ok(SwapExecutionOutcome::Pending {
                    venue_order_id: VenueOrderId::new(signed.tx_hash.clone()),
                });
            }
        };

        if receipt.status != 1 {
            anyhow::bail!(
                "swap receipt status indicates failure status={} tx_hash={}",
                receipt.status,
                signed.tx_hash
            );
        }

        let receipt = match verify_receipt_confirmations(
            self.http_rpc_client.as_ref(),
            signed.tx_hash.as_str(),
            receipt,
            runtime.confirmations_required,
            runtime.receipt_max_polls,
            runtime.receipt_poll_interval,
        )
        .await?
        {
            ConfirmationOutcome::Confirmed(receipt) => receipt,
            ConfirmationOutcome::ReorgDetected => {
                log::warn!(
                    "EXEC_ERR[REORG_DETECTED] receipt disappeared before confirmation threshold tx_hash={} required={}",
                    signed.tx_hash,
                    runtime.confirmations_required
                );
                return Ok(SwapExecutionOutcome::Pending {
                    venue_order_id: VenueOrderId::new(signed.tx_hash.clone()),
                });
            }
            ConfirmationOutcome::ConfirmationTimeout => {
                log::warn!(
                    "EXEC_ERR[CONFIRMATION_TIMEOUT] transaction {} did not reach required confirmations {}",
                    signed.tx_hash,
                    runtime.confirmations_required
                );
                return Ok(SwapExecutionOutcome::Pending {
                    venue_order_id: VenueOrderId::new(signed.tx_hash.clone()),
                });
            }
        };

        verify_receipt_identity(&receipt, self.wallet_address, swap_call.to)?;
        verify_transaction_identity(
            self.http_rpc_client.as_ref(),
            signed.tx_hash.as_str(),
            self.wallet_address,
            swap_call.to,
            nonce,
            self.chain.chain_id,
        )
        .await?;

        let fills = adapter.decode_fills_from_receipt(&receipt, pool.address, path.clone())?;

        let fill_reports = fills
            .into_iter()
            .map(|fill| {
                build_fill_report(
                    self.core.account_id,
                    cmd,
                    fill,
                    pool.token0.decimals,
                    pool.token1.decimals,
                    pool.token1.symbol.as_str(),
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let venue_order_id = VenueOrderId::new(signed.tx_hash.clone());
        Ok(SwapExecutionOutcome::Filled {
            venue_order_id,
            fill_reports,
            nonce,
        })
    }

    fn build_amm_adapter(
        &self,
        runtime: &ExecutionRuntimeConfig,
        pool: &Pool,
    ) -> anyhow::Result<Arc<dyn AmmProtocolAdapter>> {
        match pool.dex.name {
            nautilus_model::defi::DexType::PancakeSwapV2 => {
                let adapter = PancakeSwapV2Adapter::new(
                    self.http_rpc_client.clone(),
                    runtime.router_address,
                    runtime.signer_wallet_address,
                )
                .with_unsupported_tokens(runtime.unsupported_tokens.iter().copied());
                Ok(Arc::new(adapter))
            }
            other => anyhow::bail!(
                "submit_order rejected: unsupported dex {:?} for PR12b vertical slice",
                other
            ),
        }
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
        self.core.cache().account(&self.core.account_id).cloned()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        let factory = OrderEventFactory::new(
            self.core.trader_id,
            self.core.account_id,
            AccountType::Cash,
            self.core.base_currency,
        );
        let account_state =
            factory.generate_account_state(balances, margins, reported, ts_event, ts_event);
        let endpoint = MessagingSwitchboard::portfolio_update_account();
        msgbus::send_account_state(endpoint, &account_state);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }

        self.core.set_started();
        log::info!(
            "Blockchain execution client started: client_id={}, account_id={}, venue={}",
            self.core.client_id,
            self.core.account_id,
            self.core.venue,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        self.core.set_stopped();
        self.core.set_disconnected();
        log::info!(
            "Blockchain execution client stopped: client_id={}",
            self.core.client_id
        );
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let _gate = self
            .submit_gate
            .lock()
            .map_err(|e| anyhow::anyhow!("submit gate poisoned: {e}"))?;
        self.block_on_runtime(self.submit_order_async(cmd))
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        for order_init in &cmd.order_inits {
            let single = SubmitOrder::new(
                cmd.trader_id,
                cmd.client_id,
                cmd.strategy_id,
                cmd.instrument_id,
                order_init.client_order_id,
                order_init.clone(),
                cmd.exec_algorithm_id,
                cmd.position_id,
                cmd.params.clone(),
                cmd.command_id,
                cmd.ts_init,
            );
            self.submit_order(&single)?;
        }
        Ok(())
    }

    fn modify_order(&self, _cmd: &ModifyOrder) -> anyhow::Result<()> {
        anyhow::bail!("modify_order is not supported for blockchain AMM execution")
    }

    fn cancel_order(&self, _cmd: &CancelOrder) -> anyhow::Result<()> {
        anyhow::bail!("cancel_order is not supported for blockchain AMM execution")
    }

    fn cancel_all_orders(&self, _cmd: &CancelAllOrders) -> anyhow::Result<()> {
        anyhow::bail!("cancel_all_orders is not supported for blockchain AMM execution")
    }

    fn batch_cancel_orders(&self, _cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        anyhow::bail!("batch_cancel_orders is not supported for blockchain AMM execution")
    }

    fn query_account(&self, cmd: &QueryAccount) -> anyhow::Result<()> {
        let balances = self.refresh_wallet_snapshot_blocking(true)?;
        self.generate_account_state(balances, Vec::new(), true, cmd.ts_init)?;
        Ok(())
    }

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
        log::info!(
            "query_order is currently read-through via generate_order_status_report: client_order_id={} instrument_id={}",
            cmd.client_order_id,
            cmd.instrument_id
        );
        Ok(())
    }

    fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        _instrument_id: InstrumentId,
        _strategy_id: StrategyId,
        _ts_init: UnixNanos,
    ) {
        let fut = async {
            let mut state = self.execution_state.lock().await;
            state
                .external_orders
                .insert(client_order_id, venue_order_id);
            Ok(())
        };
        let _ = self.block_on_runtime(fut);
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

        if self.wallet_refresh_on_connect {
            let balances = self.refresh_wallet_snapshot(true).await?;
            let ts_event = UnixNanos::from(nanos_since_unix_epoch());
            self.generate_account_state(balances, Vec::new(), false, ts_event)?;
        }

        self.core.set_connected();
        log::info!(
            "Blockchain execution client connected on chain {}",
            self.chain.name
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.core.set_disconnected();
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let state = self.execution_state.lock().await;

        if let Some(client_order_id) = cmd.client_order_id {
            return Ok(state.order_reports.get(&client_order_id).cloned());
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            return Ok(state
                .order_reports
                .values()
                .find(|report| report.venue_order_id == venue_order_id)
                .cloned());
        }

        Ok(state
            .order_reports
            .values()
            .find(|report| {
                cmd.instrument_id
                    .map(|instrument_id| report.instrument_id == instrument_id)
                    .unwrap_or(true)
            })
            .cloned())
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let state = self.execution_state.lock().await;
        let reports = state
            .order_reports
            .values()
            .filter(|report| {
                cmd.instrument_id
                    .map(|instrument_id| report.instrument_id == instrument_id)
                    .unwrap_or(true)
            })
            .filter(|report| {
                if !cmd.open_only {
                    return true;
                }
                !matches!(
                    report.order_status,
                    OrderStatus::Canceled
                        | OrderStatus::Denied
                        | OrderStatus::Expired
                        | OrderStatus::Filled
                        | OrderStatus::Rejected
                )
            })
            .cloned()
            .collect();

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let state = self.execution_state.lock().await;
        let reports = state
            .fill_reports
            .iter()
            .filter(|report| {
                cmd.instrument_id
                    .map(|instrument_id| report.instrument_id == instrument_id)
                    .unwrap_or(true)
            })
            .filter(|report| {
                cmd.venue_order_id
                    .map(|venue_order_id| report.venue_order_id == venue_order_id)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        Ok(Vec::new())
    }

    async fn generate_mass_status(
        &self,
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let state = self.execution_state.lock().await;
        let mut mass = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            self.core.venue,
            UnixNanos::from(nanos_since_unix_epoch()),
            None,
        );
        mass.add_order_reports(state.order_reports.values().cloned().collect());
        mass.add_fill_reports(state.fill_reports.clone());
        Ok(Some(mass))
    }
}

fn path_for_side(
    pool: &Pool,
    side: OrderSide,
) -> anyhow::Result<(
    Vec<Address>,
    &nautilus_model::defi::Token,
    &nautilus_model::defi::Token,
)> {
    match side {
        // Instrument is token0/token1 (base/quote).
        // BUY base: spend quote (token1) to receive base (token0).
        OrderSide::Buy => Ok((
            vec![pool.token1.address, pool.token0.address],
            &pool.token1,
            &pool.token0,
        )),
        // SELL base: spend base (token0) to receive quote (token1).
        OrderSide::Sell => Ok((
            vec![pool.token0.address, pool.token1.address],
            &pool.token0,
            &pool.token1,
        )),
        _ => anyhow::bail!("unsupported order side for AMM execution: {side}"),
    }
}

fn selector_hex(data: &[u8]) -> anyhow::Result<String> {
    if data.len() < 4 {
        anyhow::bail!("encoded calldata is shorter than 4-byte selector");
    }
    Ok(format!("0x{}", hex::encode(&data[..4])))
}

fn quantity_to_token_amount(quantity: Quantity, token_decimals: u8) -> anyhow::Result<U256> {
    decimal_to_token_amount(quantity.as_decimal(), token_decimals)
}

fn decimal_to_token_amount(value: Decimal, token_decimals: u8) -> anyhow::Result<U256> {
    if value.is_sign_negative() {
        anyhow::bail!("token amount cannot be negative");
    }

    let normalized = value.normalize().to_string();
    let (int_part, frac_part) = match normalized.split_once('.') {
        Some((left, right)) => (left, right),
        None => (normalized.as_str(), ""),
    };

    if frac_part.len() > usize::from(token_decimals) {
        anyhow::bail!(
            "amount {} has more fractional precision ({}) than token decimals ({})",
            value,
            frac_part.len(),
            token_decimals
        );
    }

    let mut digits = String::new();
    digits.push_str(int_part);
    digits.push_str(frac_part);
    for _ in frac_part.len()..usize::from(token_decimals) {
        digits.push('0');
    }

    let digits = digits.trim_start_matches('+');
    let digits = if digits.is_empty() { "0" } else { digits };
    U256::from_str_radix(digits, 10)
        .map_err(|e| anyhow::anyhow!("failed to convert {} to token units: {e}", value))
}

fn token_amount_to_quantity(amount: U256, token_decimals: u8) -> anyhow::Result<Quantity> {
    let decimal = token_amount_to_decimal(amount, token_decimals)?;
    let precision = token_decimals.min(FIXED_PRECISION);
    Quantity::from_decimal_dp(decimal, precision)
}

fn token_amount_to_price(
    numerator: U256,
    numerator_decimals: u8,
    denominator: U256,
    denominator_decimals: u8,
) -> anyhow::Result<Price> {
    if denominator.is_zero() {
        return Ok(Price::new(0.0, 0));
    }

    let lhs = token_amount_to_decimal(numerator, numerator_decimals)?;
    let rhs = token_amount_to_decimal(denominator, denominator_decimals)?;
    let value = lhs / rhs;
    Price::from_decimal_dp(value, 9)
}

fn token_amount_to_decimal(amount: U256, token_decimals: u8) -> anyhow::Result<Decimal> {
    let digits = amount.to_string();
    if token_decimals == 0 {
        return Decimal::from_str(digits.as_str())
            .map_err(|e| anyhow::anyhow!("failed parsing integer token amount {}: {e}", amount));
    }

    let scale = usize::from(token_decimals);
    let rendered = if digits.len() > scale {
        let split = digits.len() - scale;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else {
        format!("0.{:0>width$}", digits, width = scale)
    };

    Decimal::from_str(rendered.as_str())
        .map_err(|e| anyhow::anyhow!("failed parsing scaled token amount {}: {e}", rendered))
}

fn apply_slippage_floor(amount: U256, slippage_bps: u32) -> anyhow::Result<U256> {
    if slippage_bps > 10_000 {
        anyhow::bail!("slippage bps out of range: {slippage_bps}");
    }

    let numerator = U256::from(10_000u64.saturating_sub(u64::from(slippage_bps)));
    Ok((amount * numerator) / U256::from(10_000u64))
}

fn apply_slippage_ceiling(amount: U256, slippage_bps: u32) -> anyhow::Result<U256> {
    if slippage_bps > 10_000 {
        anyhow::bail!("slippage bps out of range: {slippage_bps}");
    }

    let numerator = U256::from(10_000u64.saturating_add(u64::from(slippage_bps)));
    let denominator = U256::from(10_000u64);
    let adjusted = (amount * numerator + denominator - U256::from(1u64)) / denominator;
    Ok(adjusted)
}

fn unix_deadline(ttl_secs: u64) -> anyhow::Result<i64> {
    if ttl_secs == 0 {
        anyhow::bail!("deadline ttl must be greater than zero");
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("system clock before unix epoch: {e}"))?
        .as_secs();
    let deadline = now
        .checked_add(ttl_secs)
        .ok_or_else(|| anyhow::anyhow!("deadline overflow for now={now} ttl={ttl_secs}"))?;
    i64::try_from(deadline).map_err(|_| anyhow::anyhow!("deadline does not fit in i64"))
}

async fn send_raw_transaction_with_policy(
    rpc_client: &BlockchainHttpRpcClient,
    raw_tx_hex: &str,
    expected_tx_hash: &str,
    max_attempts: u32,
) -> anyhow::Result<String> {
    let attempts = max_attempts.max(1);
    for attempt in 1..=attempts {
        match rpc_client.send_raw_transaction(raw_tx_hex).await {
            Ok(tx_hash) => return Ok(tx_hash),
            Err(e) => match classify_send_raw_tx_error(&e) {
                SendRawTxErrorKind::AlreadyKnown => {
                    log::warn!(
                        "sendRawTransaction already known; continuing by computed hash tx_hash={expected_tx_hash}"
                    );
                    return Ok(expected_tx_hash.to_string());
                }
                SendRawTxErrorKind::AmbiguousBroadcast => {
                    log::warn!(
                        "sendRawTransaction ambiguous result; continuing by computed hash tx_hash={} error={}",
                        expected_tx_hash,
                        e
                    );
                    return Ok(expected_tx_hash.to_string());
                }
                SendRawTxErrorKind::NonceTooLow => {
                    anyhow::bail!("EXEC_ERR[NONCE_TOO_LOW] sendRawTransaction failed: {e}");
                }
                SendRawTxErrorKind::Retryable => {
                    if attempt < attempts {
                        let backoff_ms = u64::from(attempt) * 50;
                        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    anyhow::bail!(
                        "EXEC_ERR[RPC_RETRY_EXHAUSTED] sendRawTransaction failed after {} attempts: {}",
                        attempts,
                        e
                    );
                }
                SendRawTxErrorKind::NonRetryable => {
                    anyhow::bail!("EXEC_ERR[RPC_NON_RETRYABLE] sendRawTransaction failed: {e}");
                }
            },
        }
    }

    unreachable!("send_raw_transaction_with_policy loop always returns");
}

fn classify_send_raw_tx_error(error: &anyhow::Error) -> SendRawTxErrorKind {
    let lowered = error.to_string().to_ascii_lowercase();

    if lowered.contains("already known")
        || lowered.contains("already imported")
        || (lowered.contains("known transaction") && !lowered.contains("unknown transaction"))
    {
        return SendRawTxErrorKind::AlreadyKnown;
    }
    if lowered.contains("nonce too low") {
        return SendRawTxErrorKind::NonceTooLow;
    }
    if lowered.contains("timeout")
        || lowered.contains("timed out")
        || lowered.contains("deadline exceeded")
    {
        return SendRawTxErrorKind::AmbiguousBroadcast;
    }
    if lowered.contains("http 429")
        || lowered.contains("rpc_code=-32005")
        || lowered.contains("code=-32005")
        || lowered.contains("too many requests")
        || lowered.contains("rate limit")
        || lowered.contains("http 502")
        || lowered.contains("http 503")
        || lowered.contains("http 504")
        || lowered.contains("temporarily unavailable")
        || lowered.contains("connection refused")
        || lowered.contains("connection reset")
    {
        return SendRawTxErrorKind::Retryable;
    }

    SendRawTxErrorKind::NonRetryable
}

async fn poll_for_receipt(
    rpc_client: &BlockchainHttpRpcClient,
    tx_hash: &str,
    max_polls: u32,
    interval: Duration,
) -> anyhow::Result<ReceiptPollOutcome> {
    for attempt in 0..max_polls {
        if let Some(receipt) = rpc_client.get_transaction_receipt(tx_hash).await? {
            return Ok(ReceiptPollOutcome::Found(receipt));
        }

        if attempt + 1 == max_polls {
            break;
        }
        tokio::time::sleep(interval).await;
    }

    Ok(ReceiptPollOutcome::ReceiptTimeout)
}

async fn verify_receipt_confirmations(
    rpc_client: &BlockchainHttpRpcClient,
    tx_hash: &str,
    initial_receipt: nautilus_model::defi::TransactionReceipt,
    confirmations_required: u64,
    max_polls: u32,
    interval: Duration,
) -> anyhow::Result<ConfirmationOutcome> {
    if confirmations_required <= 1 {
        return Ok(ConfirmationOutcome::Confirmed(initial_receipt));
    }

    let mut receipt = initial_receipt;
    let mut consecutive_missing_receipts = 0_u32;
    for attempt in 0..max_polls {
        if attempt > 0 {
            match rpc_client.get_transaction_receipt(tx_hash).await? {
                Some(updated) => {
                    receipt = updated;
                    consecutive_missing_receipts = 0;
                }
                None => {
                    consecutive_missing_receipts = consecutive_missing_receipts.saturating_add(1);
                    if consecutive_missing_receipts >= 2 {
                        return Ok(ConfirmationOutcome::ReorgDetected);
                    }

                    if attempt + 1 == max_polls {
                        break;
                    }
                    tokio::time::sleep(interval).await;
                    continue;
                }
            }
        }

        let latest = rpc_client
            .get_block_by_number(None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("latest block unavailable while confirming tx"))?;
        let confirmations = latest
            .number
            .saturating_sub(receipt.block_number)
            .saturating_add(1);
        if confirmations >= confirmations_required {
            match rpc_client.get_transaction_receipt(tx_hash).await? {
                Some(current) => {
                    consecutive_missing_receipts = 0;
                    let current_confirmations = latest
                        .number
                        .saturating_sub(current.block_number)
                        .saturating_add(1);
                    if current_confirmations >= confirmations_required {
                        return Ok(ConfirmationOutcome::Confirmed(current));
                    }
                    receipt = current;
                }
                None => return Ok(ConfirmationOutcome::ReorgDetected),
            }
        }

        if attempt + 1 == max_polls {
            break;
        }

        tokio::time::sleep(interval).await;
    }

    Ok(ConfirmationOutcome::ConfirmationTimeout)
}

fn verify_receipt_identity(
    receipt: &nautilus_model::defi::TransactionReceipt,
    expected_from: Address,
    expected_to: Address,
) -> anyhow::Result<()> {
    if receipt.from != expected_from {
        anyhow::bail!(
            "receipt sender mismatch expected={} actual={}",
            expected_from,
            receipt.from
        );
    }
    if receipt.to != Some(expected_to) {
        anyhow::bail!(
            "receipt recipient mismatch expected={} actual={:?}",
            expected_to,
            receipt.to
        );
    }

    Ok(())
}

async fn verify_transaction_identity(
    rpc_client: &BlockchainHttpRpcClient,
    tx_hash: &str,
    expected_from: Address,
    expected_to: Address,
    expected_nonce: u64,
    expected_chain_id: u32,
) -> anyhow::Result<()> {
    let tx = rpc_client
        .get_transaction_by_hash(tx_hash)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!("transaction not found by hash {} after receipt", tx_hash)
        })?;

    if tx.from != expected_from {
        anyhow::bail!(
            "transaction sender mismatch expected={} actual={}",
            expected_from,
            tx.from
        );
    }
    if tx.to != Some(expected_to) {
        anyhow::bail!(
            "transaction recipient mismatch expected={} actual={:?}",
            expected_to,
            tx.to
        );
    }
    if tx.nonce != Some(expected_nonce) {
        anyhow::bail!(
            "transaction nonce mismatch expected={} actual={:?}",
            expected_nonce,
            tx.nonce
        );
    }
    if tx.chain.chain_id != expected_chain_id {
        anyhow::bail!(
            "transaction chain_id mismatch expected={} actual={}",
            expected_chain_id,
            tx.chain.chain_id
        );
    }

    Ok(())
}

fn build_order_report(
    account_id: AccountId,
    cmd: &SubmitOrder,
    venue_order_id: VenueOrderId,
    status: OrderStatus,
    filled_qty: Quantity,
    ts_event: UnixNanos,
) -> OrderStatusReport {
    OrderStatusReport::new(
        account_id,
        cmd.instrument_id,
        Some(cmd.client_order_id),
        venue_order_id,
        cmd.order_init.order_side,
        cmd.order_init.order_type,
        cmd.order_init.time_in_force,
        status,
        cmd.order_init.quantity,
        filled_qty,
        ts_event,
        ts_event,
        ts_event,
        None,
    )
}

fn build_fill_report(
    account_id: AccountId,
    cmd: &SubmitOrder,
    fill: AmmFill,
    base_decimals: u8,
    quote_decimals: u8,
    quote_symbol: &str,
) -> anyhow::Result<FillReport> {
    let venue_order_id = VenueOrderId::new(fill.tx_hash.clone());
    let tx_hash_fragment = fill
        .tx_hash
        .strip_prefix("0x")
        .unwrap_or(fill.tx_hash.as_str());
    if tx_hash_fragment.is_empty() {
        anyhow::bail!("fill tx hash is empty");
    }
    let tx_hash_fragment = tx_hash_fragment.get(..8).unwrap_or(tx_hash_fragment);
    let trade_id = TradeId::new(format!("{}-{}", tx_hash_fragment, fill.log_index));

    let (base_qty_amount, quote_qty_amount) = match cmd.order_init.order_side {
        OrderSide::Buy => (fill.amount_out, fill.amount_in),
        OrderSide::Sell => (fill.amount_in, fill.amount_out),
        _ => anyhow::bail!(
            "unsupported order side {} while building fill report",
            cmd.order_init.order_side
        ),
    };
    let last_qty = token_amount_to_quantity(base_qty_amount, base_decimals)?;
    let last_px = token_amount_to_price(
        quote_qty_amount,
        quote_decimals,
        base_qty_amount,
        base_decimals,
    )?;
    let commission_currency = Currency::get_or_create_crypto(quote_symbol);
    let commission = Money::new(0.0, commission_currency);
    let ts_event = UnixNanos::from(nanos_since_unix_epoch());

    Ok(FillReport::new(
        account_id,
        cmd.instrument_id,
        venue_order_id,
        trade_id,
        cmd.order_init.order_side,
        last_qty,
        last_px,
        commission,
        LiquiditySide::Taker,
        Some(cmd.client_order_id),
        None,
        ts_event,
        ts_event,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, sync::Arc};

    use super::{BlockchainExecutionClient, SendRawTxErrorKind, classify_send_raw_tx_error};
    use crate::{
        config::BlockchainExecutionClientConfig,
        execution::metadata_store::{InMemoryMetadataStore, PoolMetadataStore},
    };
    use nautilus_common::cache::Cache;
    use nautilus_core::UnixNanos;
    use nautilus_live::ExecutionClientCore;
    use nautilus_model::{
        defi::{
            AmmType, Dex, DexType, Pool, PoolIdentifier, Token, chain::chains,
            validation::validate_address,
        },
        enums::{AccountType, OmsType},
        identifiers::{AccountId, ClientId, TraderId, Venue},
        stubs::TestDefault,
    };

    fn make_token(address: &str, symbol: &str, decimals: u8) -> Token {
        Token::new(
            Arc::new(chains::BSC.clone()),
            validate_address(address).expect("token address should be valid"),
            symbol.to_string(),
            symbol.to_string(),
            decimals,
        )
    }

    fn make_pool() -> Pool {
        let chain = Arc::new(chains::BSC.clone());
        let pool_address =
            validate_address("0xd13040d4fe917EE704158CfCB3338dCd2838B245").expect("valid pool");

        let dex = Arc::new(Dex::new(
            (*chain).clone(),
            DexType::PancakeSwapV2,
            "0x10ED43C718714eb63d5aA57B78B54704E256024E",
            0,
            AmmType::CPAMM,
            "PairCreated(address,address,address,uint256)",
            "Swap(address,uint256,uint256,uint256,uint256,address)",
            "Mint(address,uint256,uint256)",
            "Burn(address,uint256,uint256,address)",
            "Sync(uint112,uint112)",
        ));

        let token0 = make_token("0x55d398326f99059fF775485246999027B3197955", "USDT", 18);
        let token1 = make_token("0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d", "USDC", 18);

        Pool::new(
            chain,
            dex,
            pool_address,
            PoolIdentifier::from_address(pool_address),
            0,
            token0,
            token1,
            Some(2500),
            None,
            UnixNanos::default(),
        )
    }

    #[test]
    fn test_token_universe_derives_pool_wnative_and_extra_tokens() {
        let mut metadata_store = InMemoryMetadataStore::new();
        let pool = make_pool();
        let pool_token0 = pool.token0.address;
        let pool_token1 = pool.token1.address;
        metadata_store.insert_pool(pool);

        let trader_id = TraderId::test_default();
        let account_id = AccountId::new("BINANCE-001");
        let mut config = BlockchainExecutionClientConfig::new(
            trader_id,
            account_id,
            Venue::new("Bsc:PancakeSwapV2"),
            chains::BSC.clone(),
            String::from("0x1111111111111111111111111111111111111111"),
            Some(vec![String::from(
                "0x0000000000000000000000000000000000000001",
            )]),
            String::from("https://bsc.example.com"),
            None,
        );
        config.wallet_extra_tokens =
            vec![String::from("0x0000000000000000000000000000000000000002")];
        config.wallet_wnative_address =
            Some(String::from("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"));

        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            trader_id,
            ClientId::new("BLOCKCHAIN"),
            config.venue,
            OmsType::Netting,
            account_id,
            AccountType::Cash,
            None,
            cache,
        );

        let client =
            BlockchainExecutionClient::with_metadata_store(core, config, Box::new(metadata_store))
                .expect("client should construct");
        let tracker = nautilus_common::live::get_runtime().block_on(client.wallet_tracker.lock());
        let token_universe = &tracker.wallet_balance().token_universe;

        assert!(token_universe.contains(&pool_token0));
        assert!(token_universe.contains(&pool_token1));
        assert!(token_universe.contains(
            &validate_address("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c").expect("valid wnative")
        ));
        assert!(
            token_universe.contains(
                &validate_address("0x0000000000000000000000000000000000000001")
                    .expect("valid configured token")
            )
        );
        assert!(
            token_universe.contains(
                &validate_address("0x0000000000000000000000000000000000000002")
                    .expect("valid extra token")
            )
        );
        assert_eq!(token_universe.len(), 5);
    }

    #[test]
    fn test_classify_send_raw_tx_error_already_known() {
        let error = anyhow::anyhow!("RPC error code=-32000 message=already known");
        assert_eq!(
            classify_send_raw_tx_error(&error),
            SendRawTxErrorKind::AlreadyKnown
        );
    }

    #[test]
    fn test_classify_send_raw_tx_error_retryable() {
        let error = anyhow::anyhow!(
            "HTTP 503 RPC request failed rpc_code=-32000 rpc_message=temporarily unavailable"
        );
        assert_eq!(
            classify_send_raw_tx_error(&error),
            SendRawTxErrorKind::Retryable
        );
    }

    #[test]
    fn test_classify_send_raw_tx_error_rate_limited_rpc_code_retryable() {
        let error = anyhow::anyhow!("RPC error code=-32005 message=rate limit exceeded data=null");
        assert_eq!(
            classify_send_raw_tx_error(&error),
            SendRawTxErrorKind::Retryable
        );
    }

    #[test]
    fn test_classify_send_raw_tx_error_nonce_too_low() {
        let error = anyhow::anyhow!("RPC error code=-32000 message=nonce too low");
        assert_eq!(
            classify_send_raw_tx_error(&error),
            SendRawTxErrorKind::NonceTooLow
        );
    }

    #[test]
    fn test_classify_send_raw_tx_error_timeout_ambiguous() {
        let error = anyhow::anyhow!("transport request timed out while waiting for provider");
        assert_eq!(
            classify_send_raw_tx_error(&error),
            SendRawTxErrorKind::AmbiguousBroadcast
        );
    }

    #[test]
    fn test_classify_send_raw_tx_error_unknown_transaction_not_already_known() {
        let error = anyhow::anyhow!("RPC error code=-32000 message=unknown transaction");
        assert_eq!(
            classify_send_raw_tx_error(&error),
            SendRawTxErrorKind::NonRetryable
        );
    }
}
