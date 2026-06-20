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

//! Host-side `HostVTable` that routes plug-in callbacks through live adapters.
//!
//! The thunks resolve the calling adapter from the per-instance
//! [`HostContextInner`] payload, then route cache reads, subscription changes,
//! msgbus publishes, timers, and order commands through the same
//! [`DataActor`] / [`Strategy`] paths as
//! compiled-in components.

#![allow(unsafe_code)]
#![allow(
    clippy::multiple_unsafe_ops_per_block,
    reason = "vtable deref and FFI call form a single boundary callback; \
              SAFETY comments cover both ops together"
)]

use std::{
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    str::FromStr,
    sync::OnceLock,
};

use nautilus_common::{
    actor::{DataActor, DataActorNative, registry::try_get_actor_unchecked},
    cache::Cache,
    component::Component,
    msgbus,
};
use nautilus_core::{Params, UnixNanos, time::duration_since_unix_epoch};
use nautilus_model::{
    data::BarType,
    enums::{BookType, FromU8},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId},
    orders::{Order, OrderAny},
};
use nautilus_trading::strategy::{Strategy, StrategyNative};
use serde::Serialize;

use crate::{
    NAUTILUS_PLUGIN_ABI_VERSION,
    boundary::{BorrowedStr, OwnedBytes, PluginError, PluginErrorCode, PluginResult, Slice},
    bridge::{
        actor::PluginActorAdapter,
        registry::{HostContextInner, controller_host_context_inner, host_context_inner},
        strategy::PluginStrategyAdapter,
    },
    host::{ControllerHostContext, ControllerHostVTable, HostContext, HostLogLevel, HostVTable},
    loader::PluginLoader,
    normalize::BoundaryCommandHandle,
    panic::{drop_payload, panic_message},
    surfaces::commands::{
        CancelAllOrdersHandle, CancelOrderHandle, CancelOrdersHandle, CloseAllPositionsHandle,
        ClosePositionHandle, ModifyOrderHandle, QueryAccountHandle, QueryOrderHandle,
        SubmitOrderHandle, SubmitOrderListHandle,
    },
};

/// Returns the process-wide `HostVTable` configured for the live node.
///
/// One static vtable is enough because plug-ins never compare vtables; they
/// only call through the function pointers. The vtable bundles the order
/// command thunks that route through [`Strategy`].
#[must_use]
pub fn host_vtable() -> *const HostVTable {
    static HOST: OnceLock<HostVTable> = OnceLock::new();
    std::ptr::from_ref(HOST.get_or_init(|| HostVTable {
        abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
        clock_now_ns: host_clock_now_ns,
        log: host_log,
        cache_instrument: host_cache_instrument,
        cache_account: host_cache_account,
        cache_order: host_cache_order,
        cache_position: host_cache_position,
        cache_orders_for_strategy: host_cache_orders_for_strategy,
        cache_positions_for_strategy: host_cache_positions_for_strategy,
        subscribe_quotes: host_subscribe_quotes,
        unsubscribe_quotes: host_unsubscribe_quotes,
        subscribe_trades: host_subscribe_trades,
        unsubscribe_trades: host_unsubscribe_trades,
        subscribe_bars: host_subscribe_bars,
        unsubscribe_bars: host_unsubscribe_bars,
        subscribe_book_deltas: host_subscribe_book_deltas,
        unsubscribe_book_deltas: host_unsubscribe_book_deltas,
        subscribe_book_at_interval: host_subscribe_book_at_interval,
        unsubscribe_book_at_interval: host_unsubscribe_book_at_interval,
        msgbus_publish: host_msgbus_publish,
        set_time_alert: host_set_time_alert,
        set_timer: host_set_timer,
        cancel_timer: host_cancel_timer,
        submit_order: host_submit_order,
        cancel_order: host_cancel_order,
        modify_order: host_modify_order,
        submit_order_list: host_submit_order_list,
        cancel_orders: host_cancel_orders,
        cancel_all_orders: host_cancel_all_orders,
        close_position: host_close_position,
        close_all_positions: host_close_all_positions,
        query_account: host_query_account,
        query_order: host_query_order,
        trader_id: host_trader_id,
        strategy_id: host_strategy_id,
        component_state: host_component_state,
        generate_client_order_id: host_generate_client_order_id,
        generate_order_list_id: host_generate_order_list_id,
    }))
}

/// Returns the process-wide `ControllerHostVTable` for plug-in controllers.
#[must_use]
pub fn controller_host_vtable() -> *const ControllerHostVTable {
    static HOST: OnceLock<ControllerHostVTable> = OnceLock::new();
    std::ptr::from_ref(HOST.get_or_init(|| ControllerHostVTable {
        abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
        create_plugin_strategy: controller_host_not_implemented,
        start_strategy: controller_host_not_implemented,
        stop_strategy: controller_host_not_implemented,
        exit_market: controller_host_not_implemented,
        remove_strategy: controller_host_not_implemented,
        instrument_exists: controller_host_not_implemented,
        log: controller_host_log,
        clock_now_ns: controller_host_clock_now_ns,
    }))
}

/// Returns a [`PluginLoader`] pre-bound to the host vtable from
/// [`host_vtable`].
///
/// The loader hands every plug-in cdylib the live-node vtable so order
/// stateful callbacks route through live adapters instead of returning
/// `NotImplemented`.
#[must_use]
pub fn plugin_loader() -> PluginLoader {
    PluginLoader::with_host(host_vtable())
}

unsafe extern "C" fn host_clock_now_ns() -> u64 {
    u64::try_from(duration_since_unix_epoch().as_nanos()).unwrap_or(u64::MAX)
}

unsafe extern "C" fn controller_host_not_implemented(
    ctx: *const ControllerHostContext,
    _request_json: BorrowedStr<'_>,
) -> PluginResult<OwnedBytes> {
    let context = controller_context_label(ctx);
    PluginResult::Err(PluginError::new(
        PluginErrorCode::NotImplemented,
        format!("{context} controller host service is not implemented"),
    ))
}

unsafe extern "C" fn controller_host_log(
    ctx: *const ControllerHostContext,
    request_json: BorrowedStr<'_>,
) -> PluginResult<OwnedBytes> {
    guard_host_dispatch("controller log", || {
        let context = controller_context_label(ctx);
        let request = borrowed_utf8(request_json, "request_json")?;
        log::info!(target: "nautilus_plugin", "[{context}] {request}");
        Ok(OwnedBytes::empty())
    })
}

unsafe extern "C" fn controller_host_clock_now_ns(
    _ctx: *const ControllerHostContext,
    _request_json: BorrowedStr<'_>,
) -> PluginResult<OwnedBytes> {
    json_bytes(&serde_json::json!({
        "unix_nanos": u64::try_from(duration_since_unix_epoch().as_nanos()).unwrap_or(u64::MAX),
    }))
}

fn controller_context_label(ctx: *const ControllerHostContext) -> String {
    // SAFETY: plug-ins round-trip the context pointer supplied at create time.
    let Some(inner) = (unsafe { controller_host_context_inner(ctx) }) else {
        return "unknown-controller".to_string();
    };
    format!("{}:{}", inner.plugin_name, inner.type_name)
}

