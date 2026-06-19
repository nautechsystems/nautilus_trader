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

mod cancellations;
mod lifecycle;
mod orders;
mod reports;
mod responses;

pub mod order_builder;
pub(crate) mod order_fill_tracker;
pub mod parse;
pub(crate) mod reconciliation;
pub(crate) mod submitter;
pub(crate) mod types;

use std::sync::{Arc, Mutex, atomic::AtomicBool};

use ahash::AHashSet;
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    cache::fifo::FifoCacheMap,
    clients::ExecutionClient,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
    msgbus::TypedHandler,
};
use nautilus_core::{
    MUTEX_POISONED, UnixNanos,
    collections::AtomicMap,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
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
use tokio::task::JoinHandle;
use ustr::Ustr;

use self::{
    order_builder::PolymarketOrderBuilder, order_fill_tracker::OrderFillTrackerMap,
    submitter::OrderSubmitter,
};
use crate::{
    common::{consts::POLYMARKET_VENUE, credential::Secrets, enums::SignatureType},
    config::PolymarketExecClientConfig,
    http::{clob::PolymarketClobHttpClient, data_api::PolymarketDataApiHttpClient},
    signing::eip712::OrderSigner,
    websocket::client::PolymarketWebSocketClient,
};

type PendingSubmitMap = Arc<Mutex<FifoCacheMap<VenueOrderId, ClientOrderId, 10_000>>>;
type PendingFillMap = Arc<Mutex<FifoCacheMap<VenueOrderId, Vec<FillReport>, 1_000>>>;
type PendingOrderReportMap = Arc<Mutex<FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>>>;

pub(crate) use self::reports::get_pusd_currency;

/// Live execution client for the Polymarket prediction market.
#[derive(Debug)]
pub struct PolymarketExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: PolymarketExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: PolymarketClobHttpClient,
    data_api_client: PolymarketDataApiHttpClient,
    submitter: OrderSubmitter,
    ws_client: PolymarketWebSocketClient,
    secrets: Secrets,
    pending_tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    stopping: Arc<AtomicBool>,
    ws_stream_handle: Mutex<Option<JoinHandle<()>>>,
    order_event_handler: Option<TypedHandler<OrderEventAny>>,
    position_event_handler: Option<TypedHandler<PositionEvent>>,
    shared_token_instruments: Arc<AtomicMap<Ustr, InstrumentAny>>,
    neg_risk_index: Arc<AtomicMap<InstrumentId, bool>>,
    fill_tracker: Arc<OrderFillTrackerMap>,
    pending_submits: PendingSubmitMap,
    pending_cancels: PendingCancelTracker,
    pending_fills: PendingFillMap,
    pending_order_reports: PendingOrderReportMap,
}

#[derive(Clone, Debug, Default)]
struct PendingCancelTracker {
    client_order_ids: Arc<Mutex<AHashSet<ClientOrderId>>>,
}

impl PendingCancelTracker {
    fn insert(&self, client_order_id: ClientOrderId) {
        self.client_order_ids
            .lock()
            .expect(MUTEX_POISONED)
            .insert(client_order_id);
    }

    fn remove(&self, client_order_id: &ClientOrderId) -> bool {
        self.client_order_ids
            .lock()
            .expect(MUTEX_POISONED)
            .remove(client_order_id)
    }

    fn contains(&self, client_order_id: &ClientOrderId) -> bool {
        self.client_order_ids
            .lock()
            .expect(MUTEX_POISONED)
            .contains(client_order_id)
    }
}

impl PolymarketExecutionClient {
    /// Creates a new [`PolymarketExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if credentials cannot be resolved or clients fail to construct.
    pub fn new(
        core: ExecutionClientCore,
        config: PolymarketExecClientConfig,
    ) -> anyhow::Result<Self> {
        let secrets = Secrets::resolve(
            config.private_key.as_deref(),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.passphrase.clone(),
            config.funder.clone(),
        )
        .context("failed to resolve Polymarket credentials")?;

        let signer_address = secrets.address.clone();
        let maker_address = secrets
            .funder
            .clone()
            .unwrap_or_else(|| signer_address.clone());
        if config.signature_type == SignatureType::Poly1271
            && maker_address.eq_ignore_ascii_case(&signer_address)
        {
            anyhow::bail!(
                "POLY_1271 signature type requires a deposit wallet funder distinct from the signing address"
            );
        }
        let http_client = PolymarketClobHttpClient::new(
            secrets.credential.clone(),
            signer_address.clone(),
            config.base_url_http.clone(),
            config.http_timeout_secs,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create CLOB HTTP client")?;

        let data_api_client =
            PolymarketDataApiHttpClient::new(Some(config.data_api_url()), config.http_timeout_secs)
                .map_err(|e| anyhow::anyhow!("{e}"))
                .context("failed to create Data API HTTP client")?;

        let order_signer =
            OrderSigner::new(&secrets.private_key).context("failed to create order signer")?;
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
        let submitter = OrderSubmitter::new(http_client.clone(), order_builder, retry_config);

        let ws_client = PolymarketWebSocketClient::new_user(
            config.base_url_ws.clone(),
            secrets.credential.clone(),
            config.transport_backend,
        );

        let clock = get_atomic_clock_realtime();
        let pusd = get_pusd_currency();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Cash,
            Some(pusd),
        );

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
            pending_tasks: Arc::new(Mutex::new(Vec::new())),
            stopping: Arc::new(AtomicBool::new(false)),
            ws_stream_handle: Mutex::new(None),
            order_event_handler: None,
            position_event_handler: None,
            shared_token_instruments: Arc::new(AtomicMap::new()),
            neg_risk_index: Arc::new(AtomicMap::new()),
            fill_tracker: Arc::new(OrderFillTrackerMap::new()),
            pending_submits: Arc::new(Mutex::new(FifoCacheMap::default())),
            pending_cancels: PendingCancelTracker::default(),
            pending_fills: Arc::new(Mutex::new(FifoCacheMap::default())),
            pending_order_reports: Arc::new(Mutex::new(FifoCacheMap::default())),
        })
    }
}

#[async_trait(?Send)]
impl ExecutionClient for PolymarketExecutionClient {
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
        *POLYMARKET_VENUE
    }

    fn oms_type(&self) -> OmsType {
        OmsType::Netting
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
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event);
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
        self.submit_order_command(&cmd)
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        self.submit_order_list_command(&cmd);
        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        self.modify_order_command(&cmd);
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.cancel_order_command(&cmd);
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        self.cancel_all_orders_command(&cmd);
        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
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
    ) -> Option<Money> {
        Some(self.calculate_commission_impl(instrument, last_qty, last_px, liquidity_side))
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
        self.generate_order_status_report_impl(cmd).await
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.generate_order_status_reports_impl(cmd).await
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.generate_fill_reports_impl(cmd).await
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        self.generate_position_status_reports_impl(cmd).await
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        self.generate_mass_status_impl(lookback_mins).await
    }
}
