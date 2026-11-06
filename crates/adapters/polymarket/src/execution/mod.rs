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

//! Live execution client implementation for the Polymarket adapter.

pub mod order_builder;
pub mod parse;

pub(crate) mod context;
pub(crate) mod fill_tracker;
pub(crate) mod pending;
pub(crate) mod position_operations;
pub(crate) mod reconciliation;
pub(crate) mod settlement;
pub(crate) mod submitter;
pub(crate) mod types;

mod cancellations;
mod collateral;
mod lifecycle;
mod orders;
mod reports;
mod responses;

use std::sync::{Arc, atomic::AtomicBool};

use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, PositionOperationResult, PositionOperationStartupRecovery,
        PositionOperationStatus, QueryAccount, QueryOrder, QueryPositionOperation, SubmitOrder,
        SubmitOrderList, SubmitPositionOperation,
    },
    msgbus::TypedHandler,
};
use nautilus_core::{
    Params, UUID4, UnixNanos,
    collections::AtomicMap,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter, SocketControl, task::TaskGroup};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, LiquiditySide, OmsType, OrderSide},
    events::{OrderEventAny, PositionEvent},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    position_operation::PositionOperationLeg,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money, Price, Quantity},
};
use nautilus_network::retry::RetryConfig;
use parking_lot::Mutex;
pub(crate) use responses::is_post_only_crossing;
use rust_decimal::Decimal;
use ustr::Ustr;

pub(crate) use self::reports::get_pusd_currency;
use self::{
    context::OrderContextRegistry,
    fill_tracker::OrderFillTrackerMap,
    order_builder::PolymarketOrderBuilder,
    pending::{PendingCancelTracker, PendingSubmitTracker},
    position_operations::{
        CorePositionOperationRecordGate, CorePositionOperationStore,
        core_position_operation_recorder,
    },
    settlement::SettlementRegistry,
    submitter::OrderSubmitter,
};
use crate::{
    common::{
        consts::POLYMARKET_VENUE,
        credential::{RelayerApiKey, Secrets},
        enums::PolymarketSignatureType,
    },
    config::PolymarketExecutionClientConfig,
    http::{
        clob::PolymarketClobHttpClient,
        data_api::PolymarketDataApiHttpClient,
        query::{AssetType, GetBalanceAllowanceParams},
    },
    signing::eip712::{OrderSigner, parse_address},
    websocket::{
        USER_STREAMS_ENDPOINT, client::PolymarketWebSocketClient, dispatch::WsDispatchState,
    },
};

/// Live execution client for the Polymarket prediction market.
#[derive(Debug)]
pub struct PolymarketExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: PolymarketExecutionClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: PolymarketClobHttpClient,
    data_api_client: PolymarketDataApiHttpClient,
    submitter: OrderSubmitter,
    ws_client: PolymarketWebSocketClient,
    secrets: Secrets,
    session_tasks: TaskGroup,
    pending_tasks: TaskGroup,
    shutdown_errors: Vec<String>,
    stopping: Arc<AtomicBool>,
    heartbeat_healthy: Arc<AtomicBool>,
    order_event_handler: Option<TypedHandler<OrderEventAny>>,
    position_event_handler: Option<TypedHandler<PositionEvent>>,
    fill_observer: Option<TypedHandler<OrderEventAny>>,
    void_observer: Option<TypedHandler<OrderEventAny>>,
    decline_observer: Option<TypedHandler<OrderEventAny>>,
    shared_token_instruments: Arc<AtomicMap<Ustr, InstrumentAny>>,
    neg_risk_index: Arc<AtomicMap<InstrumentId, bool>>,
    pending_submits: PendingSubmitTracker,
    pending_cancels: PendingCancelTracker,
    order_contexts: Arc<OrderContextRegistry>,
    collateral_locks: Arc<Mutex<collateral::CollateralLocks>>,
    fill_tracker: Arc<OrderFillTrackerMap>,
    settlement: Arc<SettlementRegistry>,
    ws_dispatch_state: Arc<Mutex<WsDispatchState>>,
    position_client: Option<Arc<crate::positions::PolymarketPositionClient>>,
    startup_recovery: Vec<PositionOperationStartupRecovery>,
}

