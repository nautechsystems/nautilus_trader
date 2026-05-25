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
//
//! Per-thunk panic and error propagation tests.
//!
//! Every fallible thunk in `surfaces::{actor, strategy, custom_data}`
//! wraps its trait-method invocation in [`panic::guard`]. The guard maps
//! a panic to [`PluginError::panic`] so the panic surfaces as a returned
//! error instead of unwinding across the FFI boundary (which is
//! undefined behaviour). Similarly, an `anyhow::Error` returned from a
//! trait method maps to:
//!
//! - `PluginErrorCode::Generic` for actor and strategy callbacks
//!   (constructed by their `ok_or_err` helpers).
//! - `PluginErrorCode::SerializationFailed` for the custom-data
//!   serialise/deserialise slots (`schema_ipc`, `from_json`,
//!   `encode_batch`, `decode_batch`, `to_json`).
//!
//! Unit tests for [`panic::guard`] live in `src/panic.rs`. These tests
//! cover the integration path: a thunk dispatching to a panicking or
//! erroring trait method on a real, generated vtable returns the
//! expected [`PluginErrorCode`] and the host can inspect the message.
//!
//! `guard_infallible` thunks (`create`, `drop_handle`, `ts_event`,
//! `ts_init`, `clone_handle`, `eq_handles`) abort the process on panic,
//! so they cannot be tested from inside the same test binary; their
//! contract is covered indirectly by the `src/panic.rs` unit tests.

#![allow(unsafe_code)]

use std::cell::Cell;

use nautilus_common::{signal::Signal, timer::TimeEvent};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, OptionChainSlice, OptionGreeks, OrderBookDelta, OrderBookDeltas,
        QuoteTick, TradeTick,
        stubs::{
            stub_bar, stub_deltas, stub_instrument_close, stub_instrument_status,
            stub_trade_ethusdt_buyer,
        },
    },
    enums::{GreeksConvention, OrderSide, PositionSide},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, PositionChanged, PositionClosed, PositionOpened, order::stubs as order_stubs,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OptionSeriesId, PositionId, StrategyId, TraderId,
        Venue, VenueOrderId,
    },
    instruments::{InstrumentAny, stubs::currency_pair_ethusdt},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_plugin::{
    boundary::{BorrowedStr, PluginError, PluginErrorCode, PluginResult, Slice},
    host::{HostContext, HostVTable},
    surfaces::{
        actor::{PluginActor, actor_vtable},
        book::OrderBookDeltasHandle,
        custom_data::{
            CustomDataHandle, MetadataEntry, PluginCustomData, PluginCustomDataRef,
            custom_data_vtable,
        },
        instrument::InstrumentAnyHandle,
        option_chain::OptionChainSliceHandle,
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

// Operating mode for the misbehaving plug-in types: panic vs return Err.
// Each thunk under test reads this to decide what failure mode to trigger
// inside the trait method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Panic,
    Err,
}

thread_local! {
    static MODE: Cell<Mode> = const { Cell::new(Mode::Panic) };
}

fn set_mode(mode: Mode) {
    MODE.with(|m| m.set(mode));
}

#[allow(
    clippy::panic_in_result_fn,
    reason = "fail() deliberately panics on Mode::Panic to drive the panic-recovery path through `guard()`"
)]
fn fail<T>() -> anyhow::Result<T> {
    match MODE.with(Cell::get) {
        Mode::Panic => panic!("panic-from-trait-method"),
        Mode::Err => anyhow::bail!("err-from-trait-method"),
    }
}

// Custom-data type whose every fallible trait method panics or errs
// based on the thread-local mode. Used to drive the `guard()`-wrapped
// custom-data thunks.
#[derive(Clone, PartialEq)]
struct MisbehavingTick;

impl PluginCustomData for MisbehavingTick {
    const TYPE_NAME: &'static str = "MisbehavingTick";

    fn ts_event(&self) -> u64 {
        // ts_event is wrapped in guard_infallible; tested only indirectly.
        0
    }

    fn ts_init(&self) -> u64 {
        0
    }

    fn to_json(&self) -> anyhow::Result<Vec<u8>> {
        fail()
    }

    fn from_json(_payload: &[u8]) -> anyhow::Result<Self> {
        fail()
    }

    fn schema_ipc() -> anyhow::Result<Vec<u8>> {
        fail()
    }

    fn encode_batch(_items: &[&Self]) -> anyhow::Result<Vec<u8>> {
        fail()
    }

    fn decode_batch(
        _ipc_bytes: &[u8],
        _metadata: &[(String, String)],
    ) -> anyhow::Result<Vec<Self>> {
        fail()
    }
}

// Actor whose every callback panics or errs based on the thread-local
// mode. Used to drive the `guard()`-wrapped actor lifecycle and event
// thunks.
struct MisbehavingActor;

