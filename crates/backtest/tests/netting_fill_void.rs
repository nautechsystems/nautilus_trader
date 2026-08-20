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

//! Realized PnL reported across a NETTING cycle boundary corrected by a fill void.
//!
//! These tests need an [`ExecutionEngine`] driving a [`Portfolio`] over one cache, and
//! `nautilus-execution` and `nautilus-portfolio` do not depend on each other, so this crate is
//! the only core crate that can wire both.

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::{Clock, TestClock},
};
use nautilus_execution::engine::{
    ExecutionEngine, config::ExecutionEngineConfig, stubs::StubExecutionClient,
};
use nautilus_model::{
    accounts::CashAccount,
    enums::{OmsType, OrderSide, OrderType, PositionSide},
    events::{OrderEventAny, order::spec::OrderFillVoidedSpec},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
        TraderId, Venue, VenueOrderId,
    },
    instruments::{CurrencyPair, InstrumentAny, stubs::audusd_sim},
    orders::{Order, OrderTestBuilder, stubs::TestOrderEventStubs},
    stubs::TestDefault,
    types::{Money, Price, Quantity},
};
use nautilus_portfolio::Portfolio;
use rstest::rstest;

/// One fill in the scripted NETTING history.
#[derive(Clone, Copy)]
struct FillSpec {
    client_order_id: &'static str,
    venue_order_id: &'static str,
    trade_id: &'static str,
    side: OrderSide,
    quantity: u64,
    last_px: &'static str,
}

const CYCLE_1_OPEN: FillSpec = FillSpec {
    client_order_id: "O-1",
    venue_order_id: "V-1",
    trade_id: "T-1",
    side: OrderSide::Buy,
    quantity: 100_000,
    last_px: "0.80000",
};
const CYCLE_1_CLOSE: FillSpec = FillSpec {
    client_order_id: "O-2",
    venue_order_id: "V-2",
    trade_id: "T-2",
    side: OrderSide::Sell,
    quantity: 100_000,
    last_px: "0.80020",
};
const CYCLE_2_OPEN: FillSpec = FillSpec {
    client_order_id: "O-3",
    venue_order_id: "V-3",
    trade_id: "T-3",
    side: OrderSide::Buy,
    quantity: 100_000,
    last_px: "0.80050",
};

/// Cycle 1 closed in two parts, so voiding part of its opening fill still leaves the corrected
/// history an exact close to reach flat on.
const CYCLE_1_CLOSE_PARTIAL: FillSpec = FillSpec {
    client_order_id: "O-6",
    venue_order_id: "V-6",
    trade_id: "T-6",
    side: OrderSide::Sell,
    quantity: 60_000,
    last_px: "0.80020",
};

/// Cycle 2 as a complete short round trip, for a history that reopens twice.
const CYCLE_2_SHORT_OPEN: FillSpec = FillSpec {
    client_order_id: "O-4",
    venue_order_id: "V-4",
    trade_id: "T-4",
    side: OrderSide::Sell,
    quantity: 40_000,
    last_px: "0.80030",
};
const CYCLE_2_SHORT_CLOSE: FillSpec = FillSpec {
    client_order_id: "O-5",
    venue_order_id: "V-5",
    trade_id: "T-5",
    side: OrderSide::Buy,
    quantity: 40_000,
    last_px: "0.80010",
};

/// An execution engine and portfolio sharing one cache, driven through a NETTING open, close,
/// and reopen of 100,000 on a single position ID with the replay log carried across the
/// boundary, so a prior-cycle fill stays correctable.
///
/// Cycle 1 realizes 20.00 less 4.00 commission and is archived as one snapshot frame; cycle 2
/// opens at 0.80050 for a commission of 2.00, so realized PnL reads 14.00 before any void.
struct NettingReopen {
    execution_engine: ExecutionEngine,
    portfolio: Portfolio,
    cache: Rc<RefCell<Cache>>,
    instrument_id: InstrumentId,
    position_id: PositionId,
    account_id: AccountId,
}

