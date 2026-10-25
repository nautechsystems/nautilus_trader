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

use futures_util::FutureExt;
use nautilus_common::clients::ExecutionReportTask;

use super::*;

#[rstest]
#[case::before(-1)]
#[case::exact(0)]
#[case::after(1)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_bulk_report_acceptance_strict_deadline(#[case] offset_ns: i64) {
    let mut node = LiveNode::build("BulkDeadline".to_string(), None).unwrap();
    node.config.timeout_reconciliation = Duration::from_secs(1);
    let finalized = Rc::new(RefCell::new(Vec::new()));
    let second_started = Rc::new(Cell::new(false));
    let release = Rc::new(tokio::sync::Notify::new());
    let mut expected = Vec::new();

    for index in 0..2 {
        let report = deadline_order_report(index);
        expected.push(report.clone());
        node.exec_clients
            .push(LiveExecutionClient::new(Box::new(DeadlineReportClient {
                client_id: ClientId::from(format!("DEADLINE-{index}")),
                report,
                finish_error: false,
                finalized: finalized.clone(),
                inline_release: (index == 1).then(|| release.clone()),
                inline_started: second_started.clone(),
            })));
    }

    let (mut task, deadline) = start_bulk_report_check(&mut node);

    while !second_started.get() {
        assert!(task.future.as_mut().now_or_never().is_none());
        tokio::task::yield_now().await;
    }

    assert_eq!(*finalized.borrow(), Vec::<ClientOrderId>::new());

    let boundary = if offset_ns < 0 {
        deadline.checked_sub(Duration::from_nanos(1)).unwrap()
    } else {
        deadline + Duration::from_nanos(offset_ns.unsigned_abs())
    };

    let remaining = boundary.saturating_duration_since(dst::time::Instant::now());
    #[cfg(all(feature = "simulation", madsim))]
    madsim::time::advance(remaining);
    #[cfg(not(all(feature = "simulation", madsim)))]
    tokio::time::advance(remaining).await;
    assert_eq!(dst::time::Instant::now(), boundary);
    release.notify_one();
    let outcome = task.future.await;
    if offset_ns < 0 {
        let ReportTaskOutcome::Completed(collection) = outcome else {
            panic!("batch before deadline is accepted")
        };

        assert_eq!(*finalized.borrow(), Vec::<ClientOrderId>::new());
        let result = collection.finish();
        assert_eq!(
            result
                .reports
                .iter()
                .map(|sourced| sourced.report.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            result.queried_clients,
            IndexSet::from([ClientId::from("DEADLINE-0"), ClientId::from("DEADLINE-1")])
        );
        assert!(result.failed_clients.is_empty());
        assert_eq!(
            *finalized.borrow(),
            vec![ClientOrderId::from("O-DEADLINE-0")]
        );
    } else {
        assert!(matches!(outcome, ReportTaskOutcome::TimedOut));
        assert_eq!(*finalized.borrow(), Vec::<ClientOrderId>::new());
    }

    let mut errors = Vec::new();
    LiveNode::finish_report_tasks(&node.exec_clients, &mut errors).await;
    assert!(errors.is_empty());
}

#[rstest]
#[case::first(0)]
#[case::last(1)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_bulk_report_acceptance_keeps_healthy_client_after_finish_error(
    #[case] failure: usize,
) {
    let mut node = LiveNode::build("BulkFinishError".to_string(), None).unwrap();
    let finalized = Rc::new(RefCell::new(Vec::new()));

    let expected = [deadline_order_report(0), deadline_order_report(1)];
    for (index, report) in expected.iter().enumerate() {
        node.exec_clients
            .push(LiveExecutionClient::new(Box::new(DeadlineReportClient {
                client_id: ClientId::from(format!("DEADLINE-{index}")),
                report: report.clone(),
                finish_error: index == failure,
                finalized: finalized.clone(),
                inline_release: None,
                inline_started: Rc::new(Cell::new(false)),
            })));
    }

    let (task, _) = start_bulk_report_check(&mut node);

    let ReportTaskOutcome::Completed(collection) = task.future.await else {
        panic!("collection is accepted")
    };

    assert_eq!(*finalized.borrow(), Vec::<ClientOrderId>::new());
    let result = collection.finish();
    let healthy = 1 - failure;
    assert_eq!(
        result.queried_clients,
        IndexSet::from([ClientId::from("DEADLINE-0"), ClientId::from("DEADLINE-1")])
    );
    assert_eq!(
        result.failed_clients,
        IndexSet::from([ClientId::from(format!("DEADLINE-{failure}"))])
    );
    assert_eq!(result.reports.len(), 1);
    assert_eq!(
        result.reports[0].client_id,
        ClientId::from(format!("DEADLINE-{healthy}"))
    );
    assert_eq!(result.reports[0].report, expected[healthy]);
    assert_eq!(
        *finalized.borrow(),
        vec![
            ClientOrderId::from("O-DEADLINE-0"),
            ClientOrderId::from("O-DEADLINE-1")
        ]
    );
}

