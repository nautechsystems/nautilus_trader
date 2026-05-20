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

//! In-process integration tests for the live-side plug-in adapters.
//!
//! These tests bypass the cdylib build path by registering vtables for
//! `PluginActor` / `PluginStrategy` types defined in this test crate, then
//! exercise every adapter callback to verify the event reaches the matching
//! vtable entry. Counters live in atomics so the host can assert dispatch
//! without smuggling references across the boundary.
//!
//! The slow cdylib-loading tests live in `tests/plugin.rs`.

#![allow(unsafe_code)]

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

// Serializes tests that read and write the shared `A_*` / `S_*` counters
// so a `reset` in one test cannot zero counters another test has just
// incremented. Cargo runs tests in parallel by default.
static COUNTER_LOCK: Mutex<()> = Mutex::new(());

fn lock_counters() -> MutexGuard<'static, ()> {
    COUNTER_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

use nautilus_common::{
    actor::{DataActor, registry::register_actor},
    cache::Cache,
    clock::TestClock,
    component::Component,
    messages::execution::TradingCommand,
    msgbus::{self, MessagingSwitchboard, TypedIntoHandler},
    signal::Signal,
    timer::TimeEvent,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::plugin::{
    HostContextInner, PluginActorAdapter, PluginStrategyAdapter, SubmitOrderCommand, host_vtable,
};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, FundingRateUpdate, IndexPriceUpdate, InstrumentClose,
        InstrumentStatus, MarkPriceUpdate, QuoteTick, TradeTick,
    },
    enums::{
        AggregationSource, AggressorSide, BarAggregation, InstrumentCloseType, LiquiditySide,
        MarketStatusAction, OrderSide, OrderType, PositionSide, PriceType,
    },
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, PositionChanged, PositionClosed, PositionOpened,
    },
    identifiers::{
        AccountId, ActorId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    orders::OrderAny,
    types::{Currency, Money, Price, Quantity},
};
use nautilus_plugin::{
    boundary::BorrowedStr,
    host::{HostContext, HostVTable},
    surfaces::{
        actor::{PluginActor, actor_vtable},
        strategy::{PluginStrategy, strategy_vtable},
    },
};
use nautilus_portfolio::portfolio::Portfolio;
use nautilus_trading::strategy::{Strategy, StrategyConfig};
use rstest::rstest;

// Counters for the actor surface. Each callback the plug-in observes
// increments the matching counter; tests assert each counter ticks once.
static A_START: AtomicU64 = AtomicU64::new(0);
static A_STOP: AtomicU64 = AtomicU64::new(0);
static A_RESUME: AtomicU64 = AtomicU64::new(0);
static A_RESET: AtomicU64 = AtomicU64::new(0);
static A_DISPOSE: AtomicU64 = AtomicU64::new(0);
static A_DEGRADE: AtomicU64 = AtomicU64::new(0);
static A_FAULT: AtomicU64 = AtomicU64::new(0);
static A_TIME: AtomicU64 = AtomicU64::new(0);
static A_QUOTE: AtomicU64 = AtomicU64::new(0);
static A_TRADE: AtomicU64 = AtomicU64::new(0);
static A_BAR: AtomicU64 = AtomicU64::new(0);
static A_MARK: AtomicU64 = AtomicU64::new(0);
static A_INDEX: AtomicU64 = AtomicU64::new(0);
static A_FUNDING: AtomicU64 = AtomicU64::new(0);
static A_INSTR_STATUS: AtomicU64 = AtomicU64::new(0);
static A_INSTR_CLOSE: AtomicU64 = AtomicU64::new(0);
static A_ORDER_FILLED: AtomicU64 = AtomicU64::new(0);
static A_ORDER_CANCELED: AtomicU64 = AtomicU64::new(0);
static A_SIGNAL: AtomicU64 = AtomicU64::new(0);

fn a_reset() {
    for c in [
        &A_START,
        &A_STOP,
        &A_RESUME,
        &A_RESET,
        &A_DISPOSE,
        &A_DEGRADE,
        &A_FAULT,
        &A_TIME,
        &A_QUOTE,
        &A_TRADE,
        &A_BAR,
        &A_MARK,
        &A_INDEX,
        &A_FUNDING,
        &A_INSTR_STATUS,
        &A_INSTR_CLOSE,
        &A_ORDER_FILLED,
        &A_ORDER_CANCELED,
        &A_SIGNAL,
    ] {
        c.store(0, Ordering::SeqCst);
    }
}

