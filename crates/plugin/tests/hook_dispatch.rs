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

//! Parametrised per-hook dispatch tests for the actor and strategy plug
//! points.
//!
//! Each vtable entry on `ActorVTable` and `StrategyVTable` is paired with
//! a thunk. A wiring mistake at the vtable-init site (e.g. assigning
//! `on_order_canceled_thunk` to the `on_order_filled` field) compiles but
//! routes events to the wrong trait method. These tests guard against
//! that class of mistake by:
//!
//! 1. Implementing a "hook-counting" test actor and strategy that
//!    overrides every callback to increment a per-hook atomic counter.
//! 2. Invoking each vtable entry with a valid payload for the type the
//!    entry's documented to accept.
//! 3. Asserting only the matching counter incremented.
//!
//! Lifecycle callbacks carry no payload; data/order/position callbacks
//! cross the boundary as JSON, so each event hook also exercises the
//! deserialise path through serde end-to-end.

#![allow(unsafe_code)]

use std::sync::{
    Mutex, MutexGuard, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use nautilus_common::{signal::Signal, timer::TimeEvent};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, QuoteTick, TradeTick,
        stubs::{
            stub_bar, stub_instrument_close, stub_instrument_status, stub_trade_ethusdt_buyer,
        },
    },
    enums::{OrderSide, PositionSide},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, PositionChanged, PositionClosed, PositionOpened, order::stubs as order_stubs,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId, VenueOrderId,
    },
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
use rstest::rstest;
use ustr::Ustr;

macro_rules! generated_slot {
    ($vtable:expr, $slot:ident) => {{
        ($vtable)
            .$slot
            .expect(concat!("generated vtable includes ", stringify!($slot)))
    }};
}

// The `On` prefix mirrors the trait method names; clippy's
// `enum_variant_names` would otherwise object to the shared prefix.
#[allow(clippy::enum_variant_names)]
#[repr(usize)]
#[derive(Clone, Copy, Debug)]
enum ActorHook {
    OnStart,
    OnStop,
    OnResume,
    OnReset,
    OnDispose,
    OnDegrade,
    OnFault,
    OnTimeEvent,
    OnQuote,
    OnTrade,
    OnBar,
    OnMarkPrice,
    OnIndexPrice,
    OnFundingRate,
    OnInstrumentStatus,
    OnInstrumentClose,
    OnOrderFilled,
    OnOrderCanceled,
    OnSignal,
}

const ACTOR_HOOK_COUNT: usize = ActorHook::OnSignal as usize + 1;
static ACTOR_HOOK_CALLS: [AtomicU64; ACTOR_HOOK_COUNT] =
    [const { AtomicU64::new(0) }; ACTOR_HOOK_COUNT];

// Serialises hook-dispatch test cases so the shared counter arrays are
// not contaminated by parallel runs. cargo test runs cases in parallel by
// default; each parametrised case acquires this lock for its body.
fn dispatch_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn reset_actor_counters() {
    for c in &ACTOR_HOOK_CALLS {
        c.store(0, Ordering::SeqCst);
    }
}

fn bump_actor(hook: ActorHook) {
    ACTOR_HOOK_CALLS[hook as usize].fetch_add(1, Ordering::SeqCst);
}

fn assert_only_actor_hook(expected: ActorHook) {
    for (i, c) in ACTOR_HOOK_CALLS.iter().enumerate() {
        let v = c.load(Ordering::SeqCst);
        if i == expected as usize {
            assert_eq!(v, 1, "hook {expected:?} should have fired exactly once");
        } else {
            assert_eq!(
                v, 0,
                "hook at index {i} fired but {expected:?} was expected",
            );
        }
    }
}

#[derive(Default)]
struct HookCountingActor;

