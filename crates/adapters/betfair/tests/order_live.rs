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

//! Live state-changing smoke against Betfair. Ignored: places, replaces, and cancels a real order.

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Context;
use nautilus_betfair::{
    common::{
        consts::{
            BETFAIR_CLIENT_ID, METHOD_CANCEL_ORDERS, METHOD_GET_ACCOUNT_DETAILS,
            METHOD_LIST_CURRENT_ORDERS, METHOD_LIST_MARKET_CATALOGUE, METHOD_PLACE_ORDERS,
            METHOD_REPLACE_ORDERS,
        },
        credential::BetfairCredential,
        enums::{
            BetfairOrderStatus, BetfairOrderType, BetfairSide, ExecutionReportErrorCode,
            ExecutionReportStatus, InstructionReportErrorCode, InstructionReportStatus,
            MarketProjection, MarketSort, MarketStatus, OrderProjection, PersistenceType,
            RunnerStatus,
        },
        parse::{make_customer_order_ref, make_instrument_id},
        types::{BetId, Handicap, MarketId, SelectionId},
    },
    config::BetfairExecConfig,
    factories::BetfairExecutionClientFactory,
    http::{
        client::BetfairHttpClient,
        models::{
            AccountDetailsResponse, CancelExecutionReport, CancelInstruction, CancelOrdersParams,
            CurrentOrderSummaryReport, LimitOrder, ListCurrentOrdersParams,
            ListMarketCatalogueParams, MarketCatalogue, MarketFilter, PlaceExecutionReport,
            PlaceInstruction, PlaceInstructionReport, PlaceOrdersParams, PriceSize,
            ReplaceExecutionReport, ReplaceInstruction, ReplaceOrdersParams,
        },
    },
    provider::{BetfairInstrumentProvider, NavigationFilter},
};
use nautilus_common::{actor::DataActor, enums::Environment, providers::InstrumentProvider};
use nautilus_core::UUID4;
use nautilus_live::{
    config::{LiveExecEngineConfig, LiveRiskEngineConfig},
    node::LiveNode,
};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderFilled,
        OrderModifyRejected, OrderRejected, OrderUpdated,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    instruments::Instrument,
    orders::OrderTestBuilder,
    types::{Currency, Price, Quantity},
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde::Deserialize;
use ustr::Ustr;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveMarketBook {
    market_id: MarketId,
    status: MarketStatus,
    inplay: bool,
    runners: Vec<LiveRunnerBook>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveRunnerBook {
    selection_id: SelectionId,
    handicap: Handicap,
    status: RunnerStatus,
    ex: LiveExchangePrices,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveExchangePrices {
    available_to_lay: Vec<PriceSize>,
}

#[derive(Debug)]
struct LiveTarget {
    market_id: MarketId,
    selection_id: SelectionId,
    handicap: Handicap,
}

#[derive(Debug)]
struct InvalidReplaceObservation {
    report: ReplaceExecutionReport,
    market_id: MarketId,
    selection_id: SelectionId,
    old_bet_id: BetId,
    stake: Decimal,
    customer_ref: String,
}

#[derive(Debug, Clone, Default)]
struct LiveExecutionProbe {
    accepted: Arc<AtomicUsize>,
    updated: Arc<AtomicUsize>,
    canceled: Arc<AtomicUsize>,
    failed: Arc<AtomicBool>,
    bet_ids: Arc<Mutex<HashSet<BetId>>>,
    failure: Arc<Mutex<Option<String>>>,
}

impl LiveExecutionProbe {
    fn record_bet_id(&self, bet_id: Option<BetId>) {
        if let Some(bet_id) = bet_id {
            self.bet_ids.lock().unwrap().insert(bet_id);
        }
    }

    fn fail(&self, reason: impl Into<String>) {
        *self.failure.lock().unwrap() = Some(reason.into());
        self.failed.store(true, Ordering::Release);
    }

    fn finished(&self) -> bool {
        self.canceled.load(Ordering::Acquire) == 1 || self.failed.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
struct LiveExecutionLifecycle {
    core: StrategyCore,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    quantity: Quantity,
    replace_price: Price,
    expect_cancelled_without_replacement: bool,
    probe: LiveExecutionProbe,
}

impl LiveExecutionLifecycle {
    fn new(
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        quantity: Quantity,
        replace_price: Price,
        expect_cancelled_without_replacement: bool,
        probe: LiveExecutionProbe,
    ) -> Self {
        Self {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(StrategyId::from("BETFAIR-LIVE-SMOKE")),
                ..Default::default()
            }),
            instrument_id,
            client_order_id,
            quantity,
            replace_price,
            expect_cancelled_without_replacement,
            probe,
        }
    }
}

impl DataActor for LiveExecutionLifecycle {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("BETFAIR-LIVE-TESTER"))
            .strategy_id(StrategyId::from("BETFAIR-LIVE-SMOKE"))
            .instrument_id(self.instrument_id)
            .client_order_id(self.client_order_id)
            .side(OrderSide::Sell)
            .price(Price::from("990"))
            .quantity(self.quantity)
            .time_in_force(TimeInForce::Day)
            .build();
        self.submit_order(order, None, Some(*BETFAIR_CLIENT_ID), None)
    }
}