fn run_netting_reopen() -> NettingReopen {
    run_netting_scenario(&[CYCLE_1_OPEN, CYCLE_1_CLOSE, CYCLE_2_OPEN])
}

/// Runs the same setup over three cycles: cycle 1 long, cycle 2 short, then a reopen. Cycle 1
/// realizes 16.00 and cycle 2 realizes 4.00, both archived, so realized PnL reads 18.00 before
/// any void.
fn run_netting_three_cycles() -> NettingReopen {
    run_netting_scenario(&[
        CYCLE_1_OPEN,
        CYCLE_1_CLOSE,
        CYCLE_2_SHORT_OPEN,
        CYCLE_2_SHORT_CLOSE,
        CYCLE_2_OPEN,
    ])
}

/// Runs one archived cycle whose close is split across two fills, then a reopen. Cycle 1 realizes
/// 18.00 as the single archived frame and cycle 2 opens for a commission of 2.00, so realized PnL
/// reads 16.00 before any void.
fn run_netting_split_close() -> NettingReopen {
    run_netting_scenario(&[
        CYCLE_1_OPEN,
        CYCLE_1_CLOSE_PARTIAL,
        CYCLE_2_SHORT_OPEN,
        CYCLE_2_OPEN,
    ])
}

fn run_netting_scenario(fills: &[FillSpec]) -> NettingReopen {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let config = ExecutionEngineConfig {
        carry_replay_events_on_reopen: true,
        ..Default::default()
    };
    let mut execution_engine =
        ExecutionEngine::new(Rc::clone(&clock), Rc::clone(&cache), Some(config));
    let portfolio = Portfolio::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let instrument = audusd_sim();
    let account_id = AccountId::test_default();
    let position_id = PositionId::new(format!("{}-{}", instrument.id, StrategyId::test_default()));

    execution_engine
        .register_client(Box::new(StubExecutionClient::new(
            ClientId::from("STUB"),
            account_id,
            Venue::test_default(),
            OmsType::Netting,
            None,
        )))
        .unwrap();
    cache
        .borrow_mut()
        .add_instrument(instrument.clone().into())
        .unwrap();
    cache
        .borrow_mut()
        .add_account(CashAccount::default().into())
        .unwrap();

    for fill in fills {
        process_filled_order(
            &mut execution_engine,
            &instrument,
            *fill,
            Money::from("2.00 USD"),
            position_id,
        );
    }

    NettingReopen {
        execution_engine,
        portfolio,
        cache,
        instrument_id: instrument.id,
        position_id,
        account_id,
    }
}

fn process_filled_order(
    execution_engine: &mut ExecutionEngine,
    instrument: &CurrencyPair,
    fill: FillSpec,
    commission: Money,
    position_id: PositionId,
) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::test_default())
        .strategy_id(StrategyId::test_default())
        .instrument_id(instrument.id)
        .client_order_id(ClientOrderId::from(fill.client_order_id))
        .side(fill.side)
        .quantity(Quantity::from(fill.quantity))
        .build();

    execution_engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(ClientId::from("STUB")), true)
        .unwrap();
    execution_engine.process(&TestOrderEventStubs::submitted(
        &order,
        AccountId::test_default(),
    ));
    execution_engine.process(&TestOrderEventStubs::accepted(
        &order,
        AccountId::test_default(),
        VenueOrderId::from(fill.venue_order_id),
    ));

    let accepted_order = execution_engine
        .cache()
        .borrow()
        .order_owned(&order.client_order_id())
        .expect("accepted order");
    let instrument_any: InstrumentAny = instrument.clone().into();
    execution_engine.process(&TestOrderEventStubs::filled(
        &accepted_order,
        &instrument_any,
        Some(TradeId::new(fill.trade_id)),
        Some(position_id),
        Some(Price::from(fill.last_px)),
        None,
        None,
        Some(commission),
        None,
        Some(AccountId::test_default()),
    ));
}