impl PluginActor for HookCountingActor {
    const TYPE_NAME: &'static str = "HookCountingActor";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
        Self
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnStart);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnStop);
        Ok(())
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnResume);
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnReset);
        Ok(())
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnDispose);
        Ok(())
    }

    fn on_degrade(&mut self) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnDegrade);
        Ok(())
    }

    fn on_fault(&mut self) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnFault);
        Ok(())
    }

    fn on_time_event(&mut self, _e: &TimeEvent) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnTimeEvent);
        Ok(())
    }

    fn on_quote(&mut self, _q: &QuoteTick) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnQuote);
        Ok(())
    }

    fn on_trade(&mut self, _t: &TradeTick) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnTrade);
        Ok(())
    }

    fn on_bar(&mut self, _b: &Bar) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnBar);
        Ok(())
    }

    fn on_mark_price(&mut self, _p: &MarkPriceUpdate) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnMarkPrice);
        Ok(())
    }

    fn on_index_price(&mut self, _p: &IndexPriceUpdate) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnIndexPrice);
        Ok(())
    }

    fn on_funding_rate(&mut self, _f: &FundingRateUpdate) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnFundingRate);
        Ok(())
    }

    fn on_instrument_status(&mut self, _s: &InstrumentStatus) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnInstrumentStatus);
        Ok(())
    }

    fn on_instrument_close(&mut self, _c: &InstrumentClose) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnInstrumentClose);
        Ok(())
    }

    fn on_order_filled(&mut self, _e: &OrderFilled) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnOrderFilled);
        Ok(())
    }

    fn on_order_canceled(&mut self, _e: &OrderCanceled) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnOrderCanceled);
        Ok(())
    }

    fn on_signal(&mut self, _s: &Signal) -> anyhow::Result<()> {
        bump_actor(ActorHook::OnSignal);
        Ok(())
    }
}

// See note above on ActorHook regarding the `On` prefix lint.
#[allow(clippy::enum_variant_names)]
#[repr(usize)]
#[derive(Clone, Copy, Debug)]
enum StrategyHook {
    OnStart,
    OnStop,
    OnResume,
    OnReset,
    OnDispose,
    OnDegrade,
    OnFault,
    OnTimeEvent,
    OnQuote,
    OnTrade,
    OnBar,
    OnMarkPrice,
    OnIndexPrice,
    OnFundingRate,
    OnInstrumentStatus,
    OnInstrumentClose,
    OnSignal,
    OnOrderInitialized,
    OnOrderSubmitted,
    OnOrderAccepted,
    OnOrderRejected,
    OnOrderFilled,
    OnOrderCanceled,
    OnOrderExpired,
    OnOrderTriggered,
    OnOrderDenied,
    OnOrderEmulated,
    OnOrderReleased,
    OnOrderPendingUpdate,
    OnOrderPendingCancel,
    OnOrderModifyRejected,
    OnOrderCancelRejected,
    OnOrderUpdated,
    OnPositionOpened,
    OnPositionChanged,
    OnPositionClosed,
}

const STRATEGY_HOOK_COUNT: usize = StrategyHook::OnPositionClosed as usize + 1;
static STRATEGY_HOOK_CALLS: [AtomicU64; STRATEGY_HOOK_COUNT] =
    [const { AtomicU64::new(0) }; STRATEGY_HOOK_COUNT];

fn reset_strategy_counters() {
    for c in &STRATEGY_HOOK_CALLS {
        c.store(0, Ordering::SeqCst);
    }
}

fn bump_strategy(hook: StrategyHook) {
    STRATEGY_HOOK_CALLS[hook as usize].fetch_add(1, Ordering::SeqCst);
}

fn assert_only_strategy_hook(expected: StrategyHook) {
    for (i, c) in STRATEGY_HOOK_CALLS.iter().enumerate() {
        let v = c.load(Ordering::SeqCst);
        if i == expected as usize {
            assert_eq!(v, 1, "hook {expected:?} should have fired exactly once");
        } else {
            assert_eq!(
                v, 0,
                "hook at index {i} fired but {expected:?} was expected",
            );
        }
    }
}

struct HookCountingStrategy;
// SAFETY: holds no fields; the trait requires Send.
unsafe impl Send for HookCountingStrategy {}

