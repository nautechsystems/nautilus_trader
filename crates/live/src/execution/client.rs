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
//! client once and hands cloneable views to both places. Owned report tasks collect on runtime
//! workers, then finish cache-dependent decisions on the core thread. Clients without owned report
//! tasks retain inline collection. Instrument updates are deferred only while an inline request
//! borrows the client and are flushed when that request completes.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    fmt::Debug,
    rc::Rc,
};

use async_trait::async_trait;
use nautilus_common::{
    clients::{ExecutionClient, ExecutionReportTask},
    live::dst::task::JoinHandle,
    messages::execution::{
        BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{Params, UnixNanos};
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
use rust_decimal::Decimal;

use crate::task::{TaskJoinOutcome, TaskSlot};

#[derive(Clone)]
pub(crate) struct LiveExecutionClient {
    client: Rc<RefCell<Box<dyn ExecutionClient>>>,
    pending_instruments: Rc<RefCell<VecDeque<InstrumentAny>>>,
    report_task: Rc<RefCell<TaskSlot<()>>>,
    report_collecting: Rc<Cell<bool>>,
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
            report_task: Rc::new(RefCell::new(TaskSlot::new())),
            report_collecting: Rc::new(Cell::new(false)),
            client_id,
            account_id,
            venue,
            oms_type,
        }
    }

    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "live report polling runs on the single-threaded node runtime"
    )]
    pub(crate) async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let task = self.client.borrow().generate_order_status_report_task(cmd);
        if let Some(task) = task {
            return self.collect_report(task).await;
        }

        log::debug!("{} collects single-order reports inline", self.client_id);
        let _guard = self.reserve_report_task().await?;
        let result = { self.client.borrow().generate_order_status_report(cmd).await };
        self.flush_pending_instruments();
        result
    }

    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "live report polling runs on the single-threaded node runtime"
    )]
    pub(crate) async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let task = self.client.borrow().generate_order_status_reports_task(cmd);
        if let Some(task) = task {
            return self.collect_report(task).await;
        }

        log::debug!("{} collects bulk order reports inline", self.client_id);
        let _guard = self.reserve_report_task().await?;

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
    pub(crate) async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let task = self.client.borrow().generate_fill_reports_task(&cmd);
        if let Some(task) = task {
            return self.collect_report(task).await;
        }

        log::debug!("{} collects fill reports inline", self.client_id);
        let _guard = self.reserve_report_task().await?;
        let result = { self.client.borrow().generate_fill_reports(cmd).await };
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
        let task = self
            .client
            .borrow()
            .generate_position_status_reports_task(cmd);

        if let Some(task) = task {
            return self.collect_report(task).await;
        }

        log::debug!("{} collects position reports inline", self.client_id);
        let _guard = self.reserve_report_task().await?;

        let result = {
            self.client
                .borrow()
                .generate_position_status_reports(cmd)
                .await
        };

        self.flush_pending_instruments();
        result
    }

    async fn collect_report<T>(&self, task: ExecutionReportTask<T>) -> anyhow::Result<T> {
        let _guard = self.reserve_report_task().await?;

        self.report_task.borrow_mut().spawn(task.collection)?;
        self.join_report_task().await?;
        let result = task.result.await;
        self.flush_pending_instruments();
        result
    }

    async fn reserve_report_task(&self) -> anyhow::Result<ReportTaskGuard> {
        anyhow::ensure!(
            !self.report_collecting.get(),
            "{} report collection is already running",
            self.client_id
        );
        self.report_collecting.set(true);

        let guard = ReportTaskGuard {
            task: Rc::clone(&self.report_task),
            collecting: Rc::clone(&self.report_collecting),
        };

        if self.report_task.borrow().is_some() {
            if !self
                .report_task
                .borrow()
                .as_ref()
                .is_some_and(JoinHandle::is_finished)
            {
                anyhow::bail!("{} report collection is still terminating", self.client_id);
            }

            self.join_report_task().await?;
        }

        Ok(guard)
    }

    pub(crate) fn cancel_report_task(&self) {
        self.report_task.borrow_mut().abort();
    }

    pub(crate) async fn join_report_task(&self) -> anyhow::Result<()> {
        let outcome =
            std::future::poll_fn(|context| self.report_task.borrow_mut().poll_join(context)).await;

        match outcome {
            Some(TaskJoinOutcome::Failed(e)) => {
                if e.is_panic() {
                    log::error!("{} report collection worker panicked: {e}", self.client_id);
                }

                Err(e.into())
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn flush_pending_instruments(&self) {
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

struct ReportTaskGuard {
    task: Rc<RefCell<TaskSlot<()>>>,
    collecting: Rc<Cell<bool>>,
}

impl Drop for ReportTaskGuard {
    fn drop(&mut self) {
        self.task.borrow_mut().abort();
        self.collecting.set(false);
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

    fn retain_unresolved_submissions(&self) -> bool {
        self.client.borrow().retain_unresolved_submissions()
    }

    fn position_reconciliation_tolerance(&self) -> Decimal {
        self.client.borrow().position_reconciliation_tolerance()
    }

    fn handles_order_venue(&self, venue: Venue) -> bool {
        self.client.borrow().handles_order_venue(venue)
    }

    fn provides_bulk_position_coverage(&self, instrument_id: InstrumentId) -> bool {
        self.client
            .borrow()
            .provides_bulk_position_coverage(instrument_id)
    }

    fn settles_contract_expirations(&self) -> bool {
        self.client.borrow().settles_contract_expirations()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        self.client
            .borrow()
            .generate_account_state(balances, margins, reported, ts_event, info)
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

    fn batch_modify_orders(&self, cmd: BatchModifyOrders) -> anyhow::Result<()> {
        self.client.borrow().batch_modify_orders(cmd)
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

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        Self::generate_order_status_report(self, cmd).await
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        Self::generate_order_status_reports(self, cmd).await
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        Self::generate_fill_reports(self, cmd).await
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
        let _guard = self.reserve_report_task().await?;
        let result = self
            .client
            .borrow()
            .generate_mass_status(lookback_mins)
            .await;
        self.flush_pending_instruments();
        result
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
    ) -> anyhow::Result<Option<Money>> {
        self.client
            .borrow()
            .calculate_commission(instrument, last_qty, last_px, liquidity_side)
    }
}

#[cfg(test)]
#[cfg(not(madsim))]
mod tests {
    use std::{
        cell::Cell,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread::ThreadId,
        time::Duration,
    };

    use rstest::rstest;

    use super::*;

    struct ReportExecutionClient;

    #[async_trait(?Send)]
    impl ExecutionClient for ReportExecutionClient {
        fn is_connected(&self) -> bool {
            true
        }
        fn client_id(&self) -> ClientId {
            ClientId::from("REPORT")
        }
        fn account_id(&self) -> AccountId {
            AccountId::from("REPORT-001")
        }
        fn venue(&self) -> Venue {
            Venue::from("REPORT")
        }
        fn oms_type(&self) -> OmsType {
            OmsType::Netting
        }
        fn get_account(&self) -> Option<AccountAny> {
            None
        }
        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
        async fn generate_mass_status(
            &self,
            _lookback_mins: Option<u64>,
        ) -> anyhow::Result<Option<ExecutionMassStatus>> {
            std::future::pending().await
        }
        fn generate_account_state(
            &self,
            _balances: Vec<AccountBalance>,
            _margins: Vec<MarginBalance>,
            _reported: bool,
            _ts_event: UnixNanos,
            _info: Option<Params>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn collection_runs_on_worker_and_finishes_with_current_core_state() {
        let client = LiveExecutionClient::new(Box::new(ReportExecutionClient));
        let core_thread = std::thread::current().id();
        let current = Rc::new(Cell::new(11));
        let current_for_finish = Rc::clone(&current);

        let (task, started, gate) = cpu_collection(move |value| {
            assert_eq!(std::thread::current().id(), core_thread);
            Ok(value + current_for_finish.get())
        });

        let mut collection = Box::pin(client.collect_report(task));
        let worker_thread = tokio::select! {
            result = &mut collection => panic!("collection finished before release: {result:?}"),
            started = started => started.unwrap(),
        };
        assert_ne!(worker_thread, core_thread);
        current.set(29);
        gate.release();
        let result = collection.await.unwrap();
        assert_eq!(result, 42);
        assert!(client.report_task.borrow().is_none());
    }

    #[rstest]
    #[case::dropped(false)]
    #[case::timed_out(true)]
    #[tokio::test(flavor = "current_thread")]
    async fn canceled_cpu_collection_stays_owned_and_never_finishes_on_core(
        #[case] timed_out: bool,
    ) {
        let client = LiveExecutionClient::new(Box::new(ReportExecutionClient));
        let finalized = Rc::new(Cell::new(0));
        let finalized_for_task = Rc::clone(&finalized);

        let (task, started, gate) = cpu_collection(move |value| {
            finalized_for_task.set(finalized_for_task.get() + 1);
            Ok(value)
        });

        let mut collection = Box::pin(client.collect_report(task));
        tokio::select! {
            result = &mut collection => panic!("collection finished before release: {result:?}"),
            started = started => { started.unwrap(); },
        }

        if timed_out {
            assert!(
                tokio::time::timeout(Duration::from_millis(10), collection)
                    .await
                    .is_err()
            );
        } else {
            drop(collection);
        }

        assert!(client.report_task.borrow().is_some());
        let rejected = client
            .collect_report(ExecutionReportTask::new(async { Ok(47) }, Ok))
            .await
            .unwrap_err();
        let mass_rejected = client.generate_mass_status(None).await.unwrap_err();
        assert_eq!(
            rejected.to_string(),
            "REPORT report collection is still terminating"
        );
        assert_eq!(mass_rejected.to_string(), rejected.to_string());
        gate.release();
        client.join_report_task().await.unwrap();
        let next = client
            .collect_report(ExecutionReportTask::new(async { Ok(53) }, Ok))
            .await
            .unwrap();
        assert_eq!(finalized.get(), 0);
        assert_eq!(next, 53);
        assert!(client.report_task.borrow().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn collection_panic_is_observed_and_slot_can_be_reused() {
        let client = LiveExecutionClient::new(Box::new(ReportExecutionClient));

        let task = ExecutionReportTask::<usize>::new::<usize, _, _>(
            async { panic!("report decoding failed") },
            Ok,
        );
        let failure = client.collect_report(task).await.unwrap_err();
        assert!(failure.to_string().contains("report decoding failed"));
        assert!(client.report_task.borrow().is_none());
        let next = client
            .collect_report(ExecutionReportTask::new(async { Ok(59) }, Ok))
            .await
            .unwrap();
        assert_eq!(next, 59);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn collection_error_does_not_run_continuation() {
        let client = LiveExecutionClient::new(Box::new(ReportExecutionClient));
        let finalized = Rc::new(Cell::new(false));
        let finalized_for_task = Rc::clone(&finalized);

        let task = ExecutionReportTask::new(
            async { Err::<usize, _>(anyhow::anyhow!("report endpoint unavailable")) },
            move |value| {
                finalized_for_task.set(true);
                Ok(value)
            },
        );

        let failure = client.collect_report(task).await.unwrap_err();
        assert_eq!(failure.to_string(), "report endpoint unavailable");
        assert!(!finalized.get());
        assert!(client.report_task.borrow().is_none());
    }

    #[rstest]
    #[case::dropped(false)]
    #[case::resumed(true)]
    #[tokio::test(flavor = "current_thread")]
    async fn finished_worker_keeps_admission_until_collector_releases(#[case] resume: bool) {
        let client = LiveExecutionClient::new(Box::new(ReportExecutionClient));
        let (task, started, gate) = cpu_collection(Ok);
        let mut collection = Box::pin(client.collect_report(task));
        tokio::select! {
            result = &mut collection => panic!("collection finished before release: {result:?}"),
            started = started => { started.unwrap(); },
        }
        gate.release();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !client.report_task.borrow().as_ref().unwrap().is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let rejected = client
            .collect_report(ExecutionReportTask::new(async { Ok(61) }, Ok))
            .await
            .unwrap_err();

        if resume {
            assert_eq!(collection.await.unwrap(), 13);
        } else {
            drop(collection);
        }

        let next = client
            .collect_report(ExecutionReportTask::new(async { Ok(67) }, Ok))
            .await
            .unwrap();
        assert_eq!(
            rejected.to_string(),
            "REPORT report collection is already running"
        );
        assert_eq!(next, 67);
        assert!(!client.report_collecting.get());
        assert!(client.report_task.borrow().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retained_worker_join_reserves_admission_before_cooperative_yield() {
        let client = LiveExecutionClient::new(Box::new(ReportExecutionClient));
        let (task, started, gate) = cpu_collection(Ok);
        let mut collection = Box::pin(client.collect_report(task));
        tokio::select! {
            result = &mut collection => panic!("collection finished before release: {result:?}"),
            started = started => { started.unwrap(); },
        }
        drop(collection);
        gate.release();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !client.report_task.borrow().as_ref().unwrap().is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let mut reservation = Box::pin(client.reserve_report_task());
        std::future::poll_fn(|context| {
            loop {
                let mut budget = std::pin::pin!(tokio::task::consume_budget());
                if budget.as_mut().poll(context).is_pending() {
                    break;
                }
            }

            assert!(reservation.as_mut().poll(context).is_pending());
            assert!(client.report_collecting.get());
            let mut overlapping =
                Box::pin(client.collect_report(ExecutionReportTask::new(async { Ok(89) }, Ok)));

            let std::task::Poll::Ready(Err(rejected)) = overlapping.as_mut().poll(context) else {
                panic!("overlapping collection was not rejected");
            };

            assert_eq!(
                rejected.to_string(),
                "REPORT report collection is already running"
            );
            std::task::Poll::Ready(())
        })
        .await;

        drop(reservation);
        let next = client
            .collect_report(ExecutionReportTask::new(async { Ok(97) }, Ok))
            .await
            .unwrap();
        assert_eq!(next, 97);
        assert!(!client.report_collecting.get());
        assert!(client.report_task.borrow().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pending_continuation_keeps_admission_until_collector_releases() {
        let client = LiveExecutionClient::new(Box::new(ReportExecutionClient));
        let (started_sender, started) = tokio::sync::oneshot::channel();

        let task = ExecutionReportTask {
            collection: Box::pin(async {}),
            result: Box::pin(async move {
                started_sender.send(()).unwrap();
                std::future::pending::<anyhow::Result<usize>>().await
            }),
        };

        let mut collection = Box::pin(client.collect_report(task));
        tokio::select! {
            result = &mut collection => panic!("continuation unexpectedly returned: {result:?}"),
            started = started => { started.unwrap(); },
        }
        let rejected = client
            .collect_report(ExecutionReportTask::new(async { Ok(71) }, Ok))
            .await
            .unwrap_err();
        drop(collection);
        let next = client
            .collect_report(ExecutionReportTask::new(async { Ok(73) }, Ok))
            .await
            .unwrap();
        assert_eq!(
            rejected.to_string(),
            "REPORT report collection is already running"
        );
        assert_eq!(next, 73);
        assert!(!client.report_collecting.get());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inline_mass_status_keeps_worker_admission_until_collector_releases() {
        let client = LiveExecutionClient::new(Box::new(ReportExecutionClient));
        let mut mass_status = Box::pin(client.generate_mass_status(None));
        std::future::poll_fn(|context| {
            assert!(mass_status.as_mut().poll(context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;

        let rejected = client
            .collect_report(ExecutionReportTask::new(async { Ok(79) }, Ok))
            .await
            .unwrap_err();
        drop(mass_status);
        let next = client
            .collect_report(ExecutionReportTask::new(async { Ok(83) }, Ok))
            .await
            .unwrap();
        assert_eq!(
            rejected.to_string(),
            "REPORT report collection is already running"
        );
        assert_eq!(next, 83);
        assert!(!client.report_collecting.get());
        assert!(client.report_task.borrow().is_none());
    }

    fn cpu_collection(
        finish: impl FnOnce(usize) -> anyhow::Result<usize> + 'static,
    ) -> (
        ExecutionReportTask<usize>,
        tokio::sync::oneshot::Receiver<ThreadId>,
        CpuGate,
    ) {
        let (started_sender, started) = tokio::sync::oneshot::channel();
        let released = Arc::new(AtomicBool::new(false));
        let released_for_worker = Arc::clone(&released);

        let task = ExecutionReportTask::new(
            async move {
                started_sender.send(std::thread::current().id()).unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(5);

                while !released_for_worker.load(Ordering::Acquire)
                    && std::time::Instant::now() < deadline
                {
                    std::hint::spin_loop();
                }

                assert!(released_for_worker.load(Ordering::Acquire));
                Ok(13)
            },
            finish,
        );

        (task, started, CpuGate(released))
    }

    struct CpuGate(Arc<AtomicBool>);

    impl CpuGate {
        fn release(&self) {
            self.0.store(true, Ordering::Release);
        }
    }

    impl Drop for CpuGate {
        fn drop(&mut self) {
            self.release();
        }
    }
}
