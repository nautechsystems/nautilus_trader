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

//! ABI layout snapshots for types that cross the host/plugin boundary.

use std::mem::{align_of, offset_of, size_of};

use nautilus_core::Params;
use nautilus_model::{
    enums::{OrderSide, PositionSide, TimeInForce},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, PositionId},
    orders::OrderAny,
    types::{Price, Quantity},
};
use nautilus_plugin::{
    NAUTILUS_PLUGIN_ABI_VERSION, PLUGIN_BUILD_ID_VERSION,
    boundary::{BorrowedStr, OwnedBytes, PluginError, PluginErrorCode, Slice},
    host::{ControllerHostContext, ControllerHostVTable, HostContext, HostLogLevel, HostVTable},
    manifest::{
        ActorRegistration, ControllerRegistration, CustomDataRegistration, PluginBuildId,
        PluginManifest, StrategyRegistration, compiled_precision_mode,
    },
    surfaces::{
        actor::{ActorVTable, PluginActorHandle},
        book::{OrderBookDeltasHandle, OrderBookHandle},
        commands::{
            CancelAllOrdersCommand, CancelAllOrdersHandle, CancelOrderCommand, CancelOrderHandle,
            CancelOrdersCommand, CancelOrdersHandle, CloseAllPositionsCommand,
            CloseAllPositionsHandle, ClosePositionCommand, ClosePositionHandle, ModifyOrderCommand,
            ModifyOrderHandle, QueryAccountCommand, QueryAccountHandle, QueryOrderCommand,
            QueryOrderHandle, SubmitOrderCommand, SubmitOrderHandle, SubmitOrderListCommand,
            SubmitOrderListHandle,
        },
        controller::{ControllerVTable, PluginControllerHandle},
        custom_data::{CustomDataHandle, CustomDataVTable, MetadataEntry, PluginCustomDataRef},
        instrument::InstrumentAnyHandle,
        option_chain::OptionChainSliceHandle,
        strategy::{PluginStrategyHandle, StrategyVTable},
    },
};
use rstest::rstest;
use ustr::Ustr;

#[derive(Clone, Copy)]
struct FieldLayout {
    name: &'static str,
    size: usize,
    align: usize,
    offset: usize,
}

macro_rules! repr_c_layout_snapshot {
    ($ty:ty, $type_name:literal, [$($field:ident: $field_ty:ty),+ $(,)?]) => {{
        let fields = [
            $(field_layout::<$field_ty>(
                stringify!($field),
                offset_of!($ty, $field),
            ),)+
        ];
        assert_repr_c_layout::<$ty>($type_name, &fields);
    }};
}

macro_rules! pointer_table_layout_snapshot {
    ($ty:ty, $type_name:literal, [$($field:ident),+ $(,)?]) => {{
        let fields = [
            $((stringify!($field), offset_of!($ty, $field)),)+
        ];
        assert_pointer_table_layout::<$ty>($type_name, &fields);
    }};
}

macro_rules! host_vtable_layout_snapshot {
    ([$($field:ident),+ $(,)?]) => {{
        let fields = [
            $((stringify!($field), offset_of!(HostVTable, $field)),)+
        ];
        assert_host_vtable_layout(&fields);
    }};
}

macro_rules! x64_layout_snapshot {
    ($ty:ty, $type_name:literal, $size:expr, $align:expr, [$($field:ident: $offset:expr),* $(,)?]) => {{
        assert_type_layout::<$ty>($type_name, $size, $align);
        $(
            assert_field_offset($type_name, stringify!($field), offset_of!($ty, $field), $offset);
        )*
    }};
}

#[rstest]
fn plugin_abi_version_matches_layout_snapshot() {
    assert_eq!(
        NAUTILUS_PLUGIN_ABI_VERSION, 1,
        "NAUTILUS_PLUGIN_ABI_VERSION changed: expected 1, was {NAUTILUS_PLUGIN_ABI_VERSION}. \
         Update the layout snapshot and ABI version decision together.",
    );
    assert_eq!(
        PLUGIN_BUILD_ID_VERSION, 1,
        "PLUGIN_BUILD_ID_VERSION changed: expected 1, was {PLUGIN_BUILD_ID_VERSION}. \
         Update the build-id layout snapshot and version decision together.",
    );
}

