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

//! Public API functions for interacting with the message bus.
//!
//! This module provides free-standing functions that wrap the thread-local
//! message bus, providing a convenient API for:
//!
//! - Registering endpoint handlers (point-to-point messaging).
//! - Subscribing to topics (pub/sub messaging).
//! - Publishing messages to subscribers.
//! - Sending messages to endpoints.

use std::{
    any::Any,
    cell::{Cell, RefCell},
    thread::LocalKey,
};

use anyhow::Context;
use bytes::Bytes;
use nautilus_core::UUID4;
#[cfg(feature = "defi")]
use nautilus_model::defi::{
    Block, DefiData, Pool, PoolFeeCollect, PoolFlash, PoolLiquidityUpdate, PoolSwap,
};
use nautilus_model::{
    data::{
        Bar, CustomData, Data, FundingRateUpdate, GreeksData, IndexPriceUpdate, MarkPriceUpdate,
        OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick,
        option_chain::{OptionChainSlice, OptionGreeks},
    },
    events::{AccountState, OrderEventAny, PortfolioSnapshot, PositionEvent},
    instruments::InstrumentAny,
    orderbook::OrderBook,
    orders::OrderAny,
    position::Position,
};
#[cfg(feature = "sbe")]
use nautilus_serialization::sbe::{FromSbe, ToSbe};
#[cfg(feature = "capnp")]
use nautilus_serialization::{
    capnp::{FromCapnp, ToCapnp},
    market_capnp,
};
use smallvec::SmallVec;
use ustr::Ustr;

use super::{
    ACCOUNT_STATE_HANDLERS, ANY_HANDLERS, BAR_HANDLERS, BOOK_HANDLERS, BusMessage, BusPayloadType,
    DELTAS_HANDLERS, DEPTH10_HANDLERS, FUNDING_RATE_HANDLERS, GREEKS_HANDLERS, HANDLER_BUFFER_CAP,
    HAS_PUBLISHER, INDEX_PRICE_HANDLERS, INSTRUMENT_HANDLERS, MARK_PRICE_HANDLERS,
    OPTION_CHAIN_HANDLERS, OPTION_GREEKS_HANDLERS, ORDER_EVENT_HANDLERS,
    PORTFOLIO_SNAPSHOT_HANDLERS, POSITION_EVENT_HANDLERS, QUOTE_HANDLERS, SUPPRESS_EXTERNAL_DEPTH,
    SuppressExternalGuard, TRADE_HANDLERS,
    core::{MessageBus, Subscription},
    dispatch_tap_publish, dispatch_tap_response, dispatch_tap_send, get_message_bus,
    matching::is_matching_backtracking,
    mstr::{Endpoint, MStr, Pattern, Topic},
    try_get_message_bus,
    typed_handler::{ShareableMessageHandler, TypedHandler, TypedIntoHandler},
};
#[cfg(feature = "defi")]
use super::{
    DEFI_BLOCK_HANDLERS, DEFI_COLLECT_HANDLERS, DEFI_FLASH_HANDLERS, DEFI_LIQUIDITY_HANDLERS,
    DEFI_POOL_HANDLERS, DEFI_SWAP_HANDLERS,
};
use crate::{
    enums::SerializationEncoding,
    messages::{
        data::{DataCommand, DataResponse},
        execution::{ExecutionReport, TradingCommand},
    },
};

/// Registers a handler for an endpoint using runtime type dispatch (Any).
pub fn register_any(endpoint: MStr<Endpoint>, handler: ShareableMessageHandler) {
    log::debug!(
        "Registering endpoint '{endpoint}' with handler ID {}",
        handler.0.id(),
    );
    get_message_bus()
        .borrow_mut()
        .endpoints
        .insert(endpoint, handler);
}

/// Registers a response handler for a correlation ID.
pub fn register_response_handler(correlation_id: &UUID4, handler: ShareableMessageHandler) {
    if let Err(e) = get_message_bus()
        .borrow_mut()
        .register_response_handler(correlation_id, handler)
    {
        log::error!("Failed to register request handler: {e}");
    }
}

/// Registers a quote tick handler at an endpoint.
pub fn register_quote_endpoint(endpoint: MStr<Endpoint>, handler: TypedHandler<QuoteTick>) {
    get_message_bus()
        .borrow_mut()
        .endpoints_quotes
        .register(endpoint, handler);
}

/// Returns whether a quote tick handler is registered for the given endpoint.
#[must_use]
pub fn has_quote_endpoint(endpoint: MStr<Endpoint>) -> bool {
    get_message_bus()
        .borrow()
        .endpoints_quotes
        .is_registered(endpoint)
}

/// Registers a trade tick handler at an endpoint.
pub fn register_trade_endpoint(endpoint: MStr<Endpoint>, handler: TypedHandler<TradeTick>) {
    get_message_bus()
        .borrow_mut()
        .endpoints_trades
        .register(endpoint, handler);
}

/// Registers a bar handler at an endpoint.
pub fn register_bar_endpoint(endpoint: MStr<Endpoint>, handler: TypedHandler<Bar>) {
    get_message_bus()
        .borrow_mut()
        .endpoints_bars
        .register(endpoint, handler);
}

/// Registers an order event handler at an endpoint (ownership-based).
pub fn register_order_event_endpoint(
    endpoint: MStr<Endpoint>,
    handler: TypedIntoHandler<OrderEventAny>,
) {
    get_message_bus()
        .borrow_mut()
        .endpoints_order_events
        .register(endpoint, handler);
}

/// Registers an account state handler at an endpoint.
pub fn register_account_state_endpoint(
    endpoint: MStr<Endpoint>,
    handler: TypedHandler<AccountState>,
) {
    get_message_bus()
        .borrow_mut()
        .endpoints_account_state
        .register(endpoint, handler);
}

/// Registers a trading command handler at an endpoint (ownership-based).
pub fn register_trading_command_endpoint(
    endpoint: MStr<Endpoint>,
    handler: TypedIntoHandler<TradingCommand>,
) {
    get_message_bus()
        .borrow_mut()
        .endpoints_trading_commands
        .register(endpoint, handler);
}

/// Registers a data command handler at an endpoint (ownership-based).
pub fn register_data_command_endpoint(
    endpoint: MStr<Endpoint>,
    handler: TypedIntoHandler<DataCommand>,
) {
    get_message_bus()
        .borrow_mut()
        .endpoints_data_commands
        .register(endpoint, handler);
}

/// Registers a data response handler at an endpoint (ownership-based).
pub fn register_data_response_endpoint(
    endpoint: MStr<Endpoint>,
    handler: TypedIntoHandler<DataResponse>,
) {
    get_message_bus()
        .borrow_mut()
        .endpoints_data_responses
        .register(endpoint, handler);
}

/// Registers an execution report handler at an endpoint (ownership-based).
pub fn register_execution_report_endpoint(
    endpoint: MStr<Endpoint>,
    handler: TypedIntoHandler<ExecutionReport>,
) {
    get_message_bus()
        .borrow_mut()
        .endpoints_exec_reports
        .register(endpoint, handler);
}

/// Registers a data handler at an endpoint (ownership-based).
pub fn register_data_endpoint(endpoint: MStr<Endpoint>, handler: TypedIntoHandler<Data>) {
    get_message_bus()
        .borrow_mut()
        .endpoints_data
        .register(endpoint, handler);
}

/// Registers a DeFi data handler at an endpoint (ownership-based).
#[cfg(feature = "defi")]
pub fn register_defi_data_endpoint(endpoint: MStr<Endpoint>, handler: TypedIntoHandler<DefiData>) {
    get_message_bus()
        .borrow_mut()
        .endpoints_defi_data
        .register(endpoint, handler);
}

/// Deregisters the handler for an endpoint (Any-based).
pub fn deregister_any(endpoint: MStr<Endpoint>) {
    log::debug!("Deregistering endpoint '{endpoint}'");
    get_message_bus()
        .borrow_mut()
        .endpoints
        .shift_remove(&endpoint);
}

/// Returns whether an endpoint handler is registered for the given endpoint name.
#[must_use]
pub fn has_endpoint(endpoint: &str) -> bool {
    let key: MStr<Endpoint> = Ustr::from(endpoint).into();
    get_message_bus().borrow().get_endpoint(key).is_some()
}

/// Subscribes a handler to a pattern using runtime type dispatch (Any).
///
/// # Warnings
///
/// Assigning priority handling is an advanced feature which *shouldn't
/// normally be needed by most users*. **Only assign a higher priority to the
/// subscription if you are certain of what you're doing**. If an inappropriate
/// priority is assigned then the handler may receive messages before core
/// system components have been able to process necessary calculations and
/// produce potential side effects for logically sound behavior.
pub fn subscribe_any(
    pattern: MStr<Pattern>,
    handler: ShareableMessageHandler,
    priority: Option<u32>,
) {
    let msgbus = get_message_bus();
    let mut msgbus_ref_mut = msgbus.borrow_mut();
    let sub = Subscription::new(pattern, handler, priority);

    log::debug!(
        "Subscribing {:?} for pattern '{}'",
        sub.handler,
        sub.pattern
    );

    if msgbus_ref_mut.subscriptions.contains(&sub) {
        log::warn!("{sub:?} already exists");
        return;
    }

    for (topic, subs) in &mut msgbus_ref_mut.topics {
        if is_matching_backtracking(*topic, sub.pattern) {
            subs.push(sub.clone());
            subs.sort();
            log::debug!("Added subscription for '{topic}'");
        }
    }

    msgbus_ref_mut.subscriptions.insert(sub);
}