/// Builds an `OrderFillVoided` for 40,000 of the cached fill for `spec`.
///
/// `Order::validate_fill_void` rejects a void whose venue order ID, account ID, side, type,
/// last price, currency, liquidity side, or position ID differs from the original fill, so
/// copying them from the fill is the only way to reach the position correction path.
fn build_fill_void_from_cached_fill(
    execution_engine: &ExecutionEngine,
    spec: FillSpec,
) -> OrderEventAny {
    let cache = execution_engine.cache();
    let cache = cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(spec.client_order_id))
        .expect("filled order should be cached");
    let trade_id = TradeId::new(spec.trade_id);
    let fill = order
        .events()
        .into_iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) if fill.trade_id == trade_id => Some(fill.clone()),
            _ => None,
        })
        .expect("order should hold the fill being voided");

    OrderEventAny::FillVoided(
        OrderFillVoidedSpec::builder()
            .trader_id(fill.trader_id)
            .strategy_id(fill.strategy_id)
            .instrument_id(fill.instrument_id)
            .client_order_id(fill.client_order_id)
            .venue_order_id(fill.venue_order_id)
            .account_id(fill.account_id)
            .trade_id(fill.trade_id)
            .voided_qty(Quantity::from(40_000))
            .commission_voided(Money::from("0.80 USD"))
            .order_side(fill.order_side)
            .order_type(fill.order_type)
            .last_px(fill.last_px)
            .currency(fill.currency)
            .liquidity_side(fill.liquidity_side)
            .maybe_position_id(fill.position_id)
            .build(),
    )
}

#[rstest]
fn test_prior_cycle_fill_void_drops_absorbed_snapshot_frames() {
    let NettingReopen {
        mut execution_engine,
        mut portfolio,
        cache,
        instrument_id,
        position_id,
        account_id,
    } = run_netting_reopen();

    let realized_before =
        portfolio.realized_pnl_for_account(&instrument_id, Some(&account_id), None);
    let snapshots_before = cache.borrow().position_snapshot_count(&position_id);

    // Void 40,000 of cycle 1's closing fill: the position never closes in the corrected
    // history, so the rebuild spans both cycles and absorbs the archived frame.
    let voided = build_fill_void_from_cached_fill(&execution_engine, CYCLE_1_CLOSE);
    execution_engine.process(&voided);

    let cache = cache.borrow();
    let position = cache.position(&position_id).expect("position stays cached");

    assert_eq!(realized_before, Some(Money::from("14.00 USD")));
    assert_eq!(snapshots_before, 1);
    assert_eq!(cache.position_snapshot_count(&position_id), 0);
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, Quantity::from(140_000));
    // Buy 100,000, sell 60,000, buy 100,000: -2.00 - 1.20 + 12.00 - 2.00
    assert_eq!(position.realized_pnl, Some(Money::from("6.80 USD")));
    assert_eq!(
        portfolio.realized_pnl_for_account(&instrument_id, Some(&account_id), None),
        Some(Money::from("6.80 USD")),
    );
}

#[rstest]
fn test_current_cycle_fill_void_keeps_snapshot_frames() {
    let NettingReopen {
        mut execution_engine,
        mut portfolio,
        cache,
        instrument_id,
        position_id,
        account_id,
    } = run_netting_reopen();

    let realized_before =
        portfolio.realized_pnl_for_account(&instrument_id, Some(&account_id), None);
    let snapshots_before = cache.borrow().position_snapshot_count(&position_id);

    // Void 40,000 of cycle 2's opening fill: the corrected history still closes where it did,
    // so cycle 1 stays a separate archived frame that realized PnL must keep counting.
    let voided = build_fill_void_from_cached_fill(&execution_engine, CYCLE_2_OPEN);
    execution_engine.process(&voided);

    let cache = cache.borrow();
    let position = cache.position(&position_id).expect("position stays cached");

    assert_eq!(realized_before, Some(Money::from("14.00 USD")));
    assert_eq!(snapshots_before, 1);
    assert_eq!(cache.position_snapshot_count(&position_id), 1);
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, Quantity::from(60_000));
    // Cycle 2 alone: commission 2.00 less the 0.80 voided with the fill
    assert_eq!(position.realized_pnl, Some(Money::from("-1.20 USD")));
    assert_eq!(
        portfolio.realized_pnl_for_account(&instrument_id, Some(&account_id), None),
        Some(Money::from("14.80 USD")),
    );
}