nautilus_strategy!(LiveExecutionLifecycle, {
    fn on_order_accepted(&mut self, event: OrderAccepted) {
        self.probe
            .record_bet_id(Some(event.venue_order_id.to_string()));

        if self.probe.accepted.fetch_add(1, Ordering::AcqRel) == 0
            && let Err(e) = self.modify_order(
                event.client_order_id,
                None,
                Some(self.replace_price),
                None,
                Some(*BETFAIR_CLIENT_ID),
                None,
            )
        {
            self.probe.fail(format!("modify_order failed: {e}"));
        }
    }

    fn on_order_updated(&mut self, event: OrderUpdated) {
        self.probe
            .record_bet_id(event.venue_order_id.map(|id| id.to_string()));
        if self.expect_cancelled_without_replacement {
            self.probe.fail(format!(
                "unexpected order update: {}",
                event.client_order_id
            ));
        } else if self.probe.updated.fetch_add(1, Ordering::AcqRel) == 0
            && let Err(e) = self.cancel_order(event.client_order_id, Some(*BETFAIR_CLIENT_ID), None)
        {
            self.probe.fail(format!("cancel_order failed: {e}"));
        }
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) {
        self.probe
            .record_bet_id(event.venue_order_id.map(|id| id.to_string()));
        self.probe.canceled.fetch_add(1, Ordering::AcqRel);
    }

    fn on_order_rejected(&mut self, event: OrderRejected) {
        self.probe.fail(format!("order rejected: {}", event.reason));
    }

    fn on_order_denied(&mut self, event: OrderDenied) {
        self.probe.fail(format!("order denied: {}", event.reason));
    }

    fn on_order_modify_rejected(&mut self, event: OrderModifyRejected) {
        self.probe
            .fail(format!("modify rejected: {}", event.reason));
    }

    fn on_order_cancel_rejected(&mut self, event: OrderCancelRejected) {
        self.probe
            .fail(format!("cancel rejected: {}", event.reason));
    }

    fn on_order_filled(&mut self, event: &OrderFilled) {
        self.probe.fail(format!(
            "live smoke order matched unexpectedly: {} @ {}",
            event.last_qty, event.last_px,
        ));
    }
});

#[rstest]
#[tokio::test]
#[ignore = "places, replaces, and cancels an order on the configured live Betfair account"]
async fn live_limit_order_place_replace_cancel() {
    let credential = BetfairCredential::from_env()
        .expect("BETFAIR_USERNAME, BETFAIR_PASSWORD, and BETFAIR_APP_KEY must be set");
    let client = BetfairHttpClient::new(credential, None, None, None, None, Some(5), Some(20))
        .expect("live HTTP client");
    client.connect().await.expect("Betfair login");

    let customer_order_ref = live_ref();
    let mut known_bet_ids = HashSet::new();
    let smoke_result =
        exercise_order_lifecycle(&client, &customer_order_ref, &mut known_bet_ids).await;
    let cleanup_result = cleanup_orders(
        &client,
        &customer_order_ref,
        &known_bet_ids,
        smoke_result.is_err(),
    )
    .await;
    client.disconnect().await;

    cleanup_result.expect("live smoke cleanup and exposure verification failed");
    smoke_result.expect("live place-replace-cancel smoke failed");
}