#[cfg(target_pointer_width = "64")]
#[rstest]
fn x64_boundary_layouts_match_absolute_snapshot() {
    let high_precision = compiled_precision_mode() == "high-precision";

    x64_layout_snapshot!(HostLogLevel, "HostLogLevel", 1, 1, []);
    x64_layout_snapshot!(PluginErrorCode, "PluginErrorCode", 4, 4, []);
    x64_layout_snapshot!(BorrowedStr<'static>, "BorrowedStr", 16, 8, [
        ptr: 0,
        len: 8,
    ]);
    x64_layout_snapshot!(Slice<'static, u8>, "Slice<u8>", 16, 8, [
        ptr: 0,
        len: 8,
    ]);
    x64_layout_snapshot!(OwnedBytes, "OwnedBytes", 32, 8, [
        ptr: 0,
        len: 8,
        cap: 16,
        drop_fn: 24,
    ]);
    x64_layout_snapshot!(PluginError, "PluginError", 40, 8, [
        code: 0,
        message: 8,
    ]);
    x64_layout_snapshot!(PluginBuildId, "PluginBuildId", 96, 8, [
        schema_version: 0,
        nautilus_plugin_version: 8,
        rustc_version: 24,
        target_triple: 40,
        build_profile: 56,
        precision_mode: 72,
        fixed_precision: 88,
    ]);
    x64_layout_snapshot!(PluginManifest, "PluginManifest", 216, 8, [
        abi_version: 0,
        plugin_name: 8,
        plugin_vendor: 24,
        plugin_version: 40,
        build_id: 56,
        custom_data: 152,
        actors: 168,
        strategies: 184,
        controllers: 200,
    ]);
    x64_layout_snapshot!(CustomDataRegistration, "CustomDataRegistration", 24, 8, [
        type_name: 0,
        vtable: 16,
    ]);
    x64_layout_snapshot!(ActorRegistration, "ActorRegistration", 24, 8, [
        type_name: 0,
        vtable: 16,
    ]);
    x64_layout_snapshot!(StrategyRegistration, "StrategyRegistration", 24, 8, [
        type_name: 0,
        vtable: 16,
    ]);
    x64_layout_snapshot!(ControllerRegistration, "ControllerRegistration", 24, 8, [
        type_name: 0,
        vtable: 16,
    ]);
    x64_layout_snapshot!(CustomDataVTable, "CustomDataVTable", 88, 8, [
        type_name: 0,
        schema_ipc: 8,
        from_json: 16,
        encode_batch: 24,
        decode_batch: 32,
        ts_event: 40,
        ts_init: 48,
        to_json: 56,
        clone_handle: 64,
        drop_handle: 72,
        eq_handles: 80,
    ]);
    x64_layout_snapshot!(MetadataEntry<'static>, "MetadataEntry", 32, 8, [
        key: 0,
        value: 16,
    ]);
    x64_layout_snapshot!(ActorVTable, "ActorVTable", 288, 8, [
        create: 0,
        drop_handle: 8,
        type_name: 16,
        on_start: 24,
        on_stop: 32,
        on_resume: 40,
        on_reset: 48,
        on_dispose: 56,
        on_degrade: 64,
        on_fault: 72,
        on_time_event: 80,
        on_data: 88,
        on_instrument: 96,
        on_book_deltas: 104,
        on_book: 112,
        on_quote: 120,
        on_trade: 128,
        on_bar: 136,
        on_mark_price: 144,
        on_index_price: 152,
        on_funding_rate: 160,
        on_option_greeks: 168,
        on_option_chain: 176,
        on_instrument_status: 184,
        on_instrument_close: 192,
        on_order_filled: 200,
        on_order_canceled: 208,
        on_signal: 216,
        on_historical_book_deltas: 224,
        on_historical_book_depth: 232,
        on_historical_quotes: 240,
        on_historical_trades: 248,
        on_historical_bars: 256,
        on_historical_mark_prices: 264,
        on_historical_index_prices: 272,
        on_historical_funding_rates: 280,
    ]);
    x64_layout_snapshot!(StrategyVTable, "StrategyVTable", 432, 8, [
        create: 0,
        drop_handle: 8,
        type_name: 16,
        on_start: 24,
        on_stop: 32,
        on_resume: 40,
        on_reset: 48,
        on_dispose: 56,
        on_degrade: 64,
        on_fault: 72,
        on_time_event: 80,
        on_data: 88,
        on_instrument: 96,
        on_book_deltas: 104,
        on_book: 112,
        on_quote: 120,
        on_trade: 128,
        on_bar: 136,
        on_mark_price: 144,
        on_index_price: 152,
        on_funding_rate: 160,
        on_option_greeks: 168,
        on_option_chain: 176,
        on_instrument_status: 184,
        on_instrument_close: 192,
        on_signal: 200,
        on_order_initialized: 208,
        on_order_submitted: 216,
        on_order_accepted: 224,
        on_order_rejected: 232,
        on_order_filled: 240,
        on_order_canceled: 248,
        on_order_expired: 256,
        on_order_triggered: 264,
        on_order_denied: 272,
        on_order_emulated: 280,
        on_order_released: 288,
        on_order_pending_update: 296,
        on_order_pending_cancel: 304,
        on_order_modify_rejected: 312,
        on_order_cancel_rejected: 320,
        on_order_updated: 328,
        on_position_opened: 336,
        on_position_changed: 344,
        on_position_closed: 352,
        on_market_exit: 360,
        on_historical_book_deltas: 368,
        on_historical_book_depth: 376,
        on_historical_quotes: 384,
        on_historical_trades: 392,
        on_historical_bars: 400,
        on_historical_mark_prices: 408,
        on_historical_index_prices: 416,
        on_historical_funding_rates: 424,
    ]);
    x64_layout_snapshot!(ControllerVTable, "ControllerVTable", 96, 8, [
        prepare: 0,
        create: 8,
        drop_handle: 16,
        type_name: 24,
        on_start: 32,
        on_stop: 40,
        on_resume: 48,
        on_reset: 56,
        on_dispose: 64,
        on_degrade: 72,
        on_fault: 80,
        on_time_event: 88,
    ]);
    x64_layout_snapshot!(HostVTable, "HostVTable", 304, 8, [
        abi_version: 0,
        clock_now_ns: 8,
        log: 16,
        cache_instrument: 24,
        cache_account: 32,
        cache_order: 40,
        cache_position: 48,
        cache_orders_for_strategy: 56,
        cache_positions_for_strategy: 64,
        subscribe_quotes: 72,
        unsubscribe_quotes: 80,
        subscribe_trades: 88,
        unsubscribe_trades: 96,
        subscribe_bars: 104,
        unsubscribe_bars: 112,
        subscribe_book_deltas: 120,
        unsubscribe_book_deltas: 128,
        subscribe_book_at_interval: 136,
        unsubscribe_book_at_interval: 144,
        msgbus_publish: 152,
        set_time_alert: 160,
        set_timer: 168,
        cancel_timer: 176,
        submit_order: 184,
        cancel_order: 192,
        modify_order: 200,
        submit_order_list: 208,
        cancel_orders: 216,
        cancel_all_orders: 224,
        close_position: 232,
        close_all_positions: 240,
        query_account: 248,
        query_order: 256,
        trader_id: 264,
        strategy_id: 272,
        component_state: 280,
        generate_client_order_id: 288,
        generate_order_list_id: 296,
    ]);
    x64_layout_snapshot!(ControllerHostVTable, "ControllerHostVTable", 72, 8, [
        abi_version: 0,
        create_plugin_strategy: 8,
        start_strategy: 16,
        stop_strategy: 24,
        exit_market: 32,
        remove_strategy: 40,
        instrument_exists: 48,
        log: 56,
        clock_now_ns: 64,
    ]);

    x64_layout_snapshot!(HostContext, "HostContext", 0, 1, []);
    x64_layout_snapshot!(ControllerHostContext, "ControllerHostContext", 0, 1, []);
    x64_layout_snapshot!(CustomDataHandle, "CustomDataHandle", 0, 1, []);
    x64_layout_snapshot!(PluginActorHandle, "PluginActorHandle", 0, 1, []);
    x64_layout_snapshot!(PluginStrategyHandle, "PluginStrategyHandle", 0, 1, []);
    x64_layout_snapshot!(PluginControllerHandle, "PluginControllerHandle", 0, 1, []);
    x64_layout_snapshot!(PluginCustomDataRef, "PluginCustomDataRef", 32, 8, []);
    x64_layout_snapshot!(OrderBookHandle, "OrderBookHandle", 8, 8, []);
    x64_layout_snapshot!(OrderBookDeltasHandle, "OrderBookDeltasHandle", 8, 8, []);
    x64_layout_snapshot!(InstrumentAnyHandle, "InstrumentAnyHandle", 8, 8, []);
    x64_layout_snapshot!(OptionChainSliceHandle, "OptionChainSliceHandle", 8, 8, []);
    x64_layout_snapshot!(SubmitOrderHandle, "SubmitOrderHandle", 8, 8, []);
    x64_layout_snapshot!(SubmitOrderListHandle, "SubmitOrderListHandle", 8, 8, []);
    x64_layout_snapshot!(CancelOrderHandle, "CancelOrderHandle", 8, 8, []);
    x64_layout_snapshot!(CancelOrdersHandle, "CancelOrdersHandle", 8, 8, []);
    x64_layout_snapshot!(CancelAllOrdersHandle, "CancelAllOrdersHandle", 8, 8, []);
    x64_layout_snapshot!(ModifyOrderHandle, "ModifyOrderHandle", 8, 8, []);
    x64_layout_snapshot!(ClosePositionHandle, "ClosePositionHandle", 8, 8, []);
    x64_layout_snapshot!(CloseAllPositionsHandle, "CloseAllPositionsHandle", 8, 8, []);
    x64_layout_snapshot!(QueryAccountHandle, "QueryAccountHandle", 8, 8, []);
    x64_layout_snapshot!(QueryOrderHandle, "QueryOrderHandle", 8, 8, []);

    x64_layout_snapshot!(SubmitOrderCommand, "SubmitOrderCommand", if high_precision { 1056 } else { 896 }, if high_precision { 16 } else { 8 }, [
        order: 0,
        position_id: if high_precision { 960 } else { 808 },
        client_id: if high_precision { 968 } else { 816 },
        params: if high_precision { 976 } else { 824 },
    ]);
    x64_layout_snapshot!(SubmitOrderListCommand, "SubmitOrderListCommand", 112, 8, [
        orders: 0,
        position_id: 24,
        client_id: 32,
        params: 40,
    ]);
    x64_layout_snapshot!(CancelOrderCommand, "CancelOrderCommand", 88, 8, [
        client_order_id: 0,
        client_id: 8,
        params: 16,
    ]);
    x64_layout_snapshot!(CancelOrdersCommand, "CancelOrdersCommand", 104, 8, [
        client_order_ids: 0,
        client_id: 24,
        params: 32,
    ]);
    x64_layout_snapshot!(CancelAllOrdersCommand, "CancelAllOrdersCommand", 104, 8, [
        instrument_id: 0,
        order_side: 16,
        client_id: 24,
        params: 32,
    ]);
    x64_layout_snapshot!(ModifyOrderCommand, "ModifyOrderCommand", if high_precision { 240 } else { 160 }, if high_precision { 16 } else { 8 }, [
        client_order_id: 0,
        quantity: if high_precision { 16 } else { 8 },
        price: if high_precision { 64 } else { 32 },
        trigger_price: if high_precision { 112 } else { 56 },
        client_id: if high_precision { 160 } else { 80 },
        params: if high_precision { 168 } else { 88 },
    ]);
    x64_layout_snapshot!(ClosePositionCommand, "ClosePositionCommand", 48, 8, [
        position_id: 0,
        client_id: 8,
        tags: 16,
        time_in_force: 40,
        reduce_only: 44,
        quote_quantity: 45,
    ]);
    x64_layout_snapshot!(CloseAllPositionsCommand, "CloseAllPositionsCommand", 64, 8, [
        instrument_id: 0,
        position_side: 16,
        client_id: 24,
        tags: 32,
        time_in_force: 56,
        reduce_only: 60,
        quote_quantity: 61,
    ]);
    x64_layout_snapshot!(QueryAccountCommand, "QueryAccountCommand", 88, 8, [
        account_id: 0,
        client_id: 8,
        params: 16,
    ]);
    x64_layout_snapshot!(QueryOrderCommand, "QueryOrderCommand", 88, 8, [
        client_order_id: 0,
        client_id: 8,
        params: 16,
    ]);
}