impl PolymarketExecutionClient {
    /// Creates a new [`PolymarketExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if configuration validation fails, credentials cannot be resolved, or clients
    /// fail to construct.
    pub fn new(
        core: ExecutionClientCore,
        config: PolymarketExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        config.validate()?;

        let proxy_url = config.validated_proxy_url()?;
        config.validate_signer()?;
        let secrets = Secrets::resolve(
            config.private_key.clone(),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.passphrase.clone(),
            config.funder.clone(),
        )
        .context("failed to resolve Polymarket credentials")?;

        let signer_address = secrets.address.clone();
        let maker_address = resolve_maker_address(
            config.signature_type,
            &signer_address,
            secrets.funder.as_deref(),
        )?;
        let http_client = PolymarketClobHttpClient::new_with_proxy(
            secrets.credential.clone(),
            signer_address.clone(),
            config.base_url_http.clone(),
            config.http_timeout_secs,
            proxy_url.clone(),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create CLOB HTTP client")?;

        let data_api_client = PolymarketDataApiHttpClient::new_with_proxy(
            Some(config.data_api_url()),
            config.http_timeout_secs,
            proxy_url.clone(),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create Data API HTTP client")?;

        let order_signer = OrderSigner::new(&secrets.private_key)
            .context("failed to create order signer")?
            .with_signer_type(config.signer_type);

        let order_builder = Arc::new(PolymarketOrderBuilder::new(
            order_signer,
            signer_address,
            maker_address,
            config.signature_type,
        ));

        let retry_config = RetryConfig {
            max_retries: config.max_retries,
            initial_delay_ms: config.retry_delay_initial_ms,
            max_delay_ms: config.retry_delay_max_ms,
            backoff_factor: 2.0,
            jitter_ms: 1_000,
            operation_timeout_ms: Some(config.http_timeout_secs * 1_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let settlement = Arc::new(SettlementRegistry::new(core.account_id));

        let clock = get_atomic_clock_realtime();
        let pusd = get_pusd_currency();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Cash,
            Some(pusd),
        );

        let (relayer_key_var, _) = crate::common::credential::relayer_credential_env_vars();
        let position_operations_enabled = config.relayer_api_key.is_some()
            || config.relayer_api_key_address.is_some()
            || std::env::var_os(relayer_key_var).is_some();

        let position_client = if position_operations_enabled {
            let relayer_key = RelayerApiKey::resolve(
                config.relayer_api_key.clone(),
                config.relayer_api_key_address.clone(),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("failed to resolve Relayer API key for position operations")?;
            let funder = secrets.funder.clone().ok_or_else(|| {
                anyhow::anyhow!("managed position operations require a Deposit Wallet funder")
            })?;
            let mut client = crate::positions::PolymarketPositionClient::new(
                &secrets.private_key,
                &funder,
                relayer_key,
                config.base_url_relayer.clone(),
                config.base_url_http.clone(),
                Some(config.http_timeout_secs),
                proxy_url.clone(),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("failed to create position operation client")?;
            if let Some(rpc_url) = &config.base_url_rpc {
                client = client.with_rpc_url(rpc_url.clone());
            }
            let record_store = Arc::new(CorePositionOperationStore::new(emitter.clone()));
            Some(Arc::new(
                client
                    .with_record_gate(Arc::new(CorePositionOperationRecordGate {
                        emitter: emitter.clone(),
                    }))
                    .with_recorder(core_position_operation_recorder(record_store.clone()))
                    .with_record_store(record_store),
            ))
        } else {
            None
        };

        let submitter = OrderSubmitter::new(
            http_client.clone(),
            order_builder,
            retry_config,
            settlement.clone(),
        );

        let ws_client = PolymarketWebSocketClient::new_user_with_proxy(
            config.base_url_ws.clone(),
            secrets.credential.clone(),
            config.transport_backend,
            proxy_url,
        );

        let ws_client = ws_client.with_socket_control(SocketControl::new(
            core.client_id,
            Some(*POLYMARKET_VENUE),
            USER_STREAMS_ENDPOINT,
        ));

        let session_tasks = TaskGroup::new();
        let pending_tasks = TaskGroup::new();

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            data_api_client,
            submitter,
            ws_client,
            secrets,
            session_tasks,
            pending_tasks,
            shutdown_errors: Vec::new(),
            stopping: Arc::new(AtomicBool::new(false)),
            heartbeat_healthy: Arc::new(AtomicBool::new(true)),
            order_event_handler: None,
            position_event_handler: None,
            fill_observer: None,
            void_observer: None,
            decline_observer: None,
            shared_token_instruments: Arc::new(AtomicMap::new()),
            neg_risk_index: Arc::new(AtomicMap::new()),
            pending_submits: PendingSubmitTracker::default(),
            pending_cancels: PendingCancelTracker::default(),
            order_contexts: Arc::new(OrderContextRegistry::default()),
            collateral_locks: Arc::new(Mutex::new(collateral::CollateralLocks::default())),
            fill_tracker: Arc::new(OrderFillTrackerMap::new()),
            settlement,
            ws_dispatch_state: Arc::new(Mutex::new(WsDispatchState::default())),
            position_client,
            startup_recovery: Vec::new(),
        })
    }

    fn refresh_position_operation_balances(&self, legs: &[PositionOperationLeg]) {
        let mut instruments = legs.iter().map(|leg| leg.instrument_id).collect::<Vec<_>>();
        instruments.sort();
        instruments.dedup();

        let mut requests = vec![GetBalanceAllowanceParams {
            asset_type: Some(AssetType::Collateral),
            token_id: None,
            signature_type: Some(self.config.signature_type),
        }];
        {
            let cache = self.core.cache();
            for instrument_id in instruments {
                let Some(instrument) = cache.instrument(&instrument_id) else {
                    continue;
                };
                let Some(info) = instrument.info() else {
                    continue;
                };
                let Some(token_id) = info.get_str("token_id") else {
                    continue;
                };
                let asset_type = if info.get_str(crate::common::consts::MARKET_VERSION_KEY)
                    == Some(crate::common::consts::MARKET_VERSION_V2)
                {
                    AssetType::ConditionalV2
                } else {
                    AssetType::Conditional
                };
                requests.push(GetBalanceAllowanceParams {
                    asset_type: Some(asset_type),
                    token_id: Some(token_id.to_string()),
                    signature_type: Some(self.config.signature_type),
                });
            }
        }

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let signature_type = self.config.signature_type;
        let collateral_locks = self.collateral_locks.clone();
        let settlement = self.settlement.clone();

        if let Err(e) = self.pending_tasks.spawn(async move {
            for request in requests {
                if let Err(e) = http_client.update_balance_allowance(request).await {
                    log::warn!("Failed to refresh position operation balance cache: {e}");
                }
            }

            if let Err(e) = reports::fetch_and_emit_account_state(
                &http_client,
                &emitter,
                clock,
                signature_type,
                &collateral_locks,
                &settlement,
            )
            .await
            {
                log::warn!("Failed to refresh position operation account state: {e}");
            }
        }) {
            log::error!("Failed to spawn position operation balance refresh: {e}");
        }
    }

    fn check_not_faulted(&self) -> anyhow::Result<()> {
        if let Some(reason) = self.settlement.client_fault_reason() {
            anyhow::bail!("Polymarket execution client is faulted closed until restart: {reason}");
        }

        Ok(())
    }
}

fn resolve_maker_address(
    signature_type: PolymarketSignatureType,
    signer_address: &str,
    funder: Option<&str>,
) -> anyhow::Result<String> {
    let maker_address = match signature_type {
        PolymarketSignatureType::Eoa => funder.unwrap_or(signer_address),
        PolymarketSignatureType::PolyProxy
        | PolymarketSignatureType::PolyGnosisSafe
        | PolymarketSignatureType::Poly1271 => funder.ok_or_else(|| {
            anyhow::anyhow!(
                "Polymarket {signature_type:?} signature type requires a funder wallet address",
            )
        })?,
    };

    let maker_address = parse_address(maker_address.trim(), "funder")?;
    let signer_address = parse_address(signer_address, "signer")?;
    if signature_type == PolymarketSignatureType::Eoa && maker_address != signer_address {
        anyhow::bail!(
            "Polymarket Eoa signature type requires the funder to equal the signing address"
        );
    }

    if signature_type != PolymarketSignatureType::Eoa && maker_address == signer_address {
        anyhow::bail!(
            "Polymarket {signature_type:?} signature type requires a funder distinct from the signing address",
        );
    }

    Ok(format!("{maker_address:#x}"))
}

#[async_trait(?Send)]
impl ExecutionClient for PolymarketExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
            && !self.settlement.client_faulted()
            && (!self.config.heartbeat_enabled
                || self
                    .heartbeat_healthy
                    .load(std::sync::atomic::Ordering::Acquire))
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *POLYMARKET_VENUE
    }

    fn oms_type(&self) -> OmsType {
        OmsType::Netting
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn position_reconciliation_tolerance(&self) -> Decimal {
        crate::common::consts::POSITION_RECONCILIATION_TOLERANCE
    }

    fn allows_reconciliation_overfill(&self, order: &OrderAny, report: &FillReport) -> bool {
        order.order_side() == OrderSide::Buy
            && report.order_side == OrderSide::Buy
            && !order.is_quote_quantity()
            && order
                .venue_order_ids()
                .iter()
                .any(|venue_order_id| **venue_order_id == report.venue_order_id)
    }

    // Redemption, including the venue's automatic redemption of winning tokens, removes a Data
    // API balance without a trade, so a missing balance is not evidence of a flat position.
    fn provides_bulk_position_coverage(&self, _instrument_id: InstrumentId) -> bool {
        false
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
            .emit_account_state(balances, margins, reported, ts_event, info);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.restore_position_operations()?;
        self.start_client();
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.stop_client();
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.reset_client();
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        self.settlement.ensure_submission_allowed()?;
        self.submit_order_command(&cmd)
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        self.settlement.ensure_submission_allowed()?;
        self.submit_order_list_command(&cmd);
        Ok(())
    }

    fn submit_position_operation(&self, cmd: SubmitPositionOperation) -> anyhow::Result<()> {
        self.check_not_faulted()?;
        self.submit_position_operation_command(&cmd)
    }

    fn query_position_operation(&self, cmd: QueryPositionOperation) -> anyhow::Result<()> {
        let Some(position_client) = &self.position_client else {
            anyhow::bail!("managed position operations are not configured");
        };
        let record = position_client
            .position_operation_record(&cmd.operation_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no known record for position operation {}",
                    cmd.operation_id
                )
            })?;

        let status = match record.application_status {
            crate::positions::record::PositionOperationApplicationStatus::Applied => {
                PositionOperationStatus::Applied
            }
            _ => match record.venue_status {
                crate::positions::record::PositionOperationVenueStatus::Prepared
                | crate::positions::record::PositionOperationVenueStatus::Submitted => {
                    PositionOperationStatus::Submitted
                }
                crate::positions::record::PositionOperationVenueStatus::Confirmed => {
                    PositionOperationStatus::Confirmed
                }
                crate::positions::record::PositionOperationVenueStatus::Failed
                | crate::positions::record::PositionOperationVenueStatus::Invalid => {
                    PositionOperationStatus::Failed
                }
                crate::positions::record::PositionOperationVenueStatus::Unknown => {
                    PositionOperationStatus::Unknown
                }
            },
        };

        let ts = self.clock.get_time_ns();
        self.emitter
            .send_position_operation_result(PositionOperationResult::new(
                cmd.trader_id,
                cmd.strategy_id,
                self.core.client_id,
                self.core.account_id,
                cmd.operation_id,
                record.operation,
                status,
                record.instrument_ids,
                record.venue_transaction_id,
                record.transaction_hash,
                record.message,
                None,
                UUID4::new(),
                ts,
                ts,
            ));
        Ok(())
    }