unsafe extern "C" fn host_log(
    level: HostLogLevel,
    target: BorrowedStr<'_>,
    message: BorrowedStr<'_>,
) {
    // No error channel here, so a panicking logger must be swallowed rather
    // than unwind out of the `extern "C"` thunk and abort the process.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: producer holds the storage live across the call.
        let target = unsafe { target.to_string_lossy() };
        // SAFETY: see above.
        let message = unsafe { message.to_string_lossy() };
        match level {
            HostLogLevel::Error => log::error!(target: "nautilus_plugin", "[{target}] {message}"),
            HostLogLevel::Warn => log::warn!(target: "nautilus_plugin", "[{target}] {message}"),
            HostLogLevel::Info => log::info!(target: "nautilus_plugin", "[{target}] {message}"),
            HostLogLevel::Debug => log::debug!(target: "nautilus_plugin", "[{target}] {message}"),
            HostLogLevel::Trace => log::trace!(target: "nautilus_plugin", "[{target}] {message}"),
        }
    }));
}

unsafe extern "C" fn host_cache_instrument(
    ctx: *const HostContext,
    instrument_id: BorrowedStr<'_>,
) -> PluginResult<OwnedBytes> {
    let instrument_id = match parse_instrument_id(instrument_id, "instrument_id") {
        Ok(id) => id,
        Err(e) => return PluginResult::Err(e),
    };

    dispatch_cache_query(ctx, "cache_instrument", |cache, _| {
        json_optional(cache.instrument(&instrument_id))
    })
}

unsafe extern "C" fn host_cache_account(
    ctx: *const HostContext,
    account_id: BorrowedStr<'_>,
) -> PluginResult<OwnedBytes> {
    let account_id = match parse_account_id(account_id, "account_id") {
        Ok(id) => id,
        Err(e) => return PluginResult::Err(e),
    };

    dispatch_cache_query(ctx, "cache_account", |cache, _| {
        let account = cache.account(&account_id).map(|account| account.cloned());
        json_optional(account.as_ref())
    })
}

unsafe extern "C" fn host_cache_order(
    ctx: *const HostContext,
    client_order_id: BorrowedStr<'_>,
) -> PluginResult<OwnedBytes> {
    let client_order_id = match parse_client_order_id(client_order_id, "client_order_id") {
        Ok(id) => id,
        Err(e) => return PluginResult::Err(e),
    };

    dispatch_cache_query(ctx, "cache_order", |cache, _| {
        let order = cache.order(&client_order_id).map(|order| order.cloned());
        json_optional(order.as_ref())
    })
}

unsafe extern "C" fn host_cache_position(
    ctx: *const HostContext,
    position_id: BorrowedStr<'_>,
) -> PluginResult<OwnedBytes> {
    let position_id = match parse_position_id(position_id, "position_id") {
        Ok(id) => id,
        Err(e) => return PluginResult::Err(e),
    };

    dispatch_cache_query(ctx, "cache_position", |cache, _| {
        let position = cache
            .position(&position_id)
            .map(|position| position.cloned());
        json_optional(position.as_ref())
    })
}

unsafe extern "C" fn host_cache_orders_for_strategy(
    ctx: *const HostContext,
    strategy_id: BorrowedStr<'_>,
) -> PluginResult<OwnedBytes> {
    dispatch_cache_query(ctx, "cache_orders_for_strategy", |cache, inner| {
        let strategy_id = match parse_strategy_id_for_context(strategy_id, inner) {
            Ok(id) => id,
            Err(e) => return PluginResult::Err(e),
        };
        let orders = cache
            .orders(None, None, Some(&strategy_id), None, None)
            .into_iter()
            .map(|order| order.cloned())
            .collect::<Vec<_>>();
        json_bytes(&orders)
    })
}

unsafe extern "C" fn host_cache_positions_for_strategy(
    ctx: *const HostContext,
    strategy_id: BorrowedStr<'_>,
) -> PluginResult<OwnedBytes> {
    dispatch_cache_query(ctx, "cache_positions_for_strategy", |cache, inner| {
        let strategy_id = match parse_strategy_id_for_context(strategy_id, inner) {
            Ok(id) => id,
            Err(e) => return PluginResult::Err(e),
        };
        let positions = cache
            .positions(None, None, Some(&strategy_id), None, None)
            .into_iter()
            .map(|position| position.cloned())
            .collect::<Vec<_>>();
        json_bytes(&positions)
    })
}

unsafe extern "C" fn host_trader_id(ctx: *const HostContext) -> PluginResult<OwnedBytes> {
    guard_host_dispatch("trader_id", || {
        let inner = resolve_context(ctx, "trader_id")?;
        let actor_id = inner.actor_id.inner();
        let trader_id = if inner.is_strategy {
            let adapter_ref = try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id)
                .ok_or_else(|| resolve_adapter_error("trader_id", inner))?;
            adapter_ref.trader_id()
        } else {
            let adapter_ref = try_get_actor_unchecked::<PluginActorAdapter>(&actor_id)
                .ok_or_else(|| resolve_adapter_error("trader_id", inner))?;
            adapter_ref.trader_id()
        }
        .ok_or_else(|| {
            PluginError::new(
                PluginErrorCode::InvalidArgument,
                format!("trader_id is unavailable for unregistered actor_id={actor_id}"),
            )
        })?;

        json_bytes(&trader_id).into_result()
    })
}

unsafe extern "C" fn host_strategy_id(ctx: *const HostContext) -> PluginResult<OwnedBytes> {
    dispatch_strategy_query(ctx, "strategy_id", |adapter| {
        let strategy_id = Strategy::core(adapter).strategy_id().ok_or_else(|| {
            PluginError::new(
                PluginErrorCode::InvalidArgument,
                "strategy_id is unavailable before strategy registration",
            )
        })?;
        json_bytes(&strategy_id).into_result()
    })
}

unsafe extern "C" fn host_component_state(ctx: *const HostContext) -> PluginResult<u8> {
    guard_host_dispatch("component_state", || {
        let inner = resolve_context(ctx, "component_state")?;
        let actor_id = inner.actor_id.inner();
        let state = if inner.is_strategy {
            let adapter_ref = try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id)
                .ok_or_else(|| resolve_adapter_error("component_state", inner))?;
            adapter_ref.state()
        } else {
            let adapter_ref = try_get_actor_unchecked::<PluginActorAdapter>(&actor_id)
                .ok_or_else(|| resolve_adapter_error("component_state", inner))?;
            adapter_ref.state()
        };

        Ok(state as u8)
    })
}

unsafe extern "C" fn host_generate_client_order_id(
    ctx: *const HostContext,
) -> PluginResult<OwnedBytes> {
    dispatch_strategy_query(ctx, "generate_client_order_id", |adapter| {
        require_registered_strategy(adapter, "generate_client_order_id")?;
        let client_order_id = Strategy::core_mut(adapter)
            .order_factory()
            .generate_client_order_id();
        json_bytes(&client_order_id).into_result()
    })
}