impl PluginActor for MisbehavingActor {
    const TYPE_NAME: &'static str = "MisbehavingActor";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
        Self
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_stop(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_resume(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_reset(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_dispose(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_degrade(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_fault(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        fail()
    }
    fn on_data(&mut self, _data: PluginCustomDataRef) -> anyhow::Result<()> {
        fail()
    }
    fn on_instrument(&mut self, _instrument: &InstrumentAny) -> anyhow::Result<()> {
        fail()
    }
    fn on_book_deltas(&mut self, _deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        fail()
    }
    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        fail()
    }
    fn on_trade(&mut self, _trade: &TradeTick) -> anyhow::Result<()> {
        fail()
    }
    fn on_bar(&mut self, _bar: &Bar) -> anyhow::Result<()> {
        fail()
    }
    fn on_mark_price(&mut self, _mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        fail()
    }
    fn on_index_price(&mut self, _index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        fail()
    }
    fn on_funding_rate(&mut self, _funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        fail()
    }

    fn on_option_greeks(&mut self, _greeks: &OptionGreeks) -> anyhow::Result<()> {
        fail()
    }

    fn on_option_chain(&mut self, _chain: &OptionChainSlice) -> anyhow::Result<()> {
        fail()
    }

    fn on_instrument_status(&mut self, _status: &InstrumentStatus) -> anyhow::Result<()> {
        fail()
    }
    fn on_instrument_close(&mut self, _close: &InstrumentClose) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_filled(&mut self, _event: &OrderFilled) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_canceled(&mut self, _event: &OrderCanceled) -> anyhow::Result<()> {
        fail()
    }
    fn on_signal(&mut self, _signal: &Signal) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_book_deltas(&mut self, _d: &[OrderBookDelta]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_quotes(&mut self, _q: &[QuoteTick]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_trades(&mut self, _t: &[TradeTick]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_bars(&mut self, _b: &[Bar]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_mark_prices(&mut self, _p: &[MarkPriceUpdate]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_index_prices(&mut self, _p: &[IndexPriceUpdate]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_funding_rates(&mut self, _f: &[FundingRateUpdate]) -> anyhow::Result<()> {
        fail()
    }
}

// Strategy whose every callback panics or errs based on the thread-local
// mode. Used to drive the `guard()`-wrapped strategy lifecycle and
// event thunks.
struct MisbehavingStrategy;

// SAFETY: zero-sized type; the trait requires Send.
unsafe impl Send for MisbehavingStrategy {}

impl PluginStrategy for MisbehavingStrategy {
    const TYPE_NAME: &'static str = "MisbehavingStrategy";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
        Self
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_stop(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_resume(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_reset(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_dispose(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_degrade(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_fault(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        fail()
    }
    fn on_data(&mut self, _data: PluginCustomDataRef) -> anyhow::Result<()> {
        fail()
    }
    fn on_instrument(&mut self, _i: &InstrumentAny) -> anyhow::Result<()> {
        fail()
    }
    fn on_book_deltas(&mut self, _d: &OrderBookDeltas) -> anyhow::Result<()> {
        fail()
    }
    fn on_quote(&mut self, _q: &QuoteTick) -> anyhow::Result<()> {
        fail()
    }
    fn on_trade(&mut self, _t: &TradeTick) -> anyhow::Result<()> {
        fail()
    }
    fn on_bar(&mut self, _b: &Bar) -> anyhow::Result<()> {
        fail()
    }
    fn on_mark_price(&mut self, _p: &MarkPriceUpdate) -> anyhow::Result<()> {
        fail()
    }
    fn on_index_price(&mut self, _p: &IndexPriceUpdate) -> anyhow::Result<()> {
        fail()
    }
    fn on_funding_rate(&mut self, _f: &FundingRateUpdate) -> anyhow::Result<()> {
        fail()
    }

    fn on_option_greeks(&mut self, _g: &OptionGreeks) -> anyhow::Result<()> {
        fail()
    }

    fn on_option_chain(&mut self, _c: &OptionChainSlice) -> anyhow::Result<()> {
        fail()
    }

    fn on_instrument_status(&mut self, _s: &InstrumentStatus) -> anyhow::Result<()> {
        fail()
    }
    fn on_instrument_close(&mut self, _c: &InstrumentClose) -> anyhow::Result<()> {
        fail()
    }
    fn on_signal(&mut self, _s: &Signal) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_initialized(&mut self, _e: &OrderInitialized) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_submitted(&mut self, _e: &OrderSubmitted) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_accepted(&mut self, _e: &OrderAccepted) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_rejected(&mut self, _e: &OrderRejected) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_filled(&mut self, _e: &OrderFilled) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_canceled(&mut self, _e: &OrderCanceled) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_expired(&mut self, _e: &OrderExpired) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_triggered(&mut self, _e: &OrderTriggered) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_denied(&mut self, _e: &OrderDenied) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_emulated(&mut self, _e: &OrderEmulated) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_released(&mut self, _e: &OrderReleased) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_pending_update(&mut self, _e: &OrderPendingUpdate) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_pending_cancel(&mut self, _e: &OrderPendingCancel) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_modify_rejected(&mut self, _e: &OrderModifyRejected) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_cancel_rejected(&mut self, _e: &OrderCancelRejected) -> anyhow::Result<()> {
        fail()
    }
    fn on_order_updated(&mut self, _e: &OrderUpdated) -> anyhow::Result<()> {
        fail()
    }
    fn on_position_opened(&mut self, _e: &PositionOpened) -> anyhow::Result<()> {
        fail()
    }
    fn on_position_changed(&mut self, _e: &PositionChanged) -> anyhow::Result<()> {
        fail()
    }
    fn on_position_closed(&mut self, _e: &PositionClosed) -> anyhow::Result<()> {
        fail()
    }
    fn on_market_exit(&mut self) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_book_deltas(&mut self, _d: &[OrderBookDelta]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_quotes(&mut self, _q: &[QuoteTick]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_trades(&mut self, _t: &[TradeTick]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_bars(&mut self, _b: &[Bar]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_mark_prices(&mut self, _p: &[MarkPriceUpdate]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_index_prices(&mut self, _p: &[IndexPriceUpdate]) -> anyhow::Result<()> {
        fail()
    }
    fn on_historical_funding_rates(&mut self, _f: &[FundingRateUpdate]) -> anyhow::Result<()> {
        fail()
    }
}

// `OwnedBytes` does not impl `Debug`, so `Result::unwrap_err` does not
// compile for `PluginResult<OwnedBytes>` returns. This helper expects an
// `Err` and returns the carried `PluginError` without requiring `Debug`.
fn expect_err<T>(r: PluginResult<T>) -> PluginError {
    match r.into_result() {
        Ok(_) => panic!("expected an error from the thunk"),
        Err(e) => e,
    }
}

fn assert_failure_code(
    actual: &PluginError,
    mode: Mode,
    panic_code: PluginErrorCode,
    err_code: PluginErrorCode,
) {
    let expected_code = match mode {
        Mode::Panic => panic_code,
        Mode::Err => err_code,
    };
    assert_eq!(actual.code, expected_code, "wrong error code");
    let msg = actual.message_string();
    let expected_msg = match mode {
        Mode::Panic => "panic-from-trait-method",
        Mode::Err => "err-from-trait-method",
    };
    assert!(
        msg.contains(expected_msg),
        "error message {msg:?} should contain {expected_msg:?}",
    );
}

#[rstest]
#[case::panic(Mode::Panic)]
#[case::err(Mode::Err)]
fn custom_data_schema_ipc_thunk_propagates_failure(#[case] mode: Mode) {
    set_mode(mode);
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*custom_data_vtable::<MisbehavingTick>() };
    // SAFETY: schema_ipc takes no arguments.
    let r = unsafe { generated_slot!(vt, schema_ipc)() };
    let err = expect_err(r);
    assert_failure_code(
        &err,
        mode,
        PluginErrorCode::Panic,
        PluginErrorCode::SerializationFailed,
    );
}

#[rstest]
#[case::panic(Mode::Panic)]
#[case::err(Mode::Err)]
fn custom_data_from_json_thunk_propagates_failure(#[case] mode: Mode) {
    set_mode(mode);
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*custom_data_vtable::<MisbehavingTick>() };
    let payload = BorrowedStr::from_str("ignored");
    // SAFETY: payload outlives the call.
    let r = unsafe { generated_slot!(vt, from_json)(payload) };
    // from_json returns *mut CustomDataHandle, not OwnedBytes, so it has
    // Debug; use the plain helper anyway for symmetry.
    let err = match r.into_result() {
        Ok(_) => panic!("expected an error from from_json"),
        Err(e) => e,
    };
    assert_failure_code(
        &err,
        mode,
        PluginErrorCode::Panic,
        PluginErrorCode::SerializationFailed,
    );
}

#[rstest]
#[case::panic(Mode::Panic)]
#[case::err(Mode::Err)]
fn custom_data_encode_batch_thunk_propagates_failure(#[case] mode: Mode) {
    set_mode(mode);
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*custom_data_vtable::<MisbehavingTick>() };
    let handles: [*const CustomDataHandle; 0] = [];
    let handles_slice = Slice::from_slice(&handles);
    // SAFETY: handles slice outlives the call.
    let r = unsafe { generated_slot!(vt, encode_batch)(handles_slice) };
    let err = expect_err(r);
    assert_failure_code(
        &err,
        mode,
        PluginErrorCode::Panic,
        PluginErrorCode::SerializationFailed,
    );
}

#[rstest]
#[case::panic(Mode::Panic)]
#[case::err(Mode::Err)]
fn custom_data_decode_batch_thunk_propagates_failure(#[case] mode: Mode) {
    set_mode(mode);
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*custom_data_vtable::<MisbehavingTick>() };
    let bytes: [u8; 0] = [];
    let bytes_slice = Slice::from_slice(&bytes);
    let md: [MetadataEntry<'_>; 0] = [];
    let md_slice = Slice::from_slice(&md);
    // SAFETY: slices outlive the call.
    let r = unsafe { generated_slot!(vt, decode_batch)(bytes_slice, md_slice) };
    let err = expect_err(r);
    assert_failure_code(
        &err,
        mode,
        PluginErrorCode::Panic,
        PluginErrorCode::SerializationFailed,
    );
}

#[rstest]
#[case::panic(Mode::Panic)]
#[case::err(Mode::Err)]
fn custom_data_to_json_thunk_propagates_failure(#[case] mode: Mode) {
    set_mode(mode);
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*custom_data_vtable::<MisbehavingTick>() };
    // Build a real handle via a sibling type so we can drive to_json on a
    // live `*const T`.
    let value = Box::new(MisbehavingTick);
    let handle = Box::into_raw(value).cast::<CustomDataHandle>();
    // SAFETY: handle is live.
    let r = unsafe { generated_slot!(vt, to_json)(handle) };
    let err = expect_err(r);
    assert_failure_code(
        &err,
        mode,
        PluginErrorCode::Panic,
        PluginErrorCode::SerializationFailed,
    );
    // SAFETY: handle still owns the boxed value; drop_handle frees it.
    unsafe {
        generated_slot!(vt, drop_handle)(handle);
    };
}

// `On` prefix mirrors the trait method names; clippy's
// `enum_variant_names` would otherwise object to the shared prefix.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug)]
enum ActorThunkUnderTest {
    OnStart,
    OnStop,
    OnResume,
    OnReset,
    OnDispose,
    OnDegrade,
    OnFault,
    OnTimeEvent,
    OnData,
    OnInstrument,
    OnBookDeltas,
    OnQuote,
    OnTrade,
    OnBar,
    OnMarkPrice,
    OnIndexPrice,
    OnFundingRate,
    OnOptionGreeks,
    OnOptionChain,
    OnInstrumentStatus,
    OnInstrumentClose,
    OnOrderFilled,
    OnOrderCanceled,
    OnSignal,
    OnHistoricalBookDeltas,
    OnHistoricalQuotes,
    OnHistoricalTrades,
    OnHistoricalBars,
    OnHistoricalMarkPrices,
    OnHistoricalIndexPrices,
    OnHistoricalFundingRates,
}

fn drive_actor_thunk(thunk: ActorThunkUnderTest) -> PluginResult<()> {
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*actor_vtable::<MisbehavingActor>() };
    let host: *const HostVTable = std::ptr::null();
    let ctx: *const HostContext = std::ptr::null();
    // SAFETY: MisbehavingActor::new never derefs host or ctx.
    let handle = unsafe { generated_slot!(vt, create)(host, ctx, BorrowedStr::empty()) };

    let r = match thunk {
        // SAFETY: handle is live for each branch below.
        ActorThunkUnderTest::OnStart => unsafe { generated_slot!(vt, on_start)(handle) },
        ActorThunkUnderTest::OnStop => unsafe { generated_slot!(vt, on_stop)(handle) },
        ActorThunkUnderTest::OnResume => unsafe { generated_slot!(vt, on_resume)(handle) },
        ActorThunkUnderTest::OnReset => unsafe { generated_slot!(vt, on_reset)(handle) },
        ActorThunkUnderTest::OnDispose => unsafe { generated_slot!(vt, on_dispose)(handle) },
        ActorThunkUnderTest::OnDegrade => unsafe { generated_slot!(vt, on_degrade)(handle) },
        ActorThunkUnderTest::OnFault => unsafe { generated_slot!(vt, on_fault)(handle) },
        ActorThunkUnderTest::OnTimeEvent => {
            let v = TimeEvent::new(
                Ustr::from("TestAlarm"),
                UUID4::new(),
                UnixNanos::from(1u64),
                UnixNanos::from(2u64),
            );
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_time_event)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnData => {
            let data_handle = custom_data_handle();
            let data = plugin_custom_data_ref(data_handle.cast_const());
            // SAFETY: both handles are live for the duration of the call.
            let r = unsafe { generated_slot!(vt, on_data)(handle, data) };
            drop_custom_data_handle(data_handle);
            r
        }
        ActorThunkUnderTest::OnInstrument => {
            let v = InstrumentAnyHandle::new(InstrumentAny::CurrencyPair(currency_pair_ethusdt()));
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_instrument)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnBookDeltas => {
            let v = OrderBookDeltasHandle::new(stub_deltas());
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_book_deltas)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnQuote => {
            let v = quote_tick_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_quote)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnTrade => {
            let v = stub_trade_ethusdt_buyer();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_trade)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnBar => {
            let v = stub_bar();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_bar)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnMarkPrice => {
            let v = mark_price_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_mark_price)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnIndexPrice => {
            let v = index_price_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_index_price)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnFundingRate => {
            let v = funding_rate_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_funding_rate)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnOptionGreeks => {
            let v = option_greeks_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_option_greeks)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnOptionChain => {
            let v = OptionChainSliceHandle::new(option_chain_value());
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_option_chain)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnInstrumentStatus => {
            let v = stub_instrument_status();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_instrument_status)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnInstrumentClose => {
            let v = stub_instrument_close();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_instrument_close)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnOrderFilled => {
            let v = order_filled_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_filled)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnOrderCanceled => {
            let v = order_canceled_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_canceled)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnSignal => {
            let v = signal_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_signal)(handle, &raw const v) }
        }
        ActorThunkUnderTest::OnHistoricalBookDeltas => {
            let v = stub_deltas();
            let s = Slice::from_slice(&v.deltas);
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_historical_book_deltas)(handle, s) }
        }
        ActorThunkUnderTest::OnHistoricalQuotes => {
            let v = vec![quote_tick_value()];
            let s = Slice::from_slice(&v);
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_historical_quotes)(handle, s) }
        }
        ActorThunkUnderTest::OnHistoricalTrades => {
            let v = vec![stub_trade_ethusdt_buyer()];
            let s = Slice::from_slice(&v);
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_historical_trades)(handle, s) }
        }
        ActorThunkUnderTest::OnHistoricalBars => {
            let v = vec![stub_bar()];
            let s = Slice::from_slice(&v);
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_historical_bars)(handle, s) }
        }
        ActorThunkUnderTest::OnHistoricalMarkPrices => {
            let v = vec![mark_price_value()];
            let s = Slice::from_slice(&v);
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_historical_mark_prices)(handle, s) }
        }
        ActorThunkUnderTest::OnHistoricalIndexPrices => {
            let v = vec![index_price_value()];
            let s = Slice::from_slice(&v);
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_historical_index_prices)(handle, s) }
        }
        ActorThunkUnderTest::OnHistoricalFundingRates => {
            let v = vec![funding_rate_value()];
            let s = Slice::from_slice(&v);
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_historical_funding_rates)(handle, s) }
        }
    };

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(handle);
    };
    r
}