#[rstest]
fn primitive_boundary_layouts_match_snapshot() {
    assert_type_layout::<HostLogLevel>("HostLogLevel", 1, 1);
    assert_type_layout::<PluginErrorCode>("PluginErrorCode", 4, 4);

    repr_c_layout_snapshot!(BorrowedStr<'static>, "BorrowedStr", [
        ptr: *const u8,
        len: usize,
    ]);
    repr_c_layout_snapshot!(Slice<'static, u8>, "Slice<u8>", [
        ptr: *const u8,
        len: usize,
    ]);
    repr_c_layout_snapshot!(OwnedBytes, "OwnedBytes", [
        ptr: *mut u8,
        len: usize,
        cap: usize,
        drop_fn: Option<unsafe extern "C" fn(*mut u8, usize, usize)>,
    ]);
    repr_c_layout_snapshot!(PluginError, "PluginError", [
        code: PluginErrorCode,
        message: OwnedBytes,
    ]);
}

#[rstest]
fn manifest_layouts_match_snapshot() {
    repr_c_layout_snapshot!(PluginBuildId, "PluginBuildId", [
        schema_version: u32,
        nautilus_plugin_version: BorrowedStr<'static>,
        rustc_version: BorrowedStr<'static>,
        target_triple: BorrowedStr<'static>,
        build_profile: BorrowedStr<'static>,
        precision_mode: BorrowedStr<'static>,
        fixed_precision: u8,
    ]);
    repr_c_layout_snapshot!(PluginManifest, "PluginManifest", [
        abi_version: u32,
        plugin_name: BorrowedStr<'static>,
        plugin_vendor: BorrowedStr<'static>,
        plugin_version: BorrowedStr<'static>,
        build_id: PluginBuildId,
        custom_data: Slice<'static, CustomDataRegistration>,
        actors: Slice<'static, ActorRegistration>,
        strategies: Slice<'static, StrategyRegistration>,
        controllers: Slice<'static, ControllerRegistration>,
    ]);
    repr_c_layout_snapshot!(CustomDataRegistration, "CustomDataRegistration", [
        type_name: BorrowedStr<'static>,
        vtable: *const CustomDataVTable,
    ]);
    repr_c_layout_snapshot!(ActorRegistration, "ActorRegistration", [
        type_name: BorrowedStr<'static>,
        vtable: *const ActorVTable,
    ]);
    repr_c_layout_snapshot!(StrategyRegistration, "StrategyRegistration", [
        type_name: BorrowedStr<'static>,
        vtable: *const StrategyVTable,
    ]);
    repr_c_layout_snapshot!(ControllerRegistration, "ControllerRegistration", [
        type_name: BorrowedStr<'static>,
        vtable: *const ControllerVTable,
    ]);
}