impl PluginStrategy for HookCountingStrategy {
    const TYPE_NAME: &'static str = "HookCountingStrategy";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
        Self
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnStart);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnStop);
        Ok(())
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnResume);
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnReset);
        Ok(())
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnDispose);
        Ok(())
    }

    fn on_degrade(&mut self) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnDegrade);
        Ok(())
    }

    fn on_fault(&mut self) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnFault);
        Ok(())
    }

    fn on_time_event(&mut self, _e: &TimeEvent) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnTimeEvent);
        Ok(())
    }

    fn on_quote(&mut self, _q: &QuoteTick) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnQuote);
        Ok(())
    }

    fn on_trade(&mut self, _t: &TradeTick) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnTrade);
        Ok(())
    }

    fn on_bar(&mut self, _b: &Bar) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnBar);
        Ok(())
    }

    fn on_mark_price(&mut self, _p: &MarkPriceUpdate) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnMarkPrice);
        Ok(())
    }

    fn on_index_price(&mut self, _p: &IndexPriceUpdate) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnIndexPrice);
        Ok(())
    }

    fn on_funding_rate(&mut self, _f: &FundingRateUpdate) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnFundingRate);
        Ok(())
    }

    fn on_instrument_status(&mut self, _s: &InstrumentStatus) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnInstrumentStatus);
        Ok(())
    }

    fn on_instrument_close(&mut self, _c: &InstrumentClose) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnInstrumentClose);
        Ok(())
    }

    fn on_signal(&mut self, _s: &Signal) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnSignal);
        Ok(())
    }

    fn on_order_initialized(&mut self, _e: &OrderInitialized) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderInitialized);
        Ok(())
    }

    fn on_order_submitted(&mut self, _e: &OrderSubmitted) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderSubmitted);
        Ok(())
    }

    fn on_order_accepted(&mut self, _e: &OrderAccepted) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderAccepted);
        Ok(())
    }

    fn on_order_rejected(&mut self, _e: &OrderRejected) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderRejected);
        Ok(())
    }

    fn on_order_filled(&mut self, _e: &OrderFilled) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderFilled);
        Ok(())
    }

    fn on_order_canceled(&mut self, _e: &OrderCanceled) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderCanceled);
        Ok(())
    }

    fn on_order_expired(&mut self, _e: &OrderExpired) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderExpired);
        Ok(())
    }

    fn on_order_triggered(&mut self, _e: &OrderTriggered) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderTriggered);
        Ok(())
    }

    fn on_order_denied(&mut self, _e: &OrderDenied) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderDenied);
        Ok(())
    }

    fn on_order_emulated(&mut self, _e: &OrderEmulated) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderEmulated);
        Ok(())
    }

    fn on_order_released(&mut self, _e: &OrderReleased) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderReleased);
        Ok(())
    }

    fn on_order_pending_update(&mut self, _e: &OrderPendingUpdate) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderPendingUpdate);
        Ok(())
    }

    fn on_order_pending_cancel(&mut self, _e: &OrderPendingCancel) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderPendingCancel);
        Ok(())
    }

    fn on_order_modify_rejected(&mut self, _e: &OrderModifyRejected) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderModifyRejected);
        Ok(())
    }

    fn on_order_cancel_rejected(&mut self, _e: &OrderCancelRejected) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderCancelRejected);
        Ok(())
    }

    fn on_order_updated(&mut self, _e: &OrderUpdated) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnOrderUpdated);
        Ok(())
    }

    fn on_position_opened(&mut self, _e: &PositionOpened) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnPositionOpened);
        Ok(())
    }

    fn on_position_changed(&mut self, _e: &PositionChanged) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnPositionChanged);
        Ok(())
    }

    fn on_position_closed(&mut self, _e: &PositionClosed) -> anyhow::Result<()> {
        bump_strategy(StrategyHook::OnPositionClosed);
        Ok(())
    }
}

fn instrument_id() -> InstrumentId {
    InstrumentId::from("ETH-USDT.BINANCE")
}

fn stub_trader_id() -> TraderId {
    order_stubs::trader_id()
}

fn stub_strategy_id() -> StrategyId {
    order_stubs::strategy_id_ema_cross()
}

fn stub_order_instrument_id() -> InstrumentId {
    order_stubs::instrument_id_btc_usdt()
}

fn stub_client_order_id() -> ClientOrderId {
    order_stubs::client_order_id()
}

fn stub_account_id() -> AccountId {
    order_stubs::account_id()
}

fn stub_venue_order_id() -> VenueOrderId {
    order_stubs::venue_order_id()
}

fn stub_uuid4() -> UUID4 {
    order_stubs::uuid4()
}

