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

//! Strategy plug point.
//!
//! Strategies extend the [`crate::surfaces::actor::PluginActor`] surface with
//! position events, order lifecycle events, and an order-command surface
//! that calls back into the host through [`crate::HostVTable`].
//!
//! # Boundary shape
//!
//! Data-bearing callbacks cross the boundary as `*const T` borrowed
//! pointers into the host's already-`#[repr(C)]` model types. The host
//! keeps the value alive for the duration of the call; the plug-in's
//! thunk dereferences once and hands an `&T` to the trait method. No
//! serialisation per event.
//!
//! Order commands go the other direction as boundary-owned handles. The
//! plug-in constructs a command struct under
//! [`crate::surfaces::commands`], wraps it in the matching `*Handle`,
//! and hands the host a `*const XHandle`. The host derefs the handle
//! once and routes the borrowed command into the calling strategy. The
//! host attributes each command to the calling strategy via the
//! [`HostContext`] pointer that was supplied at `create`. No JSON
//! crosses the boundary for any per-call command path.
//!
//! # Scope (v1)
//!
//! Phase 1 covers the full Phase 1 actor callback set plus:
//!
//! - Custom data: values registered through `PluginCustomData`, including
//!   historical custom-data responses routed through `on_data`.
//! - Position events: opened, closed, changed.
//! - Order lifecycle events: initialized, submitted, accepted, rejected,
//!   expired, triggered, denied, emulated, released, pending update,
//!   pending cancel, modify rejected, cancel rejected, updated.
//! - Market exit lifecycle: `on_market_exit`.
//! - Historical market data: bulk historical book deltas, book depth,
//!   quotes, trades, bars, mark prices, index prices, funding rates
//!   delivered as [`crate::boundary::Slice`] payloads.
//! - Order commands via [`HostVTable`]: `submit_order`, `cancel_order`,
//!   `modify_order`, `submit_order_list`, `cancel_orders`,
//!   `cancel_all_orders`, `close_position`, `close_all_positions`,
//!   `query_account`, `query_order`. Each slot takes a boundary-owned
//!   `*const XHandle` and the live adapter routes the borrowed command
//!   into the matching `Strategy::*` call so the production cache /
//!   risk / event pipeline runs unchanged; `close_position` and
//!   `query_order` resolve their `&Position` / `&OrderAny` arguments
//!   via the host cache before dispatch.
//!
//! Order books and book deltas cross as
//! [`*const OrderBookHandle`](crate::surfaces::book::OrderBookHandle) and
//! [`*const OrderBookDeltasHandle`](crate::surfaces::book::OrderBookDeltasHandle)
//! since [`OrderBook`] and [`OrderBookDeltas`] own Rust collection state
//! and cannot be `#[repr(C)]`. The host wraps each payload in a handle
//! for the duration of the call. Instruments cross as
//! [`*const InstrumentAnyHandle`](crate::surfaces::instrument::InstrumentAnyHandle)
//! for the same reason: [`InstrumentAny`] is a non-`#[repr(C)]` enum
//! whose variants own heap-allocated fields. Option chain snapshots
//! cross as
//! [`*const OptionChainSliceHandle`](crate::surfaces::option_chain::OptionChainSliceHandle)
//! because [`OptionChainSlice`] owns `BTreeMap<Price, OptionStrikeData>`
//! call and put maps.
//!
//! Deferred: generic non-plugin `on_historical_data` (`&dyn Any` payload),
//! DeFi pool/block events, and the cache-state-mutation methods
//! (`mark_order_pending_*`, `generate_order_pending_*`). The authoritative
//! list lives in `tests/surface_alignment.rs`.

#![allow(unsafe_code)]

use std::marker::PhantomData;

use nautilus_common::{signal::Signal, timer::TimeEvent};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, OptionChainSlice, OptionGreeks, OrderBookDelta, OrderBookDeltas,
        OrderBookDepth10, QuoteTick, TradeTick,
    },
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, PositionChanged, PositionClosed, PositionOpened,
    },
    instruments::InstrumentAny,
    orderbook::OrderBook,
};