fn start_bulk_report_check(node: &mut LiveNode) -> (OpenOrderReportTask, dst::time::Instant) {
    let deadline = dst::time::Instant::now() + node.config.timeout_reconciliation;
    let now = dst::time::Instant::now();
    let mut last_inflight = now;
    let mut last_open = now.checked_sub(Duration::from_secs(1)).unwrap();
    let mut last_position = now;
    let mut open_task = None;
    let mut targeted_task = None;
    let mut position_task = None;
    node.run_reconciliation_checks(
        now,
        ReconciliationCheckIntervals {
            inflight: Duration::ZERO,
            open: Duration::from_secs(1),
            position: Duration::ZERO,
        },
        &mut ReconciliationCheckState {
            last_inflight_check: &mut last_inflight,
            last_open_check: &mut last_open,
            last_position_check: &mut last_position,
            open_order_report_task: &mut open_task,
            targeted_order_report_task: &mut targeted_task,
            position_report_task: &mut position_task,
        },
    );

    (open_task.unwrap(), deadline)
}

fn deadline_order_report(index: usize) -> OrderStatusReport {
    OrderStatusReport::new(
        AccountId::from("TEST-001"),
        crypto_perpetual_ethusdt().id(),
        Some(ClientOrderId::from(format!("O-DEADLINE-{index}"))),
        VenueOrderId::from(format!("V-DEADLINE-{index}")),
        Some(OrderSide::Buy),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        Quantity::from("3.0"),
        Quantity::from("0.0"),
        UnixNanos::from(11),
        UnixNanos::from(22),
        UnixNanos::from(33),
        None,
    )
    .with_price(Price::from("100.0"))
}

struct DeadlineReportClient {
    client_id: ClientId,
    report: OrderStatusReport,
    finish_error: bool,
    finalized: Rc<RefCell<Vec<ClientOrderId>>>,
    inline_release: Option<Rc<tokio::sync::Notify>>,
    inline_started: Rc<Cell<bool>>,
}

#[async_trait::async_trait(?Send)]
impl ExecutionClient for DeadlineReportClient {
    fn is_connected(&self) -> bool {
        true
    }

    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn account_id(&self) -> AccountId {
        self.report.account_id
    }

    fn venue(&self) -> Venue {
        self.report.instrument_id.venue
    }

    fn oms_type(&self) -> OmsType {
        OmsType::Hedging
    }