#[rstest]
#[tokio::test]
#[ignore = "places an order and attempts an invalid-price replace on the configured live Betfair account"]
async fn live_replace_cancelled_not_placed() {
    let credential = BetfairCredential::from_env()
        .expect("BETFAIR_USERNAME, BETFAIR_PASSWORD, and BETFAIR_APP_KEY must be set");
    let client = BetfairHttpClient::new(credential, None, None, None, None, Some(5), Some(20))
        .expect("live HTTP client");
    client.connect().await.expect("Betfair login");

    let customer_order_ref = live_ref();
    let mut known_bet_ids = HashSet::new();
    let probe_result =
        exercise_invalid_replace(&client, &customer_order_ref, &mut known_bet_ids).await;
    let cleanup_result = cleanup_orders(
        &client,
        &customer_order_ref,
        &known_bet_ids,
        probe_result.is_err(),
    )
    .await;
    client.disconnect().await;

    cleanup_result.expect("live invalid-replace cleanup and exposure verification failed");
    let observation = probe_result.expect("live invalid-price replace request failed");
    let report = observation.report;
    assert_eq!(
        report.customer_ref.as_deref(),
        Some(observation.customer_ref.as_str())
    );
    assert_eq!(report.status, ExecutionReportStatus::Failure);
    assert_eq!(
        report.error_code,
        Some(ExecutionReportErrorCode::BetActionError)
    );
    assert_eq!(report.error_message, None);
    assert_eq!(report.market_id.as_ref(), Some(&observation.market_id));
    let reports = report
        .instruction_reports
        .expect("replace instruction reports");
    assert_eq!(reports.len(), 1);
    let instruction = &reports[0];
    assert_eq!(instruction.status, InstructionReportStatus::Failure);
    assert_eq!(
        instruction.error_code,
        Some(InstructionReportErrorCode::CancelledNotPlaced)
    );
    assert_eq!(instruction.error_message, None);
    let cancel = instruction
        .cancel_instruction_report
        .as_ref()
        .expect("replace cancel report");
    assert_eq!(cancel.status, InstructionReportStatus::Success);
    assert_eq!(cancel.error_code, None);
    assert_eq!(cancel.error_message, None);
    assert_eq!(
        cancel.instruction.as_ref().map(|value| &value.bet_id),
        Some(&observation.old_bet_id)
    );
    assert_eq!(cancel.size_cancelled, Some(observation.stake));
    assert!(cancel.cancelled_date.is_some());
    let place = instruction
        .place_instruction_report
        .as_ref()
        .expect("replace place report");
    assert_eq!(place.status, InstructionReportStatus::Failure);
    assert_eq!(
        place.error_code,
        Some(InstructionReportErrorCode::InvalidOdds)
    );
    assert_eq!(place.error_message, None);
    assert_eq!(place.order_status, None);
    let placed_instruction = place
        .instruction
        .as_ref()
        .expect("replace place instruction");
    assert_eq!(placed_instruction.order_type, BetfairOrderType::Limit);
    assert_eq!(placed_instruction.selection_id, observation.selection_id);
    assert_eq!(placed_instruction.handicap, None);
    assert_eq!(placed_instruction.side, BetfairSide::Back);
    let limit = placed_instruction
        .limit_order
        .as_ref()
        .expect("replace limit order");
    assert_eq!(limit.size, observation.stake);
    assert_eq!(limit.price, Decimal::new(257, 2));
    assert_eq!(limit.persistence_type, Some(PersistenceType::Lapse));
    assert_eq!(limit.time_in_force, None);
    assert_eq!(limit.min_fill_size, None);
    assert_eq!(limit.bet_target_type, None);
    assert_eq!(limit.bet_target_size, None);
    assert!(placed_instruction.limit_on_close_order.is_none());
    assert!(placed_instruction.market_on_close_order.is_none());
    assert_eq!(placed_instruction.customer_order_ref, None);
    assert_eq!(place.bet_id, None);
    assert_eq!(place.placed_date, None);
    assert_eq!(place.average_price_matched, None);
    assert_eq!(place.size_matched, None);
}