#[rstest]
fn custom_data_vtable_layout_matches_snapshot() {
    pointer_table_layout_snapshot!(
        CustomDataVTable,
        "CustomDataVTable",
        [
            type_name,
            schema_ipc,
            from_json,
            encode_batch,
            decode_batch,
            ts_event,
            ts_init,
            to_json,
            clone_handle,
            drop_handle,
            eq_handles,
        ]
    );
    repr_c_layout_snapshot!(MetadataEntry<'static>, "MetadataEntry", [
        key: BorrowedStr<'static>,
        value: BorrowedStr<'static>,
    ]);
}

#[rstest]
fn actor_vtable_layout_matches_snapshot() {
    pointer_table_layout_snapshot!(
        ActorVTable,
        "ActorVTable",
        [
            create,
            drop_handle,
            type_name,
            on_start,
            on_stop,
            on_resume,
            on_reset,
            on_dispose,
            on_degrade,
            on_fault,
            on_time_event,
            on_data,
            on_instrument,
            on_book_deltas,
            on_book,
            on_quote,
            on_trade,
            on_bar,
            on_mark_price,
            on_index_price,
            on_funding_rate,
            on_option_greeks,
            on_option_chain,
            on_instrument_status,
            on_instrument_close,
            on_order_filled,
            on_order_canceled,
            on_signal,
            on_historical_book_deltas,
            on_historical_book_depth,
            on_historical_quotes,
            on_historical_trades,
            on_historical_bars,
            on_historical_mark_prices,
            on_historical_index_prices,
            on_historical_funding_rates,
        ]
    );
}