#[rstest]
#[case::on_start_panic(ActorThunkUnderTest::OnStart, Mode::Panic)]
#[case::on_start_err(ActorThunkUnderTest::OnStart, Mode::Err)]
#[case::on_stop_panic(ActorThunkUnderTest::OnStop, Mode::Panic)]
#[case::on_stop_err(ActorThunkUnderTest::OnStop, Mode::Err)]
#[case::on_resume_panic(ActorThunkUnderTest::OnResume, Mode::Panic)]
#[case::on_resume_err(ActorThunkUnderTest::OnResume, Mode::Err)]
#[case::on_reset_panic(ActorThunkUnderTest::OnReset, Mode::Panic)]
#[case::on_reset_err(ActorThunkUnderTest::OnReset, Mode::Err)]
#[case::on_dispose_panic(ActorThunkUnderTest::OnDispose, Mode::Panic)]
#[case::on_dispose_err(ActorThunkUnderTest::OnDispose, Mode::Err)]
#[case::on_degrade_panic(ActorThunkUnderTest::OnDegrade, Mode::Panic)]
#[case::on_degrade_err(ActorThunkUnderTest::OnDegrade, Mode::Err)]
#[case::on_fault_panic(ActorThunkUnderTest::OnFault, Mode::Panic)]
#[case::on_fault_err(ActorThunkUnderTest::OnFault, Mode::Err)]
#[case::on_time_event_panic(ActorThunkUnderTest::OnTimeEvent, Mode::Panic)]
#[case::on_time_event_err(ActorThunkUnderTest::OnTimeEvent, Mode::Err)]
#[case::on_data_panic(ActorThunkUnderTest::OnData, Mode::Panic)]
#[case::on_data_err(ActorThunkUnderTest::OnData, Mode::Err)]
#[case::on_instrument_panic(ActorThunkUnderTest::OnInstrument, Mode::Panic)]
#[case::on_instrument_err(ActorThunkUnderTest::OnInstrument, Mode::Err)]
#[case::on_book_deltas_panic(ActorThunkUnderTest::OnBookDeltas, Mode::Panic)]
#[case::on_book_deltas_err(ActorThunkUnderTest::OnBookDeltas, Mode::Err)]
#[case::on_quote_panic(ActorThunkUnderTest::OnQuote, Mode::Panic)]
#[case::on_quote_err(ActorThunkUnderTest::OnQuote, Mode::Err)]
#[case::on_trade_panic(ActorThunkUnderTest::OnTrade, Mode::Panic)]
#[case::on_trade_err(ActorThunkUnderTest::OnTrade, Mode::Err)]
#[case::on_bar_panic(ActorThunkUnderTest::OnBar, Mode::Panic)]
#[case::on_bar_err(ActorThunkUnderTest::OnBar, Mode::Err)]
#[case::on_mark_price_panic(ActorThunkUnderTest::OnMarkPrice, Mode::Panic)]
#[case::on_mark_price_err(ActorThunkUnderTest::OnMarkPrice, Mode::Err)]
#[case::on_index_price_panic(ActorThunkUnderTest::OnIndexPrice, Mode::Panic)]
#[case::on_index_price_err(ActorThunkUnderTest::OnIndexPrice, Mode::Err)]
#[case::on_funding_rate_panic(ActorThunkUnderTest::OnFundingRate, Mode::Panic)]
#[case::on_funding_rate_err(ActorThunkUnderTest::OnFundingRate, Mode::Err)]
#[case::on_option_greeks_panic(ActorThunkUnderTest::OnOptionGreeks, Mode::Panic)]
#[case::on_option_greeks_err(ActorThunkUnderTest::OnOptionGreeks, Mode::Err)]
#[case::on_option_chain_panic(ActorThunkUnderTest::OnOptionChain, Mode::Panic)]
#[case::on_option_chain_err(ActorThunkUnderTest::OnOptionChain, Mode::Err)]
#[case::on_instrument_status_panic(ActorThunkUnderTest::OnInstrumentStatus, Mode::Panic)]
#[case::on_instrument_status_err(ActorThunkUnderTest::OnInstrumentStatus, Mode::Err)]
#[case::on_instrument_close_panic(ActorThunkUnderTest::OnInstrumentClose, Mode::Panic)]
#[case::on_instrument_close_err(ActorThunkUnderTest::OnInstrumentClose, Mode::Err)]
#[case::on_order_filled_panic(ActorThunkUnderTest::OnOrderFilled, Mode::Panic)]
#[case::on_order_filled_err(ActorThunkUnderTest::OnOrderFilled, Mode::Err)]
#[case::on_order_canceled_panic(ActorThunkUnderTest::OnOrderCanceled, Mode::Panic)]
#[case::on_order_canceled_err(ActorThunkUnderTest::OnOrderCanceled, Mode::Err)]
#[case::on_signal_panic(ActorThunkUnderTest::OnSignal, Mode::Panic)]
#[case::on_signal_err(ActorThunkUnderTest::OnSignal, Mode::Err)]
#[case::on_historical_book_deltas_panic(ActorThunkUnderTest::OnHistoricalBookDeltas, Mode::Panic)]
#[case::on_historical_book_deltas_err(ActorThunkUnderTest::OnHistoricalBookDeltas, Mode::Err)]
#[case::on_historical_quotes_panic(ActorThunkUnderTest::OnHistoricalQuotes, Mode::Panic)]
#[case::on_historical_quotes_err(ActorThunkUnderTest::OnHistoricalQuotes, Mode::Err)]
#[case::on_historical_trades_panic(ActorThunkUnderTest::OnHistoricalTrades, Mode::Panic)]
#[case::on_historical_trades_err(ActorThunkUnderTest::OnHistoricalTrades, Mode::Err)]
#[case::on_historical_bars_panic(ActorThunkUnderTest::OnHistoricalBars, Mode::Panic)]
#[case::on_historical_bars_err(ActorThunkUnderTest::OnHistoricalBars, Mode::Err)]
#[case::on_historical_mark_prices_panic(ActorThunkUnderTest::OnHistoricalMarkPrices, Mode::Panic)]
#[case::on_historical_mark_prices_err(ActorThunkUnderTest::OnHistoricalMarkPrices, Mode::Err)]
#[case::on_historical_index_prices_panic(ActorThunkUnderTest::OnHistoricalIndexPrices, Mode::Panic)]
#[case::on_historical_index_prices_err(ActorThunkUnderTest::OnHistoricalIndexPrices, Mode::Err)]
#[case::on_historical_funding_rates_panic(
    ActorThunkUnderTest::OnHistoricalFundingRates,
    Mode::Panic
)]
#[case::on_historical_funding_rates_err(ActorThunkUnderTest::OnHistoricalFundingRates, Mode::Err)]
fn actor_thunk_propagates_failure(#[case] thunk: ActorThunkUnderTest, #[case] mode: Mode) {
    set_mode(mode);
    let r = drive_actor_thunk(thunk);
    let err = r.into_result().unwrap_err();
    assert_failure_code(&err, mode, PluginErrorCode::Panic, PluginErrorCode::Generic);
}