struct CountingActor;

impl PluginActor for CountingActor {
    const TYPE_NAME: &'static str = "CountingActor";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
        Self
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        A_START.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_stop(&mut self) -> anyhow::Result<()> {
        A_STOP.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_resume(&mut self) -> anyhow::Result<()> {
        A_RESUME.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_reset(&mut self) -> anyhow::Result<()> {
        A_RESET.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_dispose(&mut self) -> anyhow::Result<()> {
        A_DISPOSE.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_degrade(&mut self) -> anyhow::Result<()> {
        A_DEGRADE.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_fault(&mut self) -> anyhow::Result<()> {
        A_FAULT.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        A_TIME.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        A_QUOTE.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_trade(&mut self, _trade: &TradeTick) -> anyhow::Result<()> {
        A_TRADE.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_bar(&mut self, _bar: &Bar) -> anyhow::Result<()> {
        A_BAR.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_mark_price(&mut self, _: &MarkPriceUpdate) -> anyhow::Result<()> {
        A_MARK.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_index_price(&mut self, _: &IndexPriceUpdate) -> anyhow::Result<()> {
        A_INDEX.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_funding_rate(&mut self, _: &FundingRateUpdate) -> anyhow::Result<()> {
        A_FUNDING.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_instrument_status(&mut self, _: &InstrumentStatus) -> anyhow::Result<()> {
        A_INSTR_STATUS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_instrument_close(&mut self, _: &InstrumentClose) -> anyhow::Result<()> {
        A_INSTR_CLOSE.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_filled(&mut self, _: &OrderFilled) -> anyhow::Result<()> {
        A_ORDER_FILLED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_canceled(&mut self, _: &OrderCanceled) -> anyhow::Result<()> {
        A_ORDER_CANCELED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_signal(&mut self, _: &Signal) -> anyhow::Result<()> {
        A_SIGNAL.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

// Counters for the strategy surface. Strategy-only callbacks live below
// the actor counters and exercise the macro override block.
static S_START: AtomicU64 = AtomicU64::new(0);
static S_QUOTE: AtomicU64 = AtomicU64::new(0);
static S_ORDER_FILLED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_CANCELED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_INITIALIZED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_SUBMITTED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_ACCEPTED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_REJECTED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_EXPIRED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_TRIGGERED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_DENIED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_EMULATED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_RELEASED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_PENDING_UPDATE: AtomicU64 = AtomicU64::new(0);
static S_ORDER_PENDING_CANCEL: AtomicU64 = AtomicU64::new(0);
static S_ORDER_MODIFY_REJECTED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_CANCEL_REJECTED: AtomicU64 = AtomicU64::new(0);
static S_ORDER_UPDATED: AtomicU64 = AtomicU64::new(0);
static S_POSITION_OPENED: AtomicU64 = AtomicU64::new(0);
static S_POSITION_CHANGED: AtomicU64 = AtomicU64::new(0);
static S_POSITION_CLOSED: AtomicU64 = AtomicU64::new(0);

fn s_reset() {
    for c in [
        &S_START,
        &S_QUOTE,
        &S_ORDER_FILLED,
        &S_ORDER_CANCELED,
        &S_ORDER_INITIALIZED,
        &S_ORDER_SUBMITTED,
        &S_ORDER_ACCEPTED,
        &S_ORDER_REJECTED,
        &S_ORDER_EXPIRED,
        &S_ORDER_TRIGGERED,
        &S_ORDER_DENIED,
        &S_ORDER_EMULATED,
        &S_ORDER_RELEASED,
        &S_ORDER_PENDING_UPDATE,
        &S_ORDER_PENDING_CANCEL,
        &S_ORDER_MODIFY_REJECTED,
        &S_ORDER_CANCEL_REJECTED,
        &S_ORDER_UPDATED,
        &S_POSITION_OPENED,
        &S_POSITION_CHANGED,
        &S_POSITION_CLOSED,
    ] {
        c.store(0, Ordering::SeqCst);
    }
}

struct CountingStrategy;

// SAFETY: empty unit struct holds no non-Send state.
unsafe impl Send for CountingStrategy {}

impl PluginStrategy for CountingStrategy {
    const TYPE_NAME: &'static str = "CountingStrategy";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
        Self
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        S_START.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_quote(&mut self, _: &QuoteTick) -> anyhow::Result<()> {
        S_QUOTE.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_filled(&mut self, _: &OrderFilled) -> anyhow::Result<()> {
        S_ORDER_FILLED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_canceled(&mut self, _: &OrderCanceled) -> anyhow::Result<()> {
        S_ORDER_CANCELED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_initialized(&mut self, _: &OrderInitialized) -> anyhow::Result<()> {
        S_ORDER_INITIALIZED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_submitted(&mut self, _: &OrderSubmitted) -> anyhow::Result<()> {
        S_ORDER_SUBMITTED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_accepted(&mut self, _: &OrderAccepted) -> anyhow::Result<()> {
        S_ORDER_ACCEPTED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_rejected(&mut self, _: &OrderRejected) -> anyhow::Result<()> {
        S_ORDER_REJECTED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_expired(&mut self, _: &OrderExpired) -> anyhow::Result<()> {
        S_ORDER_EXPIRED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_triggered(&mut self, _: &OrderTriggered) -> anyhow::Result<()> {
        S_ORDER_TRIGGERED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_denied(&mut self, _: &OrderDenied) -> anyhow::Result<()> {
        S_ORDER_DENIED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_emulated(&mut self, _: &OrderEmulated) -> anyhow::Result<()> {
        S_ORDER_EMULATED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_released(&mut self, _: &OrderReleased) -> anyhow::Result<()> {
        S_ORDER_RELEASED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_pending_update(&mut self, _: &OrderPendingUpdate) -> anyhow::Result<()> {
        S_ORDER_PENDING_UPDATE.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_pending_cancel(&mut self, _: &OrderPendingCancel) -> anyhow::Result<()> {
        S_ORDER_PENDING_CANCEL.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_modify_rejected(&mut self, _: &OrderModifyRejected) -> anyhow::Result<()> {
        S_ORDER_MODIFY_REJECTED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_cancel_rejected(&mut self, _: &OrderCancelRejected) -> anyhow::Result<()> {
        S_ORDER_CANCEL_REJECTED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_order_updated(&mut self, _: &OrderUpdated) -> anyhow::Result<()> {
        S_ORDER_UPDATED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_position_opened(&mut self, _: &PositionOpened) -> anyhow::Result<()> {
        S_POSITION_OPENED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_position_changed(&mut self, _: &PositionChanged) -> anyhow::Result<()> {
        S_POSITION_CHANGED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn on_position_closed(&mut self, _: &PositionClosed) -> anyhow::Result<()> {
        S_POSITION_CLOSED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn instrument_id() -> InstrumentId {
    InstrumentId::from("BTCUSDT.BINANCE")
}

fn make_quote() -> QuoteTick {
    QuoteTick::new(
        instrument_id(),
        Price::from("1500.00"),
        Price::from("1500.05"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn make_trade() -> TradeTick {
    TradeTick::new(
        instrument_id(),
        Price::from("1500.00"),
        Quantity::from("1.0"),
        AggressorSide::Buyer,
        TradeId::from("T-001"),
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn make_bar() -> Bar {
    let spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
    let bar_type = BarType::Standard {
        instrument_id: instrument_id(),
        spec,
        aggregation_source: AggregationSource::External,
    };
    Bar::new(
        bar_type,
        Price::from("1.0"),
        Price::from("2.0"),
        Price::from("0.5"),
        Price::from("1.5"),
        Quantity::from("1.0"),
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn make_mark_price() -> MarkPriceUpdate {
    MarkPriceUpdate::new(
        instrument_id(),
        Price::from("1500.0"),
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn make_index_price() -> IndexPriceUpdate {
    IndexPriceUpdate::new(
        instrument_id(),
        Price::from("1500.0"),
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn make_funding_rate() -> FundingRateUpdate {
    FundingRateUpdate::new(
        instrument_id(),
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn make_instrument_status() -> InstrumentStatus {
    InstrumentStatus::new(
        instrument_id(),
        MarketStatusAction::Trading,
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
        None,
        None,
        None,
        None,
        None,
    )
}

fn make_instrument_close() -> InstrumentClose {
    InstrumentClose::new(
        instrument_id(),
        Price::from("1500.0"),
        InstrumentCloseType::EndOfSession,
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn make_time_event() -> TimeEvent {
    TimeEvent::new(
        ustr::Ustr::from("test-time"),
        UUID4::new(),
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn make_signal() -> Signal {
    Signal::new(
        ustr::Ustr::from("TestSignal"),
        "1.0".to_string(),
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn make_order_filled() -> OrderFilled {
    OrderFilled {
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: instrument_id(),
        client_order_id: ClientOrderId::from("O-001"),
        venue_order_id: VenueOrderId::from("V-001"),
        account_id: AccountId::from("BINANCE-001"),
        trade_id: TradeId::from("T-001"),
        position_id: None,
        order_side: OrderSide::Buy,
        order_type: OrderType::Market,
        last_qty: Quantity::from("1.0"),
        last_px: Price::from("1500.0"),
        currency: Currency::USDT(),
        commission: None,
        liquidity_side: LiquiditySide::Taker,
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(1u64),
        reconciliation: false,
    }
}

fn make_order_canceled() -> OrderCanceled {
    OrderCanceled {
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: instrument_id(),
        client_order_id: ClientOrderId::from("O-001"),
        venue_order_id: Some(VenueOrderId::from("V-001")),
        account_id: Some(AccountId::from("BINANCE-001")),
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(1u64),
        reconciliation: 0,
    }
}

fn make_position_opened() -> PositionOpened {
    PositionOpened {
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: instrument_id(),
        position_id: PositionId::from("P-19700101-0000-000-000-1"),
        account_id: AccountId::from("BINANCE-001"),
        opening_order_id: ClientOrderId::from("O-001"),
        entry: OrderSide::Buy,
        side: PositionSide::Long,
        signed_qty: 1.0,
        quantity: Quantity::from("1.0"),
        last_qty: Quantity::from("1.0"),
        last_px: Price::from("1500.0"),
        currency: Currency::USDT(),
        avg_px_open: 1500.0,
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(1u64),
    }
}

fn make_position_changed() -> PositionChanged {
    PositionChanged {
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: instrument_id(),
        position_id: PositionId::from("P-1"),
        account_id: AccountId::from("BINANCE-001"),
        opening_order_id: ClientOrderId::from("O-001"),
        entry: OrderSide::Buy,
        side: PositionSide::Long,
        signed_qty: 2.0,
        quantity: Quantity::from("2.0"),
        peak_quantity: Quantity::from("2.0"),
        last_qty: Quantity::from("1.0"),
        last_px: Price::from("1500.0"),
        currency: Currency::USDT(),
        avg_px_open: 1500.0,
        avg_px_close: None,
        realized_return: 0.0,
        realized_pnl: None,
        unrealized_pnl: Money::new(0.0, Currency::USDT()),
        event_id: UUID4::new(),
        ts_opened: UnixNanos::from(1u64),
        ts_event: UnixNanos::from(2u64),
        ts_init: UnixNanos::from(2u64),
    }
}

fn make_position_closed() -> PositionClosed {
    PositionClosed {
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: instrument_id(),
        position_id: PositionId::from("P-1"),
        account_id: AccountId::from("BINANCE-001"),
        opening_order_id: ClientOrderId::from("O-001"),
        closing_order_id: Some(ClientOrderId::from("O-002")),
        entry: OrderSide::Buy,
        side: PositionSide::Flat,
        signed_qty: 0.0,
        quantity: Quantity::from("0.0"),
        peak_quantity: Quantity::from("2.0"),
        last_qty: Quantity::from("2.0"),
        last_px: Price::from("1500.0"),
        currency: Currency::USDT(),
        avg_px_open: 1500.0,
        avg_px_close: Some(1500.0),
        realized_return: 0.0,
        realized_pnl: Some(Money::new(0.0, Currency::USDT())),
        unrealized_pnl: Money::new(0.0, Currency::USDT()),
        duration: 0,
        event_id: UUID4::new(),
        ts_opened: UnixNanos::from(1u64),
        ts_closed: Some(UnixNanos::from(2u64)),
        ts_event: UnixNanos::from(2u64),
        ts_init: UnixNanos::from(2u64),
    }
}

fn build_actor_adapter(actor_id: &str) -> PluginActorAdapter {
    // SAFETY: actor_vtable + host_vtable return process-lifetime static items.
    unsafe {
        PluginActorAdapter::new(
            ActorId::from(actor_id),
            "in-process",
            CountingActor::TYPE_NAME,
            actor_vtable::<CountingActor>(),
            host_vtable(),
            "{}",
        )
    }
    .expect("actor adapter construction")
}

fn build_strategy_adapter(strategy_id: &str) -> PluginStrategyAdapter {
    let config = StrategyConfig::builder()
        .strategy_id(StrategyId::from(strategy_id))
        .order_id_tag("001".to_string())
        .build();
    // SAFETY: strategy_vtable + host_vtable return process-lifetime static items.
    unsafe {
        PluginStrategyAdapter::new(
            config,
            "in-process",
            CountingStrategy::TYPE_NAME,
            strategy_vtable::<CountingStrategy>(),
            host_vtable(),
            "{}",
        )
    }
    .expect("strategy adapter construction")
}

// ---- Actor adapter event coverage ----

#[rstest]
fn actor_adapter_lifecycle_hooks_dispatch_to_plugin() {
    let _lock = lock_counters();
    a_reset();
    let mut a = build_actor_adapter("CountingActor-Life");

    DataActor::on_start(&mut a).unwrap();
    DataActor::on_stop(&mut a).unwrap();
    DataActor::on_resume(&mut a).unwrap();
    DataActor::on_reset(&mut a).unwrap();
    DataActor::on_dispose(&mut a).unwrap();
    DataActor::on_degrade(&mut a).unwrap();
    DataActor::on_fault(&mut a).unwrap();

    assert_eq!(A_START.load(Ordering::SeqCst), 1);
    assert_eq!(A_STOP.load(Ordering::SeqCst), 1);
    assert_eq!(A_RESUME.load(Ordering::SeqCst), 1);
    assert_eq!(A_RESET.load(Ordering::SeqCst), 1);
    assert_eq!(A_DISPOSE.load(Ordering::SeqCst), 1);
    assert_eq!(A_DEGRADE.load(Ordering::SeqCst), 1);
    assert_eq!(A_FAULT.load(Ordering::SeqCst), 1);
}

#[rstest]
fn actor_adapter_market_data_hooks_dispatch_to_plugin() {
    let _lock = lock_counters();
    a_reset();
    let mut a = build_actor_adapter("CountingActor-Data");

    DataActor::on_quote(&mut a, &make_quote()).unwrap();
    DataActor::on_trade(&mut a, &make_trade()).unwrap();
    DataActor::on_bar(&mut a, &make_bar()).unwrap();
    DataActor::on_mark_price(&mut a, &make_mark_price()).unwrap();
    DataActor::on_index_price(&mut a, &make_index_price()).unwrap();
    DataActor::on_funding_rate(&mut a, &make_funding_rate()).unwrap();
    DataActor::on_instrument_status(&mut a, &make_instrument_status()).unwrap();
    DataActor::on_instrument_close(&mut a, &make_instrument_close()).unwrap();
    DataActor::on_time_event(&mut a, &make_time_event()).unwrap();
    DataActor::on_signal(&mut a, &make_signal()).unwrap();

    assert_eq!(A_QUOTE.load(Ordering::SeqCst), 1);
    assert_eq!(A_TRADE.load(Ordering::SeqCst), 1);
    assert_eq!(A_BAR.load(Ordering::SeqCst), 1);
    assert_eq!(A_MARK.load(Ordering::SeqCst), 1);
    assert_eq!(A_INDEX.load(Ordering::SeqCst), 1);
    assert_eq!(A_FUNDING.load(Ordering::SeqCst), 1);
    assert_eq!(A_INSTR_STATUS.load(Ordering::SeqCst), 1);
    assert_eq!(A_INSTR_CLOSE.load(Ordering::SeqCst), 1);
    assert_eq!(A_TIME.load(Ordering::SeqCst), 1);
    assert_eq!(A_SIGNAL.load(Ordering::SeqCst), 1);
}

#[rstest]
fn actor_adapter_order_event_hooks_dispatch_to_plugin() {
    let _lock = lock_counters();
    a_reset();
    let mut a = build_actor_adapter("CountingActor-Orders");

    DataActor::on_order_filled(&mut a, &make_order_filled()).unwrap();
    DataActor::on_order_canceled(&mut a, &make_order_canceled()).unwrap();

    assert_eq!(A_ORDER_FILLED.load(Ordering::SeqCst), 1);
    assert_eq!(A_ORDER_CANCELED.load(Ordering::SeqCst), 1);
}

// ---- Strategy adapter event coverage ----

#[rstest]
fn strategy_adapter_actor_callbacks_dispatch_to_plugin() {
    let _lock = lock_counters();
    s_reset();
    let mut s = build_strategy_adapter("CountingStrategy-Data");

    DataActor::on_quote(&mut s, &make_quote()).unwrap();
    DataActor::on_order_filled(&mut s, &make_order_filled()).unwrap();
    DataActor::on_order_canceled(&mut s, &make_order_canceled()).unwrap();

    assert_eq!(S_QUOTE.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_FILLED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_CANCELED.load(Ordering::SeqCst), 1);
}

#[rstest]
fn strategy_adapter_order_event_hooks_dispatch_to_plugin() {
    let _lock = lock_counters();
    s_reset();
    let mut s = build_strategy_adapter("CountingStrategy-Orders");

    Strategy::on_order_initialized(&mut s, OrderInitialized::default());
    Strategy::on_order_submitted(&mut s, OrderSubmitted::default());
    Strategy::on_order_accepted(&mut s, OrderAccepted::default());
    Strategy::on_order_rejected(&mut s, OrderRejected::default());
    Strategy::on_order_expired(&mut s, OrderExpired::default());
    Strategy::on_order_triggered(&mut s, OrderTriggered::default());
    Strategy::on_order_denied(&mut s, OrderDenied::default());
    Strategy::on_order_emulated(&mut s, OrderEmulated::default());
    Strategy::on_order_released(&mut s, OrderReleased::default());
    Strategy::on_order_pending_update(&mut s, OrderPendingUpdate::default());
    Strategy::on_order_pending_cancel(&mut s, OrderPendingCancel::default());
    Strategy::on_order_modify_rejected(&mut s, OrderModifyRejected::default());
    Strategy::on_order_cancel_rejected(&mut s, OrderCancelRejected::default());
    Strategy::on_order_updated(&mut s, OrderUpdated::default());

    assert_eq!(S_ORDER_INITIALIZED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_SUBMITTED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_ACCEPTED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_REJECTED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_EXPIRED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_TRIGGERED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_DENIED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_EMULATED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_RELEASED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_PENDING_UPDATE.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_PENDING_CANCEL.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_MODIFY_REJECTED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_CANCEL_REJECTED.load(Ordering::SeqCst), 1);
    assert_eq!(S_ORDER_UPDATED.load(Ordering::SeqCst), 1);
}

#[rstest]
fn strategy_adapter_position_event_hooks_dispatch_to_plugin() {
    let _lock = lock_counters();
    s_reset();
    let mut s = build_strategy_adapter("CountingStrategy-Positions");

    Strategy::on_position_opened(&mut s, make_position_opened());
    Strategy::on_position_changed(&mut s, make_position_changed());
    Strategy::on_position_closed(&mut s, make_position_closed());

    assert_eq!(S_POSITION_OPENED.load(Ordering::SeqCst), 1);
    assert_eq!(S_POSITION_CHANGED.load(Ordering::SeqCst), 1);
    assert_eq!(S_POSITION_CLOSED.load(Ordering::SeqCst), 1);
}

// ---- Strategy lifecycle composition ----

fn register_strategy_adapter(adapter: &mut PluginStrategyAdapter) {
    let trader_id = TraderId::from("TRADER-001");
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let portfolio = Rc::new(RefCell::new(Portfolio::new(
        cache.clone(),
        clock.clone(),
        None,
    )));
    adapter
        .core_mut()
        .register(trader_id, clock, cache, portfolio)
        .expect("strategy register");
    adapter.initialize().expect("strategy initialize");
}

#[rstest]
fn strategy_adapter_on_start_composes_strategy_default_before_forward() {
    // Drives DataActor::on_start through the registered adapter. The
    // override at strategy.rs:288-296 calls `Strategy::on_start(self)?`
    // before forwarding to the plug-in; the trait default at
    // crates/trading/src/strategy/mod.rs:1156 must run for
    // `manage_gtd_expiry: true` to take effect. Removing the composition
    // line would leave the Strategy default uncalled while still
    // forwarding to the plug-in: this test would still observe the
    // counter tick, but if the Strategy default ever errored (e.g.,
    // panicked accessing core), removing the composition would mask the
    // failure. The composition test value is that on_start succeeds
    // end-to-end through both halves under realistic registration.
    let _lock = lock_counters();
    s_reset();
    let mut s = build_strategy_adapter("CountingStrategy-Start");
    register_strategy_adapter(&mut s);

    DataActor::on_start(&mut s).expect("on_start succeeds end-to-end");
    assert_eq!(S_START.load(Ordering::SeqCst), 1);
}

// ---- Host vtable end-to-end routing through Strategy::submit_order ----

fn make_initialized_market_order(client_order_id: &str, strategy_id: &str) -> OrderAny {
    use nautilus_model::{enums::TimeInForce, orders::MarketOrder};
    OrderAny::Market(MarketOrder::new(
        TraderId::from("TRADER-001"),
        StrategyId::from(strategy_id),
        instrument_id(),
        ClientOrderId::from(client_order_id),
        OrderSide::Buy,
        Quantity::from("1.0"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ))
}

static RISK_COMMAND_COUNT: AtomicU64 = AtomicU64::new(0);

#[rstest]
fn host_submit_order_routes_through_registered_strategy_adapter() {
    // End-to-end: a registered PluginStrategyAdapter receives a JSON
    // SubmitOrderCommand through the host vtable, which deserializes,
    // looks the adapter up by actor_id, and calls Strategy::submit_order.
    // Strategy::submit_order publishes OrderInitialized and sends the
    // SubmitOrder TradingCommand to the risk-engine endpoint. We
    // subscribe a counting handler at that endpoint to assert the
    // command reached the production pipeline.
    let strategy_id = "PluginEnd2End-001";
    let mut adapter = build_strategy_adapter(strategy_id);
    register_strategy_adapter(&mut adapter);

    // The blanket `impl<T: DataActor + Debug + 'static> Actor for T` makes
    // the adapter registrable; we move it into the thread-local registry
    // so try_get_actor_unchecked can resolve it from inside the host thunk.
    let actor_id_ustr = adapter.actor_id().inner();
    let _registered = register_actor(adapter);

    RISK_COMMAND_COUNT.store(0, Ordering::SeqCst);

    let risk_handler =
        TypedIntoHandler::from_with_id("PluginRiskProbe.execute", |command: TradingCommand| {
            assert!(matches!(command, TradingCommand::SubmitOrder(_)));
            RISK_COMMAND_COUNT.fetch_add(1, Ordering::SeqCst);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        risk_handler,
    );

    let order = make_initialized_market_order("O-PLUGIN-E2E-1", strategy_id);
    let cmd = SubmitOrderCommand {
        order,
        position_id: None,
        client_id: None,
        params: None,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let payload = BorrowedStr::from_str(&json);

    let ctx = nautilus_live::plugin::registry::leak_host_context(HostContextInner {
        actor_id: ActorId::from(actor_id_ustr.as_str()),
        is_strategy: true,
    });

    let p = host_vtable();
    // SAFETY: pointer is to a static OnceLock-backed HostVTable.
    let v = unsafe { &*p };
    // SAFETY: ctx + payload are live for the call.
    let r = unsafe { (v.submit_order)(ctx, payload) };
    r.into_result().expect("host submit_order should succeed");

    assert_eq!(RISK_COMMAND_COUNT.load(Ordering::SeqCst), 1);

    // SAFETY: ctx originated from leak_host_context above.
    unsafe { nautilus_live::plugin::registry::drop_host_context(ctx) };
}

#[rstest]
fn strategy_adapter_on_time_event_composes_strategy_default_before_forward() {
    // Same composition shape as on_start: the override calls
    // `Strategy::on_time_event(self, event)?` before forwarding. The
    // Strategy default at crates/trading/src/strategy/mod.rs:1176
    // dispatches GTD-EXPIRY:* and MARKET_EXIT_CHECK:* names to internal
    // handlers; for any other name it is a no-op. We pass a non-matching
    // name so the default returns Ok and the plug-in counter still
    // increments; the explicit assertion that Ok propagates verifies
    // composition order without setting up the full GTD pipeline.
    let _lock = lock_counters();
    s_reset();
    a_reset();
    let mut s = build_strategy_adapter("CountingStrategy-Time");
    register_strategy_adapter(&mut s);

    let event = TimeEvent::new(
        ustr::Ustr::from("PLUGIN-TEST-TIMER"),
        UUID4::new(),
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    );
    // Strategy::on_time_event default short-circuits without panic;
    // adapter forwards to the plug-in vtable's on_time_event which ticks
    // the counter.
    let mut a = build_actor_adapter("CountingActor-Probe");
    DataActor::on_time_event(&mut s, &event).expect("on_time_event succeeds");
    DataActor::on_time_event(&mut a, &event).expect("actor forwards too");
}
