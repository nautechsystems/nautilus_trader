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

use std::{num::NonZeroUsize, str::FromStr, sync::OnceLock};

use nautilus_common::{
    actor::{DataActor, registry::try_get_actor_unchecked},
    cache::Cache,
    msgbus,
};
use nautilus_core::{Params, UnixNanos, time::duration_since_unix_epoch};
use nautilus_model::{
    data::BarType,
    enums::{BookType, FromU8},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId},
};
use nautilus_plugin::{
    NAUTILUS_PLUGIN_ABI_VERSION,
    boundary::{BorrowedStr, OwnedBytes, PluginError, PluginErrorCode, PluginResult, Slice},
    host::{HostContext, HostLogLevel, HostVTable},
    loader::PluginLoader,
};
use nautilus_trading::strategy::Strategy;
use serde::Serialize;

use crate::plugin::{
    actor::PluginActorAdapter,
    commands::{CancelOrderCommand, ModifyOrderCommand, SubmitOrderCommand},
    registry::{HostContextInner, host_context_inner},
    strategy::PluginStrategyAdapter,
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

unsafe extern "C" fn host_log(
    level: HostLogLevel,
    target: BorrowedStr<'_>,
    message: BorrowedStr<'_>,
) {
    // SAFETY: producer holds the storage live across the call.
    let target = unsafe { target.as_str() };
    // SAFETY: see above.
    let message = unsafe { message.as_str() };
    match level {
        HostLogLevel::Error => log::error!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Warn => log::warn!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Info => log::info!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Debug => log::debug!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Trace => log::trace!(target: "nautilus_plugin", "[{target}] {message}"),
    }
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
    let interval_ms = match NonZeroUsize::new(interval_ms) {
        Some(value) => value,
        None => {
            return PluginResult::Err(PluginError::new(
                PluginErrorCode::InvalidArgument,
                "interval_ms must be greater than zero",
            ));
        }
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
    let interval_ms = match NonZeroUsize::new(interval_ms) {
        Some(value) => value,
        None => {
            return PluginResult::Err(PluginError::new(
                PluginErrorCode::InvalidArgument,
                "interval_ms must be greater than zero",
            ));
        }
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
    let inner = match resolve_context(ctx, "msgbus_publish") {
        Ok(inner) => inner,
        Err(e) => return PluginResult::Err(e),
    };

    if let Err(e) = ensure_adapter_registered("msgbus_publish", inner) {
        return PluginResult::Err(e);
    }

    // SAFETY: topic and payload borrow storage live across this call.
    let topic = unsafe { topic.as_str() };
    // SAFETY: see above.
    let payload = unsafe { payload.as_slice() }.to_vec();
    msgbus::publish_any(topic.into(), &payload);
    PluginResult::Ok(())
}

unsafe extern "C" fn host_set_time_alert(
    ctx: *const HostContext,
    name: BorrowedStr<'_>,
    alert_time_ns: u64,
    allow_past: u8,
) -> PluginResult<()> {
    // SAFETY: name borrows storage live across this call.
    let name = unsafe { name.as_str() }.to_string();
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
    // SAFETY: name borrows storage live across this call.
    let name = unsafe { name.as_str() }.to_string();
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
    // SAFETY: name borrows storage live across this call.
    let name = unsafe { name.as_str() }.to_string();
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
    command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    dispatch_command(ctx, command_json, "submit_order", |adapter, json| {
        let cmd: SubmitOrderCommand = serde_json::from_str(json)?;
        Strategy::submit_order(
            adapter,
            cmd.order,
            cmd.position_id,
            cmd.client_id,
            cmd.params,
        )
    })
}

unsafe extern "C" fn host_cancel_order(
    ctx: *const HostContext,
    command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    dispatch_command(ctx, command_json, "cancel_order", |adapter, json| {
        let cmd: CancelOrderCommand = serde_json::from_str(json)?;
        Strategy::cancel_order(adapter, cmd.client_order_id, cmd.client_id, cmd.params)
    })
}

unsafe extern "C" fn host_modify_order(
    ctx: *const HostContext,
    command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    dispatch_command(ctx, command_json, "modify_order", |adapter, json| {
        let cmd: ModifyOrderCommand = serde_json::from_str(json)?;
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

fn dispatch_command(
    ctx: *const HostContext,
    command_json: BorrowedStr<'_>,
    method: &'static str,
    f: impl FnOnce(&mut PluginStrategyAdapter, &str) -> anyhow::Result<()>,
) -> PluginResult<()> {
    // SAFETY: command_json borrows storage that is live across the call.
    let json = unsafe { command_json.as_str() };

    // SAFETY: caller (the plug-in) round-trips the same ctx the host handed
    // back from `PluginStrategyAdapter::new`.
    let inner = match unsafe { host_context_inner(ctx) } {
        Some(inner) => inner,
        None => {
            return PluginResult::Err(PluginError::new(
                PluginErrorCode::InvalidArgument,
                format!("{method} called with null HostContext"),
            ));
        }
    };

    if !inner.is_strategy {
        return PluginResult::Err(PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!(
                "{method} called from a non-strategy plug-in context (actor_id={})",
                inner.actor_id
            ),
        ));
    }

    let actor_id = inner.actor_id.inner();
    let Some(mut adapter_ref) = try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id) else {
        return PluginResult::Err(PluginError::new(
            PluginErrorCode::Generic,
            format!(
                "{method} could not resolve strategy adapter for actor_id={}",
                inner.actor_id
            ),
        ));
    };

    match f(&mut adapter_ref, json) {
        Ok(()) => PluginResult::Ok(()),
        Err(e) => PluginResult::Err(PluginError::new(PluginErrorCode::Generic, e.to_string())),
    }
}

fn dispatch_actor_action(
    ctx: *const HostContext,
    method: &'static str,
    actor_fn: impl FnOnce(&mut PluginActorAdapter) -> anyhow::Result<()>,
    strategy_fn: impl FnOnce(&mut PluginStrategyAdapter) -> anyhow::Result<()>,
) -> PluginResult<()> {
    let inner = match resolve_context(ctx, method) {
        Ok(inner) => inner,
        Err(e) => return PluginResult::Err(e),
    };

    let actor_id = inner.actor_id.inner();
    let result = if inner.is_strategy {
        let Some(mut adapter_ref) = try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id)
        else {
            return PluginResult::Err(resolve_adapter_error(method, inner));
        };
        strategy_fn(&mut adapter_ref)
    } else {
        let Some(mut adapter_ref) = try_get_actor_unchecked::<PluginActorAdapter>(&actor_id) else {
            return PluginResult::Err(resolve_adapter_error(method, inner));
        };
        actor_fn(&mut adapter_ref)
    };

    match result {
        Ok(()) => PluginResult::Ok(()),
        Err(e) => PluginResult::Err(PluginError::new(PluginErrorCode::Generic, e.to_string())),
    }
}

fn dispatch_cache_query(
    ctx: *const HostContext,
    method: &'static str,
    f: impl FnOnce(&Cache, &HostContextInner) -> PluginResult<OwnedBytes>,
) -> PluginResult<OwnedBytes> {
    let inner = match resolve_context(ctx, method) {
        Ok(inner) => inner,
        Err(e) => return PluginResult::Err(e),
    };

    let actor_id = inner.actor_id.inner();
    if inner.is_strategy {
        let Some(adapter_ref) = try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id) else {
            return PluginResult::Err(resolve_adapter_error(method, inner));
        };
        let cache = adapter_ref.cache();
        f(&cache, inner)
    } else {
        let Some(adapter_ref) = try_get_actor_unchecked::<PluginActorAdapter>(&actor_id) else {
            return PluginResult::Err(resolve_adapter_error(method, inner));
        };
        let cache = adapter_ref.cache();
        f(&cache, inner)
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
    // SAFETY: bar_type borrows storage live across this call.
    let raw = unsafe { bar_type.as_str() };
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
    // SAFETY: value borrows storage live across this call.
    let raw = unsafe { value.as_str() };
    InstrumentId::from_str(raw).map_err(|e| {
        PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!("invalid {label} '{raw}': {e}"),
        )
    })
}

fn parse_account_id(value: BorrowedStr<'_>, label: &'static str) -> Result<AccountId, PluginError> {
    // SAFETY: value borrows storage live across this call.
    let raw = unsafe { value.as_str() };
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
    // SAFETY: value borrows storage live across this call.
    let raw = unsafe { value.as_str() };
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
    // SAFETY: value borrows storage live across this call.
    let raw = unsafe { value.as_str() };
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
    // SAFETY: value borrows storage live across this call.
    let raw = unsafe { value.as_str() };
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
    // SAFETY: value borrows storage live across this call.
    let raw = unsafe { value.as_str() };
    if raw.is_empty() {
        return Ok(None);
    }
    ClientId::new_checked(raw)
        .map(Some)
        .map_err(|e| PluginError::new(PluginErrorCode::InvalidArgument, e.to_string()))
}

fn parse_optional_params(value: BorrowedStr<'_>) -> Result<Option<Params>, PluginError> {
    // SAFETY: value borrows storage live across this call.
    let raw = unsafe { value.as_str() };
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

fn nonzero_unix_nanos(value: u64) -> Option<UnixNanos> {
    (value != 0).then_some(UnixNanos::from(value))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

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
    fn host_submit_order_rejects_null_ctx() {
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        let payload = BorrowedStr::from_str("{}");
        // SAFETY: passes null ctx; the thunk handles it.
        let r = unsafe { (v.submit_order)(std::ptr::null(), payload) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
        assert!(err.message_string().contains("null HostContext"));
    }

    #[rstest]
    fn host_cancel_order_rejects_null_ctx() {
        let p = host_vtable();
        // SAFETY: see above.
        let v = unsafe { &*p };
        let payload = BorrowedStr::from_str("{}");
        // SAFETY: passes null ctx.
        let r = unsafe { (v.cancel_order)(std::ptr::null(), payload) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
    }

    #[rstest]
    fn host_modify_order_rejects_null_ctx() {
        let p = host_vtable();
        // SAFETY: see above.
        let v = unsafe { &*p };
        let payload = BorrowedStr::from_str("{}");
        // SAFETY: passes null ctx.
        let r = unsafe { (v.modify_order)(std::ptr::null(), payload) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
    }

    #[rstest]
    fn host_submit_order_rejects_non_strategy_context() {
        // Plug-in actors must not submit orders. The host vtable thunk
        // inspects HostContextInner::is_strategy and rejects calls from
        // actor contexts with InvalidArgument.
        use nautilus_model::identifiers::ActorId;

        use crate::plugin::registry::{
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
        let payload = BorrowedStr::from_str("{}");
        // SAFETY: ctx was leaked above and is live.
        let r = unsafe { (v.submit_order)(ctx, payload) };
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

        use crate::plugin::registry::{
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
        let payload = BorrowedStr::from_str("{}");
        // SAFETY: ctx was leaked above and is live.
        let r = unsafe { (v.submit_order)(ctx, payload) };
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
