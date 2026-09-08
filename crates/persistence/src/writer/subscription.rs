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

//! Message-bus subscriptions for streaming writer sinks.

use std::{
    any::Any,
    cell::RefCell,
    fmt::Debug,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use nautilus_common::{
    clock::Clock,
    msgbus::{
        MStr, ShareableMessageHandler, TypedHandler, subscribe_account_state, subscribe_any,
        subscribe_bars, subscribe_book_deltas, subscribe_book_depth10 as subscribe_book_depths,
        subscribe_funding_rates, subscribe_index_prices, subscribe_instruments,
        subscribe_mark_prices, subscribe_option_greeks, subscribe_order_events,
        subscribe_position_events, subscribe_quotes, subscribe_trades, unsubscribe_account_state,
        unsubscribe_any, unsubscribe_bars, unsubscribe_book_deltas,
        unsubscribe_book_depth10 as unsubscribe_book_depths, unsubscribe_funding_rates,
        unsubscribe_index_prices, unsubscribe_instruments, unsubscribe_mark_prices,
        unsubscribe_option_greeks, unsubscribe_order_events, unsubscribe_position_events,
        unsubscribe_quotes, unsubscribe_trades,
    },
};
use nautilus_model::{
    data::{
        Bar, CustomData, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OptionGreeks,
        OrderBookDeltas, OrderBookDepth, QuoteTick, TradeTick,
    },
    events::{AccountState, OrderEventAny, PositionEvent},
    instruments::{Instrument, InstrumentAny},
};

use super::traits::StreamingSinkBox;

type ClockBridge = Option<(Rc<RefCell<dyn Clock>>, Arc<AtomicU64>)>;

/// Message-bus subscriptions forwarding supported messages into a streaming sink.
pub struct StreamingSinkSubscription {
    sink: Rc<RefCell<StreamingSinkBox>>,
    clock_bridge: ClockBridge,
    any_handler: ShareableMessageHandler,
    quotes_handler: TypedHandler<QuoteTick>,
    trades_handler: TypedHandler<TradeTick>,
    bars_handler: TypedHandler<Bar>,
    deltas_handler: TypedHandler<OrderBookDeltas>,
    depths_handler: TypedHandler<OrderBookDepth>,
    mark_prices_handler: TypedHandler<MarkPriceUpdate>,
    index_prices_handler: TypedHandler<IndexPriceUpdate>,
    funding_rates_handler: TypedHandler<FundingRateUpdate>,
    option_greeks_handler: TypedHandler<OptionGreeks>,
    instruments_handler: TypedHandler<InstrumentAny>,
    account_state_handler: TypedHandler<AccountState>,
    order_events_handler: TypedHandler<OrderEventAny>,
    position_events_handler: TypedHandler<PositionEvent>,
}

impl Debug for StreamingSinkSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(StreamingSinkSubscription))
            .finish_non_exhaustive()
    }
}

impl StreamingSinkSubscription {
    /// Subscribes a streaming sink to typed and dynamic message-bus routes.
    pub fn subscribe(sink: Rc<RefCell<StreamingSinkBox>>, clock_bridge: ClockBridge) -> Self {
        Self::subscribe_named(sink, clock_bridge, "streaming writer".to_string())
    }