#[rstest]
fn strategy_vtable_layout_matches_snapshot() {
    pointer_table_layout_snapshot!(
        StrategyVTable,
        "StrategyVTable",
        [
            create,
            drop_handle,
            type_name,
            on_start,
            on_stop,
            on_resume,
            on_reset,
            on_dispose,
            on_degrade,
            on_fault,
            on_time_event,
            on_data,
            on_instrument,
            on_book_deltas,
            on_book,
            on_quote,
            on_trade,
            on_bar,
            on_mark_price,
            on_index_price,
            on_funding_rate,
            on_option_greeks,
            on_option_chain,
            on_instrument_status,
            on_instrument_close,
            on_signal,
            on_order_initialized,
            on_order_submitted,
            on_order_accepted,
            on_order_rejected,
            on_order_filled,
            on_order_canceled,
            on_order_expired,
            on_order_triggered,
            on_order_denied,
            on_order_emulated,
            on_order_released,
            on_order_pending_update,
            on_order_pending_cancel,
            on_order_modify_rejected,
            on_order_cancel_rejected,
            on_order_updated,
            on_position_opened,
            on_position_changed,
            on_position_closed,
            on_market_exit,
            on_historical_book_deltas,
            on_historical_book_depth,
            on_historical_quotes,
            on_historical_trades,
            on_historical_bars,
            on_historical_mark_prices,
            on_historical_index_prices,
            on_historical_funding_rates,
        ]
    );
}