fn quote_tick_value() -> QuoteTick {
    QuoteTick::new(
        instrument_id(),
        Price::from("1500.00"),
        Price::from("1500.05"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::from(1u64),
        UnixNanos::from(2u64),
    )
}

fn mark_price_value() -> MarkPriceUpdate {
    MarkPriceUpdate::new(
        instrument_id(),
        Price::from("1500.00"),
        UnixNanos::from(1u64),
        UnixNanos::from(2u64),
    )
}

fn index_price_value() -> IndexPriceUpdate {
    IndexPriceUpdate::new(
        instrument_id(),
        Price::from("1500.00"),
        UnixNanos::from(1u64),
        UnixNanos::from(2u64),
    )
}

fn funding_rate_value() -> FundingRateUpdate {
    FundingRateUpdate::new(
        instrument_id(),
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1u64),
        UnixNanos::from(2u64),
    )
}

fn signal_value() -> Signal {
    Signal::new(
        Ustr::from("TestSignal"),
        "1.0".to_string(),
        UnixNanos::from(1u64),
        UnixNanos::from(2u64),
    )
}

fn position_opened_value() -> PositionOpened {
    PositionOpened {
        trader_id: TraderId::from("TESTER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: instrument_id(),
        position_id: PositionId::from("P-1"),
        account_id: AccountId::from("BINANCE-001"),
        opening_order_id: ClientOrderId::from("O-1"),
        entry: OrderSide::Buy,
        side: PositionSide::Long,
        signed_qty: 1.0,
        quantity: Quantity::from("1.0"),
        last_qty: Quantity::from("1.0"),
        last_px: Price::from("1500.00"),
        currency: Currency::USDT(),
        avg_px_open: 1500.0,
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(2u64),
    }
}

fn position_changed_value() -> PositionChanged {
    PositionChanged {
        trader_id: TraderId::from("TESTER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: instrument_id(),
        position_id: PositionId::from("P-1"),
        account_id: AccountId::from("BINANCE-001"),
        opening_order_id: ClientOrderId::from("O-1"),
        entry: OrderSide::Buy,
        side: PositionSide::Long,
        signed_qty: 2.0,
        quantity: Quantity::from("2.0"),
        peak_quantity: Quantity::from("2.0"),
        last_qty: Quantity::from("1.0"),
        last_px: Price::from("1500.50"),
        currency: Currency::USDT(),
        avg_px_open: 1500.0,
        avg_px_close: None,
        realized_return: 0.0,
        realized_pnl: None,
        unrealized_pnl: Money::new(0.0, Currency::USDT()),
        event_id: UUID4::new(),
        ts_opened: UnixNanos::from(1u64),
        ts_event: UnixNanos::from(2u64),
        ts_init: UnixNanos::from(3u64),
    }
}

fn position_closed_value() -> PositionClosed {
    PositionClosed {
        trader_id: TraderId::from("TESTER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: instrument_id(),
        position_id: PositionId::from("P-1"),
        account_id: AccountId::from("BINANCE-001"),
        opening_order_id: ClientOrderId::from("O-1"),
        closing_order_id: Some(ClientOrderId::from("O-2")),
        entry: OrderSide::Buy,
        side: PositionSide::Flat,
        signed_qty: 0.0,
        quantity: Quantity::from("1.0"),
        peak_quantity: Quantity::from("1.0"),
        last_qty: Quantity::from("1.0"),
        last_px: Price::from("1500.50"),
        currency: Currency::USDT(),
        avg_px_open: 1500.0,
        avg_px_close: Some(1500.50),
        realized_return: 0.0,
        realized_pnl: Some(Money::new(0.0, Currency::USDT())),
        unrealized_pnl: Money::new(0.0, Currency::USDT()),
        duration: 1,
        event_id: UUID4::new(),
        ts_opened: UnixNanos::from(1u64),
        ts_closed: Some(UnixNanos::from(2u64)),
        ts_event: UnixNanos::from(3u64),
        ts_init: UnixNanos::from(4u64),
    }
}

fn instrument_status_value() -> InstrumentStatus {
    stub_instrument_status()
}

fn order_filled_value() -> OrderFilled {
    order_stubs::order_filled(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_uuid4(),
    )
}

fn order_canceled_value() -> OrderCanceled {
    // No upstream stub; construct directly. Field order tracks the struct
    // definition in nautilus_model::events::order::canceled.
    OrderCanceled {
        trader_id: TraderId::from("TESTER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: instrument_id(),
        client_order_id: ClientOrderId::from("O-1"),
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(2u64),
        reconciliation: 0,
        venue_order_id: Some(VenueOrderId::from("V-1")),
        account_id: Some(AccountId::from("BINANCE-001")),
        causation_id: None,
    }
}

fn order_initialized_value() -> OrderInitialized {
    order_stubs::order_initialized_buy_limit(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_uuid4(),
    )
}

fn order_submitted_value() -> OrderSubmitted {
    order_stubs::order_submitted(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_account_id(),
        stub_uuid4(),
    )
}

fn order_accepted_value() -> OrderAccepted {
    order_stubs::order_accepted(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_account_id(),
        stub_venue_order_id(),
        stub_uuid4(),
    )
}

fn order_rejected_value() -> OrderRejected {
    order_stubs::order_rejected_insufficient_margin(
        stub_trader_id(),
        stub_account_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_uuid4(),
    )
}

fn order_expired_value() -> OrderExpired {
    order_stubs::order_expired(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_venue_order_id(),
        stub_account_id(),
        stub_uuid4(),
    )
}

fn order_triggered_value() -> OrderTriggered {
    order_stubs::order_triggered(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_venue_order_id(),
        stub_account_id(),
        stub_uuid4(),
    )
}

fn order_denied_value() -> OrderDenied {
    order_stubs::order_denied_max_submitted_rate(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_uuid4(),
    )
}

fn order_emulated_value() -> OrderEmulated {
    order_stubs::order_emulated(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_uuid4(),
    )
}

fn order_released_value() -> OrderReleased {
    order_stubs::order_released(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_uuid4(),
    )
}

fn order_pending_update_value() -> OrderPendingUpdate {
    order_stubs::order_pending_update(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_account_id(),
        stub_venue_order_id(),
        stub_uuid4(),
    )
}

fn order_pending_cancel_value() -> OrderPendingCancel {
    order_stubs::order_pending_cancel(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_account_id(),
        stub_venue_order_id(),
        stub_uuid4(),
    )
}

fn order_modify_rejected_value() -> OrderModifyRejected {
    order_stubs::order_modify_rejected(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_venue_order_id(),
        stub_account_id(),
        stub_uuid4(),
    )
}

fn order_cancel_rejected_value() -> OrderCancelRejected {
    order_stubs::order_cancel_rejected(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_venue_order_id(),
        stub_account_id(),
        stub_uuid4(),
    )
}

fn order_updated_value() -> OrderUpdated {
    order_stubs::order_updated(
        stub_trader_id(),
        stub_strategy_id(),
        stub_order_instrument_id(),
        stub_client_order_id(),
        stub_venue_order_id(),
        stub_account_id(),
        stub_uuid4(),
    )
}

#[rstest]
#[case::on_start(ActorHook::OnStart)]
#[case::on_stop(ActorHook::OnStop)]
#[case::on_resume(ActorHook::OnResume)]
#[case::on_reset(ActorHook::OnReset)]
#[case::on_dispose(ActorHook::OnDispose)]
#[case::on_degrade(ActorHook::OnDegrade)]
#[case::on_fault(ActorHook::OnFault)]
fn actor_lifecycle_thunk_dispatches_to_its_method(#[case] hook: ActorHook) {
    let _g = dispatch_lock();
    reset_actor_counters();
    let vt = actor_vtable::<HookCountingActor>();
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*vt };
    let host: *const HostVTable = std::ptr::null();
    let ctx: *const HostContext = std::ptr::null();
    // SAFETY: create returns a fresh handle; null pointers are fine since
    // HookCountingActor never deref's them.
    let handle = unsafe { generated_slot!(vt, create)(host, ctx, BorrowedStr::empty()) };

    let r = match hook {
        // SAFETY: handle is live for each branch below.
        ActorHook::OnStart => unsafe { generated_slot!(vt, on_start)(handle) },
        ActorHook::OnStop => unsafe { generated_slot!(vt, on_stop)(handle) },
        ActorHook::OnResume => unsafe { generated_slot!(vt, on_resume)(handle) },
        ActorHook::OnReset => unsafe { generated_slot!(vt, on_reset)(handle) },
        ActorHook::OnDispose => unsafe { generated_slot!(vt, on_dispose)(handle) },
        ActorHook::OnDegrade => unsafe { generated_slot!(vt, on_degrade)(handle) },
        ActorHook::OnFault => unsafe { generated_slot!(vt, on_fault)(handle) },
        _ => panic!("non-lifecycle hook"),
    };
    r.into_result().expect("lifecycle thunk failed");
    assert_only_actor_hook(hook);

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(handle);
    };
}

#[rstest]
#[case::on_time_event(ActorHook::OnTimeEvent)]
#[case::on_quote(ActorHook::OnQuote)]
#[case::on_trade(ActorHook::OnTrade)]
#[case::on_bar(ActorHook::OnBar)]
#[case::on_mark_price(ActorHook::OnMarkPrice)]
#[case::on_index_price(ActorHook::OnIndexPrice)]
#[case::on_funding_rate(ActorHook::OnFundingRate)]
#[case::on_instrument_status(ActorHook::OnInstrumentStatus)]
#[case::on_instrument_close(ActorHook::OnInstrumentClose)]
#[case::on_order_filled(ActorHook::OnOrderFilled)]
#[case::on_order_canceled(ActorHook::OnOrderCanceled)]
#[case::on_signal(ActorHook::OnSignal)]
fn actor_event_thunk_dispatches_to_its_method(#[case] hook: ActorHook) {
    let _g = dispatch_lock();
    reset_actor_counters();
    let vt = actor_vtable::<HookCountingActor>();
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*vt };
    let host: *const HostVTable = std::ptr::null();
    let ctx: *const HostContext = std::ptr::null();
    // SAFETY: create returns a fresh handle; null pointers are fine since
    // HookCountingActor never deref's them.
    let handle = unsafe { generated_slot!(vt, create)(host, ctx, BorrowedStr::empty()) };

    // Each match arm constructs the typed event locally and passes a
    // borrowed pointer. The temporary lives until the end of the match
    // statement, which is after the thunk call returns.
    let r = match hook {
        ActorHook::OnTimeEvent => {
            let v = TimeEvent::new(
                Ustr::from("TestAlarm"),
                UUID4::new(),
                UnixNanos::from(1u64),
                UnixNanos::from(2u64),
            );
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_time_event)(handle, &raw const v) }
        }
        ActorHook::OnQuote => {
            let v = quote_tick_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_quote)(handle, &raw const v) }
        }
        ActorHook::OnTrade => {
            let v = stub_trade_ethusdt_buyer();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_trade)(handle, &raw const v) }
        }
        ActorHook::OnBar => {
            let v = stub_bar();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_bar)(handle, &raw const v) }
        }
        ActorHook::OnMarkPrice => {
            let v = mark_price_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_mark_price)(handle, &raw const v) }
        }
        ActorHook::OnIndexPrice => {
            let v = index_price_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_index_price)(handle, &raw const v) }
        }
        ActorHook::OnFundingRate => {
            let v = funding_rate_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_funding_rate)(handle, &raw const v) }
        }
        ActorHook::OnInstrumentStatus => {
            let v = instrument_status_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_instrument_status)(handle, &raw const v) }
        }
        ActorHook::OnInstrumentClose => {
            let v = stub_instrument_close();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_instrument_close)(handle, &raw const v) }
        }
        ActorHook::OnOrderFilled => {
            let v = order_filled_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_filled)(handle, &raw const v) }
        }
        ActorHook::OnOrderCanceled => {
            let v = order_canceled_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_canceled)(handle, &raw const v) }
        }
        ActorHook::OnSignal => {
            let v = signal_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_signal)(handle, &raw const v) }
        }
        _ => panic!("non-event hook"),
    };
    r.into_result().expect("event thunk failed");
    assert_only_actor_hook(hook);

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(handle);
    };
}