unsafe extern "C" fn host_generate_order_list_id(
    ctx: *const HostContext,
) -> PluginResult<OwnedBytes> {
    dispatch_strategy_query(ctx, "generate_order_list_id", |adapter| {
        require_registered_strategy(adapter, "generate_order_list_id")?;
        let order_list_id = Strategy::core_mut(adapter)
            .order_factory()
            .generate_order_list_id();
        json_bytes(&order_list_id).into_result()
    })
}

unsafe extern "C" fn host_subscribe_quotes(
    ctx: *const HostContext,
    instrument_id: BorrowedStr<'_>,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    let args = match parse_instrument_subscription(instrument_id, client_id, params_json) {
        Ok(args) => args,
        Err(e) => return PluginResult::Err(e),
    };
    let actor_args = args.clone();
    let strategy_args = args;

    dispatch_actor_action(
        ctx,
        "subscribe_quotes",
        |actor| {
            DataActor::subscribe_quotes(
                actor,
                actor_args.instrument_id,
                actor_args.client_id,
                actor_args.params,
            );
            Ok(())
        },
        |strategy| {
            DataActor::subscribe_quotes(
                strategy,
                strategy_args.instrument_id,
                strategy_args.client_id,
                strategy_args.params,
            );
            Ok(())
        },
    )
}

unsafe extern "C" fn host_unsubscribe_quotes(
    ctx: *const HostContext,
    instrument_id: BorrowedStr<'_>,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    let args = match parse_instrument_subscription(instrument_id, client_id, params_json) {
        Ok(args) => args,
        Err(e) => return PluginResult::Err(e),
    };
    let actor_args = args.clone();
    let strategy_args = args;

    dispatch_actor_action(
        ctx,
        "unsubscribe_quotes",
        |actor| {
            DataActor::unsubscribe_quotes(
                actor,
                actor_args.instrument_id,
                actor_args.client_id,
                actor_args.params,
            );
            Ok(())
        },
        |strategy| {
            DataActor::unsubscribe_quotes(
                strategy,
                strategy_args.instrument_id,
                strategy_args.client_id,
                strategy_args.params,
            );
            Ok(())
        },
    )
}

unsafe extern "C" fn host_subscribe_trades(
    ctx: *const HostContext,
    instrument_id: BorrowedStr<'_>,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    let args = match parse_instrument_subscription(instrument_id, client_id, params_json) {
        Ok(args) => args,
        Err(e) => return PluginResult::Err(e),
    };
    let actor_args = args.clone();
    let strategy_args = args;

    dispatch_actor_action(
        ctx,
        "subscribe_trades",
        |actor| {
            DataActor::subscribe_trades(
                actor,
                actor_args.instrument_id,
                actor_args.client_id,
                actor_args.params,
            );
            Ok(())
        },
        |strategy| {
            DataActor::subscribe_trades(
                strategy,
                strategy_args.instrument_id,
                strategy_args.client_id,
                strategy_args.params,
            );
            Ok(())
        },
    )
}

unsafe extern "C" fn host_unsubscribe_trades(
    ctx: *const HostContext,
    instrument_id: BorrowedStr<'_>,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    let args = match parse_instrument_subscription(instrument_id, client_id, params_json) {
        Ok(args) => args,
        Err(e) => return PluginResult::Err(e),
    };
    let actor_args = args.clone();
    let strategy_args = args;

    dispatch_actor_action(
        ctx,
        "unsubscribe_trades",
        |actor| {
            DataActor::unsubscribe_trades(
                actor,
                actor_args.instrument_id,
                actor_args.client_id,
                actor_args.params,
            );
            Ok(())
        },
        |strategy| {
            DataActor::unsubscribe_trades(
                strategy,
                strategy_args.instrument_id,
                strategy_args.client_id,
                strategy_args.params,
            );
            Ok(())
        },
    )
}

unsafe extern "C" fn host_subscribe_bars(
    ctx: *const HostContext,
    bar_type: BorrowedStr<'_>,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    let args = match parse_bar_subscription(bar_type, client_id, params_json) {
        Ok(args) => args,
        Err(e) => return PluginResult::Err(e),
    };
    let actor_args = args.clone();
    let strategy_args = args;

    dispatch_actor_action(
        ctx,
        "subscribe_bars",
        |actor| {
            DataActor::subscribe_bars(
                actor,
                actor_args.bar_type,
                actor_args.client_id,
                actor_args.params,
            );
            Ok(())
        },
        |strategy| {
            DataActor::subscribe_bars(
                strategy,
                strategy_args.bar_type,
                strategy_args.client_id,
                strategy_args.params,
            );
            Ok(())
        },
    )
}

unsafe extern "C" fn host_unsubscribe_bars(
    ctx: *const HostContext,
    bar_type: BorrowedStr<'_>,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    let args = match parse_bar_subscription(bar_type, client_id, params_json) {
        Ok(args) => args,
        Err(e) => return PluginResult::Err(e),
    };
    let actor_args = args.clone();
    let strategy_args = args;

    dispatch_actor_action(
        ctx,
        "unsubscribe_bars",
        |actor| {
            DataActor::unsubscribe_bars(
                actor,
                actor_args.bar_type,
                actor_args.client_id,
                actor_args.params,
            );
            Ok(())
        },
        |strategy| {
            DataActor::unsubscribe_bars(
                strategy,
                strategy_args.bar_type,
                strategy_args.client_id,
                strategy_args.params,
            );
            Ok(())
        },
    )
}

unsafe extern "C" fn host_subscribe_book_deltas(
    ctx: *const HostContext,
    instrument_id: BorrowedStr<'_>,
    book_type: u8,
    depth: usize,
    client_id: BorrowedStr<'_>,
    managed: u8,
    params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    let args =
        match parse_book_subscription(instrument_id, book_type, depth, client_id, params_json) {
            Ok(args) => args,
            Err(e) => return PluginResult::Err(e),
        };
    let actor_args = args.clone();
    let strategy_args = args;
    let managed = managed != 0;

    dispatch_actor_action(
        ctx,
        "subscribe_book_deltas",
        |actor| {
            DataActor::subscribe_book_deltas(
                actor,
                actor_args.instrument_id,
                actor_args.book_type,
                actor_args.depth,
                actor_args.client_id,
                managed,
                actor_args.params,
            );
            Ok(())
        },
        |strategy| {
            DataActor::subscribe_book_deltas(
                strategy,
                strategy_args.instrument_id,
                strategy_args.book_type,
                strategy_args.depth,
                strategy_args.client_id,
                managed,
                strategy_args.params,
            );
            Ok(())
        },
    )
}

unsafe extern "C" fn host_unsubscribe_book_deltas(
    ctx: *const HostContext,
    instrument_id: BorrowedStr<'_>,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    let args = match parse_instrument_subscription(instrument_id, client_id, params_json) {
        Ok(args) => args,
        Err(e) => return PluginResult::Err(e),
    };
    let actor_args = args.clone();
    let strategy_args = args;

    dispatch_actor_action(
        ctx,
        "unsubscribe_book_deltas",
        |actor| {
            DataActor::unsubscribe_book_deltas(
                actor,
                actor_args.instrument_id,
                actor_args.client_id,
                actor_args.params,
            );
            Ok(())
        },
        |strategy| {
            DataActor::unsubscribe_book_deltas(
                strategy,
                strategy_args.instrument_id,
                strategy_args.client_id,
                strategy_args.params,
            );
            Ok(())
        },
    )
}