use crate::{
    boundary::{BorrowedStr, PluginError, PluginErrorCode, PluginResult, Slice},
    host::{HostContext, HostVTable},
    normalize::BoundaryNormalize,
    panic::{guard, guard_drop, guard_or_null},
    surfaces::{
        book::{OrderBookDeltasHandle, OrderBookHandle},
        custom_data::PluginCustomDataRef,
        instrument::InstrumentAnyHandle,
        option_chain::OptionChainSliceHandle,
    },
};

/// Opaque handle to a plug-in strategy instance owned by the cdylib.
#[repr(C)]
pub struct PluginStrategyHandle {
    _opaque: [u8; 0],
}

/// Function table for a single plug-in strategy type.
///
/// One static vtable per concrete type, generated by
/// [`nautilus_plugin!`](crate::nautilus_plugin) via the same `Tag<T>` pattern
/// that powers
/// [`actor_vtable`](crate::surfaces::actor::actor_vtable) and
/// [`custom_data_vtable`](crate::surfaces::custom_data::custom_data_vtable).
///
/// The strategy duplicates the actor callback set directly rather than
/// embedding an `ActorVTable` so dispatch stays a single function-pointer
/// call per event and so the layout is independent of any future actor
/// vtable changes.
///
/// Slots are nullable at the ABI type level so the host can reject malformed
/// manifests with null callbacks before constructing a strategy. Macro-generated
/// vtables fill every required slot.
#[repr(C)]
pub struct StrategyVTable {
    /// Constructs a fresh strategy instance bound to the supplied host
    /// vtable and instance context, and returns a non-null handle. The
    /// strategy stashes both pointers so it can route order commands
    /// (`submit_order`, `cancel_order`, `modify_order`) back through the
    /// host. The host uses the context to attribute each command.
    ///
    /// `config_json` carries the per-instance configuration the host
    /// constructed from the user's TOML or builder API. Empty when the
    /// host has no instance-specific configuration to pass.
    pub create: Option<
        unsafe extern "C" fn(
            host: *const HostVTable,
            ctx: *const HostContext,
            config_json: BorrowedStr<'_>,
        ) -> *mut PluginStrategyHandle,
    >,

    /// Drops the strategy instance and releases all of its resources.
    pub drop_handle: Option<unsafe extern "C" fn(handle: *mut PluginStrategyHandle)>,

    /// Returns the canonical type name for this strategy.
    pub type_name: Option<unsafe extern "C" fn() -> BorrowedStr<'static>>,

    pub on_start:
        Option<unsafe extern "C" fn(handle: *mut PluginStrategyHandle) -> PluginResult<()>>,
    pub on_stop:
        Option<unsafe extern "C" fn(handle: *mut PluginStrategyHandle) -> PluginResult<()>>,
    pub on_resume:
        Option<unsafe extern "C" fn(handle: *mut PluginStrategyHandle) -> PluginResult<()>>,
    pub on_reset:
        Option<unsafe extern "C" fn(handle: *mut PluginStrategyHandle) -> PluginResult<()>>,
    pub on_dispose:
        Option<unsafe extern "C" fn(handle: *mut PluginStrategyHandle) -> PluginResult<()>>,
    pub on_degrade:
        Option<unsafe extern "C" fn(handle: *mut PluginStrategyHandle) -> PluginResult<()>>,
    pub on_fault:
        Option<unsafe extern "C" fn(handle: *mut PluginStrategyHandle) -> PluginResult<()>>,