#[rstest]
fn controller_vtable_layout_matches_snapshot() {
    pointer_table_layout_snapshot!(
        ControllerVTable,
        "ControllerVTable",
        [
            prepare,
            create,
            drop_handle,
            type_name,
            on_start,
            on_stop,
            on_resume,
            on_reset,
            on_dispose,
            on_degrade,
            on_fault,
            on_time_event,
        ]
    );
}

#[rstest]
fn host_vtable_layout_matches_snapshot() {
    host_vtable_layout_snapshot!([
        clock_now_ns,
        log,
        cache_instrument,
        cache_account,
        cache_order,
        cache_position,
        cache_orders_for_strategy,
        cache_positions_for_strategy,
        subscribe_quotes,
        unsubscribe_quotes,
        subscribe_trades,
        unsubscribe_trades,
        subscribe_bars,
        unsubscribe_bars,
        subscribe_book_deltas,
        unsubscribe_book_deltas,
        subscribe_book_at_interval,
        unsubscribe_book_at_interval,
        msgbus_publish,
        set_time_alert,
        set_timer,
        cancel_timer,
        submit_order,
        cancel_order,
        modify_order,
        submit_order_list,
        cancel_orders,
        cancel_all_orders,
        close_position,
        close_all_positions,
        query_account,
        query_order,
        trader_id,
        strategy_id,
        component_state,
        generate_client_order_id,
        generate_order_list_id,
    ]);
}