// See note above on ActorThunkUnderTest regarding the `On` prefix lint.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug)]
enum StrategyThunkUnderTest {
    OnStart,
    OnStop,
    OnResume,
    OnReset,
    OnDispose,
    OnDegrade,
    OnFault,
    OnTimeEvent,
    OnData,
    OnInstrument,
    OnBookDeltas,
    OnQuote,
    OnTrade,
    OnBar,
    OnMarkPrice,
    OnIndexPrice,
    OnFundingRate,
    OnOptionGreeks,
    OnOptionChain,
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
    OnMarketExit,
    OnHistoricalBookDeltas,
    OnHistoricalQuotes,
    OnHistoricalTrades,
    OnHistoricalBars,
    OnHistoricalMarkPrices,
    OnHistoricalIndexPrices,
    OnHistoricalFundingRates,
}

fn drive_strategy_thunk(thunk: StrategyThunkUnderTest) -> PluginResult<()> {
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*strategy_vtable::<MisbehavingStrategy>() };
    let host: *const HostVTable = std::ptr::null();
    let ctx: *const HostContext = std::ptr::null();
    // SAFETY: MisbehavingStrategy::new never derefs host or ctx.
    let handle = unsafe { generated_slot!(vt, create)(host, ctx, BorrowedStr::empty()) };

    let r = match thunk {
        // SAFETY: handle is live for each branch below.
        StrategyThunkUnderTest::OnStart => unsafe { generated_slot!(vt, on_start)(handle) },
        StrategyThunkUnderTest::OnStop => unsafe { generated_slot!(vt, on_stop)(handle) },
        StrategyThunkUnderTest::OnResume => unsafe { generated_slot!(vt, on_resume)(handle) },
        StrategyThunkUnderTest::OnReset => unsafe { generated_slot!(vt, on_reset)(handle) },
        StrategyThunkUnderTest::OnDispose => unsafe { generated_slot!(vt, on_dispose)(handle) },
        StrategyThunkUnderTest::OnDegrade => unsafe { generated_slot!(vt, on_degrade)(handle) },
        StrategyThunkUnderTest::OnFault => unsafe { generated_slot!(vt, on_fault)(handle) },
        StrategyThunkUnderTest::OnTimeEvent => {
            let v = TimeEvent::new(
                Ustr::from("TestAlarm"),
                UUID4::new(),
                UnixNanos::from(1u64),
                UnixNanos::from(2u64),
            );
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_time_event)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnData => {
            let data_handle = custom_data_handle();
            let data = plugin_custom_data_ref(data_handle.cast_const());
            // SAFETY: both handles are live for the duration of the call.
            let r = unsafe { generated_slot!(vt, on_data)(handle, data) };
            drop_custom_data_handle(data_handle);
            r
        }
        StrategyThunkUnderTest::OnInstrument => {
            let v = InstrumentAnyHandle::new(InstrumentAny::CurrencyPair(currency_pair_ethusdt()));
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_instrument)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnBookDeltas => {
            let v = OrderBookDeltasHandle::new(stub_deltas());
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_book_deltas)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnQuote => {
            let v = quote_tick_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_quote)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnTrade => {
            let v = stub_trade_ethusdt_buyer();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_trade)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnBar => {
            let v = stub_bar();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_bar)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnMarkPrice => {
            let v = mark_price_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_mark_price)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnIndexPrice => {
            let v = index_price_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_index_price)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnFundingRate => {
            let v = funding_rate_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_funding_rate)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOptionGreeks => {
            let v = option_greeks_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_option_greeks)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOptionChain => {
            let v = OptionChainSliceHandle::new(option_chain_value());
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_option_chain)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnInstrumentStatus => {
            let v = stub_instrument_status();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_instrument_status)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnInstrumentClose => {
            let v = stub_instrument_close();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_instrument_close)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnSignal => {
            let v = signal_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_signal)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderInitialized => {
            let v = order_initialized_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_initialized)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderSubmitted => {
            let v = order_submitted_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_submitted)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderAccepted => {
            let v = order_accepted_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_accepted)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderRejected => {
            let v = order_rejected_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_rejected)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderFilled => {
            let v = order_filled_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_filled)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderCanceled => {
            let v = order_canceled_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_canceled)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderExpired => {
            let v = order_expired_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_expired)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderTriggered => {
            let v = order_triggered_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_triggered)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderDenied => {
            let v = order_denied_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_denied)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderEmulated => {
            let v = order_emulated_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_emulated)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderReleased => {
            let v = order_released_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_released)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderPendingUpdate => {
            let v = order_pending_update_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_pending_update)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderPendingCancel => {
            let v = order_pending_cancel_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_pending_cancel)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderModifyRejected => {
            let v = order_modify_rejected_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_modify_rejected)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderCancelRejected => {
            let v = order_cancel_rejected_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_cancel_rejected)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnOrderUpdated => {
            let v = order_updated_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_order_updated)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnPositionOpened => {
            let v = position_opened_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_position_opened)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnPositionChanged => {
            let v = position_changed_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_position_changed)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnPositionClosed => {
            let v = position_closed_value();
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_position_closed)(handle, &raw const v) }
        }
        StrategyThunkUnderTest::OnMarketExit => {
            // SAFETY: handle is live; on_market_exit takes no payload.
            unsafe { generated_slot!(vt, on_market_exit)(handle) }
        }
        StrategyThunkUnderTest::OnHistoricalBookDeltas => {
            let v = stub_deltas();
            let s = Slice::from_slice(&v.deltas);
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_historical_book_deltas)(handle, s) }
        }
        StrategyThunkUnderTest::OnHistoricalQuotes => {
            let v = vec![quote_tick_value()];
            let s = Slice::from_slice(&v);
            // SAFETY: v outlives the call.
            unsafe { generated_slot!(vt, on_historical_quotes)(handle, s) }
        }
        StrategyThunkUnderTest::OnHistoricalTrades => {
            let v = vec![stub_trade_ethusdt_buyer()];
            let s = Slice::from_slice(&v);
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_historical_trades)(handle, s) }
        }
        StrategyThunkUnderTest::OnHistoricalBars => {
            let v = vec![stub_bar()];
            let s = Slice::from_slice(&v);
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_historical_bars)(handle, s) }
        }
        StrategyThunkUnderTest::OnHistoricalMarkPrices => {
            let v = vec![mark_price_value()];
            let s = Slice::from_slice(&v);
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_historical_mark_prices)(handle, s) }
        }
        StrategyThunkUnderTest::OnHistoricalIndexPrices => {
            let v = vec![index_price_value()];
            let s = Slice::from_slice(&v);
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_historical_index_prices)(handle, s) }
        }
        StrategyThunkUnderTest::OnHistoricalFundingRates => {
            let v = vec![funding_rate_value()];
            let s = Slice::from_slice(&v);
            // SAFETY: see above.
            unsafe { generated_slot!(vt, on_historical_funding_rates)(handle, s) }
        }
    };

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(handle);
    };
    r
}