unsafe extern "C" fn host_subscribe_book_at_interval(
    ctx: *const HostContext,
    instrument_id: BorrowedStr<'_>,
    book_type: u8,
    depth: usize,
    interval_ms: usize,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    let args =
        match parse_book_subscription(instrument_id, book_type, depth, client_id, params_json) {
            Ok(args) => args,
            Err(e) => return PluginResult::Err(e),
        };
    let actor_args = args.clone();
    let strategy_args = args;
    let Some(interval_ms) = NonZeroUsize::new(interval_ms) else {
        return PluginResult::Err(PluginError::new(
            PluginErrorCode::InvalidArgument,
            "interval_ms must be greater than zero",
        ));
    };

    dispatch_actor_action(
        ctx,
        "subscribe_book_at_interval",
        |actor| {
            DataActor::subscribe_book_at_interval(
                actor,
                actor_args.instrument_id,
                actor_args.book_type,
                actor_args.depth,
                interval_ms,
                actor_args.client_id,
                actor_args.params,
            );
            Ok(())
        },
        |strategy| {
            DataActor::subscribe_book_at_interval(
                strategy,
                strategy_args.instrument_id,
                strategy_args.book_type,
                strategy_args.depth,
                interval_ms,
                strategy_args.client_id,
                strategy_args.params,
            );
            Ok(())
        },
    )
}

unsafe extern "C" fn host_unsubscribe_book_at_interval(
    ctx: *const HostContext,
    instrument_id: BorrowedStr<'_>,
    interval_ms: usize,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    let args = match parse_instrument_subscription(instrument_id, client_id, params_json) {
        Ok(args) => args,
        Err(e) => return PluginResult::Err(e),
    };
    let actor_args = args.clone();
    let strategy_args = args;
    let Some(interval_ms) = NonZeroUsize::new(interval_ms) else {
        return PluginResult::Err(PluginError::new(
            PluginErrorCode::InvalidArgument,
            "interval_ms must be greater than zero",
        ));
    };

    dispatch_actor_action(
        ctx,
        "unsubscribe_book_at_interval",
        |actor| {
            DataActor::unsubscribe_book_at_interval(
                actor,
                actor_args.instrument_id,
                interval_ms,
                actor_args.client_id,
                actor_args.params,
            );
            Ok(())
        },
        |strategy| {
            DataActor::unsubscribe_book_at_interval(
                strategy,
                strategy_args.instrument_id,
                interval_ms,
                strategy_args.client_id,
                strategy_args.params,
            );
            Ok(())
        },
    )
}

unsafe extern "C" fn host_msgbus_publish(
    ctx: *const HostContext,
    topic: BorrowedStr<'_>,
    payload: Slice<'_, u8>,
) -> PluginResult<()> {
    guard_host_dispatch("msgbus_publish", || {
        let inner = resolve_context(ctx, "msgbus_publish")?;
        ensure_adapter_registered("msgbus_publish", inner)?;

        let topic = borrowed_utf8(topic, "topic")?;
        // SAFETY: payload borrows storage live across this call.
        let payload = unsafe { payload.as_slice() }.to_vec();
        msgbus::publish_any(topic.into(), &payload);
        Ok(())
    })
}

unsafe extern "C" fn host_set_time_alert(
    ctx: *const HostContext,
    name: BorrowedStr<'_>,
    alert_time_ns: u64,
    allow_past: u8,
) -> PluginResult<()> {
    let name = match borrowed_utf8(name, "name") {
        Ok(value) => value.to_string(),
        Err(e) => return PluginResult::Err(e),
    };
    dispatch_actor_action(
        ctx,
        "set_time_alert",
        |actor| {
            actor.clock().set_time_alert_ns(
                &name,
                UnixNanos::from(alert_time_ns),
                None,
                Some(allow_past != 0),
            )
        },
        |strategy| {
            strategy.clock().set_time_alert_ns(
                &name,
                UnixNanos::from(alert_time_ns),
                None,
                Some(allow_past != 0),
            )
        },
    )
}

unsafe extern "C" fn host_set_timer(
    ctx: *const HostContext,
    name: BorrowedStr<'_>,
    interval_ns: u64,
    start_time_ns: u64,
    stop_time_ns: u64,
    allow_past: u8,
    fire_immediately: u8,
) -> PluginResult<()> {
    let name = match borrowed_utf8(name, "name") {
        Ok(value) => value.to_string(),
        Err(e) => return PluginResult::Err(e),
    };
    let start_time_ns = nonzero_unix_nanos(start_time_ns);
    let stop_time_ns = nonzero_unix_nanos(stop_time_ns);

    dispatch_actor_action(
        ctx,
        "set_timer",
        |actor| {
            actor.clock().set_timer_ns(
                &name,
                interval_ns,
                start_time_ns,
                stop_time_ns,
                None,
                Some(allow_past != 0),
                Some(fire_immediately != 0),
            )
        },
        |strategy| {
            strategy.clock().set_timer_ns(
                &name,
                interval_ns,
                start_time_ns,
                stop_time_ns,
                None,
                Some(allow_past != 0),
                Some(fire_immediately != 0),
            )
        },
    )
}

unsafe extern "C" fn host_cancel_timer(
    ctx: *const HostContext,
    name: BorrowedStr<'_>,
) -> PluginResult<()> {
    let name = match borrowed_utf8(name, "name") {
        Ok(value) => value.to_string(),
        Err(e) => return PluginResult::Err(e),
    };
    dispatch_actor_action(
        ctx,
        "cancel_timer",
        |actor| {
            actor.clock().cancel_timer(&name);
            Ok(())
        },
        |strategy| {
            strategy.clock().cancel_timer(&name);
            Ok(())
        },
    )
}

unsafe extern "C" fn host_submit_order(
    ctx: *const HostContext,
    command: *const SubmitOrderHandle,
) -> PluginResult<()> {
    // SAFETY: plug-in keeps the handle alive for the duration of the call.
    unsafe {
        dispatch_handle_plugin_error(ctx, command, "submit_order", |adapter, cmd| {
            validate_order_identity(adapter, &cmd.order)?;
            Strategy::submit_order(
                adapter,
                cmd.order,
                cmd.position_id,
                cmd.client_id,
                cmd.params,
            )
            .map_err(|e| generic_plugin_error(&e))
        })
    }
}

unsafe extern "C" fn host_cancel_order(
    ctx: *const HostContext,
    command: *const CancelOrderHandle,
) -> PluginResult<()> {
    // SAFETY: plug-in keeps the handle alive for the duration of the call.
    unsafe {
        dispatch_handle(ctx, command, "cancel_order", |adapter, cmd| {
            Strategy::cancel_order(adapter, cmd.client_order_id, cmd.client_id, cmd.params)
        })
    }
}