#[rstest]
#[case("980", false)]
#[case("2.57", true)]
#[tokio::test]
#[ignore = "runs a production LiveNode and mutates orders on the configured live Betfair account"]
async fn live_execution_client_replace_via_stream(
    #[case] replace_price: &str,
    #[case] expect_cancelled_without_replacement: bool,
) {
    let credential = BetfairCredential::from_env()
        .expect("BETFAIR_USERNAME, BETFAIR_PASSWORD, and BETFAIR_APP_KEY must be set");
    let discovery = Arc::new(
        BetfairHttpClient::new(
            credential.clone(),
            None,
            None,
            None,
            None,
            Some(5),
            Some(20),
        )
        .expect("live discovery client"),
    );
    discovery.connect().await.expect("Betfair discovery login");

    let account: AccountDetailsResponse = discovery
        .send_accounts(METHOD_GET_ACCOUNT_DETAILS, serde_json::json!({}))
        .await
        .expect("getAccountDetails");
    let currency_code = account
        .currency_code
        .expect("account details omitted currencyCode");
    let currency = currency_code
        .parse::<Currency>()
        .expect("account currency must be supported");
    let stake = minimum_stake(currency_code.as_str()).expect("minimum account stake");
    let target = find_unmatched_target(discovery.as_ref())
        .await
        .expect("passive live target");
    let market_id = target.market_id.clone();
    let instrument_id =
        make_instrument_id(market_id.as_str(), target.selection_id, target.handicap);
    let mut provider = BetfairInstrumentProvider::new(
        Arc::clone(&discovery),
        NavigationFilter {
            market_ids: Some(vec![market_id.clone()]),
            ..Default::default()
        },
        currency,
        None,
    );
    provider
        .load_all(None)
        .await
        .expect("load live Betfair instruments");
    let instrument = provider
        .store()
        .list_all()
        .into_iter()
        .find(|instrument| instrument.id() == instrument_id)
        .cloned()
        .expect("selected live instrument must be loaded");
    discovery.disconnect().await;

    let trader_id = TraderId::from("BETFAIR-LIVE-TESTER");
    let account_id = AccountId::from("BETFAIR-001");
    let exec_config = BetfairExecConfig {
        trader_id,
        account_id,
        account_currency: currency_code.to_string(),
        stream_market_ids_filter: Some(vec![market_id.clone()]),
        ignore_external_orders: true,
        calculate_account_state: false,
        reconcile_market_ids_only: true,
        reconcile_market_ids: Some(vec![market_id]),
        ..Default::default()
    };
    let exec_engine_config = LiveExecEngineConfig {
        open_check_interval_secs: Some(5.0),
        position_check_interval_secs: Some(10.0),
        ..Default::default()
    };
    let mut node = LiveNode::builder(trader_id, Environment::Live)
        .expect("live node builder")
        .with_name("BetfairLiveExecutionSmoke".to_string())
        .with_exec_engine_config(exec_engine_config)
        .with_risk_engine_config(LiveRiskEngineConfig {
            bypass: true,
            ..Default::default()
        })
        .add_exec_client(
            None,
            Box::new(BetfairExecutionClientFactory::new()),
            Box::new(exec_config),
        )
        .expect("add Betfair execution client")
        .with_reconciliation(false)
        .with_delay_post_stop_secs(2)
        .build()
        .expect("build live node");
    node.kernel()
        .cache
        .borrow_mut()
        .add_instrument(instrument)
        .expect("cache live instrument");

    let unique = live_ref();
    let client_order_id = ClientOrderId::from(format!("L{}", &unique[..31]));
    let customer_order_ref = make_customer_order_ref(client_order_id.as_str());
    let probe = LiveExecutionProbe::default();
    node.add_strategy(LiveExecutionLifecycle::new(
        instrument_id,
        client_order_id,
        Quantity::from(stake.to_string()),
        Price::from(replace_price),
        expect_cancelled_without_replacement,
        probe.clone(),
    ))
    .expect("add live execution strategy");

    let handle = node.handle();
    let stop_handle = handle.clone();
    let monitor_probe = probe.clone();

    let monitor = tokio::spawn(async move {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !monitor_probe.finished() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        if !monitor_probe.finished() {
            monitor_probe.fail("live execution lifecycle timed out");
        }
        stop_handle.stop();
    });

    let run_result = tokio::time::timeout(Duration::from_secs(75), node.run()).await;
    let _ = monitor.await;

    let known_bet_ids = probe.bet_ids.lock().unwrap().clone();
    let cleanup_client =
        BetfairHttpClient::new(credential, None, None, None, None, Some(5), Some(20))
            .expect("live cleanup client");
    cleanup_client
        .connect()
        .await
        .expect("Betfair cleanup login");
    let cleanup_result = cleanup_orders(
        &cleanup_client,
        &customer_order_ref,
        &known_bet_ids,
        run_result.is_err()
            || run_result.as_ref().is_ok_and(|result| result.is_err())
            || probe.failed.load(Ordering::Acquire),
    )
    .await;
    cleanup_client.disconnect().await;

    cleanup_result.expect("live execution smoke cleanup and exposure verification failed");
    let node_result = run_result.expect("live node did not stop within 75 seconds");
    node_result.expect("live node run failed");
    if let Some(failure) = probe.failure.lock().unwrap().clone() {
        panic!("live execution lifecycle failed: {failure}");
    }
    assert_eq!(probe.accepted.load(Ordering::Acquire), 1);
    if expect_cancelled_without_replacement {
        assert_eq!(probe.updated.load(Ordering::Acquire), 0);
        assert_eq!(probe.canceled.load(Ordering::Acquire), 1);
        assert_eq!(known_bet_ids.len(), 1);
    } else {
        assert_eq!(probe.updated.load(Ordering::Acquire), 1);
        assert_eq!(probe.canceled.load(Ordering::Acquire), 1);
        assert_eq!(
            known_bet_ids.len(),
            2,
            "place and replace bet IDs must differ"
        );
    }
}