#[rstest]
#[case::on_start_panic(StrategyThunkUnderTest::OnStart, Mode::Panic)]
#[case::on_start_err(StrategyThunkUnderTest::OnStart, Mode::Err)]
#[case::on_stop_panic(StrategyThunkUnderTest::OnStop, Mode::Panic)]
#[case::on_stop_err(StrategyThunkUnderTest::OnStop, Mode::Err)]
#[case::on_resume_panic(StrategyThunkUnderTest::OnResume, Mode::Panic)]
#[case::on_resume_err(StrategyThunkUnderTest::OnResume, Mode::Err)]
#[case::on_reset_panic(StrategyThunkUnderTest::OnReset, Mode::Panic)]
#[case::on_reset_err(StrategyThunkUnderTest::OnReset, Mode::Err)]
#[case::on_dispose_panic(StrategyThunkUnderTest::OnDispose, Mode::Panic)]
#[case::on_dispose_err(StrategyThunkUnderTest::OnDispose, Mode::Err)]
#[case::on_degrade_panic(StrategyThunkUnderTest::OnDegrade, Mode::Panic)]
#[case::on_degrade_err(StrategyThunkUnderTest::OnDegrade, Mode::Err)]
#[case::on_fault_panic(StrategyThunkUnderTest::OnFault, Mode::Panic)]
#[case::on_fault_err(StrategyThunkUnderTest::OnFault, Mode::Err)]
#[case::on_time_event_panic(StrategyThunkUnderTest::OnTimeEvent, Mode::Panic)]
#[case::on_time_event_err(StrategyThunkUnderTest::OnTimeEvent, Mode::Err)]
#[case::on_data_panic(StrategyThunkUnderTest::OnData, Mode::Panic)]
#[case::on_data_err(StrategyThunkUnderTest::OnData, Mode::Err)]
#[case::on_instrument_panic(StrategyThunkUnderTest::OnInstrument, Mode::Panic)]
#[case::on_instrument_err(StrategyThunkUnderTest::OnInstrument, Mode::Err)]
#[case::on_book_deltas_panic(StrategyThunkUnderTest::OnBookDeltas, Mode::Panic)]
#[case::on_book_deltas_err(StrategyThunkUnderTest::OnBookDeltas, Mode::Err)]
#[case::on_quote_panic(StrategyThunkUnderTest::OnQuote, Mode::Panic)]
#[case::on_quote_err(StrategyThunkUnderTest::OnQuote, Mode::Err)]
#[case::on_trade_panic(StrategyThunkUnderTest::OnTrade, Mode::Panic)]
#[case::on_trade_err(StrategyThunkUnderTest::OnTrade, Mode::Err)]
#[case::on_bar_panic(StrategyThunkUnderTest::OnBar, Mode::Panic)]
#[case::on_bar_err(StrategyThunkUnderTest::OnBar, Mode::Err)]
#[case::on_mark_price_panic(StrategyThunkUnderTest::OnMarkPrice, Mode::Panic)]
#[case::on_mark_price_err(StrategyThunkUnderTest::OnMarkPrice, Mode::Err)]
#[case::on_index_price_panic(StrategyThunkUnderTest::OnIndexPrice, Mode::Panic)]
#[case::on_index_price_err(StrategyThunkUnderTest::OnIndexPrice, Mode::Err)]
#[case::on_funding_rate_panic(StrategyThunkUnderTest::OnFundingRate, Mode::Panic)]
#[case::on_funding_rate_err(StrategyThunkUnderTest::OnFundingRate, Mode::Err)]
#[case::on_option_greeks_panic(StrategyThunkUnderTest::OnOptionGreeks, Mode::Panic)]
#[case::on_option_greeks_err(StrategyThunkUnderTest::OnOptionGreeks, Mode::Err)]
#[case::on_option_chain_panic(StrategyThunkUnderTest::OnOptionChain, Mode::Panic)]
#[case::on_option_chain_err(StrategyThunkUnderTest::OnOptionChain, Mode::Err)]
#[case::on_instrument_status_panic(StrategyThunkUnderTest::OnInstrumentStatus, Mode::Panic)]
#[case::on_instrument_status_err(StrategyThunkUnderTest::OnInstrumentStatus, Mode::Err)]
#[case::on_instrument_close_panic(StrategyThunkUnderTest::OnInstrumentClose, Mode::Panic)]
#[case::on_instrument_close_err(StrategyThunkUnderTest::OnInstrumentClose, Mode::Err)]
#[case::on_signal_panic(StrategyThunkUnderTest::OnSignal, Mode::Panic)]
#[case::on_signal_err(StrategyThunkUnderTest::OnSignal, Mode::Err)]
#[case::on_order_initialized_panic(StrategyThunkUnderTest::OnOrderInitialized, Mode::Panic)]
#[case::on_order_initialized_err(StrategyThunkUnderTest::OnOrderInitialized, Mode::Err)]
#[case::on_order_submitted_panic(StrategyThunkUnderTest::OnOrderSubmitted, Mode::Panic)]
#[case::on_order_submitted_err(StrategyThunkUnderTest::OnOrderSubmitted, Mode::Err)]
#[case::on_order_accepted_panic(StrategyThunkUnderTest::OnOrderAccepted, Mode::Panic)]
#[case::on_order_accepted_err(StrategyThunkUnderTest::OnOrderAccepted, Mode::Err)]
#[case::on_order_rejected_panic(StrategyThunkUnderTest::OnOrderRejected, Mode::Panic)]
#[case::on_order_rejected_err(StrategyThunkUnderTest::OnOrderRejected, Mode::Err)]
#[case::on_order_filled_panic(StrategyThunkUnderTest::OnOrderFilled, Mode::Panic)]
#[case::on_order_filled_err(StrategyThunkUnderTest::OnOrderFilled, Mode::Err)]
#[case::on_order_canceled_panic(StrategyThunkUnderTest::OnOrderCanceled, Mode::Panic)]
#[case::on_order_canceled_err(StrategyThunkUnderTest::OnOrderCanceled, Mode::Err)]
#[case::on_order_expired_panic(StrategyThunkUnderTest::OnOrderExpired, Mode::Panic)]
#[case::on_order_expired_err(StrategyThunkUnderTest::OnOrderExpired, Mode::Err)]
#[case::on_order_triggered_panic(StrategyThunkUnderTest::OnOrderTriggered, Mode::Panic)]
#[case::on_order_triggered_err(StrategyThunkUnderTest::OnOrderTriggered, Mode::Err)]
#[case::on_order_denied_panic(StrategyThunkUnderTest::OnOrderDenied, Mode::Panic)]
#[case::on_order_denied_err(StrategyThunkUnderTest::OnOrderDenied, Mode::Err)]
#[case::on_order_emulated_panic(StrategyThunkUnderTest::OnOrderEmulated, Mode::Panic)]
#[case::on_order_emulated_err(StrategyThunkUnderTest::OnOrderEmulated, Mode::Err)]
#[case::on_order_released_panic(StrategyThunkUnderTest::OnOrderReleased, Mode::Panic)]
#[case::on_order_released_err(StrategyThunkUnderTest::OnOrderReleased, Mode::Err)]
#[case::on_order_pending_update_panic(StrategyThunkUnderTest::OnOrderPendingUpdate, Mode::Panic)]
#[case::on_order_pending_update_err(StrategyThunkUnderTest::OnOrderPendingUpdate, Mode::Err)]
#[case::on_order_pending_cancel_panic(StrategyThunkUnderTest::OnOrderPendingCancel, Mode::Panic)]
#[case::on_order_pending_cancel_err(StrategyThunkUnderTest::OnOrderPendingCancel, Mode::Err)]
#[case::on_order_modify_rejected_panic(StrategyThunkUnderTest::OnOrderModifyRejected, Mode::Panic)]
#[case::on_order_modify_rejected_err(StrategyThunkUnderTest::OnOrderModifyRejected, Mode::Err)]
#[case::on_order_cancel_rejected_panic(StrategyThunkUnderTest::OnOrderCancelRejected, Mode::Panic)]
#[case::on_order_cancel_rejected_err(StrategyThunkUnderTest::OnOrderCancelRejected, Mode::Err)]
#[case::on_order_updated_panic(StrategyThunkUnderTest::OnOrderUpdated, Mode::Panic)]
#[case::on_order_updated_err(StrategyThunkUnderTest::OnOrderUpdated, Mode::Err)]
#[case::on_position_opened_panic(StrategyThunkUnderTest::OnPositionOpened, Mode::Panic)]
#[case::on_position_opened_err(StrategyThunkUnderTest::OnPositionOpened, Mode::Err)]
#[case::on_position_changed_panic(StrategyThunkUnderTest::OnPositionChanged, Mode::Panic)]
#[case::on_position_changed_err(StrategyThunkUnderTest::OnPositionChanged, Mode::Err)]
#[case::on_position_closed_panic(StrategyThunkUnderTest::OnPositionClosed, Mode::Panic)]
#[case::on_position_closed_err(StrategyThunkUnderTest::OnPositionClosed, Mode::Err)]
#[case::on_market_exit_panic(StrategyThunkUnderTest::OnMarketExit, Mode::Panic)]
#[case::on_market_exit_err(StrategyThunkUnderTest::OnMarketExit, Mode::Err)]
#[case::on_historical_book_deltas_panic(
    StrategyThunkUnderTest::OnHistoricalBookDeltas,
    Mode::Panic
)]
#[case::on_historical_book_deltas_err(StrategyThunkUnderTest::OnHistoricalBookDeltas, Mode::Err)]
#[case::on_historical_quotes_panic(StrategyThunkUnderTest::OnHistoricalQuotes, Mode::Panic)]
#[case::on_historical_quotes_err(StrategyThunkUnderTest::OnHistoricalQuotes, Mode::Err)]
#[case::on_historical_trades_panic(StrategyThunkUnderTest::OnHistoricalTrades, Mode::Panic)]
#[case::on_historical_trades_err(StrategyThunkUnderTest::OnHistoricalTrades, Mode::Err)]
#[case::on_historical_bars_panic(StrategyThunkUnderTest::OnHistoricalBars, Mode::Panic)]
#[case::on_historical_bars_err(StrategyThunkUnderTest::OnHistoricalBars, Mode::Err)]
#[case::on_historical_mark_prices_panic(
    StrategyThunkUnderTest::OnHistoricalMarkPrices,
    Mode::Panic
)]
#[case::on_historical_mark_prices_err(StrategyThunkUnderTest::OnHistoricalMarkPrices, Mode::Err)]
#[case::on_historical_index_prices_panic(
    StrategyThunkUnderTest::OnHistoricalIndexPrices,
    Mode::Panic
)]
#[case::on_historical_index_prices_err(StrategyThunkUnderTest::OnHistoricalIndexPrices, Mode::Err)]
#[case::on_historical_funding_rates_panic(
    StrategyThunkUnderTest::OnHistoricalFundingRates,
    Mode::Panic
)]
#[case::on_historical_funding_rates_err(
    StrategyThunkUnderTest::OnHistoricalFundingRates,
    Mode::Err
)]
fn strategy_thunk_propagates_failure(#[case] thunk: StrategyThunkUnderTest, #[case] mode: Mode) {
    set_mode(mode);
    let r = drive_strategy_thunk(thunk);
    let err = r.into_result().unwrap_err();
    assert_failure_code(&err, mode, PluginErrorCode::Panic, PluginErrorCode::Generic);
}