    pub on_time_event: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const TimeEvent,
        ) -> PluginResult<()>,
    >,
    pub on_data: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            data: PluginCustomDataRef,
        ) -> PluginResult<()>,
    >,

    pub on_instrument: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            instrument: *const InstrumentAnyHandle,
        ) -> PluginResult<()>,
    >,
    pub on_book_deltas: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            deltas: *const OrderBookDeltasHandle,
        ) -> PluginResult<()>,
    >,
    pub on_book: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            book: *const OrderBookHandle,
        ) -> PluginResult<()>,
    >,
    pub on_quote: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            quote: *const QuoteTick,
        ) -> PluginResult<()>,
    >,
    pub on_trade: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            trade: *const TradeTick,
        ) -> PluginResult<()>,
    >,
    pub on_bar: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            bar: *const Bar,
        ) -> PluginResult<()>,
    >,
    pub on_mark_price: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            mark_price: *const MarkPriceUpdate,
        ) -> PluginResult<()>,
    >,
    pub on_index_price: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            index_price: *const IndexPriceUpdate,
        ) -> PluginResult<()>,
    >,
    pub on_funding_rate: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            funding_rate: *const FundingRateUpdate,
        ) -> PluginResult<()>,
    >,
    pub on_option_greeks: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            greeks: *const OptionGreeks,
        ) -> PluginResult<()>,
    >,
    pub on_option_chain: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            chain: *const OptionChainSliceHandle,
        ) -> PluginResult<()>,
    >,
    pub on_instrument_status: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            status: *const InstrumentStatus,
        ) -> PluginResult<()>,
    >,
    pub on_instrument_close: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            close: *const InstrumentClose,
        ) -> PluginResult<()>,
    >,
    pub on_signal: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            signal: *const Signal,
        ) -> PluginResult<()>,
    >,

    pub on_order_initialized: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderInitialized,
        ) -> PluginResult<()>,
    >,
    pub on_order_submitted: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderSubmitted,
        ) -> PluginResult<()>,
    >,
    pub on_order_accepted: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderAccepted,
        ) -> PluginResult<()>,
    >,
    pub on_order_rejected: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderRejected,
        ) -> PluginResult<()>,
    >,
    pub on_order_filled: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderFilled,
        ) -> PluginResult<()>,
    >,
    pub on_order_canceled: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderCanceled,
        ) -> PluginResult<()>,
    >,
    pub on_order_expired: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderExpired,
        ) -> PluginResult<()>,
    >,
    pub on_order_triggered: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderTriggered,
        ) -> PluginResult<()>,
    >,
    pub on_order_denied: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderDenied,
        ) -> PluginResult<()>,
    >,
    pub on_order_emulated: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderEmulated,
        ) -> PluginResult<()>,
    >,
    pub on_order_released: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderReleased,
        ) -> PluginResult<()>,
    >,
    pub on_order_pending_update: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderPendingUpdate,
        ) -> PluginResult<()>,
    >,
    pub on_order_pending_cancel: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderPendingCancel,
        ) -> PluginResult<()>,
    >,
    pub on_order_modify_rejected: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderModifyRejected,
        ) -> PluginResult<()>,
    >,
    pub on_order_cancel_rejected: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderCancelRejected,
        ) -> PluginResult<()>,
    >,
    pub on_order_updated: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const OrderUpdated,
        ) -> PluginResult<()>,
    >,

    pub on_position_opened: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const PositionOpened,
        ) -> PluginResult<()>,
    >,
    pub on_position_changed: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const PositionChanged,
        ) -> PluginResult<()>,
    >,
    pub on_position_closed: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            event: *const PositionClosed,
        ) -> PluginResult<()>,
    >,

    pub on_market_exit:
        Option<unsafe extern "C" fn(handle: *mut PluginStrategyHandle) -> PluginResult<()>>,

    pub on_historical_book_deltas: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            deltas: Slice<'_, OrderBookDelta>,
        ) -> PluginResult<()>,
    >,
    pub on_historical_book_depth: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            depths: Slice<'_, OrderBookDepth10>,
        ) -> PluginResult<()>,
    >,
    pub on_historical_quotes: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            quotes: Slice<'_, QuoteTick>,
        ) -> PluginResult<()>,
    >,
    pub on_historical_trades: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            trades: Slice<'_, TradeTick>,
        ) -> PluginResult<()>,
    >,
    pub on_historical_bars: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            bars: Slice<'_, Bar>,
        ) -> PluginResult<()>,
    >,
    pub on_historical_mark_prices: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            mark_prices: Slice<'_, MarkPriceUpdate>,
        ) -> PluginResult<()>,
    >,
    pub on_historical_index_prices: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            index_prices: Slice<'_, IndexPriceUpdate>,
        ) -> PluginResult<()>,
    >,
    pub on_historical_funding_rates: Option<
        unsafe extern "C" fn(
            handle: *mut PluginStrategyHandle,
            funding_rates: Slice<'_, FundingRateUpdate>,
        ) -> PluginResult<()>,
    >,
}