unsafe extern "C" fn host_modify_order(
    ctx: *const HostContext,
    command: *const ModifyOrderHandle,
) -> PluginResult<()> {
    // SAFETY: plug-in keeps the handle alive for the duration of the call.
    unsafe {
        dispatch_handle(ctx, command, "modify_order", |adapter, cmd| {
            Strategy::modify_order(
                adapter,
                cmd.client_order_id,
                cmd.quantity,
                cmd.price,
                cmd.trigger_price,
                cmd.client_id,
                cmd.params,
            )
        })
    }
}

unsafe extern "C" fn host_submit_order_list(
    ctx: *const HostContext,
    command: *const SubmitOrderListHandle,
) -> PluginResult<()> {
    // SAFETY: plug-in keeps the handle alive for the duration of the call.
    unsafe {
        dispatch_handle_plugin_error(ctx, command, "submit_order_list", |adapter, cmd| {
            for order in &cmd.orders {
                validate_order_identity(adapter, order)?;
            }
            Strategy::submit_order_list(
                adapter,
                cmd.orders,
                cmd.position_id,
                cmd.client_id,
                cmd.params,
            )
            .map_err(|e| generic_plugin_error(&e))
        })
    }
}

unsafe extern "C" fn host_cancel_orders(
    ctx: *const HostContext,
    command: *const CancelOrdersHandle,
) -> PluginResult<()> {
    // SAFETY: plug-in keeps the handle alive for the duration of the call.
    unsafe {
        dispatch_handle(ctx, command, "cancel_orders", |adapter, cmd| {
            Strategy::cancel_orders(adapter, cmd.client_order_ids, cmd.client_id, cmd.params)
        })
    }
}

unsafe extern "C" fn host_cancel_all_orders(
    ctx: *const HostContext,
    command: *const CancelAllOrdersHandle,
) -> PluginResult<()> {
    // SAFETY: plug-in keeps the handle alive for the duration of the call.
    unsafe {
        dispatch_handle(ctx, command, "cancel_all_orders", |adapter, cmd| {
            Strategy::cancel_all_orders(
                adapter,
                cmd.instrument_id,
                cmd.order_side,
                cmd.client_id,
                cmd.params,
            )
        })
    }
}

unsafe extern "C" fn host_close_position(
    ctx: *const HostContext,
    command: *const ClosePositionHandle,
) -> PluginResult<()> {
    // SAFETY: plug-in keeps the handle alive for the duration of the call.
    unsafe {
        dispatch_handle(ctx, command, "close_position", |adapter, cmd| {
            let position = {
                let cache = Strategy::core(adapter).cache_ref();
                cache.position(&cmd.position_id).map(|p| p.cloned())
            };
            let position = position.ok_or_else(|| {
                anyhow::anyhow!("position '{}' not found in cache", cmd.position_id)
            })?;
            Strategy::close_position(
                adapter,
                &position,
                cmd.client_id,
                cmd.tags,
                cmd.time_in_force,
                cmd.reduce_only,
                cmd.quote_quantity,
            )
        })
    }
}

unsafe extern "C" fn host_close_all_positions(
    ctx: *const HostContext,
    command: *const CloseAllPositionsHandle,
) -> PluginResult<()> {
    // SAFETY: plug-in keeps the handle alive for the duration of the call.
    unsafe {
        dispatch_handle(ctx, command, "close_all_positions", |adapter, cmd| {
            Strategy::close_all_positions(
                adapter,
                cmd.instrument_id,
                cmd.position_side,
                cmd.client_id,
                cmd.tags,
                cmd.time_in_force,
                cmd.reduce_only,
                cmd.quote_quantity,
            )
        })
    }
}

unsafe extern "C" fn host_query_account(
    ctx: *const HostContext,
    command: *const QueryAccountHandle,
) -> PluginResult<()> {
    // SAFETY: plug-in keeps the handle alive for the duration of the call.
    unsafe {
        dispatch_handle(ctx, command, "query_account", |adapter, cmd| {
            Strategy::query_account(adapter, cmd.account_id, cmd.client_id, cmd.params)
        })
    }
}

unsafe extern "C" fn host_query_order(
    ctx: *const HostContext,
    command: *const QueryOrderHandle,
) -> PluginResult<()> {
    // SAFETY: plug-in keeps the handle alive for the duration of the call.
    unsafe {
        dispatch_handle(ctx, command, "query_order", |adapter, cmd| {
            let order = {
                let cache = Strategy::core(adapter).cache_ref();
                cache.order(&cmd.client_order_id).map(|o| o.cloned())
            };
            let order = order.ok_or_else(|| {
                anyhow::anyhow!("order '{}' not found in cache", cmd.client_order_id)
            })?;
            Strategy::query_order(adapter, &order, cmd.client_id, cmd.params)
        })
    }
}

// Resolves the calling strategy adapter from `ctx` and invokes `f` with a
// borrowed reference to the plug-in-owned handle. Used by the boundary-owned
// command slots (`cancel_order`, `modify_order`, etc.) that take
// `*const XHandle` rather than JSON. The handle stays owned by the plug-in
// for the duration of the call; the host only borrows it.
//
// SAFETY contract for callers: `command` must be a non-null pointer to a
// live handle whose layout matches the host's view of `H`. The loader pins
// `rustc_version` and `nautilus_plugin_version` at load time by default
// (`LoadError::BuildMismatch`); an operator who opts out via
// `PluginLoader::set_allow_build_mismatch` accepts that a plug-in built
// against a mismatched toolchain derefs through this path with whatever
// layout it happened to compile against.
unsafe fn dispatch_handle<H>(
    ctx: *const HostContext,
    command: *const H,
    method: &'static str,
    f: impl FnOnce(&mut PluginStrategyAdapter, H::Command) -> anyhow::Result<()>,
) -> PluginResult<()>
where
    H: BoundaryCommandHandle,
{
    // SAFETY: forwards the caller's handle contract unchanged.
    unsafe {
        dispatch_handle_plugin_error(ctx, command, method, |adapter, command| {
            f(adapter, command).map_err(|e| generic_plugin_error(&e))
        })
    }
}

unsafe fn dispatch_handle_plugin_error<H>(
    ctx: *const HostContext,
    command: *const H,
    method: &'static str,
    f: impl FnOnce(&mut PluginStrategyAdapter, H::Command) -> Result<(), PluginError>,
) -> PluginResult<()>
where
    H: BoundaryCommandHandle,
{
    guard_host_dispatch(method, || {
        if command.is_null() {
            return Err(PluginError::new(
                PluginErrorCode::InvalidArgument,
                format!("{method} called with null command handle"),
            ));
        }

        // SAFETY: caller (the plug-in) round-trips the same ctx the host
        // handed back from `PluginStrategyAdapter::new`.
        let inner = unsafe { host_context_inner(ctx) }.ok_or_else(|| {
            PluginError::new(
                PluginErrorCode::InvalidArgument,
                format!("{method} called with null HostContext"),
            )
        })?;

        if !inner.is_strategy {
            return Err(PluginError::new(
                PluginErrorCode::InvalidArgument,
                format!(
                    "{method} called from a non-strategy plug-in context (actor_id={})",
                    inner.actor_id
                ),
            ));
        }

        let actor_id = inner.actor_id.inner();
        let mut adapter_ref = try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id)
            .ok_or_else(|| resolve_adapter_error(method, inner))?;

        // SAFETY: command is non-null (checked above) and the plug-in commits
        // to keeping the handle live for the duration of this call.
        let handle = unsafe { &*command };
        let command = handle.boundary_normalized_command();
        f(&mut adapter_ref, command)
    })
}