    /// Subscribes a sink and includes its name in write-failure diagnostics.
    pub fn subscribe_named(
        sink: Rc<RefCell<StreamingSinkBox>>,
        clock_bridge: ClockBridge,
        name: String,
    ) -> Self {
        let name: Rc<str> = Rc::from(name);
        let any_handler = any_handler(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let quotes_handler =
            typed_handler::<QuoteTick>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let trades_handler =
            typed_handler::<TradeTick>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let bars_handler =
            typed_handler::<Bar>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let deltas_handler =
            typed_handler::<OrderBookDeltas>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let depths_handler =
            typed_handler::<OrderBookDepth>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let mark_prices_handler =
            typed_handler::<MarkPriceUpdate>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let index_prices_handler =
            typed_handler::<IndexPriceUpdate>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let funding_rates_handler = typed_handler::<FundingRateUpdate>(
            Rc::clone(&sink),
            clock_bridge.clone(),
            name.clone(),
        );
        let option_greeks_handler =
            typed_handler::<OptionGreeks>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let instruments_handler =
            typed_handler::<InstrumentAny>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let account_state_handler =
            typed_handler::<AccountState>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let order_events_handler =
            typed_handler::<OrderEventAny>(Rc::clone(&sink), clock_bridge.clone(), name.clone());
        let position_events_handler =
            typed_handler::<PositionEvent>(Rc::clone(&sink), clock_bridge.clone(), name);
        let pattern = MStr::pattern("*");
        // Order events publish once on `events.order.{strategy_id}` and fills re-publish on
        // `events.order_filled.{instrument_id}`; a `*` pattern captures both and duplicates
        // every fill row, so the sink subscribes to the strategy-scoped topic only.
        let order_events_pattern = MStr::pattern("events.order.*");

        subscribe_any(pattern, any_handler.clone(), None);
        subscribe_quotes(pattern, quotes_handler.clone(), None);
        subscribe_trades(pattern, trades_handler.clone(), None);
        subscribe_bars(pattern, bars_handler.clone(), None);
        subscribe_book_deltas(pattern, deltas_handler.clone(), None);
        subscribe_book_depths(pattern, depths_handler.clone(), None);
        subscribe_mark_prices(pattern, mark_prices_handler.clone(), None);
        subscribe_index_prices(pattern, index_prices_handler.clone(), None);
        subscribe_funding_rates(pattern, funding_rates_handler.clone(), None);
        subscribe_option_greeks(pattern, option_greeks_handler.clone(), None);
        subscribe_instruments(pattern, instruments_handler.clone(), None);
        subscribe_account_state(pattern, account_state_handler.clone(), None);
        subscribe_order_events(order_events_pattern, order_events_handler.clone(), None);
        subscribe_position_events(pattern, position_events_handler.clone(), None);

        Self {
            sink,
            clock_bridge,
            any_handler,
            quotes_handler,
            trades_handler,
            bars_handler,
            deltas_handler,
            depths_handler,
            mark_prices_handler,
            index_prices_handler,
            funding_rates_handler,
            option_greeks_handler,
            instruments_handler,
            account_state_handler,
            order_events_handler,
            position_events_handler,
        }
    }

    /// Unsubscribes and closes the sink.
    /// # Errors
    ///
    /// Returns an error if the underlying sink cannot be closed.
    pub fn close(&self) -> anyhow::Result<()> {
        let pattern = MStr::pattern("*");
        let order_events_pattern = MStr::pattern("events.order.*");

        unsubscribe_any(pattern, &self.any_handler);
        unsubscribe_quotes(pattern, &self.quotes_handler);
        unsubscribe_trades(pattern, &self.trades_handler);
        unsubscribe_bars(pattern, &self.bars_handler);
        unsubscribe_book_deltas(pattern, &self.deltas_handler);
        unsubscribe_book_depths(pattern, &self.depths_handler);
        unsubscribe_mark_prices(pattern, &self.mark_prices_handler);
        unsubscribe_index_prices(pattern, &self.index_prices_handler);
        unsubscribe_funding_rates(pattern, &self.funding_rates_handler);
        unsubscribe_option_greeks(pattern, &self.option_greeks_handler);
        unsubscribe_instruments(pattern, &self.instruments_handler);
        unsubscribe_account_state(pattern, &self.account_state_handler);
        unsubscribe_order_events(order_events_pattern, &self.order_events_handler);
        unsubscribe_position_events(pattern, &self.position_events_handler);

        refresh_writer_clock(&self.clock_bridge);
        self.sink.borrow_mut().close()
    }
}

fn typed_handler<T: 'static>(
    sink: Rc<RefCell<StreamingSinkBox>>,
    clock_bridge: ClockBridge,
    name: Rc<str>,
) -> TypedHandler<T> {
    TypedHandler::from(move |message: &T| {
        write_bus_message(&sink, &clock_bridge, &name, message);
    })
}

fn any_handler(
    sink: Rc<RefCell<StreamingSinkBox>>,
    clock_bridge: ClockBridge,
    name: Rc<str>,
) -> ShareableMessageHandler {
    ShareableMessageHandler::from_any(move |message: &dyn Any| {
        write_bus_message(&sink, &clock_bridge, &name, message);
    })
}

fn write_bus_message(
    sink: &Rc<RefCell<StreamingSinkBox>>,
    clock_bridge: &ClockBridge,
    name: &str,
    message: &dyn Any,
) {
    refresh_writer_clock(clock_bridge);

    if let Err(e) = sink.borrow_mut().write_any(message) {
        log::warn!(
            "Failed to write {name} {} message: {e}",
            streaming_message_label(message)
        );
    }
}

fn refresh_writer_clock(clock_bridge: &ClockBridge) {
    if let Some((clock, shared)) = clock_bridge {
        shared.store(clock.borrow().timestamp_ns().as_u64(), Ordering::Relaxed);
    }
}

fn streaming_message_label(message: &dyn Any) -> String {
    if let Some(custom) = message.downcast_ref::<CustomData>() {
        return format!(
            "CustomData({}, identifier={:?})",
            custom.data.type_name(),
            custom.data_type.identifier()
        );
    }

    if let Some(instrument) = message.downcast_ref::<InstrumentAny>() {
        return format!("InstrumentAny({})", instrument.id());
    }

    if let Some(bar) = message.downcast_ref::<Bar>() {
        return format!("Bar({})", bar.bar_type);
    }

    if let Some(quotes) = message.downcast_ref::<QuoteTick>() {
        return format!("QuoteTick({})", quotes.instrument_id);
    }

    if let Some(trade) = message.downcast_ref::<TradeTick>() {
        return format!("TradeTick({})", trade.instrument_id);
    }

    "unknown".to_string()
}
