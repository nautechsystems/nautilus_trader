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
pub(crate) mod order_fill_tracker;
pub(crate) mod pending;
pub(crate) mod reconciliation;
pub(crate) mod settlement;
pub(crate) mod submitter;
pub(crate) mod types;

mod cancellations;
mod lifecycle;
mod orders;
mod reports;
mod responses;

use std::sync::{Arc, atomic::AtomicBool};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
    msgbus::TypedHandler,
};
use nautilus_core::{
    Params, UnixNanos,
    collections::AtomicMap,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter, SocketControl, task::TaskGroup};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, LiquiditySide, OmsType},
    events::{OrderEventAny, PositionEvent},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    instruments::InstrumentAny,
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
    order_builder::PolymarketOrderBuilder,
    order_fill_tracker::OrderFillTrackerMap,
    pending::{PendingCancelTracker, PendingSubmitTracker},
    settlement::SettlementRegistry,
    submitter::OrderSubmitter,
};
use crate::{
    common::{consts::POLYMARKET_VENUE, credential::Secrets, enums::PolymarketSignatureType},
    config::PolymarketExecutionClientConfig,
    http::{clob::PolymarketClobHttpClient, data_api::PolymarketDataApiHttpClient},
    signing::eip712::OrderSigner,
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
    order_reservations: Arc<Mutex<AHashMap<ClientOrderId, Money>>>,
    fill_tracker: Arc<OrderFillTrackerMap>,
    settlement: Arc<SettlementRegistry>,
    ws_dispatch_state: Arc<Mutex<WsDispatchState>>,
}

impl PolymarketExecutionClient {
    /// Creates a new [`PolymarketExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if credentials cannot be resolved or clients fail to construct.
    pub fn new(
        core: ExecutionClientCore,
        config: PolymarketExecutionClientConfig,
    ) -> anyhow::Result<Self> {
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

        let clock = get_atomic_clock_realtime();
        let pusd = get_pusd_currency();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Cash,
            Some(pusd),
        );

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
            order_reservations: Arc::new(Mutex::new(AHashMap::new())),
            fill_tracker: Arc::new(OrderFillTrackerMap::new()),
            settlement,
            ws_dispatch_state: Arc::new(Mutex::new(WsDispatchState::default())),
        })
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

    if signature_type != PolymarketSignatureType::Eoa
        && maker_address.eq_ignore_ascii_case(signer_address)
    {
        anyhow::bail!(
            "Polymarket {signature_type:?} signature type requires a funder distinct from the signing address",
        );
    }

    Ok(maker_address.to_string())
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
        self.check_not_faulted()?;
        self.submit_order_command(&cmd)
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        self.check_not_faulted()?;
        self.submit_order_list_command(&cmd);
        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        self.check_not_faulted()?;
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
                    .ensure_resolved(cmd.instrument_id, "order status report")
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
                    .ensure_resolved(cmd.instrument_id, "order status reports")
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
        let error =
            resolve_maker_address(signature_type, "0xsigner", Some("0xSIGNER")).unwrap_err();

        assert!(error.to_string().contains("requires a funder distinct"));
    }

    #[rstest]
    #[case(None, "0xsigner")]
    #[case(Some("0xfunder"), "0xfunder")]
    fn eoa_uses_configured_funder_or_signer(#[case] funder: Option<&str>, #[case] expected: &str) {
        let maker_address =
            resolve_maker_address(PolymarketSignatureType::Eoa, "0xsigner", funder).unwrap();

        assert_eq!(maker_address, expected);
    }
}