fn validate_order_identity(
    adapter: &PluginStrategyAdapter,
    order: &OrderAny,
) -> Result<(), PluginError> {
    let expected_trader_id = adapter.trader_id().ok_or_else(|| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            "trader_id is unavailable before strategy registration",
        )
    })?;
    let expected_strategy_id = Strategy::core(adapter).strategy_id().ok_or_else(|| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            "strategy_id is unavailable before strategy registration",
        )
    })?;

    if order.trader_id() != expected_trader_id {
        return Err(PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!(
                "order {} trader_id mismatch: expected {}, found {}",
                order.client_order_id(),
                expected_trader_id,
                order.trader_id()
            ),
        ));
    }

    if order.strategy_id() != expected_strategy_id {
        return Err(PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!(
                "order {} strategy_id mismatch: expected {}, found {}",
                order.client_order_id(),
                expected_strategy_id,
                order.strategy_id()
            ),
        ));
    }

    Ok(())
}

fn require_registered_strategy(
    adapter: &PluginStrategyAdapter,
    method: &'static str,
) -> Result<(), PluginError> {
    if adapter.is_registered() {
        Ok(())
    } else {
        Err(PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("{method} requires a registered strategy"),
        ))
    }
}

fn dispatch_actor_action(
    ctx: *const HostContext,
    method: &'static str,
    actor_fn: impl FnOnce(&mut PluginActorAdapter) -> anyhow::Result<()>,
    strategy_fn: impl FnOnce(&mut PluginStrategyAdapter) -> anyhow::Result<()>,
) -> PluginResult<()> {
    guard_host_dispatch(method, || {
        let inner = resolve_context(ctx, method)?;

        let actor_id = inner.actor_id.inner();
        let result = if inner.is_strategy {
            let mut adapter_ref = try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id)
                .ok_or_else(|| resolve_adapter_error(method, inner))?;
            strategy_fn(&mut adapter_ref)
        } else {
            let mut adapter_ref = try_get_actor_unchecked::<PluginActorAdapter>(&actor_id)
                .ok_or_else(|| resolve_adapter_error(method, inner))?;
            actor_fn(&mut adapter_ref)
        };

        result.map_err(|e| PluginError::new(PluginErrorCode::Generic, e.to_string()))
    })
}

fn dispatch_strategy_query<T>(
    ctx: *const HostContext,
    method: &'static str,
    f: impl FnOnce(&mut PluginStrategyAdapter) -> Result<T, PluginError>,
) -> PluginResult<T> {
    guard_host_dispatch(method, || {
        let inner = resolve_context(ctx, method)?;
        if !inner.is_strategy {
            return Err(PluginError::new(
                PluginErrorCode::InvalidArgument,
                format!(
                    "{method} called from a non-strategy plug-in context (actor_id={})",
                    inner.actor_id
                ),
            ));
        }

        let actor_id = inner.actor_id.inner();
        let mut adapter_ref = try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id)
            .ok_or_else(|| resolve_adapter_error(method, inner))?;

        f(&mut adapter_ref)
    })
}

fn dispatch_cache_query(
    ctx: *const HostContext,
    method: &'static str,
    f: impl FnOnce(&Cache, &HostContextInner) -> PluginResult<OwnedBytes>,
) -> PluginResult<OwnedBytes> {
    guard_host_dispatch(method, || {
        let inner = resolve_context(ctx, method)?;

        let actor_id = inner.actor_id.inner();
        if inner.is_strategy {
            let adapter_ref = try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id)
                .ok_or_else(|| resolve_adapter_error(method, inner))?;
            let cache = Strategy::core(&*adapter_ref).cache_ref();
            f(&cache, inner).into_result()
        } else {
            let adapter_ref = try_get_actor_unchecked::<PluginActorAdapter>(&actor_id)
                .ok_or_else(|| resolve_adapter_error(method, inner))?;
            let cache = DataActorNative::cache_ref(&*adapter_ref);
            f(&cache, inner).into_result()
        }
    })
}

// Wraps host-side dispatch in `catch_unwind` so a panic inside engine code
// (command normalization, cache access, msgbus subscribers, order paths)
// surfaces to the calling plug-in as a `Panic` error instead of unwinding
// out of the `extern "C"` thunk, which would abort the process.
fn guard_host_dispatch<T>(
    method: &'static str,
    f: impl FnOnce() -> Result<T, PluginError>,
) -> PluginResult<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => PluginResult::from_result(r),
        Err(payload) => {
            let msg = panic_message(payload.as_ref());
            drop_payload(payload);
            PluginResult::Err(PluginError::new(
                PluginErrorCode::Panic,
                format!("{method} panicked in host dispatch: {msg}"),
            ))
        }
    }
}

fn resolve_context(
    ctx: *const HostContext,
    method: &'static str,
) -> Result<&'static HostContextInner, PluginError> {
    // SAFETY: plug-ins round-trip the context pointer supplied at create time.
    unsafe { host_context_inner(ctx) }.ok_or_else(|| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("{method} called with null HostContext"),
        )
    })
}

fn resolve_adapter_error(method: &str, inner: &HostContextInner) -> PluginError {
    let kind = if inner.is_strategy {
        "strategy"
    } else {
        "actor"
    };
    PluginError::new(
        PluginErrorCode::Generic,
        format!(
            "{method} could not resolve {kind} adapter for actor_id={}",
            inner.actor_id
        ),
    )
}

fn generic_plugin_error(error: &anyhow::Error) -> PluginError {
    PluginError::new(PluginErrorCode::Generic, error.to_string())
}

fn ensure_adapter_registered(method: &str, inner: &HostContextInner) -> Result<(), PluginError> {
    let actor_id = inner.actor_id.inner();
    let found = if inner.is_strategy {
        try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id).is_some()
    } else {
        try_get_actor_unchecked::<PluginActorAdapter>(&actor_id).is_some()
    };

    if found {
        Ok(())
    } else {
        Err(resolve_adapter_error(method, inner))
    }
}

fn json_optional<T>(value: Option<&T>) -> PluginResult<OwnedBytes>
where
    T: Serialize,
{
    match value {
        Some(value) => json_bytes(value),
        None => PluginResult::Ok(OwnedBytes::empty()),
    }
}

fn json_bytes<T>(value: &T) -> PluginResult<OwnedBytes>
where
    T: Serialize,
{
    match serde_json::to_vec(value) {
        Ok(bytes) => PluginResult::Ok(OwnedBytes::from_vec(bytes)),
        Err(e) => PluginResult::Err(PluginError::new(
            PluginErrorCode::SerializationFailed,
            e.to_string(),
        )),
    }
}