async fn exercise_order_lifecycle(
    client: &BetfairHttpClient,
    customer_order_ref: &str,
    known_bet_ids: &mut HashSet<BetId>,
) -> anyhow::Result<()> {
    let account: AccountDetailsResponse = client
        .send_accounts(METHOD_GET_ACCOUNT_DETAILS, serde_json::json!({}))
        .await
        .context("getAccountDetails")?;
    let currency = account
        .currency_code
        .context("account details omitted currencyCode")?;
    let stake = minimum_stake(currency.as_str())?;
    let target = find_unmatched_target(client).await?;

    let place_params = PlaceOrdersParams {
        market_id: target.market_id.clone(),
        instructions: vec![PlaceInstruction {
            order_type: BetfairOrderType::Limit,
            selection_id: target.selection_id,
            handicap: Some(target.handicap),
            side: BetfairSide::Back,
            limit_order: Some(LimitOrder {
                size: stake,
                price: price_passive(),
                persistence_type: Some(PersistenceType::Lapse),
                time_in_force: None,
                min_fill_size: None,
                bet_target_type: None,
                bet_target_size: None,
            }),
            limit_on_close_order: None,
            market_on_close_order: None,
            customer_order_ref: Some(customer_order_ref.to_string()),
        }],
        customer_ref: Some(live_ref()),
        market_version: None,
        customer_strategy_ref: None,
    };
    let place: PlaceExecutionReport = client
        .send_betting_order(METHOD_PLACE_ORDERS, &place_params)
        .await
        .context("placeOrders")?;
    anyhow::ensure!(
        place.status == ExecutionReportStatus::Success,
        "placeOrders returned {:?}: {:?}",
        place.status,
        place.error_code,
    );
    let place_instruction = one_place_instruction(&place)?;
    let old_bet_id = place_instruction
        .bet_id
        .clone()
        .context("successful placeOrders omitted betId")?;
    known_bet_ids.insert(old_bet_id.clone());
    anyhow::ensure!(
        place_instruction.size_matched.unwrap_or(Decimal::ZERO) == Decimal::ZERO,
        "live passive order matched during placement",
    );

    let replace_params = ReplaceOrdersParams {
        market_id: target.market_id.clone(),
        instructions: vec![ReplaceInstruction {
            bet_id: old_bet_id,
            new_price: price_replace(),
        }],
        customer_ref: Some(live_ref()),
        market_version: None,
    };
    let replace: ReplaceExecutionReport = client
        .send_betting_order(METHOD_REPLACE_ORDERS, &replace_params)
        .await
        .context("replaceOrders")?;
    anyhow::ensure!(
        replace.status == ExecutionReportStatus::Success,
        "replaceOrders returned {:?}: {:?}",
        replace.status,
        replace.error_code,
    );
    let replace_instruction = replace
        .instruction_reports
        .as_deref()
        .filter(|reports| reports.len() == 1)
        .and_then(|reports| reports.first())
        .context("replaceOrders did not return one instruction report")?;
    anyhow::ensure!(
        replace_instruction.status == InstructionReportStatus::Success,
        "replace instruction returned {:?}: {:?}",
        replace_instruction.status,
        replace_instruction.error_code,
    );
    anyhow::ensure!(
        replace_instruction
            .cancel_instruction_report
            .as_ref()
            .is_some_and(|report| report.status == InstructionReportStatus::Success),
        "replace cancel phase did not succeed",
    );
    let replacement = replace_instruction
        .place_instruction_report
        .as_ref()
        .context("replace report omitted place phase")?;
    anyhow::ensure!(
        replacement.status == InstructionReportStatus::Success,
        "replace place phase returned {:?}: {:?}",
        replacement.status,
        replacement.error_code,
    );
    let new_bet_id = replacement
        .bet_id
        .clone()
        .context("replace place phase omitted betId")?;
    known_bet_ids.insert(new_bet_id.clone());
    anyhow::ensure!(
        replacement.size_matched.unwrap_or(Decimal::ZERO) == Decimal::ZERO,
        "live passive order matched during replacement",
    );

    let cancel_params = CancelOrdersParams {
        market_id: Some(target.market_id),
        instructions: Some(vec![CancelInstruction {
            bet_id: new_bet_id,
            size_reduction: None,
        }]),
        customer_ref: Some(live_ref()),
    };
    let cancel: CancelExecutionReport = client
        .send_betting_order(METHOD_CANCEL_ORDERS, &cancel_params)
        .await
        .context("cancelOrders")?;
    anyhow::ensure!(
        cancel.status == ExecutionReportStatus::Success,
        "cancelOrders returned {:?}: {:?}",
        cancel.status,
        cancel.error_code,
    );
    let cancel_instruction = cancel
        .instruction_reports
        .as_deref()
        .filter(|reports| reports.len() == 1)
        .and_then(|reports| reports.first())
        .context("cancelOrders did not return one instruction report")?;
    anyhow::ensure!(
        cancel_instruction.status == InstructionReportStatus::Success
            || cancel_instruction.error_code == Some(InstructionReportErrorCode::BetTakenOrLapsed),
        "cancel instruction returned {:?}: {:?}",
        cancel_instruction.status,
        cancel_instruction.error_code,
    );

    Ok(())
}