#[rstest]
#[case::on_start(StrategyHook::OnStart)]
#[case::on_stop(StrategyHook::OnStop)]
#[case::on_resume(StrategyHook::OnResume)]
#[case::on_reset(StrategyHook::OnReset)]
#[case::on_dispose(StrategyHook::OnDispose)]
#[case::on_degrade(StrategyHook::OnDegrade)]
#[case::on_fault(StrategyHook::OnFault)]
fn strategy_lifecycle_thunk_dispatches_to_its_method(#[case] hook: StrategyHook) {
    let _g = dispatch_lock();
    reset_strategy_counters();
    let vt = strategy_vtable::<HookCountingStrategy>();
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*vt };
    let host: *const HostVTable = std::ptr::null();
    let ctx: *const HostContext = std::ptr::null();
    // SAFETY: create returns a fresh handle; null pointers are fine since
    // HookCountingStrategy never deref's them.
    let handle = unsafe { generated_slot!(vt, create)(host, ctx, BorrowedStr::empty()) };

    let r = match hook {
        // SAFETY: handle is live for each branch below.
        StrategyHook::OnStart => unsafe { generated_slot!(vt, on_start)(handle) },
        StrategyHook::OnStop => unsafe { generated_slot!(vt, on_stop)(handle) },
        StrategyHook::OnResume => unsafe { generated_slot!(vt, on_resume)(handle) },
        StrategyHook::OnReset => unsafe { generated_slot!(vt, on_reset)(handle) },
        StrategyHook::OnDispose => unsafe { generated_slot!(vt, on_dispose)(handle) },
        StrategyHook::OnDegrade => unsafe { generated_slot!(vt, on_degrade)(handle) },
        StrategyHook::OnFault => unsafe { generated_slot!(vt, on_fault)(handle) },
        _ => panic!("non-lifecycle hook"),
    };
    r.into_result().expect("lifecycle thunk failed");
    assert_only_strategy_hook(hook);

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(handle);
    };
}