#[rstest]
fn test_settle_replacing_one_frame_with_one_frame_refreshes_realized_pnl() {
    let NettingReopen {
        mut execution_engine,
        mut portfolio,
        cache,
        instrument_id,
        position_id,
        account_id,
    } = run_netting_split_close();

    let realized_before =
        portfolio.realized_pnl_for_account(&instrument_id, Some(&account_id), None);
    let snapshots_before = cache.borrow().position_snapshot_count(&position_id);

    // Void 40,000 of cycle 1's opening fill. The corrected history is buy 60,000, sell 60,000,
    // sell 40,000, buy 100,000, so it reaches flat once and settles into one frame. The archive
    // holds one frame either side of the void, so the frame count alone cannot tell the portfolio
    // its cached aggregate went stale.
    let voided = build_fill_void_from_cached_fill(&execution_engine, CYCLE_1_OPEN);
    execution_engine.process(&voided);

    let realized_after =
        portfolio.realized_pnl_for_account(&instrument_id, Some(&account_id), None);
    let cache = cache.borrow();
    let position = cache.position(&position_id).expect("position stays cached");
    let settled = cache.position_snapshots(Some(&position_id), None);

    assert_eq!(realized_before, Some(Money::from("16.00 USD")));
    assert_eq!(snapshots_before, 1);
    assert_eq!(cache.position_snapshot_count(&position_id), 1);
    // The corrected first cycle: -1.20 on the reduced buy, then 12.00 less 2.00 on the sell
    assert_eq!(settled[0].realized_pnl, Some(Money::from("8.80 USD")));
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, Quantity::from(60_000));
    // Reopened short 40,000 closed 2 pips against it, plus commission on both fills
    assert_eq!(position.realized_pnl, Some(Money::from("-12.00 USD")));
    assert_eq!(realized_after, Some(Money::from("-3.20 USD")));
}

#[rstest]
fn test_prior_cycle_fill_void_settles_cycles_the_rebuild_reshaped() {
    let NettingReopen {
        mut execution_engine,
        mut portfolio,
        cache,
        instrument_id,
        position_id,
        account_id,
    } = run_netting_three_cycles();

    let realized_before =
        portfolio.realized_pnl_for_account(&instrument_id, Some(&account_id), None);
    let snapshots_before = cache.borrow().position_snapshot_count(&position_id);

    // Void 40,000 of cycle 1's closing fill. The corrected history is buy 100,000, sell 60,000,
    // sell 40,000, buy 40,000, buy 100,000, so it still reaches flat, but one fill later than
    // before: the two archived cycles become a single corrected cycle worth 18.80.
    let voided = build_fill_void_from_cached_fill(&execution_engine, CYCLE_1_CLOSE);
    execution_engine.process(&voided);

    let cache = cache.borrow();
    let position = cache.position(&position_id).expect("position stays cached");
    let settled = cache.position_snapshots(Some(&position_id), None);

    assert_eq!(realized_before, Some(Money::from("18.00 USD")));
    assert_eq!(snapshots_before, 2);
    assert_eq!(cache.position_snapshot_count(&position_id), 1);
    // -2.00 - 1.20 + 12.00 + 12.00 - 2.00, the corrected history up to the surviving flat point
    assert_eq!(settled[0].realized_pnl, Some(Money::from("18.80 USD")));
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, Quantity::from(140_000));
    // The reopened cycle alone: commissions on the 40,000 buy and the 100,000 buy
    assert_eq!(position.realized_pnl, Some(Money::from("-4.00 USD")));
    assert_eq!(
        portfolio.realized_pnl_for_account(&instrument_id, Some(&account_id), None),
        Some(Money::from("14.80 USD")),
    );
}