    fn on_position_operation_applied(
        &self,
        operation_id: UUID4,
        legs: &[PositionOperationLeg],
        transaction_hash: Option<&str>,
    ) {
        if let Some(position_client) = &self.position_client {
            position_client.record_operation_applied(operation_id, legs, transaction_hash);
        }
        self.refresh_position_operation_balances(legs);
    }

    async fn prepare_startup_recovery(&mut self) -> anyhow::Result<()> {
        self.prepare_startup_recovery_inner().await?;
        Ok(())
    }

    fn take_startup_recovery(&mut self) -> Vec<PositionOperationStartupRecovery> {
        std::mem::take(&mut self.startup_recovery)
    }

    fn on_position_operation_recovery_unresolved(
        &self,
        operation_id: UUID4,
        venue_status: PositionOperationStatus,
        transaction_id: Option<&str>,
        transaction_hash: Option<&str>,
        message: &str,
    ) {
        if let Some(position_client) = &self.position_client {
            position_client.record_recovery_unresolved(
                operation_id,
                venue_status,
                transaction_id,
                transaction_hash,
                message,
            );
        }
    }

    fn on_position_operation_recovery_failed(
        &self,
        operation_id: UUID4,
        transaction_id: Option<&str>,
        transaction_hash: Option<&str>,
        message: &str,
    ) {
        let Some(position_client) = &self.position_client else {
            return;
        };
        position_client.record_recovery_failed(operation_id, message);

        let Some(record) = position_client.position_operation_record(&operation_id) else {
            return;
        };
        let ts = self.clock.get_time_ns();
        self.emitter
            .send_position_operation_result(PositionOperationResult::new(
                record.trader_id,
                record.strategy_id,
                self.core.client_id,
                record.account_id,
                operation_id,
                record.operation,
                PositionOperationStatus::Failed,
                record.barrier_instrument_ids,
                transaction_id.map(str::to_string),
                transaction_hash.map(str::to_string),
                Some(message.to_string()),
                None,
                UUID4::new(),
                ts,
                ts,
            ));
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        self.check_not_faulted()?;

        if let Err(e) = self.settlement.ensure_submission_allowed() {
            self.emitter.emit_order_modify_denied_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                cmd.venue_order_id,
                &e.to_string(),
                self.clock.get_time_ns(),
                Some(cmd.command_id),
            );
            return Ok(());
        }