#[rstest]
fn controller_host_vtable_layout_matches_snapshot() {
    repr_c_layout_snapshot!(ControllerHostVTable, "ControllerHostVTable", [
        abi_version: u32,
        create_plugin_strategy: unsafe extern "C" fn(
            *const ControllerHostContext,
            BorrowedStr<'_>,
        ) -> nautilus_plugin::PluginResult<OwnedBytes>,
        start_strategy: unsafe extern "C" fn(
            *const ControllerHostContext,
            BorrowedStr<'_>,
        ) -> nautilus_plugin::PluginResult<OwnedBytes>,
        stop_strategy: unsafe extern "C" fn(
            *const ControllerHostContext,
            BorrowedStr<'_>,
        ) -> nautilus_plugin::PluginResult<OwnedBytes>,
        exit_market: unsafe extern "C" fn(
            *const ControllerHostContext,
            BorrowedStr<'_>,
        ) -> nautilus_plugin::PluginResult<OwnedBytes>,
        remove_strategy: unsafe extern "C" fn(
            *const ControllerHostContext,
            BorrowedStr<'_>,
        ) -> nautilus_plugin::PluginResult<OwnedBytes>,
        instrument_exists: unsafe extern "C" fn(
            *const ControllerHostContext,
            BorrowedStr<'_>,
        ) -> nautilus_plugin::PluginResult<OwnedBytes>,
        log: unsafe extern "C" fn(
            *const ControllerHostContext,
            BorrowedStr<'_>,
        ) -> nautilus_plugin::PluginResult<OwnedBytes>,
        clock_now_ns: unsafe extern "C" fn(
            *const ControllerHostContext,
            BorrowedStr<'_>,
        ) -> nautilus_plugin::PluginResult<OwnedBytes>,
    ]);
}

#[rstest]
fn opaque_boundary_handles_match_snapshot() {
    assert_type_layout::<HostContext>("HostContext", 0, 1);
    assert_type_layout::<ControllerHostContext>("ControllerHostContext", 0, 1);
    assert_type_layout::<CustomDataHandle>("CustomDataHandle", 0, 1);
    assert_type_layout::<PluginActorHandle>("PluginActorHandle", 0, 1);
    assert_type_layout::<PluginStrategyHandle>("PluginStrategyHandle", 0, 1);
    assert_type_layout::<PluginControllerHandle>("PluginControllerHandle", 0, 1);
    assert_type_layout::<PluginCustomDataRef>(
        "PluginCustomDataRef",
        size_of::<BorrowedStr<'static>>() + (2 * pointer_size()),
        pointer_size(),
    );

    assert_pointer_sized::<OrderBookHandle>("OrderBookHandle");
    assert_pointer_sized::<OrderBookDeltasHandle>("OrderBookDeltasHandle");
    assert_pointer_sized::<InstrumentAnyHandle>("InstrumentAnyHandle");
    assert_pointer_sized::<OptionChainSliceHandle>("OptionChainSliceHandle");

    assert_pointer_sized::<SubmitOrderHandle>("SubmitOrderHandle");
    assert_pointer_sized::<SubmitOrderListHandle>("SubmitOrderListHandle");
    assert_pointer_sized::<CancelOrderHandle>("CancelOrderHandle");
    assert_pointer_sized::<CancelOrdersHandle>("CancelOrdersHandle");
    assert_pointer_sized::<CancelAllOrdersHandle>("CancelAllOrdersHandle");
    assert_pointer_sized::<ModifyOrderHandle>("ModifyOrderHandle");
    assert_pointer_sized::<ClosePositionHandle>("ClosePositionHandle");
    assert_pointer_sized::<CloseAllPositionsHandle>("CloseAllPositionsHandle");
    assert_pointer_sized::<QueryAccountHandle>("QueryAccountHandle");
    assert_pointer_sized::<QueryOrderHandle>("QueryOrderHandle");
}