async fn exercise_invalid_replace(
    client: &BetfairHttpClient,
    customer_order_ref: &str,
    known_bet_ids: &mut HashSet<BetId>,
) -> anyhow::Result<InvalidReplaceObservation> {
    let account: AccountDetailsResponse = client
        .send_accounts(METHOD_GET_ACCOUNT_DETAILS, serde_json::json!({}))
        .await
        .context("getAccountDetails")?;
    let currency = account
        .currency_code
        .context("account details omitted currencyCode")?;
    let stake = minimum_stake(currency.as_str())?;
    let target = find_unmatched_target(client).await?;

    let place_params = PlaceOrdersParams {
        market_id: target.market_id.clone(),
        instructions: vec![PlaceInstruction {
            order_type: BetfairOrderType::Limit,
            selection_id: target.selection_id,
            handicap: Some(target.handicap),
            side: BetfairSide::Back,
            limit_order: Some(LimitOrder {
                size: stake,
                price: price_passive(),
                persistence_type: Some(PersistenceType::Lapse),
                time_in_force: None,
                min_fill_size: None,
                bet_target_type: None,
                bet_target_size: None,
            }),
            limit_on_close_order: None,
            market_on_close_order: None,
            customer_order_ref: Some(customer_order_ref.to_string()),
        }],
        customer_ref: Some(live_ref()),
        market_version: None,
        customer_strategy_ref: None,
    };
    let place: PlaceExecutionReport = client
        .send_betting_order(METHOD_PLACE_ORDERS, &place_params)
        .await
        .context("placeOrders")?;
    let old_bet_id = one_place_instruction(&place)?
        .bet_id
        .clone()
        .context("successful placeOrders omitted betId")?;
    known_bet_ids.insert(old_bet_id.clone());

    let customer_ref = live_ref();
    let replace_params = ReplaceOrdersParams {
        market_id: target.market_id.clone(),
        instructions: vec![ReplaceInstruction {
            bet_id: old_bet_id.clone(),
            new_price: Decimal::new(257, 2),
        }],
        customer_ref: Some(customer_ref.clone()),
        market_version: None,
    };
    let report = client
        .send_betting_order(METHOD_REPLACE_ORDERS, &replace_params)
        .await
        .context("replaceOrders")?;

    Ok(InvalidReplaceObservation {
        report,
        market_id: target.market_id,
        selection_id: target.selection_id,
        old_bet_id,
        stake,
        customer_ref,
    })
}