/// Subscribes a handler to instrument messages matching a pattern.
pub fn subscribe_instruments(
    pattern: MStr<Pattern>,
    handler: TypedHandler<InstrumentAny>,
    priority: Option<u32>,
) {
    get_message_bus().borrow_mut().router_instruments.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

/// Subscribes a handler to instrument close messages matching a pattern.
pub fn subscribe_instrument_close(
    pattern: MStr<Pattern>,
    handler: ShareableMessageHandler,
    priority: Option<u32>,
) {
    subscribe_any(pattern, handler, priority);
}

/// Subscribes a handler to order book deltas matching a pattern.
pub fn subscribe_book_deltas(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OrderBookDeltas>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_deltas
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to order book depth10 snapshots matching a pattern.
pub fn subscribe_book_depth10(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OrderBookDepth10>,
    priority: Option<u32>,
) {
    get_message_bus().borrow_mut().router_depth10.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

/// Subscribes a handler to order book snapshots matching a pattern.
pub fn subscribe_book_snapshots(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OrderBook>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_book_snapshots
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to quote ticks matching a pattern.
pub fn subscribe_quotes(
    pattern: MStr<Pattern>,
    handler: TypedHandler<QuoteTick>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_quotes
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to trade ticks matching a pattern.
pub fn subscribe_trades(
    pattern: MStr<Pattern>,
    handler: TypedHandler<TradeTick>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_trades
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to bars matching a pattern.
pub fn subscribe_bars(pattern: MStr<Pattern>, handler: TypedHandler<Bar>, priority: Option<u32>) {
    get_message_bus()
        .borrow_mut()
        .router_bars
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to mark price updates matching a pattern.
pub fn subscribe_mark_prices(
    pattern: MStr<Pattern>,
    handler: TypedHandler<MarkPriceUpdate>,
    priority: Option<u32>,
) {
    get_message_bus().borrow_mut().router_mark_prices.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

/// Subscribes a handler to index price updates matching a pattern.
pub fn subscribe_index_prices(
    pattern: MStr<Pattern>,
    handler: TypedHandler<IndexPriceUpdate>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_index_prices
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to funding rate updates matching a pattern.
pub fn subscribe_funding_rates(
    pattern: MStr<Pattern>,
    handler: TypedHandler<FundingRateUpdate>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_funding_rates
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to greeks data matching a pattern.
pub fn subscribe_greeks(
    pattern: MStr<Pattern>,
    handler: TypedHandler<GreeksData>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_greeks
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to option greeks updates matching a pattern.
pub fn subscribe_option_greeks(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OptionGreeks>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_option_greeks
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to option chain slice updates matching a pattern.
pub fn subscribe_option_chain(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OptionChainSlice>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_option_chain
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to order events matching a pattern.
pub fn subscribe_order_events(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OrderEventAny>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_order_events
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to position events matching a pattern.
pub fn subscribe_position_events(
    pattern: MStr<Pattern>,
    handler: TypedHandler<PositionEvent>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_position_events
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to positions matching a pattern.
pub fn subscribe_positions(
    pattern: MStr<Pattern>,
    handler: TypedHandler<Position>,
    priority: Option<u32>,
) {
    get_message_bus().borrow_mut().router_positions.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

/// Subscribes a handler to account state updates matching a pattern.
pub fn subscribe_account_state(
    pattern: MStr<Pattern>,
    handler: TypedHandler<AccountState>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_account_state
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to portfolio snapshots matching a pattern.
pub fn subscribe_portfolio_snapshot(
    pattern: MStr<Pattern>,
    handler: TypedHandler<PortfolioSnapshot>,
    priority: Option<u32>,
) {
    get_message_bus().borrow_mut().router_portfolio.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

/// Subscribes a handler to DeFi blocks matching a pattern.
#[cfg(feature = "defi")]
pub fn subscribe_defi_blocks(
    pattern: MStr<Pattern>,
    handler: TypedHandler<Block>,
    priority: Option<u32>,
) {
    get_message_bus().borrow_mut().router_defi_blocks.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

/// Subscribes a handler to DeFi pools matching a pattern.
#[cfg(feature = "defi")]
pub fn subscribe_defi_pools(
    pattern: MStr<Pattern>,
    handler: TypedHandler<Pool>,
    priority: Option<u32>,
) {
    get_message_bus().borrow_mut().router_defi_pools.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

/// Subscribes a handler to DeFi pool swaps matching a pattern.
#[cfg(feature = "defi")]
pub fn subscribe_defi_swaps(
    pattern: MStr<Pattern>,
    handler: TypedHandler<PoolSwap>,
    priority: Option<u32>,
) {
    get_message_bus().borrow_mut().router_defi_swaps.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

/// Subscribes a handler to DeFi liquidity updates matching a pattern.
#[cfg(feature = "defi")]
pub fn subscribe_defi_liquidity(
    pattern: MStr<Pattern>,
    handler: TypedHandler<PoolLiquidityUpdate>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_liquidity
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to DeFi fee collects matching a pattern.
#[cfg(feature = "defi")]
pub fn subscribe_defi_collects(
    pattern: MStr<Pattern>,
    handler: TypedHandler<PoolFeeCollect>,
    priority: Option<u32>,
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_collects
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

/// Subscribes a handler to DeFi flash loans matching a pattern.
#[cfg(feature = "defi")]
pub fn subscribe_defi_flash(
    pattern: MStr<Pattern>,
    handler: TypedHandler<PoolFlash>,
    priority: Option<u32>,
) {
    get_message_bus().borrow_mut().router_defi_flash.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

/// Unsubscribes a handler from instrument messages.
pub fn unsubscribe_instruments(pattern: MStr<Pattern>, handler: &TypedHandler<InstrumentAny>) {
    get_message_bus()
        .borrow_mut()
        .router_instruments
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from instrument close messages.
pub fn unsubscribe_instrument_close(pattern: MStr<Pattern>, handler: &ShareableMessageHandler) {
    unsubscribe_any(pattern, handler);
}

/// Unsubscribes a handler from order book deltas.
pub fn unsubscribe_book_deltas(pattern: MStr<Pattern>, handler: &TypedHandler<OrderBookDeltas>) {
    get_message_bus()
        .borrow_mut()
        .router_deltas
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from order book depth10 snapshots.
pub fn unsubscribe_book_depth10(pattern: MStr<Pattern>, handler: &TypedHandler<OrderBookDepth10>) {
    get_message_bus()
        .borrow_mut()
        .router_depth10
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from order book snapshots.
pub fn unsubscribe_book_snapshots(pattern: MStr<Pattern>, handler: &TypedHandler<OrderBook>) {
    get_message_bus()
        .borrow_mut()
        .router_book_snapshots
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from quote ticks.
pub fn unsubscribe_quotes(pattern: MStr<Pattern>, handler: &TypedHandler<QuoteTick>) {
    get_message_bus()
        .borrow_mut()
        .router_quotes
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from trade ticks.
pub fn unsubscribe_trades(pattern: MStr<Pattern>, handler: &TypedHandler<TradeTick>) {
    get_message_bus()
        .borrow_mut()
        .router_trades
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from bars.
pub fn unsubscribe_bars(pattern: MStr<Pattern>, handler: &TypedHandler<Bar>) {
    get_message_bus()
        .borrow_mut()
        .router_bars
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from mark price updates.
pub fn unsubscribe_mark_prices(pattern: MStr<Pattern>, handler: &TypedHandler<MarkPriceUpdate>) {
    get_message_bus()
        .borrow_mut()
        .router_mark_prices
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from index price updates.
pub fn unsubscribe_index_prices(pattern: MStr<Pattern>, handler: &TypedHandler<IndexPriceUpdate>) {
    get_message_bus()
        .borrow_mut()
        .router_index_prices
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from funding rate updates.
pub fn unsubscribe_funding_rates(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<FundingRateUpdate>,
) {
    get_message_bus()
        .borrow_mut()
        .router_funding_rates
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from account state updates.
pub fn unsubscribe_account_state(pattern: MStr<Pattern>, handler: &TypedHandler<AccountState>) {
    get_message_bus()
        .borrow_mut()
        .router_account_state
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from portfolio snapshots.
pub fn unsubscribe_portfolio_snapshot(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<PortfolioSnapshot>,
) {
    get_message_bus()
        .borrow_mut()
        .router_portfolio
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from order events.
pub fn unsubscribe_order_events(pattern: MStr<Pattern>, handler: &TypedHandler<OrderEventAny>) {
    get_message_bus()
        .borrow_mut()
        .router_order_events
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from position events.
pub fn unsubscribe_position_events(pattern: MStr<Pattern>, handler: &TypedHandler<PositionEvent>) {
    get_message_bus()
        .borrow_mut()
        .router_position_events
        .unsubscribe(pattern, handler);
}

/// Removes a specific order event handler by pattern and handler ID.
pub fn remove_order_event_handler(pattern: MStr<Pattern>, handler_id: Ustr) {
    get_message_bus()
        .borrow_mut()
        .router_order_events
        .remove_handler(pattern, handler_id);
}

/// Removes a specific position event handler by pattern and handler ID.
pub fn remove_position_event_handler(pattern: MStr<Pattern>, handler_id: Ustr) {
    get_message_bus()
        .borrow_mut()
        .router_position_events
        .remove_handler(pattern, handler_id);
}

/// Unsubscribes a handler from orders.
pub fn unsubscribe_orders(pattern: MStr<Pattern>, handler: &TypedHandler<OrderAny>) {
    get_message_bus()
        .borrow_mut()
        .router_orders
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from positions.
pub fn unsubscribe_positions(pattern: MStr<Pattern>, handler: &TypedHandler<Position>) {
    get_message_bus()
        .borrow_mut()
        .router_positions
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from greeks data.
pub fn unsubscribe_greeks(pattern: MStr<Pattern>, handler: &TypedHandler<GreeksData>) {
    get_message_bus()
        .borrow_mut()
        .router_greeks
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from option greeks updates.
pub fn unsubscribe_option_greeks(pattern: MStr<Pattern>, handler: &TypedHandler<OptionGreeks>) {
    get_message_bus()
        .borrow_mut()
        .router_option_greeks
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from option chain slice updates.
pub fn unsubscribe_option_chain(pattern: MStr<Pattern>, handler: &TypedHandler<OptionChainSlice>) {
    get_message_bus()
        .borrow_mut()
        .router_option_chain
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from DeFi blocks.
#[cfg(feature = "defi")]
pub fn unsubscribe_defi_blocks(pattern: MStr<Pattern>, handler: &TypedHandler<Block>) {
    get_message_bus()
        .borrow_mut()
        .router_defi_blocks
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from DeFi pools.
#[cfg(feature = "defi")]
pub fn unsubscribe_defi_pools(pattern: MStr<Pattern>, handler: &TypedHandler<Pool>) {
    get_message_bus()
        .borrow_mut()
        .router_defi_pools
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from DeFi pool swaps.
#[cfg(feature = "defi")]
pub fn unsubscribe_defi_swaps(pattern: MStr<Pattern>, handler: &TypedHandler<PoolSwap>) {
    get_message_bus()
        .borrow_mut()
        .router_defi_swaps
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from DeFi liquidity updates.
#[cfg(feature = "defi")]
pub fn unsubscribe_defi_liquidity(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<PoolLiquidityUpdate>,
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_liquidity
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from DeFi fee collects.
#[cfg(feature = "defi")]
pub fn unsubscribe_defi_collects(pattern: MStr<Pattern>, handler: &TypedHandler<PoolFeeCollect>) {
    get_message_bus()
        .borrow_mut()
        .router_defi_collects
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from DeFi flash loans.
#[cfg(feature = "defi")]
pub fn unsubscribe_defi_flash(pattern: MStr<Pattern>, handler: &TypedHandler<PoolFlash>) {
    get_message_bus()
        .borrow_mut()
        .router_defi_flash
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from a pattern (Any-based).
pub fn unsubscribe_any(pattern: MStr<Pattern>, handler: &ShareableMessageHandler) {
    log::debug!("Unsubscribing {handler:?} from pattern '{pattern}'");

    let handler_id = handler.0.id();
    let bus_rc = get_message_bus();
    let mut bus = bus_rc.borrow_mut();

    let count_before = bus.subscriptions.len();

    bus.topics.values_mut().for_each(|subs| {
        subs.retain(|s| !(s.pattern == pattern && s.handler_id == handler_id));
    });

    bus.subscriptions
        .retain(|s| !(s.pattern == pattern && s.handler_id == handler_id));

    let removed = bus.subscriptions.len() < count_before;

    if removed {
        log::debug!("Handler for pattern '{pattern}' was removed");
    } else {
        log::debug!("No matching handler for pattern '{pattern}' was found");
    }
}

/// Checks if a handler is subscribed to a pattern (Any-based).
pub fn is_subscribed_any<T: AsRef<str>>(pattern: T, handler: ShareableMessageHandler) -> bool {
    let pattern = MStr::from(pattern.as_ref());
    let sub = Subscription::new(pattern, handler, None);
    get_message_bus().borrow().subscriptions.contains(&sub)
}

/// Returns the count of Any-based subscriptions for a topic.
///
/// # Errors
///
/// Returns an error if the `topic` is not a valid topic string.
pub fn subscriptions_count_any<S: AsRef<str>>(topic: S) -> anyhow::Result<usize> {
    get_message_bus().borrow().subscriptions_count(topic)
}

/// Returns the subscriber count for order book deltas on a topic.
pub fn subscriber_count_deltas(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_deltas
        .subscriber_count(topic)
}

/// Returns the subscriber count for order book depth10 on a topic.
pub fn subscriber_count_depth10(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_depth10
        .subscriber_count(topic)
}

/// Returns the subscriber count for order book snapshots on a topic.
pub fn subscriber_count_book_snapshots(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_book_snapshots
        .subscriber_count(topic)
}

/// Returns the exact subscriber count for quotes on a topic,
/// excluding wildcard pattern subscriptions.
pub fn exact_subscriber_count_quotes(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_quotes
        .exact_subscriber_count(topic)
}

/// Returns the exact subscriber count for trades on a topic,
/// excluding wildcard pattern subscriptions.
pub fn exact_subscriber_count_trades(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_trades
        .exact_subscriber_count(topic)
}

/// Returns the exact subscriber count for mark prices on a topic,
/// excluding wildcard pattern subscriptions.
pub fn exact_subscriber_count_mark_prices(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_mark_prices
        .exact_subscriber_count(topic)
}

/// Returns the exact subscriber count for index prices on a topic,
/// excluding wildcard pattern subscriptions.
pub fn exact_subscriber_count_index_prices(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_index_prices
        .exact_subscriber_count(topic)
}

/// Returns the exact subscriber count for funding rates on a topic,
/// excluding wildcard pattern subscriptions.
pub fn exact_subscriber_count_funding_rates(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_funding_rates
        .exact_subscriber_count(topic)
}

/// Returns the exact subscriber count for option greeks on a topic,
/// excluding wildcard pattern subscriptions.
pub fn exact_subscriber_count_option_greeks(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_option_greeks
        .exact_subscriber_count(topic)
}

/// Returns the exact subscriber count for bars on a topic,
/// excluding wildcard pattern subscriptions.
pub fn exact_subscriber_count_bars(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_bars
        .exact_subscriber_count(topic)
}

/// Publishes a message to the topic using runtime type dispatch (Any).
pub fn publish_any(topic: MStr<Topic>, message: &dyn Any) {
    dispatch_tap_publish(topic, message);

    // Take buffer (re-entrancy safe)
    let mut handlers = ANY_HANDLERS.with_borrow_mut(std::mem::take);

    {
        let bus_rc = get_message_bus();
        let mut bus = bus_rc.borrow_mut();
        bus.fill_matching_any_handlers(topic, &mut handlers);
        bus.increment_pub_count();
    }

    for handler in &handlers {
        handler.0.handle(message);
    }

    handlers.clear(); // Release refs before restore
    ANY_HANDLERS.with_borrow_mut(|buf| *buf = handlers);

    let Some(custom) = message.downcast_ref::<CustomData>() else {
        return;
    };

    forward_to_publisher(
        topic,
        BusPayloadType::Custom(Ustr::from(custom.data.type_name())),
        custom,
    );
}

/// Tries to publish a message to the current thread's registered message bus.
///
/// Returns `false` when the thread has no bus or the bus is already borrowed.
pub fn try_publish_any(topic: MStr<Topic>, message: &dyn Any) -> bool {
    let Some(bus_rc) = try_get_message_bus() else {
        return false;
    };

    if bus_rc.try_borrow_mut().is_err() {
        return false;
    }

    dispatch_tap_publish(topic, message);

    let Ok(mut bus) = bus_rc.try_borrow_mut() else {
        return false;
    };

    // Take buffer (re-entrancy safe)
    let mut handlers = ANY_HANDLERS.with_borrow_mut(std::mem::take);

    bus.fill_matching_any_handlers(topic, &mut handlers);
    bus.increment_pub_count();
    drop(bus);

    for handler in &handlers {
        handler.0.handle(message);
    }

    handlers.clear(); // Release refs before restore
    ANY_HANDLERS.with_borrow_mut(|buf| *buf = handlers);
    true
}

/// Publishes an instrument to subscribers on a topic.
pub fn publish_instrument(topic: MStr<Topic>, instrument: &InstrumentAny) {
    publish_typed(
        topic,
        &INSTRUMENT_HANDLERS,
        |bus, h| bus.router_instruments.fill_matching_handlers(topic, h),
        instrument,
    );

    forward_to_publisher(topic, BusPayloadType::Instrument, instrument);
}

/// Publishes order book deltas to subscribers on a topic.
pub fn publish_deltas(topic: MStr<Topic>, deltas: &OrderBookDeltas) {
    publish_typed(
        topic,
        &DELTAS_HANDLERS,
        |bus, h| bus.router_deltas.fill_matching_handlers(topic, h),
        deltas,
    );

    forward_to_publisher(topic, BusPayloadType::OrderBookDeltas, deltas);
}

/// Publishes order book depth10 to subscribers on a topic.
pub fn publish_depth10(topic: MStr<Topic>, depth: &OrderBookDepth10) {
    publish_typed(
        topic,
        &DEPTH10_HANDLERS,
        |bus, h| bus.router_depth10.fill_matching_handlers(topic, h),
        depth,
    );

    forward_to_publisher(topic, BusPayloadType::OrderBookDepth10, depth);
}

/// Publishes an order book snapshot to subscribers on a topic.
pub fn publish_book(topic: MStr<Topic>, book: &OrderBook) {
    publish_typed(
        topic,
        &BOOK_HANDLERS,
        |bus, h| bus.router_book_snapshots.fill_matching_handlers(topic, h),
        book,
    );
}

/// Publishes a quote tick to subscribers on a topic.
pub fn publish_quote(topic: MStr<Topic>, quote: &QuoteTick) {
    publish_typed(
        topic,
        &QUOTE_HANDLERS,
        |bus, h| bus.router_quotes.fill_matching_handlers(topic, h),
        quote,
    );

    forward_to_publisher(topic, BusPayloadType::QuoteTick, quote);
}

/// Publishes a trade tick to subscribers on a topic.
pub fn publish_trade(topic: MStr<Topic>, trade: &TradeTick) {
    publish_typed(
        topic,
        &TRADE_HANDLERS,
        |bus, h| bus.router_trades.fill_matching_handlers(topic, h),
        trade,
    );

    forward_to_publisher(topic, BusPayloadType::TradeTick, trade);
}

/// Publishes a bar to subscribers on a topic.
pub fn publish_bar(topic: MStr<Topic>, bar: &Bar) {
    publish_typed(
        topic,
        &BAR_HANDLERS,
        |bus, h| bus.router_bars.fill_matching_handlers(topic, h),
        bar,
    );

    forward_to_publisher(topic, BusPayloadType::Bar, bar);
}

/// Publishes a mark price update to subscribers on a topic.
pub fn publish_mark_price(topic: MStr<Topic>, mark_price: &MarkPriceUpdate) {
    publish_typed(
        topic,
        &MARK_PRICE_HANDLERS,
        |bus, h| bus.router_mark_prices.fill_matching_handlers(topic, h),
        mark_price,
    );

    forward_to_publisher(topic, BusPayloadType::MarkPriceUpdate, mark_price);
}

/// Publishes an index price update to subscribers on a topic.
pub fn publish_index_price(topic: MStr<Topic>, index_price: &IndexPriceUpdate) {
    publish_typed(
        topic,
        &INDEX_PRICE_HANDLERS,
        |bus, h| bus.router_index_prices.fill_matching_handlers(topic, h),
        index_price,
    );

    forward_to_publisher(topic, BusPayloadType::IndexPriceUpdate, index_price);
}

/// Publishes a funding rate update to subscribers on a topic.
pub fn publish_funding_rate(topic: MStr<Topic>, funding_rate: &FundingRateUpdate) {
    publish_typed(
        topic,
        &FUNDING_RATE_HANDLERS,
        |bus, h| bus.router_funding_rates.fill_matching_handlers(topic, h),
        funding_rate,
    );

    forward_to_publisher(topic, BusPayloadType::FundingRateUpdate, funding_rate);
}

/// Publishes greeks data to subscribers on a topic.
pub fn publish_greeks(topic: MStr<Topic>, greeks: &GreeksData) {
    publish_typed(
        topic,
        &GREEKS_HANDLERS,
        |bus, h| bus.router_greeks.fill_matching_handlers(topic, h),
        greeks,
    );
}

/// Publishes option greeks to subscribers on a topic.
pub fn publish_option_greeks(topic: MStr<Topic>, option_greeks: &OptionGreeks) {
    publish_typed(
        topic,
        &OPTION_GREEKS_HANDLERS,
        |bus, h| bus.router_option_greeks.fill_matching_handlers(topic, h),
        option_greeks,
    );

    forward_to_publisher(topic, BusPayloadType::OptionGreeks, option_greeks);
}

/// Publishes an option chain slice to subscribers on a topic.
pub fn publish_option_chain(topic: MStr<Topic>, slice: &OptionChainSlice) {
    publish_typed(
        topic,
        &OPTION_CHAIN_HANDLERS,
        |bus, h| bus.router_option_chain.fill_matching_handlers(topic, h),
        slice,
    );
}

/// Publishes an account state to subscribers on a topic.
pub fn publish_account_state(topic: MStr<Topic>, state: &AccountState) {
    publish_typed(
        topic,
        &ACCOUNT_STATE_HANDLERS,
        |bus, h| bus.router_account_state.fill_matching_handlers(topic, h),
        state,
    );

    forward_to_publisher(topic, BusPayloadType::AccountState, state);
}

/// Publishes a portfolio snapshot to subscribers on a topic.
pub fn publish_portfolio_snapshot(topic: MStr<Topic>, snapshot: &PortfolioSnapshot) {
    publish_typed(
        topic,
        &PORTFOLIO_SNAPSHOT_HANDLERS,
        |bus, h| {
            bus.router_portfolio.fill_matching_handlers(topic, h);
        },
        snapshot,
    );

    forward_to_publisher(topic, BusPayloadType::PortfolioSnapshot, snapshot);
}

/// Publishes an order event to subscribers on a topic.
pub fn publish_order_event(topic: MStr<Topic>, event: &OrderEventAny) {
    publish_typed(
        topic,
        &ORDER_EVENT_HANDLERS,
        |bus, h| bus.router_order_events.fill_matching_handlers(topic, h),
        event,
    );

    forward_to_publisher(topic, BusPayloadType::OrderEvent, event);
}

/// Publishes a position event to subscribers on a topic.
pub fn publish_position_event(topic: MStr<Topic>, event: &PositionEvent) {
    publish_typed(
        topic,
        &POSITION_EVENT_HANDLERS,
        |bus, h| bus.router_position_events.fill_matching_handlers(topic, h),
        event,
    );

    forward_to_publisher(topic, BusPayloadType::PositionEvent, event);
}

/// Publishes a DeFi block to subscribers on a topic.
#[cfg(feature = "defi")]
pub fn publish_defi_block(topic: MStr<Topic>, block: &Block) {
    publish_typed(
        topic,
        &DEFI_BLOCK_HANDLERS,
        |bus, h| bus.router_defi_blocks.fill_matching_handlers(topic, h),
        block,
    );

    forward_to_publisher(topic, BusPayloadType::Block, block);
}

/// Publishes a DeFi pool to subscribers on a topic.
#[cfg(feature = "defi")]
pub fn publish_defi_pool(topic: MStr<Topic>, pool: &Pool) {
    publish_typed(
        topic,
        &DEFI_POOL_HANDLERS,
        |bus, h| bus.router_defi_pools.fill_matching_handlers(topic, h),
        pool,
    );

    forward_to_publisher(topic, BusPayloadType::Pool, pool);
}

/// Publishes a DeFi pool swap to subscribers on a topic.
#[cfg(feature = "defi")]
pub fn publish_defi_swap(topic: MStr<Topic>, swap: &PoolSwap) {
    publish_typed(
        topic,
        &DEFI_SWAP_HANDLERS,
        |bus, h| bus.router_defi_swaps.fill_matching_handlers(topic, h),
        swap,
    );
}

/// Publishes a DeFi liquidity update to subscribers on a topic.
#[cfg(feature = "defi")]
pub fn publish_defi_liquidity(topic: MStr<Topic>, update: &PoolLiquidityUpdate) {
    publish_typed(
        topic,
        &DEFI_LIQUIDITY_HANDLERS,
        |bus, h| bus.router_defi_liquidity.fill_matching_handlers(topic, h),
        update,
    );

    forward_to_publisher(topic, BusPayloadType::PoolLiquidityUpdate, update);
}

/// Publishes a DeFi fee collect to subscribers on a topic.
#[cfg(feature = "defi")]
pub fn publish_defi_collect(topic: MStr<Topic>, collect: &PoolFeeCollect) {
    publish_typed(
        topic,
        &DEFI_COLLECT_HANDLERS,
        |bus, h| bus.router_defi_collects.fill_matching_handlers(topic, h),
        collect,
    );

    forward_to_publisher(topic, BusPayloadType::PoolFeeCollect, collect);
}

/// Publishes a DeFi flash loan to subscribers on a topic.
#[cfg(feature = "defi")]
pub fn publish_defi_flash(topic: MStr<Topic>, flash: &PoolFlash) {
    publish_typed(
        topic,
        &DEFI_FLASH_HANDLERS,
        |bus, h| bus.router_defi_flash.fill_matching_handlers(topic, h),
        flash,
    );

    forward_to_publisher(topic, BusPayloadType::PoolFlash, flash);
}

#[inline(always)]
fn forward_to_publisher<T>(topic: MStr<Topic>, payload_type: BusPayloadType, message: &T)
where
    T: serde::Serialize + Any,
{
    if !HAS_PUBLISHER.with(Cell::get) {
        return;
    }

    forward_transport_message(topic, payload_type, message);
}

#[cold]
#[inline(never)]
fn forward_transport_message<T>(topic: MStr<Topic>, payload_type: BusPayloadType, message: &T)
where
    T: serde::Serialize + Any,
{
    if SUPPRESS_EXTERNAL_DEPTH.with(Cell::get) > 0 {
        return;
    }

    let bus_rc = get_message_bus();
    let bus = bus_rc.borrow();
    let Some(publisher) = bus.publisher().filter(|publisher| !publisher.is_closed()) else {
        return;
    };

    let type_name = payload_type.as_str();
    if bus.types_filter().contains(type_name) {
        return;
    }

    let encoding = bus.encoding_for(payload_type);
    let payload = match encode_publisher_payload(encoding, payload_type, message) {
        Ok(payload) => payload,
        Err(PublisherPayloadError::Dropped(e)) => {
            log::debug!("{e}");
            return;
        }
        Err(PublisherPayloadError::Failed(e)) => {
            log::error!("{e}");
            return;
        }
    };

    // Build after drop checks to avoid allocating discarded transport messages
    publisher.publish(BusMessage::new(*topic, encoding, payload_type, payload));
}

/// Decodes an externally-received [`BusMessage`] and republishes it onto the internal bus.
///
/// The message `payload_type` header selects the concrete type and the message `encoding` selects
/// the wire codec, so the message is decoded with the producer's encoding rather than the local
/// configuration. Republishing runs under a [`SuppressExternalGuard`] so the message is not
/// forwarded straight back out to the external transport, which would create an echo loop on a
/// node that both publishes and subscribes externally.
///
/// # Errors
///
/// Returns an error if the payload cannot be decoded for the message's encoding.
pub fn republish_transport_message(message: &BusMessage) -> anyhow::Result<()> {
    let _guard = SuppressExternalGuard::new();
    let topic: MStr<Topic> = message.topic.into();

    match message.payload_type {
        BusPayloadType::QuoteTick => handle_quote(topic, message.encoding, &message.payload)?,
        other => log::warn!(
            "External payload type '{}' is not yet supported for inbound republishing",
            other.as_str()
        ),
    }

    Ok(())
}

fn handle_quote(
    topic: MStr<Topic>,
    encoding: SerializationEncoding,
    payload: &[u8],
) -> anyhow::Result<()> {
    let quote = match encoding {
        SerializationEncoding::Json => parse_quote_json(payload),
        SerializationEncoding::MsgPack => parse_quote_msgpack(payload),
        SerializationEncoding::Sbe => decode_quote_sbe(payload),
        SerializationEncoding::Capnp => decode_quote_capnp(payload),
    }?;

    publish_quote(topic, &quote);
    Ok(())
}

fn parse_quote_json(payload: &[u8]) -> anyhow::Result<QuoteTick> {
    serde_json::from_slice(payload).context("failed to decode JSON QuoteTick")
}

fn parse_quote_msgpack(payload: &[u8]) -> anyhow::Result<QuoteTick> {
    rmp_serde::from_slice(payload).context("failed to decode MsgPack QuoteTick")
}

#[cfg(feature = "sbe")]
fn decode_quote_sbe(payload: &[u8]) -> anyhow::Result<QuoteTick> {
    QuoteTick::from_sbe(payload).map_err(|e| anyhow::anyhow!("failed to decode SBE QuoteTick: {e}"))
}

#[cfg(not(feature = "sbe"))]
fn decode_quote_sbe(_payload: &[u8]) -> anyhow::Result<QuoteTick> {
    anyhow::bail!("SBE decoding requires the `sbe` feature")
}

#[cfg(feature = "capnp")]
fn decode_quote_capnp(payload: &[u8]) -> anyhow::Result<QuoteTick> {
    let reader =
        capnp::serialize::read_message(&mut &payload[..], capnp::message::ReaderOptions::new())
            .context("failed to read Cap'n Proto message")?;
    let root = reader
        .get_root::<market_capnp::quote_tick::Reader>()
        .context("Cap'n Proto payload has no QuoteTick root")?;
    QuoteTick::from_capnp(root)
        .map_err(|e| anyhow::anyhow!("failed to decode Cap'n Proto QuoteTick: {e}"))
}

#[cfg(not(feature = "capnp"))]
fn decode_quote_capnp(_payload: &[u8]) -> anyhow::Result<QuoteTick> {
    anyhow::bail!("Cap'n Proto decoding requires the `capnp` feature")
}

#[derive(Debug)]
enum PublisherPayloadError {
    Dropped(String),
    Failed(String),
}

fn encode_publisher_payload<T>(
    encoding: SerializationEncoding,
    payload_type: BusPayloadType,
    message: &T,
) -> Result<Bytes, PublisherPayloadError>
where
    T: serde::Serialize + Any,
{
    let type_name = payload_type.as_str();

    match encoding {
        SerializationEncoding::Json => serde_json::to_vec(message).map(Bytes::from).map_err(|e| {
            PublisherPayloadError::Failed(format!("JSON serialization failed for {type_name}: {e}"))
        }),
        SerializationEncoding::MsgPack => rmp_serde::to_vec_named(message)
            .map(Bytes::from)
            .map_err(|e| {
                PublisherPayloadError::Failed(format!(
                    "MsgPack serialization failed for {type_name}: {e}"
                ))
            }),
        SerializationEncoding::Capnp => encode_capnp_payload(payload_type, message),
        SerializationEncoding::Sbe => encode_sbe_payload(payload_type, message),
    }
}

#[cfg(feature = "capnp")]
macro_rules! encode_capnp_payload_as {
    ($message:expr, $type_name:expr, $ty:ty, $root:ty) => {{
        let Some(value) = $message.downcast_ref::<$ty>() else {
            return Err(PublisherPayloadError::Failed(format!(
                "Cap'n Proto payload type mismatch for {}",
                $type_name
            )));
        };

        let mut capnp_message = capnp::message::Builder::new_default();
        let builder = capnp_message.init_root::<$root>();
        value.to_capnp(builder);

        let mut bytes = Vec::new();
        capnp::serialize::write_message(&mut bytes, &capnp_message).map_err(|e| {
            PublisherPayloadError::Failed(format!(
                "Cap'n Proto serialization failed for {}: {}",
                $type_name, e
            ))
        })?;
        Ok(Bytes::from(bytes))
    }};
}

#[cfg(feature = "capnp")]
fn encode_capnp_payload(
    payload_type: BusPayloadType,
    message: &dyn Any,
) -> Result<Bytes, PublisherPayloadError> {
    let type_name = payload_type.as_str();
    match payload_type {
        BusPayloadType::OrderBookDeltas => encode_capnp_payload_as!(
            message,
            type_name,
            OrderBookDeltas,
            market_capnp::order_book_deltas::Builder
        ),
        BusPayloadType::OrderBookDepth10 => encode_capnp_payload_as!(
            message,
            type_name,
            OrderBookDepth10,
            market_capnp::order_book_depth10::Builder
        ),
        BusPayloadType::QuoteTick => encode_capnp_payload_as!(
            message,
            type_name,
            QuoteTick,
            market_capnp::quote_tick::Builder
        ),
        BusPayloadType::TradeTick => encode_capnp_payload_as!(
            message,
            type_name,
            TradeTick,
            market_capnp::trade_tick::Builder
        ),
        BusPayloadType::Bar => {
            encode_capnp_payload_as!(message, type_name, Bar, market_capnp::bar::Builder)
        }
        BusPayloadType::MarkPriceUpdate => encode_capnp_payload_as!(
            message,
            type_name,
            MarkPriceUpdate,
            market_capnp::mark_price_update::Builder
        ),
        BusPayloadType::IndexPriceUpdate => encode_capnp_payload_as!(
            message,
            type_name,
            IndexPriceUpdate,
            market_capnp::index_price_update::Builder
        ),
        BusPayloadType::FundingRateUpdate => encode_capnp_payload_as!(
            message,
            type_name,
            FundingRateUpdate,
            market_capnp::funding_rate_update::Builder
        ),
        _ => Err(PublisherPayloadError::Dropped(format!(
            "Cap'n Proto serialization is not supported for {type_name}"
        ))),
    }
}

#[cfg(not(feature = "capnp"))]
fn encode_capnp_payload(
    payload_type: BusPayloadType,
    _message: &dyn Any,
) -> Result<Bytes, PublisherPayloadError> {
    let type_name = payload_type.as_str();
    Err(PublisherPayloadError::Dropped(format!(
        "Cap'n Proto serialization for {type_name} requires the `capnp` feature"
    )))
}

#[cfg(feature = "sbe")]
fn encode_sbe_payload(
    payload_type: BusPayloadType,
    message: &dyn Any,
) -> Result<Bytes, PublisherPayloadError> {
    let type_name = payload_type.as_str();
    match payload_type {
        BusPayloadType::OrderBookDeltas => {
            encode_sbe_payload_as::<OrderBookDeltas>(type_name, message)
        }
        BusPayloadType::OrderBookDepth10 => {
            encode_sbe_payload_as::<OrderBookDepth10>(type_name, message)
        }
        BusPayloadType::QuoteTick => encode_sbe_payload_as::<QuoteTick>(type_name, message),
        BusPayloadType::TradeTick => encode_sbe_payload_as::<TradeTick>(type_name, message),
        BusPayloadType::Bar => encode_sbe_payload_as::<Bar>(type_name, message),
        BusPayloadType::MarkPriceUpdate => {
            encode_sbe_payload_as::<MarkPriceUpdate>(type_name, message)
        }
        BusPayloadType::IndexPriceUpdate => {
            encode_sbe_payload_as::<IndexPriceUpdate>(type_name, message)
        }
        BusPayloadType::FundingRateUpdate => {
            encode_sbe_payload_as::<FundingRateUpdate>(type_name, message)
        }
        _ => Err(PublisherPayloadError::Dropped(format!(
            "SBE serialization is not supported for {type_name}"
        ))),
    }
}

#[cfg(feature = "sbe")]
fn encode_sbe_payload_as<T>(
    type_name: &str,
    message: &dyn Any,
) -> Result<Bytes, PublisherPayloadError>
where
    T: Any + ToSbe,
{
    let Some(value) = message.downcast_ref::<T>() else {
        return Err(PublisherPayloadError::Failed(format!(
            "SBE payload type mismatch for {type_name}"
        )));
    };

    value.to_sbe().map(Bytes::from).map_err(|e| {
        PublisherPayloadError::Failed(format!("SBE serialization failed for {type_name}: {e}"))
    })
}

#[cfg(not(feature = "sbe"))]
fn encode_sbe_payload(
    payload_type: BusPayloadType,
    _message: &dyn Any,
) -> Result<Bytes, PublisherPayloadError> {
    let type_name = payload_type.as_str();
    Err(PublisherPayloadError::Dropped(format!(
        "SBE serialization for {type_name} requires the `sbe` feature"
    )))
}

/// Publishes a message to typed handlers using thread-local buffer reuse.
///
/// The `fill_fn` receives a mutable reference to the `MessageBus`, avoiding
/// redundant TLS access and Rc clone/drop overhead per publish.
///
/// Before fanout the registered bus tap (if any) observes the message. Capture must
/// precede subscriber dispatch so the durable record exists before any handler reacts
/// to the message.
///
/// # Invariants
///
/// - `fill_fn` must not call any publish path (would panic from `RefCell` double-borrow).
/// - Handler panics drop the buffer, losing reuse optimization (acceptable as panics are fatal).
#[inline]
fn publish_typed<T: 'static>(
    topic: MStr<Topic>,
    tls: &'static LocalKey<RefCell<SmallVec<[TypedHandler<T>; HANDLER_BUFFER_CAP]>>>,
    fill_fn: impl FnOnce(&mut MessageBus, &mut SmallVec<[TypedHandler<T>; HANDLER_BUFFER_CAP]>),
    message: &T,
) {
    dispatch_tap_publish(topic, message);

    // Take buffer (re-entrancy safe)
    let mut handlers = tls.with_borrow_mut(std::mem::take);

    // Borrow scope ends before dispatch to support re-entrant publishes
    let bus_rc = get_message_bus();
    {
        let mut bus = bus_rc.borrow_mut();
        fill_fn(&mut bus, &mut handlers);
        bus.increment_pub_count();
    }

    for handler in &handlers {
        handler.handle(message);
    }

    handlers.clear(); // Release refs before restore
    tls.with_borrow_mut(|buf| *buf = handlers);
}

/// Sends a message to an endpoint handler using runtime type dispatch (Any).
pub fn send_any(endpoint: MStr<Endpoint>, message: &dyn Any) {
    dispatch_tap_send(endpoint, message);

    let handler = {
        let bus = get_message_bus();
        let mut bus = bus.borrow_mut();
        let handler = bus.get_endpoint(endpoint).cloned();
        if handler.is_some() {
            bus.increment_sent_count();
        }
        handler
    };

    if let Some(handler) = handler {
        handler.0.handle(message);
    } else {
        log::error!("send_any: no registered endpoint '{endpoint}'");
    }
}

/// Sends a message to an endpoint, converting to Any (convenience wrapper).
pub fn send_any_value<T: 'static>(endpoint: MStr<Endpoint>, message: &T) {
    dispatch_tap_send(endpoint, message);

    let handler = {
        let bus = get_message_bus();
        let mut bus = bus.borrow_mut();
        let handler = bus.get_endpoint(endpoint).cloned();
        if handler.is_some() {
            bus.increment_sent_count();
        }
        handler
    };

    if let Some(handler) = handler {
        handler.0.handle(message);
    } else {
        log::error!("send_any_value: no registered endpoint '{endpoint}'");
    }
}

/// Sends the [`DataResponse`] to the registered correlation ID handler.
pub fn send_response(correlation_id: &UUID4, message: &DataResponse) {
    dispatch_tap_response(correlation_id, message);

    let handler = {
        let bus = get_message_bus();
        let mut bus = bus.borrow_mut();
        let handler = bus.get_response_handler(correlation_id).cloned();
        bus.increment_res_count();
        handler
    };

    if let Some(handler) = handler {
        match message {
            DataResponse::Data(resp) => handler.0.handle(resp),
            DataResponse::Instrument(resp) => handler.0.handle(resp.as_ref()),
            DataResponse::Instruments(resp) => handler.0.handle(resp),
            DataResponse::Book(resp) => handler.0.handle(resp),
            DataResponse::BookDeltas(resp) => handler.0.handle(resp),
            DataResponse::BookDepth(resp) => handler.0.handle(resp),
            DataResponse::Quotes(resp) => handler.0.handle(resp),
            DataResponse::Trades(resp) => handler.0.handle(resp),
            DataResponse::FundingRates(resp) => handler.0.handle(resp),
            DataResponse::ForwardPrices(resp) => handler.0.handle(resp),
            DataResponse::Bars(resp) => handler.0.handle(resp),
        }
    } else {
        log::error!("send_response: handler not found for correlation_id '{correlation_id}'");
    }
}

/// Sends a quote tick to an endpoint handler.
pub fn send_quote(endpoint: MStr<Endpoint>, quote: &QuoteTick) {
    send_endpoint_ref(
        endpoint,
        quote,
        |bus| bus.endpoints_quotes.get(endpoint),
        "send_quote",
    );
}

/// Sends a trade tick to an endpoint handler.
pub fn send_trade(endpoint: MStr<Endpoint>, trade: &TradeTick) {
    send_endpoint_ref(
        endpoint,
        trade,
        |bus| bus.endpoints_trades.get(endpoint),
        "send_trade",
    );
}

/// Sends a bar to an endpoint handler.
pub fn send_bar(endpoint: MStr<Endpoint>, bar: &Bar) {
    send_endpoint_ref(
        endpoint,
        bar,
        |bus| bus.endpoints_bars.get(endpoint),
        "send_bar",
    );
}

/// Sends an order event to an endpoint handler, transferring ownership.
pub fn send_order_event(endpoint: MStr<Endpoint>, event: OrderEventAny) {
    send_endpoint_owned(
        endpoint,
        event,
        |bus| bus.endpoints_order_events.get(endpoint),
        "send_order_event",
    );
}

/// Sends an account state to an endpoint handler.
pub fn send_account_state(endpoint: MStr<Endpoint>, state: &AccountState) {
    send_endpoint_ref(
        endpoint,
        state,
        |bus| bus.endpoints_account_state.get(endpoint),
        "send_account_state",
    );
}

/// Sends a trading command to an endpoint handler, transferring ownership.
pub fn send_trading_command(endpoint: MStr<Endpoint>, command: TradingCommand) {
    send_endpoint_owned(
        endpoint,
        command,
        |bus| bus.endpoints_trading_commands.get(endpoint),
        "send_trading_command",
    );
}

/// Sends a data command to an endpoint handler, transferring ownership.
pub fn send_data_command(endpoint: MStr<Endpoint>, command: DataCommand) {
    let is_request = data_command_is_request(&command);
    send_endpoint_owned_counted(
        endpoint,
        command,
        |bus| bus.endpoints_data_commands.get(endpoint),
        "send_data_command",
        is_request,
    );
}

/// Sends a data response to an endpoint handler, transferring ownership.
pub fn send_data_response(endpoint: MStr<Endpoint>, response: DataResponse) {
    send_endpoint_owned(
        endpoint,
        response,
        |bus| bus.endpoints_data_responses.get(endpoint),
        "send_data_response",
    );
}

/// Sends an execution report to an endpoint handler, transferring ownership.
pub fn send_execution_report(endpoint: MStr<Endpoint>, report: ExecutionReport) {
    send_endpoint_owned(
        endpoint,
        report,
        |bus| bus.endpoints_exec_reports.get(endpoint),
        "send_execution_report",
    );
}

/// Sends data to an endpoint handler, transferring ownership.
pub fn send_data(endpoint: MStr<Endpoint>, data: Data) {
    send_endpoint_owned(
        endpoint,
        data,
        |bus| bus.endpoints_data.get(endpoint),
        "send_data",
    );
}

/// Sends DeFi data to an endpoint handler, transferring ownership.
#[cfg(feature = "defi")]
pub fn send_defi_data(endpoint: MStr<Endpoint>, data: DefiData) {
    send_endpoint_owned(
        endpoint,
        data,
        |bus| bus.endpoints_defi_data.get(endpoint),
        "send_defi_data",
    );
}

#[inline]
fn send_endpoint_ref<T: 'static, F>(
    endpoint: MStr<Endpoint>,
    message: &T,
    get_handler: F,
    fn_name: &str,
) where
    F: FnOnce(&MessageBus) -> Option<&TypedHandler<T>>,
{
    dispatch_tap_send(endpoint, message);

    let handler = {
        let bus = get_message_bus();
        let mut bus = bus.borrow_mut();
        let handler = get_handler(&bus).cloned();
        if handler.is_some() {
            bus.increment_sent_count();
        }
        handler
    };

    if let Some(handler) = handler {
        handler.handle(message);
    } else {
        log::error!("{fn_name}: no registered endpoint '{endpoint}'");
    }
}

#[inline]
fn send_endpoint_owned<T: 'static, F>(
    endpoint: MStr<Endpoint>,
    message: T,
    get_handler: F,
    fn_name: &str,
) where
    F: FnOnce(&MessageBus) -> Option<&TypedIntoHandler<T>>,
{
    send_endpoint_owned_counted(endpoint, message, get_handler, fn_name, false);
}

#[inline]
fn send_endpoint_owned_counted<T: 'static, F>(
    endpoint: MStr<Endpoint>,
    message: T,
    get_handler: F,
    fn_name: &str,
    count_request: bool,
) where
    F: FnOnce(&MessageBus) -> Option<&TypedIntoHandler<T>>,
{
    // Capture before the dispatch consumes `message`
    dispatch_tap_send(endpoint, &message);

    let handler = {
        let bus = get_message_bus();
        let mut bus = bus.borrow_mut();
        let handler = get_handler(&bus).cloned();
        if handler.is_some() {
            bus.increment_sent_count();
            if count_request {
                bus.increment_req_count();
            }
        }
        handler
    };

    if let Some(handler) = handler {
        handler.handle(message);
    } else {
        log::error!("{fn_name}: no registered endpoint '{endpoint}'");
    }
}

#[inline]
fn data_command_is_request(command: &DataCommand) -> bool {
    match command {
        DataCommand::Request(_) => true,
        #[cfg(feature = "defi")]
        DataCommand::DefiRequest(_) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the message bus API functions.
    //!
    //! Includes re-entrancy tests that verify handlers can call back into the
    //! message bus without causing `RefCell` borrow conflicts. This is the scenario
    //! where `send_*` holds a borrow, calls the handler, and the handler needs to
    //! call `borrow_mut()` for topic getters or other operations.

    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        thread,
    };

    use bytes::Bytes;
    use nautilus_core::UUID4;
    use nautilus_model::{
        data::{
            Bar, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick, stubs::stub_custom_data,
        },
        enums::OrderSide,
        events::order::spec::OrderDeniedSpec,
        identifiers::{ClientId, InstrumentId, StrategyId, TraderId},
    };
    #[cfg(feature = "sbe")]
    use nautilus_serialization::sbe::FromSbe;
    #[cfg(feature = "capnp")]
    use nautilus_serialization::{capnp::FromCapnp, market_capnp};
    use rstest::rstest;

    use super::*;
    use crate::{
        enums::SerializationEncoding,
        messages::{
            data::{
                DataCommand, DataResponse, QuotesResponse, RequestCommand, RequestQuotes,
                SubscribeCommand, SubscribeQuotes,
            },
            execution::{CancelAllOrders, TradingCommand},
        },
        msgbus::{
            BusTap, MessageBusPublisher, SuppressExternalGuard, backing::MessageBusConfig,
            clear_bus_tap, set_bus_tap, set_message_bus, stubs::get_call_check_handler,
        },
    };

    #[derive(Debug)]
    struct CapturedPublication {
        topic: String,
        encoding: SerializationEncoding,
        payload_type: BusPayloadType,
        payload: Bytes,
    }

    struct CapturingPublisher {
        publications: Rc<RefCell<Vec<CapturedPublication>>>,
        closed: Cell<bool>,
    }

    impl CapturingPublisher {
        fn new() -> (Self, Rc<RefCell<Vec<CapturedPublication>>>) {
            let publications = Rc::new(RefCell::new(Vec::new()));
            (
                Self {
                    publications: publications.clone(),
                    closed: Cell::new(false),
                },
                publications,
            )
        }
    }

    impl MessageBusPublisher for CapturingPublisher {
        fn is_closed(&self) -> bool {
            self.closed.get()
        }

        fn publish(&self, message: BusMessage) {
            self.publications.borrow_mut().push(CapturedPublication {
                topic: message.topic.to_string(),
                encoding: message.encoding,
                payload_type: message.payload_type,
                payload: message.payload,
            });
        }

        fn close(&mut self) {
            self.closed.set(true);
        }
    }

    fn install_capturing_publisher(
        encoding: SerializationEncoding,
    ) -> Rc<RefCell<Vec<CapturedPublication>>> {
        let msgbus = Rc::new(RefCell::new(MessageBus::default()));
        set_message_bus(msgbus.clone());
        let (publisher, publications) = CapturingPublisher::new();
        msgbus
            .borrow_mut()
            .set_publisher(Box::new(publisher), encoding);
        publications
    }

    fn install_capturing_publisher_config(
        config: &MessageBusConfig,
    ) -> Rc<RefCell<Vec<CapturedPublication>>> {
        let msgbus = Rc::new(RefCell::new(MessageBus::default()));
        set_message_bus(msgbus.clone());
        let (publisher, publications) = CapturingPublisher::new();
        msgbus
            .borrow_mut()
            .set_publisher_config(Box::new(publisher), config)
            .expect("message bus config must be valid");
        publications
    }

    fn reset_message_bus() {
        get_message_bus().borrow_mut().dispose();
        set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
    }

    #[rstest]
    #[case(SerializationEncoding::MsgPack)]
    #[case(SerializationEncoding::Json)]
    fn publish_quote_forwards_decodable_payload_to_publisher(
        #[case] encoding: SerializationEncoding,
    ) {
        let publications = install_capturing_publisher(encoding);
        let quote = QuoteTick::default();

        publish_quote("data.quotes.TEST".into(), &quote);

        let publications = publications.borrow();
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].topic, "data.quotes.TEST");

        let decoded: QuoteTick = match encoding {
            SerializationEncoding::MsgPack => rmp_serde::from_slice(&publications[0].payload)
                .expect("MsgPack payload must decode as QuoteTick"),
            SerializationEncoding::Json => serde_json::from_slice(&publications[0].payload)
                .expect("JSON payload must decode as QuoteTick"),
            SerializationEncoding::Sbe | SerializationEncoding::Capnp => {
                unreachable!("schema encodings are tested separately")
            }
        };
        let payload_value: serde_json::Value = match encoding {
            SerializationEncoding::MsgPack => rmp_serde::from_slice(&publications[0].payload)
                .expect("MsgPack payload must decode as a value"),
            SerializationEncoding::Json => serde_json::from_slice(&publications[0].payload)
                .expect("JSON payload must decode as a value"),
            SerializationEncoding::Sbe | SerializationEncoding::Capnp => {
                unreachable!("schema encodings are tested separately")
            }
        };
        assert_eq!(
            payload_value.get("type").and_then(|value| value.as_str()),
            Some("QuoteTick")
        );
        assert_eq!(decoded, quote);
        drop(publications);
        reset_message_bus();
    }

    fn assert_quote_round_trips(encoding: SerializationEncoding) {
        let publications = install_capturing_publisher(encoding);
        let quote = QuoteTick::default();

        publish_quote("data.quotes.TEST".into(), &quote);

        let bus_message = {
            let publications = publications.borrow();
            assert_eq!(publications.len(), 1);
            assert_eq!(publications[0].payload_type, BusPayloadType::QuoteTick);
            assert_eq!(publications[0].encoding, encoding);
            BusMessage::with_str_topic(
                publications[0].topic.clone(),
                publications[0].encoding,
                publications[0].payload_type,
                publications[0].payload.clone(),
            )
        };
        publications.borrow_mut().clear();

        let received = Rc::new(RefCell::new(Vec::<QuoteTick>::new()));
        let received_handler = received.clone();
        let handler = TypedHandler::from(move |quote: &QuoteTick| {
            received_handler.borrow_mut().push(*quote);
        });
        subscribe_quotes("data.quotes.*".into(), handler, None);

        republish_transport_message(&bus_message).unwrap();

        assert_eq!(*received.borrow(), vec![quote]);
        assert!(
            publications.borrow().is_empty(),
            "republished message must not be forwarded back out externally"
        );
        reset_message_bus();
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn republish_transport_message_round_trips_quote(#[case] encoding: SerializationEncoding) {
        assert_quote_round_trips(encoding);
    }

    #[cfg(feature = "sbe")]
    #[rstest]
    fn republish_transport_message_round_trips_quote_sbe() {
        assert_quote_round_trips(SerializationEncoding::Sbe);
    }

    #[cfg(feature = "capnp")]
    #[rstest]
    fn republish_transport_message_round_trips_quote_capnp() {
        assert_quote_round_trips(SerializationEncoding::Capnp);
    }

    #[cfg(feature = "sbe")]
    #[rstest]
    fn publish_quote_sbe_forwards_decodable_payload_to_publisher() {
        let publications = install_capturing_publisher(SerializationEncoding::Sbe);
        let quote = QuoteTick::default();

        publish_quote("data.quotes.TEST".into(), &quote);

        let publications = publications.borrow();
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].topic, "data.quotes.TEST");
        assert_eq!(
            QuoteTick::from_sbe(&publications[0].payload)
                .expect("SBE payload must decode as QuoteTick"),
            quote
        );
        drop(publications);
        reset_message_bus();
    }

    #[cfg(not(feature = "sbe"))]
    #[rstest]
    fn publish_quote_sbe_without_feature_drops_payload() {
        let publications = install_capturing_publisher(SerializationEncoding::Sbe);
        let quote = QuoteTick::default();

        publish_quote("data.quotes.TEST".into(), &quote);

        assert!(publications.borrow().is_empty());
        reset_message_bus();
    }

    #[cfg(feature = "capnp")]
    #[rstest]
    fn publish_quote_capnp_forwards_decodable_payload_to_publisher() {
        let publications = install_capturing_publisher(SerializationEncoding::Capnp);
        let quote = QuoteTick::default();

        publish_quote("data.quotes.TEST".into(), &quote);

        let publications = publications.borrow();
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].topic, "data.quotes.TEST");
        let reader = capnp::serialize::read_message(
            &mut &publications[0].payload[..],
            capnp::message::ReaderOptions::new(),
        )
        .expect("Cap'n Proto payload must be readable");
        let root = reader
            .get_root::<market_capnp::quote_tick::Reader>()
            .expect("Cap'n Proto payload must have a QuoteTick root");
        let decoded =
            QuoteTick::from_capnp(root).expect("Cap'n Proto payload must decode as QuoteTick");
        assert_eq!(decoded, quote);
        drop(publications);
        reset_message_bus();
    }

    #[cfg(not(feature = "capnp"))]
    #[rstest]
    fn publish_quote_capnp_without_feature_drops_payload() {
        let publications = install_capturing_publisher(SerializationEncoding::Capnp);
        let quote = QuoteTick::default();

        publish_quote("data.quotes.TEST".into(), &quote);

        assert!(publications.borrow().is_empty());
        reset_message_bus();
    }

    #[cfg(feature = "sbe")]
    #[rstest]
    fn unsupported_payload_under_sbe_is_classified_as_dropped() {
        let custom = stub_custom_data(100, 42, None, Some("stub-id".to_string()));

        let error = encode_publisher_payload(
            SerializationEncoding::Sbe,
            BusPayloadType::Custom(Ustr::from("StubCustomData")),
            &custom,
        )
        .expect_err("unsupported SBE payload must be dropped");

        assert!(matches!(error, PublisherPayloadError::Dropped(_)));
    }

    #[cfg(not(feature = "sbe"))]
    #[rstest]
    fn sbe_without_feature_is_classified_as_dropped() {
        let quote = QuoteTick::default();

        let error = encode_publisher_payload(
            SerializationEncoding::Sbe,
            BusPayloadType::QuoteTick,
            &quote,
        )
        .expect_err("SBE without feature must be dropped");

        assert!(matches!(error, PublisherPayloadError::Dropped(_)));
    }

    #[cfg(feature = "capnp")]
    #[rstest]
    fn unsupported_payload_under_capnp_is_classified_as_dropped() {
        let custom = stub_custom_data(100, 42, None, Some("stub-id".to_string()));

        let error = encode_publisher_payload(
            SerializationEncoding::Capnp,
            BusPayloadType::Custom(Ustr::from("StubCustomData")),
            &custom,
        )
        .expect_err("unsupported Cap'n Proto payload must be dropped");

        assert!(matches!(error, PublisherPayloadError::Dropped(_)));
    }

    #[cfg(not(feature = "capnp"))]
    #[rstest]
    fn capnp_without_feature_is_classified_as_dropped() {
        let quote = QuoteTick::default();

        let error = encode_publisher_payload(
            SerializationEncoding::Capnp,
            BusPayloadType::QuoteTick,
            &quote,
        )
        .expect_err("Cap'n Proto without feature must be dropped");

        assert!(matches!(error, PublisherPayloadError::Dropped(_)));
    }

    #[rstest]
    fn publish_quote_publisher_respects_filter_and_suppress_guard() {
        let publications = install_capturing_publisher(SerializationEncoding::MsgPack);
        let quote = QuoteTick::default();

        get_message_bus()
            .borrow_mut()
            .set_types_filter(vec!["QuoteTick".to_string()]);
        publish_quote("data.quotes.FILTERED".into(), &quote);

        get_message_bus().borrow_mut().set_types_filter(Vec::new());
        {
            let _guard = SuppressExternalGuard::new();
            publish_quote("data.quotes.SUPPRESSED".into(), &quote);
        }
        publish_quote("data.quotes.PUBLISHED".into(), &quote);

        let publications = publications.borrow();
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].topic, "data.quotes.PUBLISHED");
        drop(publications);
        reset_message_bus();
    }

    #[rstest]
    fn publish_quote_uses_market_data_encoding_override() {
        let publications = install_capturing_publisher_config(&MessageBusConfig {
            encoding: SerializationEncoding::Json,
            encoding_market_data: Some(SerializationEncoding::MsgPack),
            ..Default::default()
        });
        let quote = QuoteTick::default();

        publish_quote("data.quotes.TEST".into(), &quote);

        let publications = publications.borrow();
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].encoding, SerializationEncoding::MsgPack);
        assert_eq!(
            rmp_serde::from_slice::<QuoteTick>(&publications[0].payload)
                .expect("MsgPack payload must decode as QuoteTick"),
            quote
        );
        drop(publications);
        reset_message_bus();
    }

    #[rstest]
    fn publish_custom_data_forwards_envelope_to_publisher_and_respects_filter() {
        let publications = install_capturing_publisher(SerializationEncoding::Json);
        let custom = stub_custom_data(100, 42, None, Some("stub-id".to_string()));

        publish_any("data.custom.StubCustomData".into(), &custom);

        get_message_bus()
            .borrow_mut()
            .set_types_filter(vec!["StubCustomData".to_string()]);
        publish_any("data.custom.FILTERED".into(), &custom);

        let publications = publications.borrow();
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].topic, "data.custom.StubCustomData");

        let payload_value: serde_json::Value = serde_json::from_slice(&publications[0].payload)
            .expect("JSON payload must decode as a CustomData envelope");
        assert_eq!(
            payload_value.get("type").and_then(|value| value.as_str()),
            Some("StubCustomData")
        );
        assert_eq!(
            payload_value
                .pointer("/data_type/type_name")
                .and_then(|value| value.as_str()),
            Some("StubCustomData")
        );
        assert_eq!(
            payload_value
                .pointer("/data_type/identifier")
                .and_then(|value| value.as_str()),
            Some("stub-id")
        );
        assert_eq!(
            payload_value
                .pointer("/payload/value")
                .and_then(serde_json::Value::as_i64),
            Some(42)
        );
        drop(publications);
        reset_message_bus();
    }

    #[rstest]
    fn test_typed_quote_publish_subscribe_integration() {
        let msgbus = get_message_bus();
        let pub_count = msgbus.borrow().pub_count();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |quote: &QuoteTick| {
            received_clone.borrow_mut().push(*quote);
        });

        subscribe_quotes("data.quotes.*".into(), handler, None);

        let quote = QuoteTick::default();
        publish_quote("data.quotes.TEST".into(), &quote);
        publish_quote("data.quotes.TEST".into(), &quote);

        assert_eq!(received.borrow().len(), 2);
        assert_eq!(msgbus.borrow().pub_count(), pub_count + 2);
    }

    #[rstest]
    fn test_typed_trade_publish_subscribe_integration() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |trade: &TradeTick| {
            received_clone.borrow_mut().push(*trade);
        });

        subscribe_trades("data.trades.*".into(), handler, None);

        let trade = TradeTick::default();
        publish_trade("data.trades.TEST".into(), &trade);

        assert_eq!(received.borrow().len(), 1);
    }

    #[rstest]
    fn test_typed_bar_publish_subscribe_integration() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |bar: &Bar| {
            received_clone.borrow_mut().push(*bar);
        });

        subscribe_bars("data.bars.*".into(), handler, None);

        let bar = Bar::default();
        publish_bar("data.bars.TEST".into(), &bar);

        assert_eq!(received.borrow().len(), 1);
    }

    #[rstest]
    fn test_typed_deltas_publish_subscribe_integration() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |deltas: &OrderBookDeltas| {
            received_clone.borrow_mut().push(deltas.clone());
        });

        subscribe_book_deltas("data.book.deltas.*".into(), handler, None);

        let instrument_id = InstrumentId::from("TEST.VENUE");
        let delta = OrderBookDelta::clear(instrument_id, 0, 1.into(), 2.into());
        let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);
        publish_deltas("data.book.deltas.TEST".into(), &deltas);

        assert_eq!(received.borrow().len(), 1);
    }

    #[rstest]
    fn test_typed_unsubscribe_stops_delivery() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from_with_id("unsub-test", move |quote: &QuoteTick| {
            received_clone.borrow_mut().push(*quote);
        });

        subscribe_quotes("data.quotes.UNSUB".into(), handler.clone(), None);

        let quote = QuoteTick::default();
        publish_quote("data.quotes.UNSUB".into(), &quote);
        assert_eq!(received.borrow().len(), 1);

        unsubscribe_quotes("data.quotes.UNSUB".into(), &handler);

        publish_quote("data.quotes.UNSUB".into(), &quote);
        assert_eq!(received.borrow().len(), 1);
    }

    #[rstest]
    fn test_typed_wildcard_pattern_matching() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |quote: &QuoteTick| {
            received_clone.borrow_mut().push(*quote);
        });

        subscribe_quotes("data.quotes.WILD.*".into(), handler, None);

        let quote = QuoteTick::default();
        publish_quote("data.quotes.WILD.AAPL".into(), &quote);
        publish_quote("data.quotes.WILD.MSFT".into(), &quote);
        publish_quote("data.quotes.OTHER.AAPL".into(), &quote);

        assert_eq!(received.borrow().len(), 2);
    }

    #[rstest]
    fn test_typed_priority_ordering() {
        let _msgbus = get_message_bus();
        let order = Rc::new(RefCell::new(Vec::new()));

        let order1 = order.clone();
        let handler_low = TypedHandler::from_with_id("low-priority", move |_: &QuoteTick| {
            order1.borrow_mut().push("low");
        });

        let order2 = order.clone();
        let handler_high = TypedHandler::from_with_id("high-priority", move |_: &QuoteTick| {
            order2.borrow_mut().push("high");
        });

        subscribe_quotes("data.quotes.PRIO.*".into(), handler_low, Some(1));
        subscribe_quotes("data.quotes.PRIO.*".into(), handler_high, Some(10));

        let quote = QuoteTick::default();
        publish_quote("data.quotes.PRIO.TEST".into(), &quote);

        assert_eq!(*order.borrow(), vec!["high", "low"]);
    }

    #[rstest]
    fn test_typed_routing_isolation() {
        let _msgbus = get_message_bus();
        let quote_received = Rc::new(RefCell::new(false));
        let trade_received = Rc::new(RefCell::new(false));

        let qr = quote_received.clone();
        let quote_handler = TypedHandler::from(move |_: &QuoteTick| {
            *qr.borrow_mut() = true;
        });

        let tr = trade_received.clone();
        let trade_handler = TypedHandler::from(move |_: &TradeTick| {
            *tr.borrow_mut() = true;
        });

        subscribe_quotes("data.iso.*".into(), quote_handler, None);
        subscribe_trades("data.iso.*".into(), trade_handler, None);

        let quote = QuoteTick::default();
        publish_quote("data.iso.TEST".into(), &quote);

        assert!(*quote_received.borrow());
        assert!(!*trade_received.borrow());
    }

    #[rstest]
    fn test_send_data_allows_reentrant_topic_access() {
        use crate::msgbus::switchboard::get_quotes_topic;

        let _msgbus = get_message_bus();
        let topic_retrieved = Rc::new(RefCell::new(false));
        let topic_clone = topic_retrieved.clone();

        let handler = TypedIntoHandler::from(move |data: Data| {
            let instrument_id = data.instrument_id();
            let _topic = get_quotes_topic(instrument_id);
            *topic_clone.borrow_mut() = true;
        });

        let endpoint: MStr<Endpoint> = "ReentrantTest.data".into();
        register_data_endpoint(endpoint, handler);

        let quote = QuoteTick::default();
        send_data(endpoint, Data::Quote(quote));

        assert!(*topic_retrieved.borrow());
    }

    #[rstest]
    fn test_send_trading_command_allows_reentrant_topic_access() {
        use nautilus_model::{
            enums::OrderSide,
            identifiers::{StrategyId, TraderId},
        };

        use crate::{
            messages::execution::{TradingCommand, cancel::CancelAllOrders},
            msgbus::switchboard::get_trades_topic,
        };

        let _msgbus = get_message_bus();
        let topic_retrieved = Rc::new(RefCell::new(false));
        let topic_clone = topic_retrieved.clone();

        let handler = TypedIntoHandler::from(move |cmd: TradingCommand| {
            let instrument_id = cmd.instrument_id();
            let _topic = get_trades_topic(instrument_id);
            *topic_clone.borrow_mut() = true;
        });

        let endpoint: MStr<Endpoint> = "ReentrantTest.tradingCmd".into();
        register_trading_command_endpoint(endpoint, handler);

        let cmd = TradingCommand::CancelAllOrders(CancelAllOrders::new(
            TraderId::new("TESTER-001"),
            None,
            StrategyId::new("S-001"),
            InstrumentId::from("TEST.VENUE"),
            OrderSide::NoOrderSide,
            UUID4::new(),
            0.into(),
            None,
            None, // correlation_id
        ));
        send_trading_command(endpoint, cmd);

        assert!(*topic_retrieved.borrow());
    }

    #[rstest]
    fn test_send_account_state_allows_reentrant_topic_access() {
        use nautilus_model::{enums::AccountType, identifiers::AccountId, types::Currency};

        use crate::msgbus::switchboard::get_quotes_topic;

        let _msgbus = get_message_bus();
        let topic_retrieved = Rc::new(RefCell::new(false));
        let topic_clone = topic_retrieved.clone();

        let handler = TypedHandler::from(move |_state: &AccountState| {
            let instrument_id = InstrumentId::from("TEST.VENUE");
            let _topic = get_quotes_topic(instrument_id);
            *topic_clone.borrow_mut() = true;
        });

        let endpoint: MStr<Endpoint> = "ReentrantTest.accountState".into();
        register_account_state_endpoint(endpoint, handler);

        let state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::new(),
            0.into(),
            0.into(),
            Some(Currency::USD()),
        );
        send_account_state(endpoint, &state);

        assert!(*topic_retrieved.borrow());
    }

    #[rstest]
    fn test_send_order_event_allows_reentrant_topic_access() {
        use crate::msgbus::switchboard::get_quotes_topic;

        let _msgbus = get_message_bus();
        let topic_retrieved = Rc::new(RefCell::new(false));
        let topic_clone = topic_retrieved.clone();

        let handler = TypedIntoHandler::from(move |_event: OrderEventAny| {
            let instrument_id = InstrumentId::from("TEST.VENUE");
            let _topic = get_quotes_topic(instrument_id);
            *topic_clone.borrow_mut() = true;
        });

        let endpoint: MStr<Endpoint> = "ReentrantTest.orderEvent".into();
        register_order_event_endpoint(endpoint, handler);

        let event = OrderEventAny::Denied(OrderDeniedSpec::builder().build());
        send_order_event(endpoint, event);

        assert!(*topic_retrieved.borrow());
    }

    #[rstest]
    fn test_send_data_command_allows_reentrant_topic_access() {
        use crate::msgbus::switchboard::get_trades_topic;

        let msgbus = get_message_bus();
        let sent_count = msgbus.borrow().sent_count();
        let req_count = msgbus.borrow().req_count();
        let topic_retrieved = Rc::new(RefCell::new(false));
        let topic_clone = topic_retrieved.clone();

        let handler = TypedIntoHandler::from(move |_cmd: DataCommand| {
            let _topic = get_trades_topic(InstrumentId::from("TEST.VENUE"));
            *topic_clone.borrow_mut() = true;
        });

        let endpoint: MStr<Endpoint> = "ReentrantTest.dataCmd".into();
        register_data_command_endpoint(endpoint, handler);

        let cmd = DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
            InstrumentId::from("TEST.VENUE"),
            Some(ClientId::new("SIM")),
            None,
            UUID4::new(),
            0.into(),
            None,
            None,
        )));
        send_data_command(endpoint, cmd);

        assert!(*topic_retrieved.borrow());
        assert_eq!(msgbus.borrow().sent_count(), sent_count + 1);
        assert_eq!(msgbus.borrow().req_count(), req_count);

        let request = DataCommand::Request(RequestCommand::Quotes(RequestQuotes::new(
            InstrumentId::from("TEST.VENUE"),
            None,
            None,
            None,
            Some(ClientId::new("SIM")),
            UUID4::new(),
            0.into(),
            None,
        )));
        send_data_command(endpoint, request);

        assert_eq!(msgbus.borrow().sent_count(), sent_count + 2);
        assert_eq!(msgbus.borrow().req_count(), req_count + 1);
    }

    #[rstest]
    fn test_send_data_request_without_endpoint_does_not_increment_counts() {
        let msgbus = get_message_bus();
        let sent_count = msgbus.borrow().sent_count();
        let req_count = msgbus.borrow().req_count();

        let request = DataCommand::Request(RequestCommand::Quotes(RequestQuotes::new(
            InstrumentId::from("MISSING.VENUE"),
            None,
            None,
            None,
            Some(ClientId::new("SIM")),
            UUID4::new(),
            0.into(),
            None,
        )));
        send_data_command("Missing.dataCmd".into(), request);

        assert_eq!(msgbus.borrow().sent_count(), sent_count);
        assert_eq!(msgbus.borrow().req_count(), req_count);
    }

    #[rstest]
    fn test_send_data_response_allows_reentrant_topic_access() {
        use nautilus_model::identifiers::ClientId;

        use crate::{
            messages::data::{DataResponse, QuotesResponse},
            msgbus::switchboard::get_quotes_topic,
        };

        let _msgbus = get_message_bus();
        let topic_retrieved = Rc::new(RefCell::new(false));
        let topic_clone = topic_retrieved.clone();

        let handler = TypedIntoHandler::from(move |_resp: DataResponse| {
            let _topic = get_quotes_topic(InstrumentId::from("TEST.VENUE"));
            *topic_clone.borrow_mut() = true;
        });

        let endpoint: MStr<Endpoint> = "ReentrantTest.dataResp".into();
        register_data_response_endpoint(endpoint, handler);

        let resp = DataResponse::Quotes(QuotesResponse {
            correlation_id: UUID4::new(),
            client_id: ClientId::new("SIM"),
            instrument_id: InstrumentId::from("TEST.VENUE"),
            data: vec![],
            start: None,
            end: None,
            ts_init: 0.into(),
            params: None,
        });
        send_data_response(endpoint, resp);

        assert!(*topic_retrieved.borrow());
    }

    #[rstest]
    fn test_send_response_increments_response_count() {
        use nautilus_model::identifiers::ClientId;

        use crate::messages::data::{DataResponse, QuotesResponse};

        let msgbus = get_message_bus();
        let res_count = msgbus.borrow().res_count();
        let resp = DataResponse::Quotes(QuotesResponse {
            correlation_id: UUID4::new(),
            client_id: ClientId::new("SIM"),
            instrument_id: InstrumentId::from("TEST.VENUE"),
            data: vec![],
            start: None,
            end: None,
            ts_init: 0.into(),
            params: None,
        });

        send_response(&UUID4::new(), &resp);

        assert_eq!(msgbus.borrow().res_count(), res_count + 1);
    }

    #[rstest]
    fn test_send_execution_report_allows_reentrant_topic_access() {
        use nautilus_model::{
            identifiers::{AccountId, ClientId, Venue},
            reports::ExecutionMassStatus,
        };

        use crate::{messages::execution::ExecutionReport, msgbus::switchboard::get_trades_topic};

        let _msgbus = get_message_bus();
        let topic_retrieved = Rc::new(RefCell::new(false));
        let topic_clone = topic_retrieved.clone();

        let handler = TypedIntoHandler::from(move |_report: ExecutionReport| {
            let _topic = get_trades_topic(InstrumentId::from("TEST.VENUE"));
            *topic_clone.borrow_mut() = true;
        });

        let endpoint: MStr<Endpoint> = "ReentrantTest.execReport".into();
        register_execution_report_endpoint(endpoint, handler);

        let report = ExecutionReport::MassStatus(Box::new(ExecutionMassStatus::new(
            ClientId::new("SIM"),
            AccountId::new("SIM-001"),
            Venue::new("TEST"),
            0.into(),
            None,
        )));
        send_execution_report(endpoint, report);

        assert!(*topic_retrieved.borrow());
    }

    #[rstest]
    fn test_order_event_handler_can_send_trading_command() {
        // Tests that a handler processing an order event can send a trading command
        // without causing a borrow conflict. This simulates the scenario where a
        // strategy's on_order_accepted() handler calls cancel_order().
        let _msgbus = get_message_bus();
        let command_sent = Rc::new(RefCell::new(false));
        let command_sent_clone = command_sent.clone();

        let cmd_received = Rc::new(RefCell::new(false));
        let cmd_received_clone = cmd_received.clone();
        let cmd_handler = TypedIntoHandler::from(move |_cmd: TradingCommand| {
            *cmd_received_clone.borrow_mut() = true;
        });
        let cmd_endpoint: MStr<Endpoint> = "ReentrantTest.execCmd".into();
        register_trading_command_endpoint(cmd_endpoint, cmd_handler);

        let event_handler = TypedIntoHandler::from(move |_event: OrderEventAny| {
            // Simulate strategy calling cancel_order from on_order_accepted
            let command = TradingCommand::CancelAllOrders(CancelAllOrders::new(
                TraderId::new("TESTER-001"),
                None,
                StrategyId::new("S-001"),
                InstrumentId::from("TEST.VENUE"),
                OrderSide::Buy,
                UUID4::new(),
                0.into(),
                None,
                None, // correlation_id
            ));
            send_trading_command(cmd_endpoint, command);
            *command_sent_clone.borrow_mut() = true;
        });

        let event_endpoint: MStr<Endpoint> = "ReentrantTest.orderEvt".into();
        register_order_event_endpoint(event_endpoint, event_handler);

        let event = OrderEventAny::Denied(OrderDeniedSpec::builder().build());
        send_order_event(event_endpoint, event);

        assert!(
            *command_sent.borrow(),
            "Order event handler should have run"
        );
        assert!(
            *cmd_received.borrow(),
            "Trading command should have been received"
        );
    }

    #[rstest]
    fn test_data_handler_can_send_data_command() {
        // Tests that a handler processing data can send a data command
        // without causing a borrow conflict.
        let _msgbus = get_message_bus();
        let command_sent = Rc::new(RefCell::new(false));
        let command_sent_clone = command_sent.clone();

        let cmd_received = Rc::new(RefCell::new(false));
        let cmd_received_clone = cmd_received.clone();
        let cmd_handler = TypedIntoHandler::from(move |_cmd: DataCommand| {
            *cmd_received_clone.borrow_mut() = true;
        });
        let cmd_endpoint: MStr<Endpoint> = "ReentrantTest.dataCmd2".into();
        register_data_command_endpoint(cmd_endpoint, cmd_handler);

        let data_handler = TypedIntoHandler::from(move |_data: Data| {
            let command = DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
                InstrumentId::from("TEST.VENUE"),
                Some(ClientId::new("SIM")),
                None,
                UUID4::new(),
                0.into(),
                None,
                None,
            )));
            send_data_command(cmd_endpoint, command);
            *command_sent_clone.borrow_mut() = true;
        });

        let data_endpoint: MStr<Endpoint> = "ReentrantTest.data2".into();
        register_data_endpoint(data_endpoint, data_handler);

        let quote = QuoteTick::default();
        send_data(data_endpoint, Data::Quote(quote));

        assert!(*command_sent.borrow(), "Data handler should have run");
        assert!(
            *cmd_received.borrow(),
            "Data command should have been received"
        );
    }

    #[rstest]
    fn test_trading_command_handler_can_send_order_event() {
        // Tests that a handler processing a trading command can send an order event
        // without causing a borrow conflict. This is the reverse direction of the
        // common re-entrancy scenario.
        let _msgbus = get_message_bus();
        let event_sent = Rc::new(RefCell::new(false));
        let event_sent_clone = event_sent.clone();

        let evt_received = Rc::new(RefCell::new(false));
        let evt_received_clone = evt_received.clone();
        let evt_handler = TypedIntoHandler::from(move |_event: OrderEventAny| {
            *evt_received_clone.borrow_mut() = true;
        });
        let evt_endpoint: MStr<Endpoint> = "ReentrantTest.orderEvt2".into();
        register_order_event_endpoint(evt_endpoint, evt_handler);

        let cmd_handler = TypedIntoHandler::from(move |_cmd: TradingCommand| {
            let event = OrderEventAny::Denied(OrderDeniedSpec::builder().build());
            send_order_event(evt_endpoint, event);
            *event_sent_clone.borrow_mut() = true;
        });

        let cmd_endpoint: MStr<Endpoint> = "ReentrantTest.execCmd2".into();
        register_trading_command_endpoint(cmd_endpoint, cmd_handler);

        let command = TradingCommand::CancelAllOrders(CancelAllOrders::new(
            TraderId::new("TESTER-001"),
            None,
            StrategyId::new("S-001"),
            InstrumentId::from("TEST.VENUE"),
            OrderSide::Buy,
            UUID4::new(),
            0.into(),
            None,
            None, // correlation_id
        ));
        send_trading_command(cmd_endpoint, command);

        assert!(
            *event_sent.borrow(),
            "Trading command handler should have run"
        );
        assert!(
            *evt_received.borrow(),
            "Order event should have been received"
        );
    }

    #[rstest]
    fn test_nested_reentrant_calls() {
        // Tests deeply nested re-entrant calls: order event -> trading command -> order event.
        // This simulates a complex scenario where handlers chain multiple calls.
        let _msgbus = get_message_bus();
        let call_depth = Rc::new(RefCell::new(0u32));

        let final_received = Rc::new(RefCell::new(false));
        let final_received_clone = final_received.clone();
        let final_evt_handler = TypedIntoHandler::from(move |_event: OrderEventAny| {
            *final_received_clone.borrow_mut() = true;
        });
        let final_evt_endpoint: MStr<Endpoint> = "ReentrantTest.finalEvt".into();
        register_order_event_endpoint(final_evt_endpoint, final_evt_handler);

        let call_depth_clone2 = call_depth.clone();
        let mid_cmd_handler = TypedIntoHandler::from(move |_cmd: TradingCommand| {
            *call_depth_clone2.borrow_mut() += 1;
            let event = OrderEventAny::Denied(OrderDeniedSpec::builder().build());
            send_order_event(final_evt_endpoint, event);
        });
        let mid_cmd_endpoint: MStr<Endpoint> = "ReentrantTest.midCmd".into();
        register_trading_command_endpoint(mid_cmd_endpoint, mid_cmd_handler);

        let call_depth_clone1 = call_depth.clone();
        let init_evt_handler = TypedIntoHandler::from(move |_event: OrderEventAny| {
            *call_depth_clone1.borrow_mut() += 1;
            let command = TradingCommand::CancelAllOrders(CancelAllOrders::new(
                TraderId::new("TESTER-001"),
                None,
                StrategyId::new("S-001"),
                InstrumentId::from("TEST.VENUE"),
                OrderSide::Buy,
                UUID4::new(),
                0.into(),
                None,
                None, // correlation_id
            ));
            send_trading_command(mid_cmd_endpoint, command);
        });
        let init_evt_endpoint: MStr<Endpoint> = "ReentrantTest.initEvt".into();
        register_order_event_endpoint(init_evt_endpoint, init_evt_handler);

        let event = OrderEventAny::Denied(OrderDeniedSpec::builder().build());
        send_order_event(init_evt_endpoint, event);

        assert_eq!(
            *call_depth.borrow(),
            2,
            "Both intermediate handlers should have run"
        );
        assert!(
            *final_received.borrow(),
            "Final event handler should have received the event"
        );
    }

    /// Recording tap used by the bus-tap registration tests. Stores every dispatched
    /// topic / endpoint plus a digest of the message so a test can assert the tap
    /// observed the exact dispatches it expected.
    #[derive(Default)]
    struct RecordingTap {
        publishes: RefCell<Vec<(String, std::any::TypeId)>>,
        sends: RefCell<Vec<(String, std::any::TypeId)>>,
        responses: RefCell<Vec<(UUID4, std::any::TypeId)>>,
    }

    impl RecordingTap {
        fn publish_topics(&self) -> Vec<String> {
            self.publishes
                .borrow()
                .iter()
                .map(|(t, _)| t.clone())
                .collect()
        }

        fn send_endpoints(&self) -> Vec<String> {
            self.sends.borrow().iter().map(|(e, _)| e.clone()).collect()
        }

        fn response_correlation_ids(&self) -> Vec<UUID4> {
            self.responses.borrow().iter().map(|(id, _)| *id).collect()
        }
    }

    impl BusTap for RecordingTap {
        fn on_publish(&self, topic: MStr<Topic>, message: &dyn std::any::Any) {
            self.publishes
                .borrow_mut()
                .push((topic.to_string(), message.type_id()));
        }

        fn on_send(&self, endpoint: MStr<Endpoint>, message: &dyn std::any::Any) {
            self.sends
                .borrow_mut()
                .push((endpoint.to_string(), message.type_id()));
        }

        fn on_response(&self, correlation_id: &UUID4, message: &dyn std::any::Any) {
            self.responses
                .borrow_mut()
                .push((*correlation_id, message.type_id()));
        }
    }

    #[rstest]
    fn try_publish_any_dispatches_handler_and_tap() {
        let msgbus = Rc::new(RefCell::new(MessageBus::default()));
        set_message_bus(msgbus.clone());
        clear_bus_tap();

        let tap = Rc::new(RecordingTap::default());
        set_bus_tap(tap.clone());

        let topic = "data.any.try.test";
        let (handler, checker) = get_call_check_handler(None);
        let pub_count = msgbus.borrow().pub_count();
        subscribe_any(topic.into(), handler, None);

        let payload: u32 = 42;
        let published = try_publish_any(topic.into(), &payload);

        clear_bus_tap();

        assert!(published);
        assert!(checker.was_called());
        assert_eq!(msgbus.borrow().pub_count(), pub_count + 1);
        assert_eq!(tap.publish_topics(), vec![topic]);
    }

    #[rstest]
    fn try_publish_any_without_registered_bus_returns_false() {
        let published = thread::spawn(|| {
            let payload: u32 = 42;
            try_publish_any("data.any.no-bus.test".into(), &payload)
        })
        .join()
        .expect("thread should join");

        assert!(!published);
    }

    #[rstest]
    fn try_publish_any_with_borrowed_bus_returns_false_without_tap() {
        let msgbus = Rc::new(RefCell::new(MessageBus::default()));
        set_message_bus(msgbus.clone());
        clear_bus_tap();

        let tap = Rc::new(RecordingTap::default());
        set_bus_tap(tap.clone());

        let bus_borrow = msgbus.borrow_mut();
        let payload: u32 = 42;
        let published = try_publish_any("data.any.borrowed.test".into(), &payload);
        drop(bus_borrow);

        clear_bus_tap();

        assert!(!published);
        assert_eq!(msgbus.borrow().pub_count(), 0);
        assert!(tap.publish_topics().is_empty());
    }

    #[rstest]
    fn set_bus_tap_then_publish_typed_invokes_tap() {
        let _msgbus = get_message_bus();
        let tap = Rc::new(RecordingTap::default());
        set_bus_tap(tap.clone());

        let quote = QuoteTick::default();
        publish_quote("data.quotes.tap.test".into(), &quote);

        clear_bus_tap();

        assert_eq!(tap.publish_topics(), vec!["data.quotes.tap.test"]);
    }

    #[rstest]
    fn set_bus_tap_then_publish_any_invokes_tap() {
        let _msgbus = get_message_bus();
        let tap = Rc::new(RecordingTap::default());
        set_bus_tap(tap.clone());

        let payload: u32 = 42;
        publish_any("data.any.tap.test".into(), &payload);

        clear_bus_tap();

        assert_eq!(tap.publish_topics(), vec!["data.any.tap.test"]);
    }

    #[rstest]
    fn set_bus_tap_then_send_any_value_invokes_tap() {
        let _msgbus = get_message_bus();
        let tap = Rc::new(RecordingTap::default());
        set_bus_tap(tap.clone());

        let payload: u32 = 7;
        send_any_value("endpoint.send.any.value.test".into(), &payload);

        clear_bus_tap();

        assert_eq!(tap.send_endpoints(), vec!["endpoint.send.any.value.test"],);
    }

    #[rstest]
    fn set_bus_tap_then_send_endpoint_owned_invokes_tap() {
        // send_trading_command (and the other owned send helpers) reach the tap
        // through send_endpoint_owned_counted. Without this site instrumented, real
        // production order commands would bypass the audit log.
        let _msgbus = get_message_bus();
        let tap = Rc::new(RecordingTap::default());
        set_bus_tap(tap.clone());

        let cancel_all = CancelAllOrders::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("BINANCE")),
            StrategyId::from("S-001"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            OrderSide::Buy,
            UUID4::new(),
            nautilus_core::UnixNanos::from(1),
            None,
            None, // correlation_id
        );
        send_trading_command(
            "endpoint.send.trading.command.test".into(),
            TradingCommand::CancelAllOrders(cancel_all),
        );

        clear_bus_tap();

        assert_eq!(
            tap.send_endpoints(),
            vec!["endpoint.send.trading.command.test"],
        );
    }

    #[rstest]
    fn set_bus_tap_then_send_endpoint_ref_invokes_tap() {
        // send_quote (and the other typed-ref send helpers) reach the tap through
        // send_endpoint_ref. Mirrors the owned path coverage.
        let _msgbus = get_message_bus();
        let tap = Rc::new(RecordingTap::default());
        set_bus_tap(tap.clone());

        let quote = QuoteTick::default();
        send_quote("endpoint.send.quote.test".into(), &quote);

        clear_bus_tap();

        assert_eq!(tap.send_endpoints(), vec!["endpoint.send.quote.test"]);
    }

    #[rstest]
    fn has_quote_endpoint_returns_registration_state() {
        let _msgbus = get_message_bus();
        let endpoint: MStr<Endpoint> = "endpoint.has.quote.registered".into();

        assert!(!has_quote_endpoint(endpoint));

        let handler = TypedHandler::from_with_id(endpoint, |_quote: &QuoteTick| {});
        register_quote_endpoint(endpoint, handler);

        assert!(has_quote_endpoint(endpoint));
    }

    #[rstest]
    fn set_bus_tap_then_send_response_invokes_tap() {
        let _msgbus = get_message_bus();
        let tap = Rc::new(RecordingTap::default());
        set_bus_tap(tap.clone());

        let correlation_id = UUID4::new();
        let handler_called = Rc::new(RefCell::new(false));
        let handler_called_clone = handler_called.clone();
        register_response_handler(
            &correlation_id,
            ShareableMessageHandler::from_typed(move |_resp: &QuotesResponse| {
                *handler_called_clone.borrow_mut() = true;
            }),
        );

        let response = DataResponse::Quotes(QuotesResponse {
            correlation_id,
            client_id: ClientId::new("SIM"),
            instrument_id: InstrumentId::from("TEST.VENUE"),
            data: vec![],
            start: None,
            end: None,
            ts_init: 0.into(),
            params: None,
        });
        send_response(&correlation_id, &response);

        clear_bus_tap();

        assert!(*handler_called.borrow());
        assert_eq!(tap.response_correlation_ids(), vec![correlation_id]);
    }

    #[rstest]
    fn clear_bus_tap_prevents_subsequent_dispatches_from_invoking_tap() {
        let _msgbus = get_message_bus();
        let tap = Rc::new(RecordingTap::default());
        set_bus_tap(tap.clone());
        clear_bus_tap();

        let quote = QuoteTick::default();
        publish_quote("data.quotes.after.clear".into(), &quote);
        send_quote("endpoint.send.after.clear".into(), &quote);

        let correlation_id = UUID4::new();
        register_response_handler(
            &correlation_id,
            ShareableMessageHandler::from_typed(|_resp: &QuotesResponse| {}),
        );
        let response = DataResponse::Quotes(QuotesResponse {
            correlation_id,
            client_id: ClientId::new("SIM"),
            instrument_id: InstrumentId::from("TEST.VENUE"),
            data: vec![],
            start: None,
            end: None,
            ts_init: 0.into(),
            params: None,
        });
        send_response(&correlation_id, &response);

        assert!(tap.publish_topics().is_empty());
        assert!(tap.send_endpoints().is_empty());
        assert!(tap.response_correlation_ids().is_empty());
    }

    #[rstest]
    fn dispatch_with_no_tap_installed_is_a_noop() {
        // A fresh thread starts with BUS_TAP=None; dispatches must not panic and must
        // not allocate any tap state. Sanity check that the Option::None branch in
        // dispatch_tap_* is hit cleanly.
        let _msgbus = get_message_bus();

        let quote = QuoteTick::default();
        publish_quote("data.quotes.no.tap".into(), &quote);
        send_quote("endpoint.no.tap".into(), &quote);
    }

    struct ReinstallTap;

    impl BusTap for ReinstallTap {
        fn on_publish(&self, _topic: MStr<Topic>, _message: &dyn std::any::Any) {
            // Replace ourselves mid-dispatch; must not deadlock the RefCell
            set_bus_tap(Rc::new(NoopTap));
        }

        fn on_send(&self, _endpoint: MStr<Endpoint>, _message: &dyn std::any::Any) {}
    }

    struct NoopTap;

    impl BusTap for NoopTap {
        fn on_publish(&self, _topic: MStr<Topic>, _message: &dyn std::any::Any) {}
        fn on_send(&self, _endpoint: MStr<Endpoint>, _message: &dyn std::any::Any) {}
    }

    #[rstest]
    fn reentrant_set_bus_tap_during_dispatch_does_not_panic() {
        // dispatch_tap_publish clones the Rc out of BUS_TAP before invoking on_publish,
        // so a tap whose on_publish reinstalls a different tap must not panic on the
        // cell. The replaced tap stays alive through the cloned Rc until the dispatch
        // returns.
        let _msgbus = get_message_bus();
        set_bus_tap(Rc::new(ReinstallTap));

        let quote = QuoteTick::default();
        publish_quote("data.quotes.reentrant".into(), &quote);

        clear_bus_tap();
    }
}