/// Author-facing trait for a plug-in strategy.
///
/// Strategies receive every callback an actor does plus position and order
/// lifecycle events, and they have access to a [`HostContext`] pointer
/// they can use to issue order commands through the host.
///
/// Every callback has a no-op default. Override only what you need.
pub trait PluginStrategy: 'static + Send + Sized {
    /// Canonical type name. Must be unique across a Nautilus deployment.
    const TYPE_NAME: &'static str;

    /// Constructs a fresh strategy instance bound to the supplied host
    /// vtable and instance context. Implementations typically store both
    /// pointers in fields so that order-command callbacks (`submit_order`,
    /// `cancel_order`, `modify_order`) can be issued through
    /// `host.submit_order(ctx, ...)` etc.
    ///
    /// `config_json` is the per-instance JSON configuration the host
    /// constructed from the user's TOML or builder API. The string is
    /// empty when no instance-specific configuration is supplied.
    fn new(host: *const HostVTable, ctx: *const HostContext, config_json: &str) -> Self;

    #[allow(unused_variables)]
    fn on_start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_resume(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_degrade(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_fault(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_data(&mut self, data: PluginCustomDataRef) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_funding_rate(&mut self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_option_greeks(&mut self, greeks: &OptionGreeks) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_option_chain(&mut self, chain: &OptionChainSlice) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_instrument_status(&mut self, status: &InstrumentStatus) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_instrument_close(&mut self, close: &InstrumentClose) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_initialized(&mut self, event: &OrderInitialized) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_submitted(&mut self, event: &OrderSubmitted) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_accepted(&mut self, event: &OrderAccepted) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_rejected(&mut self, event: &OrderRejected) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_canceled(&mut self, event: &OrderCanceled) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_expired(&mut self, event: &OrderExpired) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_triggered(&mut self, event: &OrderTriggered) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_denied(&mut self, event: &OrderDenied) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_emulated(&mut self, event: &OrderEmulated) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_released(&mut self, event: &OrderReleased) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_pending_update(&mut self, event: &OrderPendingUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_pending_cancel(&mut self, event: &OrderPendingCancel) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_modify_rejected(&mut self, event: &OrderModifyRejected) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_cancel_rejected(&mut self, event: &OrderCancelRejected) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_order_updated(&mut self, event: &OrderUpdated) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_position_opened(&mut self, event: &PositionOpened) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_position_changed(&mut self, event: &PositionChanged) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_position_closed(&mut self, event: &PositionClosed) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_market_exit(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_historical_book_deltas(&mut self, deltas: &[OrderBookDelta]) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_historical_book_depth(&mut self, depths: &[OrderBookDepth10]) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_historical_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_historical_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_historical_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_historical_mark_prices(&mut self, mark_prices: &[MarkPriceUpdate]) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_historical_index_prices(
        &mut self,
        index_prices: &[IndexPriceUpdate],
    ) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_historical_funding_rates(
        &mut self,
        funding_rates: &[FundingRateUpdate],
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Returns a `*const StrategyVTable` for the given [`PluginStrategy`] type.
///
/// One static vtable per monomorphisation via the `Tag<T>`-shaped pattern
/// proven by [`actor_vtable`](crate::surfaces::actor::actor_vtable).
#[must_use]
pub fn strategy_vtable<T>() -> *const StrategyVTable
where
    T: PluginStrategy,
{
    &VTableTag::<T>::VTABLE
}

struct VTableTag<T>(PhantomData<T>);

impl<T> VTableTag<T>
where
    T: PluginStrategy,
{
    const VTABLE: StrategyVTable = StrategyVTable {
        create: Some(create_thunk::<T>),
        drop_handle: Some(drop_handle_thunk::<T>),
        type_name: Some(type_name_thunk::<T>),
        on_start: Some(on_start_thunk::<T>),
        on_stop: Some(on_stop_thunk::<T>),
        on_resume: Some(on_resume_thunk::<T>),
        on_reset: Some(on_reset_thunk::<T>),
        on_dispose: Some(on_dispose_thunk::<T>),
        on_degrade: Some(on_degrade_thunk::<T>),
        on_fault: Some(on_fault_thunk::<T>),
        on_time_event: Some(on_time_event_thunk::<T>),
        on_data: Some(on_data_thunk::<T>),
        on_instrument: Some(on_instrument_thunk::<T>),
        on_book_deltas: Some(on_book_deltas_thunk::<T>),
        on_book: Some(on_book_thunk::<T>),
        on_quote: Some(on_quote_thunk::<T>),
        on_trade: Some(on_trade_thunk::<T>),
        on_bar: Some(on_bar_thunk::<T>),
        on_mark_price: Some(on_mark_price_thunk::<T>),
        on_index_price: Some(on_index_price_thunk::<T>),
        on_funding_rate: Some(on_funding_rate_thunk::<T>),
        on_option_greeks: Some(on_option_greeks_thunk::<T>),
        on_option_chain: Some(on_option_chain_thunk::<T>),
        on_instrument_status: Some(on_instrument_status_thunk::<T>),
        on_instrument_close: Some(on_instrument_close_thunk::<T>),
        on_signal: Some(on_signal_thunk::<T>),
        on_order_initialized: Some(on_order_initialized_thunk::<T>),
        on_order_submitted: Some(on_order_submitted_thunk::<T>),
        on_order_accepted: Some(on_order_accepted_thunk::<T>),
        on_order_rejected: Some(on_order_rejected_thunk::<T>),
        on_order_filled: Some(on_order_filled_thunk::<T>),
        on_order_canceled: Some(on_order_canceled_thunk::<T>),
        on_order_expired: Some(on_order_expired_thunk::<T>),
        on_order_triggered: Some(on_order_triggered_thunk::<T>),
        on_order_denied: Some(on_order_denied_thunk::<T>),
        on_order_emulated: Some(on_order_emulated_thunk::<T>),
        on_order_released: Some(on_order_released_thunk::<T>),
        on_order_pending_update: Some(on_order_pending_update_thunk::<T>),
        on_order_pending_cancel: Some(on_order_pending_cancel_thunk::<T>),
        on_order_modify_rejected: Some(on_order_modify_rejected_thunk::<T>),
        on_order_cancel_rejected: Some(on_order_cancel_rejected_thunk::<T>),
        on_order_updated: Some(on_order_updated_thunk::<T>),
        on_position_opened: Some(on_position_opened_thunk::<T>),
        on_position_changed: Some(on_position_changed_thunk::<T>),
        on_position_closed: Some(on_position_closed_thunk::<T>),
        on_market_exit: Some(on_market_exit_thunk::<T>),
        on_historical_book_deltas: Some(on_historical_book_deltas_thunk::<T>),
        on_historical_book_depth: Some(on_historical_book_depth_thunk::<T>),
        on_historical_quotes: Some(on_historical_quotes_thunk::<T>),
        on_historical_trades: Some(on_historical_trades_thunk::<T>),
        on_historical_bars: Some(on_historical_bars_thunk::<T>),
        on_historical_mark_prices: Some(on_historical_mark_prices_thunk::<T>),
        on_historical_index_prices: Some(on_historical_index_prices_thunk::<T>),
        on_historical_funding_rates: Some(on_historical_funding_rates_thunk::<T>),
    };
}

unsafe extern "C" fn create_thunk<T: PluginStrategy>(
    host: *const HostVTable,
    ctx: *const HostContext,
    config_json: BorrowedStr<'_>,
) -> *mut PluginStrategyHandle {
    guard_or_null("strategy::create", || {
        // SAFETY: host promises `config_json` borrows storage that is live
        // for the duration of this call.
        let cfg = unsafe { config_json.as_str() };
        Box::into_raw(Box::new(T::new(host, ctx, cfg))).cast::<PluginStrategyHandle>()
    })
}

unsafe extern "C" fn drop_handle_thunk<T: PluginStrategy>(handle: *mut PluginStrategyHandle) {
    if handle.is_null() {
        return;
    }
    guard_drop("strategy::drop", || {
        // SAFETY: handle was allocated via `Box::into_raw(Box::new(T))`.
        unsafe {
            drop(Box::from_raw(handle.cast::<T>()));
        }
    });
}

unsafe extern "C" fn type_name_thunk<T: PluginStrategy>() -> BorrowedStr<'static> {
    BorrowedStr::from_str(T::TYPE_NAME)
}

fn handle_as_mut<'a, T: PluginStrategy>(handle: *mut PluginStrategyHandle) -> &'a mut T {
    // SAFETY: handle is non-null and originates from a `Box::into_raw` of a
    // `T`. The host promises exclusive access while a callback is running.
    unsafe { &mut *handle.cast::<T>() }
}

fn ok_or_err<E: ::core::fmt::Display>(r: Result<(), E>) -> Result<(), PluginError> {
    r.map_err(|e| PluginError::new(PluginErrorCode::Generic, e.to_string()))
}

macro_rules! lifecycle_thunk {
    ($name:ident, $method:ident) => {
        unsafe extern "C" fn $name<T: PluginStrategy>(
            handle: *mut PluginStrategyHandle,
        ) -> PluginResult<()> {
            guard(|| {
                let strategy = handle_as_mut::<T>(handle);
                ok_or_err(strategy.$method())
            })
        }
    };
}

lifecycle_thunk!(on_start_thunk, on_start);
lifecycle_thunk!(on_stop_thunk, on_stop);
lifecycle_thunk!(on_resume_thunk, on_resume);
lifecycle_thunk!(on_reset_thunk, on_reset);
lifecycle_thunk!(on_dispose_thunk, on_dispose);
lifecycle_thunk!(on_degrade_thunk, on_degrade);
lifecycle_thunk!(on_fault_thunk, on_fault);
lifecycle_thunk!(on_market_exit_thunk, on_market_exit);

macro_rules! event_thunk {
    ($name:ident, $method:ident, $ty:ty) => {
        unsafe extern "C" fn $name<T: PluginStrategy>(
            handle: *mut PluginStrategyHandle,
            value: *const $ty,
        ) -> PluginResult<()> {
            guard(|| {
                // SAFETY: host keeps `value` live for the duration of the call;
                // the plug-in only borrows it for the trait-method invocation.
                let v = unsafe { &*value }.boundary_normalized();
                let strategy = handle_as_mut::<T>(handle);
                ok_or_err(strategy.$method(&v))
            })
        }
    };
}

event_thunk!(on_time_event_thunk, on_time_event, TimeEvent);

unsafe extern "C" fn on_data_thunk<T: PluginStrategy>(
    handle: *mut PluginStrategyHandle,
    data: PluginCustomDataRef,
) -> PluginResult<()> {
    guard(|| {
        let strategy = handle_as_mut::<T>(handle);
        ok_or_err(strategy.on_data(data))
    })
}

unsafe extern "C" fn on_instrument_thunk<T: PluginStrategy>(
    handle: *mut PluginStrategyHandle,
    instrument: *const InstrumentAnyHandle,
) -> PluginResult<()> {
    guard(|| {
        // SAFETY: host keeps the handle live for the duration of the call;
        // the plug-in only borrows the wrapped instrument via the trait method.
        let v: InstrumentAny = unsafe { (*instrument).instrument() }.boundary_normalized();
        let strategy = handle_as_mut::<T>(handle);
        ok_or_err(strategy.on_instrument(&v))
    })
}

unsafe extern "C" fn on_book_deltas_thunk<T: PluginStrategy>(
    handle: *mut PluginStrategyHandle,
    deltas: *const OrderBookDeltasHandle,
) -> PluginResult<()> {
    guard(|| {
        // SAFETY: host keeps the handle live for the duration of the call;
        // the plug-in only borrows the wrapped deltas via the trait method.
        let v: OrderBookDeltas = unsafe { (*deltas).deltas() }.boundary_normalized();
        let strategy = handle_as_mut::<T>(handle);
        ok_or_err(strategy.on_book_deltas(&v))
    })
}

unsafe extern "C" fn on_book_thunk<T: PluginStrategy>(
    handle: *mut PluginStrategyHandle,
    book: *const OrderBookHandle,
) -> PluginResult<()> {
    guard(|| {
        // SAFETY: host keeps the handle live for the duration of the call;
        // the plug-in only borrows the wrapped book via the trait method.
        let v: OrderBook = unsafe { (*book).book() }.boundary_normalized();
        let strategy = handle_as_mut::<T>(handle);
        ok_or_err(strategy.on_book(&v))
    })
}

event_thunk!(on_quote_thunk, on_quote, QuoteTick);
event_thunk!(on_trade_thunk, on_trade, TradeTick);
event_thunk!(on_bar_thunk, on_bar, Bar);
event_thunk!(on_mark_price_thunk, on_mark_price, MarkPriceUpdate);
event_thunk!(on_index_price_thunk, on_index_price, IndexPriceUpdate);
event_thunk!(on_funding_rate_thunk, on_funding_rate, FundingRateUpdate);
event_thunk!(on_option_greeks_thunk, on_option_greeks, OptionGreeks);

unsafe extern "C" fn on_option_chain_thunk<T: PluginStrategy>(
    handle: *mut PluginStrategyHandle,
    chain: *const OptionChainSliceHandle,
) -> PluginResult<()> {
    guard(|| {
        // SAFETY: host keeps the handle live for the duration of the call;
        // the plug-in only borrows the wrapped chain via the trait method.
        let v: OptionChainSlice = unsafe { (*chain).chain() }.boundary_normalized();
        let strategy = handle_as_mut::<T>(handle);
        ok_or_err(strategy.on_option_chain(&v))
    })
}

event_thunk!(
    on_instrument_status_thunk,
    on_instrument_status,
    InstrumentStatus
);
event_thunk!(
    on_instrument_close_thunk,
    on_instrument_close,
    InstrumentClose
);
event_thunk!(on_signal_thunk, on_signal, Signal);

event_thunk!(
    on_order_initialized_thunk,
    on_order_initialized,
    OrderInitialized
);
event_thunk!(on_order_submitted_thunk, on_order_submitted, OrderSubmitted);
event_thunk!(on_order_accepted_thunk, on_order_accepted, OrderAccepted);
event_thunk!(on_order_rejected_thunk, on_order_rejected, OrderRejected);
event_thunk!(on_order_filled_thunk, on_order_filled, OrderFilled);
event_thunk!(on_order_canceled_thunk, on_order_canceled, OrderCanceled);
event_thunk!(on_order_expired_thunk, on_order_expired, OrderExpired);
event_thunk!(on_order_triggered_thunk, on_order_triggered, OrderTriggered);
event_thunk!(on_order_denied_thunk, on_order_denied, OrderDenied);
event_thunk!(on_order_emulated_thunk, on_order_emulated, OrderEmulated);
event_thunk!(on_order_released_thunk, on_order_released, OrderReleased);
event_thunk!(
    on_order_pending_update_thunk,
    on_order_pending_update,
    OrderPendingUpdate
);
event_thunk!(
    on_order_pending_cancel_thunk,
    on_order_pending_cancel,
    OrderPendingCancel
);
event_thunk!(
    on_order_modify_rejected_thunk,
    on_order_modify_rejected,
    OrderModifyRejected
);
event_thunk!(
    on_order_cancel_rejected_thunk,
    on_order_cancel_rejected,
    OrderCancelRejected
);
event_thunk!(on_order_updated_thunk, on_order_updated, OrderUpdated);

event_thunk!(on_position_opened_thunk, on_position_opened, PositionOpened);
event_thunk!(
    on_position_changed_thunk,
    on_position_changed,
    PositionChanged
);
event_thunk!(on_position_closed_thunk, on_position_closed, PositionClosed);

macro_rules! slice_thunk {
    ($name:ident, $method:ident, $ty:ty) => {
        unsafe extern "C" fn $name<T: PluginStrategy>(
            handle: *mut PluginStrategyHandle,
            values: Slice<'_, $ty>,
        ) -> PluginResult<()> {
            guard(|| {
                // SAFETY: host keeps the slice storage live for the call;
                // the plug-in only borrows it for the trait-method invocation.
                let v: Vec<$ty> = unsafe { values.as_slice() }
                    .iter()
                    .map(BoundaryNormalize::boundary_normalized)
                    .collect();
                let strategy = handle_as_mut::<T>(handle);
                ok_or_err(strategy.$method(&v))
            })
        }
    };
}

slice_thunk!(
    on_historical_book_deltas_thunk,
    on_historical_book_deltas,
    OrderBookDelta
);
slice_thunk!(
    on_historical_book_depth_thunk,
    on_historical_book_depth,
    OrderBookDepth10
);
slice_thunk!(on_historical_quotes_thunk, on_historical_quotes, QuoteTick);
slice_thunk!(on_historical_trades_thunk, on_historical_trades, TradeTick);
slice_thunk!(on_historical_bars_thunk, on_historical_bars, Bar);
slice_thunk!(
    on_historical_mark_prices_thunk,
    on_historical_mark_prices,
    MarkPriceUpdate
);
slice_thunk!(
    on_historical_index_prices_thunk,
    on_historical_index_prices,
    IndexPriceUpdate
);
slice_thunk!(
    on_historical_funding_rates_thunk,
    on_historical_funding_rates,
    FundingRateUpdate
);