fn one_place_instruction(report: &PlaceExecutionReport) -> anyhow::Result<&PlaceInstructionReport> {
    let instruction = report
        .instruction_reports
        .as_deref()
        .filter(|reports| reports.len() == 1)
        .and_then(|reports| reports.first())
        .context("placeOrders did not return one instruction report")?;
    anyhow::ensure!(
        instruction.status == InstructionReportStatus::Success,
        "place instruction returned {:?}: {:?}",
        instruction.status,
        instruction.error_code,
    );
    Ok(instruction)
}

async fn find_unmatched_target(client: &BetfairHttpClient) -> anyhow::Result<LiveTarget> {
    let params = ListMarketCatalogueParams {
        filter: MarketFilter {
            in_play_only: Some(false),
            market_type_codes: Some(vec![Ustr::from("MATCH_ODDS")]),
            turn_in_play_enabled: Some(true),
            ..Default::default()
        },
        market_projection: Some(vec![
            MarketProjection::MarketStartTime,
            MarketProjection::RunnerDescription,
        ]),
        sort: Some(MarketSort::MaximumTraded),
        max_results: Some(20),
        locale: None,
    };
    let catalogues: Vec<MarketCatalogue> = client
        .send_betting(METHOD_LIST_MARKET_CATALOGUE, &params)
        .await
        .context("listMarketCatalogue")?;
    anyhow::ensure!(!catalogues.is_empty(), "no candidate markets available");

    let market_ids: Vec<_> = catalogues
        .into_iter()
        .map(|catalogue| catalogue.market_id)
        .collect();
    let books: Vec<LiveMarketBook> = client
        .send_betting(
            "SportsAPING/v1.0/listMarketBook",
            serde_json::json!({
                "marketIds": market_ids,
                "priceProjection": {"priceData": ["EX_BEST_OFFERS"]},
            }),
        )
        .await
        .context("listMarketBook")?;

    books
        .into_iter()
        .filter(|book| book.status == MarketStatus::Open && !book.inplay)
        .find_map(|book| {
            book.runners
                .into_iter()
                .filter(|runner| runner.status == RunnerStatus::Active)
                .find(|runner| {
                    runner
                        .ex
                        .available_to_lay
                        .first()
                        .is_some_and(|price| price.price < price_replace())
                })
                .map(|runner| LiveTarget {
                    market_id: book.market_id,
                    selection_id: runner.selection_id,
                    handicap: runner.handicap,
                })
        })
        .context("no open non-in-play runner has a safely separated lay price")
}