// Stub event values used by drive_actor_thunk / drive_strategy_thunk. The
// values must be valid `#[repr(C)]` payloads since the thunks dereference
// them once before handing them to the panicking trait method.

fn instrument_id() -> InstrumentId {
    InstrumentId::from("ETH-USDT.BINANCE")
}

fn custom_data_handle() -> *mut CustomDataHandle {
    Box::into_raw(Box::new(MisbehavingTick)).cast::<CustomDataHandle>()
}

fn plugin_custom_data_ref(handle: *const CustomDataHandle) -> PluginCustomDataRef {
    // SAFETY: the handle is allocated as MisbehavingTick and remains live
    // until the caller drops it through the same vtable.
    unsafe {
        PluginCustomDataRef::from_raw_parts(
            BorrowedStr::from_str(MisbehavingTick::TYPE_NAME),
            custom_data_vtable::<MisbehavingTick>(),
            handle,
        )
    }
}

fn drop_custom_data_handle(handle: *mut CustomDataHandle) {
    // SAFETY: handle was allocated as MisbehavingTick by
    // custom_data_handle and remains valid for this vtable.
    let vtable = unsafe { &*custom_data_vtable::<MisbehavingTick>() };
    // SAFETY: handle was allocated as MisbehavingTick and has not been dropped.
    unsafe { generated_slot!(vtable, drop_handle)(handle) };
}