        self.modify_order_command(&cmd);
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.check_not_faulted()?;
        self.cancel_order_command(&cmd);
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        self.check_not_faulted()?;
        self.cancel_all_orders_command(&cmd)
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        self.check_not_faulted()?;
        self.batch_cancel_orders_command(&cmd);
        Ok(())
    }

    fn query_account(&self, cmd: QueryAccount) -> anyhow::Result<()> {
        self.query_account_command(cmd);
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        self.query_order_command(&cmd);
        Ok(())
    }

    fn retain_unresolved_submissions(&self) -> bool {
        true
    }

    fn register_external_order(
        &self,
        _client_order_id: ClientOrderId,
        _venue_order_id: VenueOrderId,
        _instrument_id: InstrumentId,
        _strategy_id: StrategyId,
        _ts_init: UnixNanos,
    ) {
    }

    fn on_instrument(&mut self, instrument: InstrumentAny) {
        self.on_instrument_update(&instrument);
    }

    fn calculate_commission(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
    ) -> anyhow::Result<Option<Money>> {
        self.calculate_commission_impl(instrument, last_qty, last_px, liquidity_side)
            .map(Some)
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        self.connect_client().await
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.disconnect_client().await
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        gate_report(
            || {
                self.settlement
                    .ensure_order_status_available(cmd.instrument_id, "order status report")
            },
            Box::pin(self.generate_order_status_report_impl(cmd)),
        )
        .await
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        gate_report(
            || {
                self.settlement
                    .ensure_order_status_available(cmd.instrument_id, "order status reports")
            },
            self.generate_order_status_reports_impl(cmd),
        )
        .await
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let (instrument_id, venue_order_id) = (cmd.instrument_id, cmd.venue_order_id);
        gate_report(
            || match venue_order_id {
                Some(venue_order_id) => self
                    .settlement
                    .ensure_order_resolved(&venue_order_id, "fill reports"),
                None => self
                    .settlement
                    .ensure_resolved(instrument_id, "fill reports"),
            },
            self.generate_fill_reports_impl(cmd),
        )
        .await
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        gate_report(
            || {
                self.settlement
                    .ensure_resolved(cmd.instrument_id, "position status reports")
            },
            self.generate_position_status_reports_impl(cmd),
        )
        .await
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        gate_report(
            || self.settlement.ensure_resolved(None, "mass status"),
            self.generate_mass_status_impl(lookback_mins),
        )
        .await
    }
}