#[derive(Clone)]
struct InstrumentSubscriptionArgs {
    instrument_id: InstrumentId,
    client_id: Option<ClientId>,
    params: Option<Params>,
}

#[derive(Clone)]
struct BarSubscriptionArgs {
    bar_type: BarType,
    client_id: Option<ClientId>,
    params: Option<Params>,
}

#[derive(Clone)]
struct BookSubscriptionArgs {
    instrument_id: InstrumentId,
    book_type: BookType,
    depth: Option<NonZeroUsize>,
    client_id: Option<ClientId>,
    params: Option<Params>,
}

fn parse_instrument_subscription(
    instrument_id: BorrowedStr<'_>,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> Result<InstrumentSubscriptionArgs, PluginError> {
    Ok(InstrumentSubscriptionArgs {
        instrument_id: parse_instrument_id(instrument_id, "instrument_id")?,
        client_id: parse_optional_client_id(client_id)?,
        params: parse_optional_params(params_json)?,
    })
}

fn parse_bar_subscription(
    bar_type: BorrowedStr<'_>,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> Result<BarSubscriptionArgs, PluginError> {
    let raw = borrowed_utf8(bar_type, "bar_type")?;
    let bar_type = BarType::from_str(raw).map_err(|e| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("invalid bar_type '{raw}': {e}"),
        )
    })?;
    Ok(BarSubscriptionArgs {
        bar_type,
        client_id: parse_optional_client_id(client_id)?,
        params: parse_optional_params(params_json)?,
    })
}

fn parse_book_subscription(
    instrument_id: BorrowedStr<'_>,
    book_type: u8,
    depth: usize,
    client_id: BorrowedStr<'_>,
    params_json: BorrowedStr<'_>,
) -> Result<BookSubscriptionArgs, PluginError> {
    let book_type = BookType::from_u8(book_type).ok_or_else(|| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("invalid book_type discriminant {book_type}"),
        )
    })?;
    Ok(BookSubscriptionArgs {
        instrument_id: parse_instrument_id(instrument_id, "instrument_id")?,
        book_type,
        depth: NonZeroUsize::new(depth),
        client_id: parse_optional_client_id(client_id)?,
        params: parse_optional_params(params_json)?,
    })
}

fn parse_instrument_id(
    value: BorrowedStr<'_>,
    label: &'static str,
) -> Result<InstrumentId, PluginError> {
    let raw = borrowed_utf8(value, label)?;
    InstrumentId::from_str(raw).map_err(|e| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("invalid {label} '{raw}': {e}"),
        )
    })
}

fn parse_account_id(value: BorrowedStr<'_>, label: &'static str) -> Result<AccountId, PluginError> {
    let raw = borrowed_utf8(value, label)?;
    AccountId::new_checked(raw).map_err(|e| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("invalid {label} '{raw}': {e}"),
        )
    })
}

fn parse_client_order_id(
    value: BorrowedStr<'_>,
    label: &'static str,
) -> Result<ClientOrderId, PluginError> {
    let raw = borrowed_utf8(value, label)?;
    ClientOrderId::new_checked(raw).map_err(|e| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("invalid {label} '{raw}': {e}"),
        )
    })
}

fn parse_position_id(
    value: BorrowedStr<'_>,
    label: &'static str,
) -> Result<PositionId, PluginError> {
    let raw = borrowed_utf8(value, label)?;
    PositionId::new_checked(raw).map_err(|e| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("invalid {label} '{raw}': {e}"),
        )
    })
}

fn parse_strategy_id_for_context(
    value: BorrowedStr<'_>,
    inner: &HostContextInner,
) -> Result<StrategyId, PluginError> {
    let raw = borrowed_utf8(value, "strategy_id")?;
    if !raw.is_empty() {
        return StrategyId::new_checked(raw).map_err(|e| {
            PluginError::new(
                PluginErrorCode::InvalidArgument,
                format!("invalid strategy_id '{raw}': {e}"),
            )
        });
    }

    if !inner.is_strategy {
        return Err(PluginError::new(
            PluginErrorCode::InvalidArgument,
            "empty strategy_id is only valid for strategy plug-in contexts",
        ));
    }

    StrategyId::new_checked(inner.actor_id.inner().as_str()).map_err(|e| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("invalid calling strategy_id '{}': {e}", inner.actor_id),
        )
    })
}

fn parse_optional_client_id(value: BorrowedStr<'_>) -> Result<Option<ClientId>, PluginError> {
    let raw = borrowed_utf8(value, "client_id")?;
    if raw.is_empty() {
        return Ok(None);
    }
    ClientId::new_checked(raw)
        .map(Some)
        .map_err(|e| PluginError::new(PluginErrorCode::InvalidArgument, e.to_string()))
}

fn parse_optional_params(value: BorrowedStr<'_>) -> Result<Option<Params>, PluginError> {
    let raw = borrowed_utf8(value, "params_json")?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(raw).map(Some).map_err(|e| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("invalid params_json: {e}"),
        )
    })
}

// Validates UTF-8 on plug-in-supplied strings. The plug-in side commits to
// UTF-8, but the host verifies at the trust boundary instead of assuming;
// `BorrowedStr::as_str` would be library UB on a violating producer.
fn borrowed_utf8<'a>(value: BorrowedStr<'a>, label: &'static str) -> Result<&'a str, PluginError> {
    // SAFETY: value borrows storage live across this call.
    unsafe { value.try_as_str() }.map_err(|e| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("invalid {label}: not valid UTF-8: {e}"),
        )
    })
}