#[rstest]
#[case::on_time_event(StrategyHook::OnTimeEvent)]
#[case::on_quote(StrategyHook::OnQuote)]
#[case::on_trade(StrategyHook::OnTrade)]
#[case::on_bar(StrategyHook::OnBar)]
#[case::on_mark_price(StrategyHook::OnMarkPrice)]
#[case::on_index_price(StrategyHook::OnIndexPrice)]
#[case::on_funding_rate(StrategyHook::OnFundingRate)]
#[case::on_instrument_status(StrategyHook::OnInstrumentStatus)]
#[case::on_instrument_close(StrategyHook::OnInstrumentClose)]
#[case::on_signal(StrategyHook::OnSignal)]
#[case::on_order_initialized(StrategyHook::OnOrderInitialized)]
#[case::on_order_submitted(StrategyHook::OnOrderSubmitted)]
#[case::on_order_accepted(StrategyHook::OnOrderAccepted)]
#[case::on_order_rejected(StrategyHook::OnOrderRejected)]
#[case::on_order_filled(StrategyHook::OnOrderFilled)]
#[case::on_order_canceled(StrategyHook::OnOrderCanceled)]
#[case::on_order_expired(StrategyHook::OnOrderExpired)]
#[case::on_order_triggered(StrategyHook::OnOrderTriggered)]
#[case::on_order_denied(StrategyHook::OnOrderDenied)]
#[case::on_order_emulated(StrategyHook::OnOrderEmulated)]
#[case::on_order_released(StrategyHook::OnOrderReleased)]
#[case::on_order_pending_update(StrategyHook::OnOrderPendingUpdate)]
#[case::on_order_pending_cancel(StrategyHook::OnOrderPendingCancel)]
#[case::on_order_modify_rejected(StrategyHook::OnOrderModifyRejected)]
#[case::on_order_cancel_rejected(StrategyHook::OnOrderCancelRejected)]
#[case::on_order_updated(StrategyHook::OnOrderUpdated)]
#[case::on_position_opened(StrategyHook::OnPositionOpened)]
#[case::on_position_changed(StrategyHook::OnPositionChanged)]
#[case::on_position_closed(StrategyHook::OnPositionClosed)]
fn strategy_event_thunk_dispatches_to_its_method(#[case] hook: StrategyHook) {
    let _g = dispatch_lock();
    reset_strategy_counters();
    let vt = strategy_vtable::<HookCountingStrategy>();
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*vt };
    let host: *const HostVTable = std::ptr::null();
    let ctx: *const HostContext = std::ptr::null();
    // SAFETY: create returns a fresh handle; null pointers are fine since
    // HookCountingStrategy never deref's them.
    let handle = unsafe { generated_slot!(vt, create)(host, ctx, BorrowedStr::empty()) };

    // Each match arm constructs the typed event locally and passes a
    // borrowed pointer. The temporary lives until the end of the match
    // statement, which is after the thunk call returns.
    let r = match hook {
        StrategyHook::OnTimeEvent => {
            let v = TimeEvent::new(
                Ustr::from("TestAlarm"),
                UUID4::new(),
                UnixNanos::from(1u64),
                UnixNanos::from(2u64),
            );
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_time_event)(handle, &raw const v) }
        }
        StrategyHook::OnQuote => {
            let v = quote_tick_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_quote)(handle, &raw const v) }
        }
        StrategyHook::OnTrade => {
            let v = stub_trade_ethusdt_buyer();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_trade)(handle, &raw const v) }
        }
        StrategyHook::OnBar => {
            let v = stub_bar();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_bar)(handle, &raw const v) }
        }
        StrategyHook::OnMarkPrice => {
            let v = mark_price_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_mark_price)(handle, &raw const v) }
        }
        StrategyHook::OnIndexPrice => {
            let v = index_price_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_index_price)(handle, &raw const v) }
        }
        StrategyHook::OnFundingRate => {
            let v = funding_rate_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_funding_rate)(handle, &raw const v) }
        }
        StrategyHook::OnInstrumentStatus => {
            let v = instrument_status_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_instrument_status)(handle, &raw const v) }
        }
        StrategyHook::OnInstrumentClose => {
            let v = stub_instrument_close();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_instrument_close)(handle, &raw const v) }
        }
        StrategyHook::OnSignal => {
            let v = signal_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_signal)(handle, &raw const v) }
        }
        StrategyHook::OnOrderInitialized => {
            let v = order_initialized_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_initialized)(handle, &raw const v) }
        }
        StrategyHook::OnOrderSubmitted => {
            let v = order_submitted_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_submitted)(handle, &raw const v) }
        }
        StrategyHook::OnOrderAccepted => {
            let v = order_accepted_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_accepted)(handle, &raw const v) }
        }
        StrategyHook::OnOrderRejected => {
            let v = order_rejected_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_rejected)(handle, &raw const v) }
        }
        StrategyHook::OnOrderFilled => {
            let v = order_filled_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_filled)(handle, &raw const v) }
        }
        StrategyHook::OnOrderCanceled => {
            let v = order_canceled_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_canceled)(handle, &raw const v) }
        }
        StrategyHook::OnOrderExpired => {
            let v = order_expired_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_expired)(handle, &raw const v) }
        }
        StrategyHook::OnOrderTriggered => {
            let v = order_triggered_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_triggered)(handle, &raw const v) }
        }
        StrategyHook::OnOrderDenied => {
            let v = order_denied_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_denied)(handle, &raw const v) }
        }
        StrategyHook::OnOrderEmulated => {
            let v = order_emulated_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_emulated)(handle, &raw const v) }
        }
        StrategyHook::OnOrderReleased => {
            let v = order_released_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_released)(handle, &raw const v) }
        }
        StrategyHook::OnOrderPendingUpdate => {
            let v = order_pending_update_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_pending_update)(handle, &raw const v) }
        }
        StrategyHook::OnOrderPendingCancel => {
            let v = order_pending_cancel_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_pending_cancel)(handle, &raw const v) }
        }
        StrategyHook::OnOrderModifyRejected => {
            let v = order_modify_rejected_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_modify_rejected)(handle, &raw const v) }
        }
        StrategyHook::OnOrderCancelRejected => {
            let v = order_cancel_rejected_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_cancel_rejected)(handle, &raw const v) }
        }
        StrategyHook::OnOrderUpdated => {
            let v = order_updated_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_order_updated)(handle, &raw const v) }
        }
        StrategyHook::OnPositionOpened => {
            let v = position_opened_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_position_opened)(handle, &raw const v) }
        }
        StrategyHook::OnPositionChanged => {
            let v = position_changed_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_position_changed)(handle, &raw const v) }
        }
        StrategyHook::OnPositionClosed => {
            let v = position_closed_value();
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_position_closed)(handle, &raw const v) }
        }
        _ => panic!("non-event hook"),
    };
    r.into_result().expect("event thunk failed");
    assert_only_strategy_hook(hook);

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(handle);
    };
}