/// Runs a report's venue reads between two settlement gate checks, so evidence that becomes
/// unresolved while the reads are in flight still fails the report.
async fn gate_report<T>(
    gate: impl Fn() -> anyhow::Result<()>,
    report: impl Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    gate()?;
    let report = report.await?;
    gate()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[tokio::test]
    async fn test_gate_report_rechecks_settlement_after_venue_reads() {
        let settlement = SettlementRegistry::new(AccountId::from("POLYMARKET-001"));

        let result = gate_report(|| settlement.ensure_resolved(None, "mass status"), async {
            settlement.quarantine_invalid_trade("trade-quarantined-during-read");
            Ok(())
        })
        .await;

        assert_eq!(
            result.unwrap_err().to_string(),
            "cannot generate mass status: Polymarket settlement registry holds 1 record(s) with \
             unresolved evidence"
        );
    }

    #[rstest]
    #[case(PolymarketSignatureType::PolyProxy)]
    #[case(PolymarketSignatureType::PolyGnosisSafe)]
    #[case(PolymarketSignatureType::Poly1271)]
    fn proxy_signature_types_require_funder(#[case] signature_type: PolymarketSignatureType) {
        let error = resolve_maker_address(signature_type, "0xsigner", None).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("requires a funder wallet address")
        );
    }

    #[rstest]
    #[case(PolymarketSignatureType::PolyProxy)]
    #[case(PolymarketSignatureType::PolyGnosisSafe)]
    #[case(PolymarketSignatureType::Poly1271)]
    fn proxy_signature_types_require_distinct_funder(
        #[case] signature_type: PolymarketSignatureType,
    ) {
        let error = resolve_maker_address(
            signature_type,
            "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd",
            Some("0xABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("requires a funder distinct"));
    }

    #[rstest]
    #[case(None)]
    #[case(Some("0xABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD"))]
    fn eoa_uses_configured_funder_or_signer(#[case] funder: Option<&str>) {
        let maker_address = resolve_maker_address(
            PolymarketSignatureType::Eoa,
            "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd",
            funder,
        )
        .unwrap();

        assert_eq!(maker_address, "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd");
    }

    #[rstest]
    fn test_execution_client_rejects_duplicate_config_before_setup() {
        use std::{cell::RefCell, rc::Rc};

        use nautilus_common::{cache::Cache, config::ConfigError};
        use nautilus_model::{
            enums::{AccountType, OmsType},
            identifiers::{TraderId, Venue},
        };

        use crate::config::PolymarketInstrumentProviderConfig;

        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            ClientId::from("POLYMARKET"),
            Venue::from("POLYMARKET"),
            OmsType::Netting,
            AccountId::from("POLYMARKET-001"),
            AccountType::Cash,
            None,
            Rc::new(RefCell::new(Cache::default())),
        );

        let config = PolymarketExecutionClientConfig {
            instrument_config: Some(PolymarketInstrumentProviderConfig {
                series_ids: Some(vec![1, 1]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let error = PolymarketExecutionClient::new(core, config)
            .expect_err("duplicate config must fail before credentials or client setup");
        assert_eq!(
            error.downcast_ref::<ConfigError>(),
            Some(&ConfigError::duplicate("series_ids", None))
        );
    }

    #[rstest]
    #[case(
        PolymarketSignatureType::Eoa,
        "0x1111111111111111111111111111111111111111",
        "requires the funder to equal"
    )]
    #[case(PolymarketSignatureType::Eoa, "invalid", "Invalid funder address")]
    #[case(
        PolymarketSignatureType::PolyProxy,
        "invalid",
        "Invalid funder address"
    )]
    #[case(
        PolymarketSignatureType::PolyGnosisSafe,
        "invalid",
        "Invalid funder address"
    )]
    #[case(PolymarketSignatureType::Poly1271, "invalid", "Invalid funder address")]
    fn maker_address_rejects_invalid_configuration(
        #[case] signature_type: PolymarketSignatureType,
        #[case] funder: &str,
        #[case] expected: &str,
    ) {
        let error = resolve_maker_address(
            signature_type,
            "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd",
            Some(funder),
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected));
    }
}