fn nonzero_unix_nanos(value: u64) -> Option<UnixNanos> {
    (value != 0).then_some(UnixNanos::from(value))
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, TimeInForce},
        identifiers::{
            ClientOrderId as TestClientOrderId, InstrumentId as TestInstrumentId, StrategyId,
            TraderId,
        },
        orders::{MarketOrder, OrderAny},
        types::Quantity,
    };
    use rstest::rstest;

    use super::*;
    use crate::surfaces::commands::{CancelOrderCommand, ModifyOrderCommand, SubmitOrderCommand};

    fn make_market_submit_command() -> SubmitOrderCommand {
        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            TestInstrumentId::from("ETH-USDT.BINANCE"),
            TestClientOrderId::from("O-1"),
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
        ));
        SubmitOrderCommand::new(order, None, None, None)
    }

    #[rstest]
    fn host_vtable_carries_compiled_abi() {
        let p = host_vtable();
        assert!(!p.is_null());
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        assert_eq!(v.abi_version, NAUTILUS_PLUGIN_ABI_VERSION);
    }

    #[rstest]
    fn host_vtable_binds_live_node_callbacks() {
        // Locks in that the live-node host vtable installs the routing thunks
        // defined in this module, not the loader.rs NotImplemented stubs.
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        assert_eq!(
            v.cache_order as *const () as usize,
            host_cache_order as *const () as usize,
        );
        assert_eq!(
            v.subscribe_quotes as *const () as usize,
            host_subscribe_quotes as *const () as usize,
        );
        assert_eq!(
            v.msgbus_publish as *const () as usize,
            host_msgbus_publish as *const () as usize,
        );
        assert_eq!(
            v.set_timer as *const () as usize,
            host_set_timer as *const () as usize,
        );
        assert_eq!(
            v.submit_order as *const () as usize,
            host_submit_order as *const () as usize,
        );
        assert_eq!(
            v.cancel_order as *const () as usize,
            host_cancel_order as *const () as usize,
        );
        assert_eq!(
            v.modify_order as *const () as usize,
            host_modify_order as *const () as usize,
        );
        assert_eq!(
            v.trader_id as *const () as usize,
            host_trader_id as *const () as usize,
        );
        assert_eq!(
            v.strategy_id as *const () as usize,
            host_strategy_id as *const () as usize,
        );
        assert_eq!(
            v.component_state as *const () as usize,
            host_component_state as *const () as usize,
        );
        assert_eq!(
            v.generate_client_order_id as *const () as usize,
            host_generate_client_order_id as *const () as usize,
        );
        assert_eq!(
            v.generate_order_list_id as *const () as usize,
            host_generate_order_list_id as *const () as usize,
        );
        assert_eq!(
            v.clock_now_ns as *const () as usize,
            host_clock_now_ns as *const () as usize,
        );
        assert_eq!(v.log as *const () as usize, host_log as *const () as usize);
    }

    #[rstest]
    fn host_clock_now_ns_returns_unix_nanos_after_2020() {
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        // SAFETY: fn pointer is non-null; clock_now_ns dereferences no input.
        let now = unsafe { (v.clock_now_ns)() };
        // Any time after 2020-01-01 in UNIX nanoseconds.
        assert!(now > 1_577_836_800_000_000_000u64);
    }

    #[rstest]
    fn guard_host_dispatch_maps_panic_to_error() {
        let r: PluginResult<()> =
            guard_host_dispatch("test_method", || panic!("engine panic message"));
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::Panic);
        let message = err.message_string();
        assert!(
            message.contains("test_method") && message.contains("engine panic message"),
            "expected method and panic payload in message, was: {message}",
        );
    }

    #[rstest]
    fn guard_host_dispatch_passes_through_ok_and_err() {
        let r = guard_host_dispatch("test_method", || Ok(7u32));
        assert_eq!(r.into_result().unwrap(), 7);

        let r: PluginResult<u32> = guard_host_dispatch("test_method", || {
            Err(PluginError::new(PluginErrorCode::InvalidArgument, "bad"))
        });
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
    }

    #[rstest]
    fn borrowed_utf8_rejects_invalid_bytes() {
        static INVALID: [u8; 2] = [0xff, 0xfe];
        let mut value = BorrowedStr::empty();
        value.ptr = INVALID.as_ptr();
        value.len = INVALID.len();
        let err = borrowed_utf8(value, "topic").unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
        assert!(err.message_string().contains("topic"));
    }

    #[rstest]
    fn host_submit_order_rejects_null_ctx() {
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        let handle = SubmitOrderHandle::new(make_market_submit_command());
        // SAFETY: passes null ctx; handle is live.
        let r = unsafe { (v.submit_order)(std::ptr::null(), &raw const handle) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
        assert!(err.message_string().contains("null HostContext"));
    }

    #[rstest]
    fn host_cancel_order_rejects_null_ctx() {
        use nautilus_model::identifiers::ClientOrderId;

        let p = host_vtable();
        // SAFETY: see above.
        let v = unsafe { &*p };
        let handle = CancelOrderHandle::new(CancelOrderCommand::new(
            ClientOrderId::from("O-1"),
            None,
            None,
        ));
        // SAFETY: passes null ctx; handle is live.
        let r = unsafe { (v.cancel_order)(std::ptr::null(), &raw const handle) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
    }

    #[rstest]
    fn host_modify_order_rejects_null_ctx() {
        use nautilus_model::identifiers::ClientOrderId;

        let p = host_vtable();
        // SAFETY: see above.
        let v = unsafe { &*p };
        let handle = ModifyOrderHandle::new(ModifyOrderCommand::new(
            ClientOrderId::from("O-1"),
            None,
            None,
            None,
            None,
            None,
        ));
        // SAFETY: passes null ctx; handle is live.
        let r = unsafe { (v.modify_order)(std::ptr::null(), &raw const handle) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
    }

    #[rstest]
    fn host_cancel_order_rejects_null_command() {
        let p = host_vtable();
        // SAFETY: see above.
        let v = unsafe { &*p };
        // SAFETY: passes null command handle; ctx irrelevant.
        let r = unsafe { (v.cancel_order)(std::ptr::null(), std::ptr::null()) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
        assert!(err.message_string().contains("null command handle"));
    }

    #[rstest]
    fn host_modify_order_rejects_null_command() {
        let p = host_vtable();
        // SAFETY: see above.
        let v = unsafe { &*p };
        // SAFETY: passes null command handle; ctx irrelevant.
        let r = unsafe { (v.modify_order)(std::ptr::null(), std::ptr::null()) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
        assert!(err.message_string().contains("null command handle"));
    }

    #[rstest]
    fn host_submit_order_rejects_non_strategy_context() {
        // Plug-in actors must not submit orders. The host vtable thunk
        // inspects HostContextInner::is_strategy and rejects calls from
        // actor contexts with InvalidArgument.
        use nautilus_model::identifiers::ActorId;

        use crate::bridge::registry::{
            HostContextInner, drop_host_context, host_context_test_lock, leak_host_context,
        };

        let _guard = host_context_test_lock();
        let ctx = leak_host_context(HostContextInner {
            actor_id: ActorId::from("ActorContextProbe"),
            is_strategy: false,
        });
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        let handle = SubmitOrderHandle::new(make_market_submit_command());
        // SAFETY: ctx was leaked above and is live; handle outlives the call.
        let r = unsafe { (v.submit_order)(ctx, &raw const handle) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
        assert!(
            err.message_string().contains("non-strategy"),
            "expected non-strategy rejection, was: {}",
            err.message_string(),
        );
        // SAFETY: ctx came from leak_host_context above.
        unsafe { drop_host_context(ctx) };
    }

    #[rstest]
    fn host_submit_order_rejects_unregistered_actor_id() {
        // ctx points to a strategy actor_id that no PluginStrategyAdapter
        // has registered into the thread-local actor registry, so the
        // host vtable's try_get_actor_unchecked lookup returns None.
        use nautilus_model::identifiers::ActorId;

        use crate::bridge::registry::{
            HostContextInner, drop_host_context, host_context_test_lock, leak_host_context,
        };

        let _guard = host_context_test_lock();
        let ctx = leak_host_context(HostContextInner {
            actor_id: ActorId::from("UnregisteredStrategyAdapter"),
            is_strategy: true,
        });
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        let handle = SubmitOrderHandle::new(make_market_submit_command());
        // SAFETY: ctx was leaked above and is live; handle outlives the call.
        let r = unsafe { (v.submit_order)(ctx, &raw const handle) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::Generic);
        assert!(
            err.message_string().contains("could not resolve"),
            "expected unresolved-adapter rejection, was: {}",
            err.message_string(),
        );
        // SAFETY: ctx came from leak_host_context above.
        unsafe { drop_host_context(ctx) };
    }
}