fn option_chain_value() -> OptionChainSlice {
    OptionChainSlice::new(OptionSeriesId::new(
        Venue::new("DERIBIT"),
        Ustr::from("BTC"),
        Ustr::from("BTC"),
        UnixNanos::from(1_700_000_000_000_000_000u64),
    ))
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

fn option_greeks_value() -> OptionGreeks {
    OptionGreeks {
        instrument_id: instrument_id(),
        convention: GreeksConvention::BlackScholes,
        greeks: Default::default(),
        mark_iv: Some(0.25),
        bid_iv: Some(0.24),
        ask_iv: Some(0.26),
        underlying_price: Some(1500.0),
        open_interest: Some(1000.0),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(2u64),
    }
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

fn order_filled_value() -> OrderFilled {
    order_stubs::order_filled(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::uuid4(),
    )
}

fn order_canceled_value() -> OrderCanceled {
    OrderCanceled {
        trader_id: TraderId::from("TESTER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: instrument_id(),
        client_order_id: ClientOrderId::from("O-1"),
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(2u64),
        reconciliation: false,
        venue_order_id: Some(VenueOrderId::from("V-1")),
        account_id: Some(AccountId::from("BINANCE-001")),
        causation_id: None,
    }
}

fn order_initialized_value() -> OrderInitialized {
    order_stubs::order_initialized_buy_limit(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::uuid4(),
    )
}

fn order_submitted_value() -> OrderSubmitted {
    order_stubs::order_submitted(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::account_id(),
        order_stubs::uuid4(),
    )
}

fn order_accepted_value() -> OrderAccepted {
    order_stubs::order_accepted(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::account_id(),
        order_stubs::venue_order_id(),
        order_stubs::uuid4(),
    )
}

fn order_rejected_value() -> OrderRejected {
    order_stubs::order_rejected_insufficient_margin(
        order_stubs::trader_id(),
        order_stubs::account_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::uuid4(),
    )
}

fn order_expired_value() -> OrderExpired {
    order_stubs::order_expired(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::venue_order_id(),
        order_stubs::account_id(),
        order_stubs::uuid4(),
    )
}

fn order_triggered_value() -> OrderTriggered {
    order_stubs::order_triggered(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::venue_order_id(),
        order_stubs::account_id(),
        order_stubs::uuid4(),
    )
}

fn order_denied_value() -> OrderDenied {
    order_stubs::order_denied_max_submitted_rate(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::uuid4(),
    )
}

fn order_emulated_value() -> OrderEmulated {
    order_stubs::order_emulated(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::uuid4(),
    )
}

fn order_released_value() -> OrderReleased {
    order_stubs::order_released(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::uuid4(),
    )
}

fn order_pending_update_value() -> OrderPendingUpdate {
    order_stubs::order_pending_update(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::account_id(),
        order_stubs::venue_order_id(),
        order_stubs::uuid4(),
    )
}

fn order_pending_cancel_value() -> OrderPendingCancel {
    order_stubs::order_pending_cancel(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::account_id(),
        order_stubs::venue_order_id(),
        order_stubs::uuid4(),
    )
}

fn order_modify_rejected_value() -> OrderModifyRejected {
    order_stubs::order_modify_rejected(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::venue_order_id(),
        order_stubs::account_id(),
        order_stubs::uuid4(),
    )
}

fn order_cancel_rejected_value() -> OrderCancelRejected {
    order_stubs::order_cancel_rejected(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::venue_order_id(),
        order_stubs::account_id(),
        order_stubs::uuid4(),
    )
}

fn order_updated_value() -> OrderUpdated {
    order_stubs::order_updated(
        order_stubs::trader_id(),
        order_stubs::strategy_id_ema_cross(),
        order_stubs::instrument_id_btc_usdt(),
        order_stubs::client_order_id(),
        order_stubs::venue_order_id(),
        order_stubs::account_id(),
        order_stubs::uuid4(),
    )
}