async fn cleanup_orders(
    client: &BetfairHttpClient,
    customer_order_ref: &str,
    known_bet_ids: &HashSet<BetId>,
    await_unknown_order: bool,
) -> anyhow::Result<()> {
    let mut last_error = None;
    let mut matched_exposure = false;
    let mut canceled_known = false;
    let wait = if await_unknown_order {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(2)
    };
    let deadline = Instant::now() + wait;

    loop {
        let report = current_orders(client, customer_order_ref, OrderProjection::All).await;
        let executable = match report {
            Ok(report) => {
                matched_exposure |= report
                    .current_orders
                    .iter()
                    .any(|order| order.size_matched.unwrap_or(Decimal::ZERO) != Decimal::ZERO);
                report
                    .current_orders
                    .iter()
                    .filter(|order| order.status == BetfairOrderStatus::Executable)
                    .map(|order| order.bet_id.clone())
                    .collect()
            }
            Err(e) => {
                last_error = Some(e);

                if canceled_known {
                    HashSet::new()
                } else {
                    canceled_known = true;
                    known_bet_ids.clone()
                }
            }
        };

        if !executable.is_empty() {
            let params = CancelOrdersParams {
                market_id: None,
                instructions: Some(
                    executable
                        .into_iter()
                        .map(|bet_id| CancelInstruction {
                            bet_id,
                            size_reduction: None,
                        })
                        .collect(),
                ),
                customer_ref: Some(live_ref()),
            };
            let _: Result<CancelExecutionReport, _> = client
                .send_betting_order(METHOD_CANCEL_ORDERS, &params)
                .await;
        }

        if Instant::now() >= deadline {
            break;
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let report = current_orders(client, customer_order_ref, OrderProjection::All)
        .await
        .map_err(|e| last_error.unwrap_or(e))?;
    matched_exposure |= report
        .current_orders
        .iter()
        .any(|order| order.size_matched.unwrap_or(Decimal::ZERO) != Decimal::ZERO);
    let executable: Vec<_> = report
        .current_orders
        .iter()
        .filter(|order| order.status == BetfairOrderStatus::Executable)
        .map(|order| &order.bet_id)
        .collect();
    anyhow::ensure!(
        executable.is_empty(),
        "task-created executable orders remain after cleanup: {executable:?}",
    );
    anyhow::ensure!(
        !matched_exposure,
        "a task-created live order has matched exposure",
    );
    Ok(())
}

async fn current_orders(
    client: &BetfairHttpClient,
    customer_order_ref: &str,
    projection: OrderProjection,
) -> anyhow::Result<CurrentOrderSummaryReport> {
    let params = ListCurrentOrdersParams {
        bet_ids: None,
        market_ids: None,
        order_projection: Some(projection),
        customer_order_refs: Some(vec![customer_order_ref.to_string()]),
        customer_strategy_refs: None,
        date_range: None,
        order_by: None,
        sort_dir: None,
        from_record: None,
        record_count: Some(100),
    };
    client
        .send_betting(METHOD_LIST_CURRENT_ORDERS, &params)
        .await
        .context("listCurrentOrders")
}

fn minimum_stake(currency: &str) -> anyhow::Result<Decimal> {
    let units = match currency {
        "GBP" => 1,
        "EUR" | "NZD" => 2,
        "USD" => 3,
        "AUD" => 5,
        "CAD" | "SGD" => 6,
        "BRL" | "GEL" | "PEN" | "RON" => 10,
        "HKD" => 25,
        "DKK" | "NOK" | "SEK" => 30,
        "MXN" => 60,
        "ARS" => 100,
        "ISK" => 350,
        "HUF" => 800,
        other => anyhow::bail!("unsupported live-test account currency {other}"),
    };
    Ok(Decimal::from(units))
}

fn live_ref() -> String {
    UUID4::new().to_string().replace('-', "")
}

fn price_passive() -> Decimal {
    Decimal::from(990)
}

fn price_replace() -> Decimal {
    Decimal::from(980)
}