#[rstest]
fn command_struct_layouts_match_snapshot() {
    repr_c_layout_snapshot!(SubmitOrderCommand, "SubmitOrderCommand", [
        order: OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ]);
    repr_c_layout_snapshot!(SubmitOrderListCommand, "SubmitOrderListCommand", [
        orders: Vec<OrderAny>,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ]);
    repr_c_layout_snapshot!(CancelOrderCommand, "CancelOrderCommand", [
        client_order_id: ClientOrderId,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ]);
    repr_c_layout_snapshot!(CancelOrdersCommand, "CancelOrdersCommand", [
        client_order_ids: Vec<ClientOrderId>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ]);
    repr_c_layout_snapshot!(CancelAllOrdersCommand, "CancelAllOrdersCommand", [
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ]);
    repr_c_layout_snapshot!(ModifyOrderCommand, "ModifyOrderCommand", [
        client_order_id: ClientOrderId,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ]);
    repr_c_layout_snapshot!(ClosePositionCommand, "ClosePositionCommand", [
        position_id: PositionId,
        client_id: Option<ClientId>,
        tags: Option<Vec<Ustr>>,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
    ]);
    repr_c_layout_snapshot!(CloseAllPositionsCommand, "CloseAllPositionsCommand", [
        instrument_id: InstrumentId,
        position_side: Option<PositionSide>,
        client_id: Option<ClientId>,
        tags: Option<Vec<Ustr>>,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
    ]);
    repr_c_layout_snapshot!(QueryAccountCommand, "QueryAccountCommand", [
        account_id: AccountId,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ]);
    repr_c_layout_snapshot!(QueryOrderCommand, "QueryOrderCommand", [
        client_order_id: ClientOrderId,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ]);
}

fn field_layout<T>(name: &'static str, offset: usize) -> FieldLayout {
    FieldLayout {
        name,
        size: size_of::<T>(),
        align: align_of::<T>(),
        offset,
    }
}

fn assert_repr_c_layout<T>(type_name: &str, fields: &[FieldLayout]) {
    let mut expected_offset = 0;
    let mut expected_align = 1;

    for field in fields {
        expected_align = expected_align.max(field.align);
        expected_offset = align_up(expected_offset, field.align);
        assert_field_offset(type_name, field.name, field.offset, expected_offset);
        expected_offset += field.size;
    }

    assert_type_layout::<T>(
        type_name,
        align_up(expected_offset, expected_align),
        expected_align,
    );
}

fn assert_pointer_table_layout<T>(type_name: &str, fields: &[(&str, usize)]) {
    let ptr = pointer_size();
    assert_type_layout::<T>(type_name, fields.len() * ptr, ptr);
    for (index, (field_name, actual_offset)) in fields.iter().enumerate() {
        assert_field_offset(type_name, field_name, *actual_offset, index * ptr);
    }
}

fn assert_host_vtable_layout(fields: &[(&str, usize)]) {
    let ptr = pointer_size();
    assert_type_layout::<HostVTable>("HostVTable", (fields.len() + 1) * ptr, ptr);
    assert_field_offset(
        "HostVTable",
        "abi_version",
        offset_of!(HostVTable, abi_version),
        0,
    );

    for (index, (field_name, actual_offset)) in fields.iter().enumerate() {
        assert_field_offset("HostVTable", field_name, *actual_offset, (index + 1) * ptr);
    }
}

fn assert_pointer_sized<T>(type_name: &str) {
    let ptr = pointer_size();
    assert_type_layout::<T>(type_name, ptr, ptr);
}

fn assert_type_layout<T>(type_name: &str, expected_size: usize, expected_align: usize) {
    let actual_size = size_of::<T>();
    let actual_align = align_of::<T>();
    assert_eq!(
        actual_size, expected_size,
        "{type_name} size changed: expected {expected_size}, was {actual_size}",
    );
    assert_eq!(
        actual_align, expected_align,
        "{type_name} alignment changed: expected {expected_align}, was {actual_align}",
    );
}

fn assert_field_offset(
    type_name: &str,
    field_name: &str,
    actual_offset: usize,
    expected_offset: usize,
) {
    assert_eq!(
        actual_offset, expected_offset,
        "{type_name}.{field_name} offset changed: expected {expected_offset}, was {actual_offset}",
    );
}

fn align_up(value: usize, align: usize) -> usize {
    let remainder = value % align;
    if remainder == 0 {
        value
    } else {
        value + align - remainder
    }
}

fn pointer_size() -> usize {
    size_of::<usize>()
}