    fn get_account(&self) -> Option<AccountAny> {
        None
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

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn generate_order_status_reports_task(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> Option<ExecutionReportTask<Vec<OrderStatusReport>>> {
        if self.inline_release.is_some() {
            return None;
        }

        let report = self.report.clone();
        let finalized = self.finalized.clone();
        let finish_error = self.finish_error;
        Some(ExecutionReportTask::new(
            async move { Ok(vec![report]) },
            move |reports| {
                finalized
                    .borrow_mut()
                    .extend(reports.iter().filter_map(|report| report.client_order_id));

                if finish_error {
                    anyhow::bail!("finalization rejected");
                }

                Ok(reports)
            },
        ))
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.inline_started.set(true);
        self.inline_release.as_ref().unwrap().notified().await;
        Ok(vec![self.report.clone()])
    }
}

#[derive(Debug)]
struct CarriedReportClient {
    connected: Rc<Cell<bool>>,
    fills_started: std::sync::Arc<std::sync::atomic::AtomicBool>,
    finalized: Rc<RefCell<Vec<ClientOrderId>>>,
}

#[async_trait::async_trait(?Send)]
impl ExecutionClient for CarriedReportClient {
    fn is_connected(&self) -> bool {
        self.connected.get()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        self.connected.set(true);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.connected.set(false);
        Ok(())
    }

    fn client_id(&self) -> ClientId {
        ClientId::from("CARRIED")
    }

    fn account_id(&self) -> AccountId {
        AccountId::from("TEST-001")
    }

    fn venue(&self) -> Venue {
        Venue::from("BINANCE")
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

    fn generate_account_state(
        &self,
        _: Vec<AccountBalance>,
        _: Vec<MarginBalance>,
        _: bool,
        _: UnixNanos,
        _: Option<Params>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn generate_order_status_reports(
        &self,
        _: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        Ok(Vec::new())
    }

    fn generate_order_status_report_task(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> Option<ExecutionReportTask<Option<OrderStatusReport>>> {
        let id = cmd.client_order_id.unwrap();
        let mut report = deadline_order_report(0);
        report.client_order_id = Some(id);
        report.venue_order_id = cmd.venue_order_id.unwrap();
        report.order_status = OrderStatus::Canceled;
        report.quantity = Quantity::from("10.0");
        report.filled_qty = Quantity::from(if id == ClientOrderId::from("O-CARRIED-0") {
            "0.0"
        } else {
            "1.0"
        });

        let finalized = self.finalized.clone();
        Some(ExecutionReportTask::new(
            async move { Ok(Some(report)) },
            move |report| {
                finalized.borrow_mut().push(id);
                Ok(report)
            },
        ))
    }

    fn generate_fill_reports_task(
        &self,
        _: &GenerateFillReports,
    ) -> Option<ExecutionReportTask<Vec<FillReport>>> {
        let started = self.fills_started.clone();
        Some(ExecutionReportTask::new(
            async move {
                started.store(true, std::sync::atomic::Ordering::SeqCst);
                std::future::pending::<anyhow::Result<Vec<FillReport>>>().await
            },
            Ok,
        ))
    }
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_targeted_report_acceptance_cancellation_dispatches_only_before_fatal_error(
    #[values(false, true)] fatal: bool,
) {
    actor::clear_callbacks().unwrap();
    let fills_started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let finalized = Rc::new(RefCell::new(Vec::new()));

    let config = LiveNodeConfig {
        exec_engine: crate::config::LiveExecutionEngineConfig {
            reconciliation: false,
            inflight_check_interval_ms: 0,
            inflight_check_threshold_ms: 0,
            open_check_interval_secs: Some(0.1),
            open_check_lookback_mins: None,
            open_check_threshold_ms: 0,
            open_check_missing_retries: 1,
            open_check_open_only: false,
            max_single_order_queries_per_cycle: 2,
            single_order_query_delay_ms: 0,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        ..Default::default()
    };

    let mut node = LiveNode::build("CarriedFatalBoundary".to_string(), Some(config)).unwrap();

    let client = LiveExecutionClient::new(Box::new(CarriedReportClient {
        connected: Rc::new(Cell::new(false)),
        fills_started: fills_started.clone(),
        finalized: finalized.clone(),
    }));

    node.kernel
        .exec_engine
        .borrow_mut()
        .register_client(Box::new(client.clone()))
        .unwrap();
    node.exec_clients.push(client);
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    node.kernel
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    for index in 0..2 {
        insert_accepted_limit_order_in_node(
            &node,
            AccountId::from("TEST-001"),
            ClientId::from("CARRIED"),
            instrument.id(),
            ClientOrderId::from(format!("O-CARRIED-{index}")),
            VenueOrderId::from(format!("V-CARRIED-{index}")),
        );
    }

    let handle = node.handle();

    let driver = async {
        while !fills_started.load(std::sync::atomic::Ordering::SeqCst) {
            dst::time::sleep(Duration::from_millis(1)).await;
        }

        if fatal {
            crate::dispatch::tests::latch_callback_failure();
        } else {
            handle.stop();
        }
    };

    let (result, ()) = dst::time::timeout(Duration::from_secs(5), async {
        tokio::join!(node.run_with_mode(NodeRunMode::Hosted), driver)
    })
    .await
    .unwrap();

    assert_eq!(result.is_err(), fatal);

    if fatal {
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Callback delivery unwound")
        );
    }

    assert_eq!(
        *finalized.borrow(),
        vec![
            ClientOrderId::from("O-CARRIED-0"),
            ClientOrderId::from("O-CARRIED-1")
        ]
    );
    assert_eq!(
        node.kernel
            .cache
            .borrow()
            .order(&ClientOrderId::from("O-CARRIED-0"))
            .unwrap()
            .status(),
        if fatal {
            OrderStatus::Accepted
        } else {
            OrderStatus::Canceled
        }
    );
    assert_eq!(
        node.kernel
            .cache
            .borrow()
            .order(&ClientOrderId::from("O-CARRIED-1"))
            .unwrap()
            .status(),
        OrderStatus::Accepted
    );
    let id = ClientOrderId::from("O-CARRIED-1");
    node.exec_manager.register_inflight(id);
    #[cfg(all(feature = "simulation", madsim))]
    madsim::time::advance(Duration::from_nanos(1));
    #[cfg(not(all(feature = "simulation", madsim)))]
    tokio::time::advance(Duration::from_nanos(1)).await;
    let check = node.exec_manager.check_inflight_orders();
    assert!(
        check.queries.iter().any(
            |cmd| matches!(cmd,TradingCommand::QueryOrder(query) if query.client_order_id==id)
        )
    );
    node.dispose();
    assert_eq!(actor::callback_failure(), None);
}
