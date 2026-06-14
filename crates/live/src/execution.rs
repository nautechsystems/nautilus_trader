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

//! Live execution client facade for sharing adapter clients with the live node.
//!
//! The execution engine stores execution clients as trait objects, but continuous reconciliation
//! also needs to issue bulk report requests from the live node loop. This facade wraps the adapter
//! client once and hands cloneable views to both places, so the live crate can poll reconciliation
//! futures without adding live-only request methods to the shared execution traits or creating a
//! second adapter client instance. Instrument updates are deferred while a client request is in
//! progress and flushed when the request completes.

use std::{cell::RefCell, collections::VecDeque, fmt::Debug, rc::Rc};

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
use nautilus_model::{
    accounts::AccountAny,
    enums::{LiquiditySide, OmsType},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money, Price, Quantity},
};

#[derive(Clone)]
pub(crate) struct LiveExecutionClient {
    client: Rc<RefCell<Box<dyn ExecutionClient>>>,
    pending_instruments: Rc<RefCell<VecDeque<InstrumentAny>>>,
    client_id: ClientId,
    account_id: AccountId,
    venue: Venue,
    oms_type: OmsType,
}

impl Debug for LiveExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(LiveExecutionClient))
            .field("client_id", &self.client_id)
            .field("account_id", &self.account_id)
            .field("venue", &self.venue)
            .field("oms_type", &self.oms_type)
            .finish_non_exhaustive()
    }
}

impl LiveExecutionClient {
    pub(crate) fn new(client: Box<dyn ExecutionClient>) -> Self {
        let client_id = client.client_id();
        let account_id = client.account_id();
        let venue = client.venue();
        let oms_type = client.oms_type();

        Self {
            client: Rc::new(RefCell::new(client)),
            pending_instruments: Rc::new(RefCell::new(VecDeque::new())),
            client_id,
            account_id,
            venue,
            oms_type,
        }
    }

    pub(crate) const fn client_id(&self) -> ClientId {
        self.client_id
    }

    pub(crate) const fn venue(&self) -> Venue {
        self.venue
    }

    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "live report polling runs on the single-threaded node runtime"
    )]
    pub(crate) async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let result = {
            self.client
                .borrow()
                .generate_order_status_reports(cmd)
                .await
        };
        self.flush_pending_instruments();
        result
    }

    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "live report polling runs on the single-threaded node runtime"
    )]
    pub(crate) async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let result = {
            self.client
                .borrow()
                .generate_position_status_reports(cmd)
                .await
        };
        self.flush_pending_instruments();
        result
    }

    fn flush_pending_instruments(&self) {
        let mut pending = self.pending_instruments.borrow_mut();
        if pending.is_empty() {
            return;
        }

        let count = pending.len();
        let mut client = self.client.borrow_mut();
        while let Some(instrument) = pending.pop_front() {
            client.on_instrument(instrument);
        }
        log::debug!("Flushed {count} deferred execution client instrument update(s)");
    }
}

#[async_trait(?Send)]
impl ExecutionClient for LiveExecutionClient {
    fn is_connected(&self) -> bool {
        self.client.borrow().is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn account_id(&self) -> AccountId {
        self.account_id
    }

    fn venue(&self) -> Venue {
        self.venue
    }

    fn oms_type(&self) -> OmsType {
        self.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.client.borrow().get_account()
    }

    fn handles_order_venue(&self, venue: Venue) -> bool {
        self.client.borrow().handles_order_venue(venue)
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.client
            .borrow()
            .generate_account_state(balances, margins, reported, ts_event)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.client.borrow_mut().start()
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.client.borrow_mut().stop()
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.client.borrow_mut().reset()
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.client.borrow_mut().dispose()
    }

    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "client lifecycle is driven on the single-threaded live runtime"
    )]
    async fn connect(&mut self) -> anyhow::Result<()> {
        self.client.borrow_mut().connect().await
    }

    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "client lifecycle is driven on the single-threaded live runtime"
    )]
    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.client.borrow_mut().disconnect().await
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        self.client.borrow().submit_order(cmd)
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        self.client.borrow().submit_order_list(cmd)
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        self.client.borrow().modify_order(cmd)
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.client.borrow().cancel_order(cmd)
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        self.client.borrow().cancel_all_orders(cmd)
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        self.client.borrow().batch_cancel_orders(cmd)
    }

    fn query_account(&self, cmd: QueryAccount) -> anyhow::Result<()> {
        self.client.borrow().query_account(cmd)
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        self.client.borrow().query_order(cmd)
    }

    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "report generation uses a shared client handle while the live loop keeps running"
    )]
    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        self.client.borrow().generate_order_status_report(cmd).await
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        Self::generate_order_status_reports(self, cmd).await
    }

    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "report generation uses a shared client handle while the live loop keeps running"
    )]
    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.client.borrow().generate_fill_reports(cmd).await
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        Self::generate_position_status_reports(self, cmd).await
    }

    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "report generation uses a shared client handle during lifecycle-controlled calls"
    )]
    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        self.client
            .borrow()
            .generate_mass_status(lookback_mins)
            .await
    }

    fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        ts_init: UnixNanos,
    ) {
        self.client.borrow().register_external_order(
            client_order_id,
            venue_order_id,
            instrument_id,
            strategy_id,
            ts_init,
        );
    }

    fn on_instrument(&mut self, instrument: InstrumentAny) {
        let instrument_id = instrument.id();

        match self.client.try_borrow_mut() {
            Ok(mut client) => {
                client.on_instrument(instrument);
            }
            Err(_) => {
                log::debug!(
                    "Deferring execution client instrument update for {instrument_id}: \
                     client request in progress"
                );
                self.pending_instruments.borrow_mut().push_back(instrument);
            }
        }
    }

    fn calculate_commission(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
    ) -> Option<Money> {
        self.client
            .borrow()
            .calculate_commission(instrument, last_qty, last_px, liquidity_side)
    }
}
