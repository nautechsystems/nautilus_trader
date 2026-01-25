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

//! Core message bus implementation.
//!
//! # Design decisions
//!
//! ## Why two routing mechanisms?
//!
//! The message bus provides typed and Any-based routing to balance performance
//! and flexibility:
//!
//! **Typed routing** optimizes for throughput on known data types:
//! - `TopicRouter<T>` for pub/sub, `EndpointMap<T>` for point-to-point.
//! - Handlers implement `Handler<T>`, receive `&T` directly.
//! - No runtime type checking enables inlining and static dispatch.
//! - Built-in routers: `QuoteTick`, `TradeTick`, `Bar`, `OrderBookDeltas`,
//!   `OrderBookDepth10`, `OrderEventAny`, `PositionEvent`, `AccountState`.
//!
//! **Any-based routing** provides flexibility for extensibility:
//! - `subscriptions`/`topics` maps with `ShareableMessageHandler`.
//! - Handlers implement `Handler<dyn Any>`, receive `&dyn Any`.
//! - Supports arbitrary message types without modifying the bus.
//! - Required for Python interop where types aren't known at compile time.
//!
//! ## Handler semantics
//!
//! **Typed handlers receive `&T` references:**
//! - Same message delivered to N handlers without cloning.
//! - Handler decides whether to clone (only if storing).
//! - Zero-cost for `Copy` types (`QuoteTick`, `TradeTick`, `Bar`).
//! - Efficient for large types (`OrderBookDeltas`).
//!
//! **Any-based handlers pay per-handler overhead:**
//! - Each handler receives `&dyn Any`, must downcast to `&T`.
//! - N handlers = N downcasts + N potential clones.
//! - Runtime type checking on every dispatch.
//!
//! ## Performance trade-off
//!
//! Typed routing is faster (see `benches/msgbus_typed.rs`, AMD Ryzen 9 7950X):
//!
//! | Scenario                    | Typed vs Any |
//! |-----------------------------|--------------|
//! | Handler dispatch (noop)     | ~10x faster  |
//! | Router with 5 subscribers   | ~3.5x faster |
//! | Router with 10 subscribers  | ~2x faster   |
//! | High volume (1M messages)   | ~7% faster   |
//!
//! Any-based routing pays for flexibility with runtime type checking. Use
//! typed routing for hot-path data; Any-based for custom types and Python.
//!
//! ## Routing paths are separate
//!
//! Typed and Any-based routing use separate data structures:
//! - `publish_quote` routes through `router_quotes`.
//! - `publish_any` routes through `topics`.
//!
//! Publishers and subscribers must use matching APIs. Mixing them causes
//! silent message loss.
//!
//! ## When to use each
//!
//! **Typed** (`publish_quote`, `subscribe_quotes`, etc.):
//! - Market data (quotes, trades, bars, order book updates).
//! - Order and position events.
//! - High-frequency data with known types.
//!
//! **Any-based** (`publish_any`, `subscribe_any`):
//! - Custom or user-defined data types.
//! - Low-frequency messages.
//! - Python callbacks.

use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    hash::{Hash, Hasher},
    rc::Rc,
};

use ahash::{AHashMap, AHashSet};
use indexmap::IndexMap;
use nautilus_core::{UUID4, correctness::FAILED};
use nautilus_model::{
    data::{
        Bar, Data, FundingRateUpdate, GreeksData, IndexPriceUpdate, MarkPriceUpdate,
        OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick,
    },
    events::{AccountState, OrderEventAny, PositionEvent},
    identifiers::TraderId,
    orderbook::OrderBook,
    orders::OrderAny,
    position::Position,
};
use smallvec::SmallVec;
use ustr::Ustr;

use super::{
    ShareableMessageHandler,
    matching::is_matching_backtracking,
    mstr::{Endpoint, MStr, Pattern, Topic},
    set_message_bus,
    switchboard::MessagingSwitchboard,
    typed_endpoints::{EndpointMap, IntoEndpointMap},
    typed_router::TopicRouter,
};
use crate::messages::{
    data::{DataCommand, DataResponse},
    execution::{ExecutionReport, TradingCommand},
};

/// Represents a subscription to a particular topic.
///
/// This is an internal class intended to be used by the message bus to organize
/// topics and their subscribers.
///
#[derive(Clone, Debug)]
pub struct Subscription {
    /// The shareable message handler for the subscription.
    pub handler: ShareableMessageHandler,
    /// Store a copy of the handler ID for faster equality checks.
    pub handler_id: Ustr,
    /// The pattern for the subscription.
    pub pattern: MStr<Pattern>,
    /// The priority for the subscription determines the ordering of handlers receiving
    /// messages being processed, higher priority handlers will receive messages before
    /// lower priority handlers.
    pub priority: u8,
}

impl Subscription {
    /// Creates a new [`Subscription`] instance.
    #[must_use]
    pub fn new(
        pattern: MStr<Pattern>,
        handler: ShareableMessageHandler,
        priority: Option<u8>,
    ) -> Self {
        Self {
            handler_id: handler.0.id(),
            pattern,
            handler,
            priority: priority.unwrap_or(0),
        }
    }
}

impl PartialEq<Self> for Subscription {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern && self.handler_id == other.handler_id
    }
}

impl Eq for Subscription {}

impl PartialOrd for Subscription {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Subscription {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| self.pattern.cmp(&other.pattern))
            .then_with(|| self.handler_id.cmp(&other.handler_id))
    }
}

impl Hash for Subscription {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pattern.hash(state);
        self.handler_id.hash(state);
    }
}

/// A generic message bus to facilitate various messaging patterns.
///
/// The bus provides both a producer and consumer API for Pub/Sub, Req/Rep, as
/// well as direct point-to-point messaging to registered endpoints.
///
/// Pub/Sub wildcard patterns for hierarchical topics are possible:
///  - `*` asterisk represents one or more characters in a pattern.
///  - `?` question mark represents a single character in a pattern.
///
/// Given a topic and pattern potentially containing wildcard characters, i.e.
/// `*` and `?`, where `?` can match any single character in the topic, and `*`
/// can match any number of characters including zero characters.
///
/// The asterisk in a wildcard matches any character zero or more times. For
/// example, `comp*` matches anything beginning with `comp` which means `comp`,
/// `complete`, and `computer` are all matched.
///
/// A question mark matches a single character once. For example, `c?mp` matches
/// `camp` and `comp`. The question mark can also be used more than once.
/// For example, `c??p` would match both of the above examples and `coop`.
#[derive(Debug)]
pub struct MessageBus {
    /// The trader ID associated with the message bus.
    pub trader_id: TraderId,
    /// The instance ID associated with the message bus.
    pub instance_id: UUID4,
    /// The name for the message bus.
    pub name: String,
    /// If the message bus is backed by a database.
    pub has_backing: bool,
    pub(crate) switchboard: MessagingSwitchboard,
    pub(crate) subscriptions: AHashSet<Subscription>,
    pub(crate) topics: IndexMap<MStr<Topic>, Vec<Subscription>>,
    pub(crate) endpoints: IndexMap<MStr<Endpoint>, ShareableMessageHandler>,
    pub(crate) correlation_index: AHashMap<UUID4, ShareableMessageHandler>,
    pub(crate) router_quotes: TopicRouter<QuoteTick>,
    pub(crate) router_trades: TopicRouter<TradeTick>,
    pub(crate) router_bars: TopicRouter<Bar>,
    pub(crate) router_deltas: TopicRouter<OrderBookDeltas>,
    pub(crate) router_depth10: TopicRouter<OrderBookDepth10>,
    pub(crate) router_book_snapshots: TopicRouter<OrderBook>,
    pub(crate) router_mark_prices: TopicRouter<MarkPriceUpdate>,
    pub(crate) router_index_prices: TopicRouter<IndexPriceUpdate>,
    pub(crate) router_funding_rates: TopicRouter<FundingRateUpdate>,
    pub(crate) router_order_events: TopicRouter<OrderEventAny>,
    pub(crate) router_position_events: TopicRouter<PositionEvent>,
    pub(crate) router_account_state: TopicRouter<AccountState>,
    pub(crate) router_orders: TopicRouter<OrderAny>,
    pub(crate) router_positions: TopicRouter<Position>,
    pub(crate) router_greeks: TopicRouter<GreeksData>,
    #[cfg(feature = "defi")]
    pub(crate) router_defi_blocks: TopicRouter<nautilus_model::defi::Block>, // nautilus-import-ok
    #[cfg(feature = "defi")]
    pub(crate) router_defi_pools: TopicRouter<nautilus_model::defi::Pool>, // nautilus-import-ok
    #[cfg(feature = "defi")]
    pub(crate) router_defi_swaps: TopicRouter<nautilus_model::defi::PoolSwap>, // nautilus-import-ok
    #[cfg(feature = "defi")]
    pub(crate) router_defi_liquidity: TopicRouter<nautilus_model::defi::PoolLiquidityUpdate>, // nautilus-import-ok
    #[cfg(feature = "defi")]
    pub(crate) router_defi_collects: TopicRouter<nautilus_model::defi::PoolFeeCollect>, // nautilus-import-ok
    #[cfg(feature = "defi")]
    pub(crate) router_defi_flash: TopicRouter<nautilus_model::defi::PoolFlash>, // nautilus-import-ok
    #[cfg(feature = "defi")]
    pub(crate) endpoints_defi_data: IntoEndpointMap<nautilus_model::defi::DefiData>, // nautilus-import-ok
    pub(crate) endpoints_quotes: EndpointMap<QuoteTick>,
    pub(crate) endpoints_trades: EndpointMap<TradeTick>,
    pub(crate) endpoints_bars: EndpointMap<Bar>,
    pub(crate) endpoints_account_state: EndpointMap<AccountState>,
    pub(crate) endpoints_trading_commands: IntoEndpointMap<TradingCommand>,
    pub(crate) endpoints_data_commands: IntoEndpointMap<DataCommand>,
    pub(crate) endpoints_data_responses: IntoEndpointMap<DataResponse>,
    pub(crate) endpoints_exec_reports: IntoEndpointMap<ExecutionReport>,
    pub(crate) endpoints_order_events: IntoEndpointMap<OrderEventAny>,
    pub(crate) endpoints_data: IntoEndpointMap<Data>,
    routers_typed: AHashMap<TypeId, Box<dyn Any>>,
    endpoints_typed: AHashMap<TypeId, Box<dyn Any>>,
}

impl Default for MessageBus {
    /// Creates a new default [`MessageBus`] instance.
    fn default() -> Self {
        Self::new(TraderId::from("TRADER-001"), UUID4::new(), None, None)
    }
}

impl MessageBus {
    /// Creates a new [`MessageBus`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        name: Option<String>,
        _config: Option<HashMap<String, serde_json::Value>>,
    ) -> Self {
        Self {
            trader_id,
            instance_id,
            name: name.unwrap_or(stringify!(MessageBus).to_owned()),
            switchboard: MessagingSwitchboard::default(),
            subscriptions: AHashSet::new(),
            topics: IndexMap::new(),
            endpoints: IndexMap::new(),
            correlation_index: AHashMap::new(),
            has_backing: false,
            router_quotes: TopicRouter::new(),
            router_trades: TopicRouter::new(),
            router_bars: TopicRouter::new(),
            router_deltas: TopicRouter::new(),
            router_depth10: TopicRouter::new(),
            router_book_snapshots: TopicRouter::new(),
            router_mark_prices: TopicRouter::new(),
            router_index_prices: TopicRouter::new(),
            router_funding_rates: TopicRouter::new(),
            router_order_events: TopicRouter::new(),
            router_position_events: TopicRouter::new(),
            router_account_state: TopicRouter::new(),
            router_orders: TopicRouter::new(),
            router_positions: TopicRouter::new(),
            router_greeks: TopicRouter::new(),
            #[cfg(feature = "defi")]
            router_defi_blocks: TopicRouter::new(),
            #[cfg(feature = "defi")]
            router_defi_pools: TopicRouter::new(),
            #[cfg(feature = "defi")]
            router_defi_swaps: TopicRouter::new(),
            #[cfg(feature = "defi")]
            router_defi_liquidity: TopicRouter::new(),
            #[cfg(feature = "defi")]
            router_defi_collects: TopicRouter::new(),
            #[cfg(feature = "defi")]
            router_defi_flash: TopicRouter::new(),
            #[cfg(feature = "defi")]
            endpoints_defi_data: IntoEndpointMap::new(),
            endpoints_quotes: EndpointMap::new(),
            endpoints_trades: EndpointMap::new(),
            endpoints_bars: EndpointMap::new(),
            endpoints_account_state: EndpointMap::new(),
            endpoints_trading_commands: IntoEndpointMap::new(),
            endpoints_data_commands: IntoEndpointMap::new(),
            endpoints_data_responses: IntoEndpointMap::new(),
            endpoints_exec_reports: IntoEndpointMap::new(),
            endpoints_order_events: IntoEndpointMap::new(),
            endpoints_data: IntoEndpointMap::new(),
            routers_typed: AHashMap::new(),
            endpoints_typed: AHashMap::new(),
        }
    }

    /// Registers message bus for the current thread.
    pub fn register_message_bus(self) -> Rc<RefCell<Self>> {
        let msgbus = Rc::new(RefCell::new(self));
        set_message_bus(msgbus.clone());
        msgbus
    }

    /// Gets or creates a typed router for custom message type `T`.
    ///
    /// # Panics
    ///
    /// Panics if the stored router type doesn't match `T` (internal bug).
    pub fn router<T: 'static>(&mut self) -> &mut TopicRouter<T> {
        self.routers_typed
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(TopicRouter::<T>::new()))
            .downcast_mut::<TopicRouter<T>>()
            .expect("TopicRouter type mismatch - this is a bug")
    }

    /// Gets or creates a typed endpoint map for custom message type `T`.
    ///
    /// # Panics
    ///
    /// Panics if the stored endpoint map type doesn't match `T` (internal bug).
    pub fn endpoint_map<T: 'static>(&mut self) -> &mut EndpointMap<T> {
        self.endpoints_typed
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(EndpointMap::<T>::new()))
            .downcast_mut::<EndpointMap<T>>()
            .expect("EndpointMap type mismatch - this is a bug")
    }

    /// Returns the memory address of this instance as a hexadecimal string.
    #[must_use]
    pub fn mem_address(&self) -> String {
        format!("{self:p}")
    }

    /// Returns a reference to the switchboard.
    #[must_use]
    pub fn switchboard(&self) -> &MessagingSwitchboard {
        &self.switchboard
    }

    /// Returns the registered endpoint addresses.
    #[must_use]
    pub fn endpoints(&self) -> Vec<&str> {
        self.endpoints.iter().map(|e| e.0.as_str()).collect()
    }

    /// Returns actively subscribed patterns.
    #[must_use]
    pub fn patterns(&self) -> Vec<&str> {
        self.subscriptions
            .iter()
            .map(|s| s.pattern.as_str())
            .collect()
    }

    /// Returns whether there are subscribers for the `topic`.
    pub fn has_subscribers<T: AsRef<str>>(&self, topic: T) -> bool {
        self.subscriptions_count(topic) > 0
    }

    /// Returns the count of subscribers for the `topic`.
    ///
    /// # Panics
    ///
    /// Returns an error if the topic is not valid.
    #[must_use]
    pub fn subscriptions_count<T: AsRef<str>>(&self, topic: T) -> usize {
        let topic = MStr::<Topic>::topic(topic).expect(FAILED);
        self.topics
            .get(&topic)
            .map_or_else(|| self.find_topic_matches(topic).len(), |subs| subs.len())
    }

    /// Returns active subscriptions.
    #[must_use]
    pub fn subscriptions(&self) -> Vec<&Subscription> {
        self.subscriptions.iter().collect()
    }

    /// Returns the handler IDs for actively subscribed patterns.
    #[must_use]
    pub fn subscription_handler_ids(&self) -> Vec<&str> {
        self.subscriptions
            .iter()
            .map(|s| s.handler_id.as_str())
            .collect()
    }

    /// Returns whether the endpoint is registered.
    ///
    /// # Panics
    ///
    /// Returns an error if the endpoint is not valid topic string.
    #[must_use]
    pub fn is_registered<T: Into<MStr<Endpoint>>>(&self, endpoint: T) -> bool {
        let endpoint: MStr<Endpoint> = endpoint.into();
        self.endpoints.contains_key(&endpoint)
    }

    /// Returns whether the `handler` is subscribed to the `pattern`.
    #[must_use]
    pub fn is_subscribed<T: AsRef<str>>(
        &self,
        pattern: T,
        handler: ShareableMessageHandler,
    ) -> bool {
        let pattern = MStr::<Pattern>::pattern(pattern);
        let sub = Subscription::new(pattern, handler, None);
        self.subscriptions.contains(&sub)
    }

    /// Close the message bus which will close the sender channel and join the thread.
    ///
    /// # Errors
    ///
    /// This function never returns an error (TBD once backing database added).
    pub const fn close(&self) -> anyhow::Result<()> {
        // TODO: Integrate the backing database
        Ok(())
    }

    /// Returns the handler for the `endpoint`.
    #[must_use]
    pub fn get_endpoint(&self, endpoint: MStr<Endpoint>) -> Option<&ShareableMessageHandler> {
        self.endpoints.get(&endpoint)
    }

    /// Returns the handler for the `correlation_id`.
    #[must_use]
    pub fn get_response_handler(&self, correlation_id: &UUID4) -> Option<&ShareableMessageHandler> {
        self.correlation_index.get(correlation_id)
    }

    /// Finds the subscriptions with pattern matching the `topic`.
    pub(crate) fn find_topic_matches(&self, topic: MStr<Topic>) -> Vec<Subscription> {
        self.subscriptions
            .iter()
            .filter_map(|sub| {
                if is_matching_backtracking(topic, sub.pattern) {
                    Some(sub.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Finds the subscriptions which match the `topic` and caches the
    /// results in the `patterns` map.
    #[must_use]
    pub fn matching_subscriptions<T: Into<MStr<Topic>>>(&mut self, topic: T) -> Vec<Subscription> {
        self.inner_matching_subscriptions(topic.into())
    }

    pub(crate) fn inner_matching_subscriptions(&mut self, topic: MStr<Topic>) -> Vec<Subscription> {
        self.topics.get(&topic).cloned().unwrap_or_else(|| {
            let mut matches = self.find_topic_matches(topic);
            matches.sort();
            self.topics.insert(topic, matches.clone());
            matches
        })
    }

    /// Fills a buffer with handlers matching a topic.
    pub(crate) fn fill_matching_any_handlers(
        &mut self,
        topic: MStr<Topic>,
        buf: &mut SmallVec<[ShareableMessageHandler; 64]>,
    ) {
        if let Some(subs) = self.topics.get(&topic) {
            for sub in subs {
                buf.push(sub.handler.clone());
            }
        } else {
            let mut matches = self.find_topic_matches(topic);
            matches.sort();

            for sub in &matches {
                buf.push(sub.handler.clone());
            }

            self.topics.insert(topic, matches);
        }
    }

    /// Registers a response handler for a specific correlation ID.
    ///
    /// # Errors
    ///
    /// Returns an error if `handler` is already registered for the `correlation_id`.
    pub fn register_response_handler(
        &mut self,
        correlation_id: &UUID4,
        handler: ShareableMessageHandler,
    ) -> anyhow::Result<()> {
        if self.correlation_index.contains_key(correlation_id) {
            anyhow::bail!("Correlation ID <{correlation_id}> already has a registered handler");
        }

        self.correlation_index.insert(*correlation_id, handler);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rand::{Rng, SeedableRng, rngs::StdRng};
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::msgbus::{
        self, ShareableMessageHandler, get_message_bus,
        matching::is_matching_backtracking,
        stubs::{get_call_check_handler, get_stub_shareable_handler},
        subscriptions_count_any,
    };

    #[rstest]
    fn test_new() {
        let trader_id = TraderId::default();
        let msgbus = MessageBus::new(trader_id, UUID4::new(), None, None);

        assert_eq!(msgbus.trader_id, trader_id);
        assert_eq!(msgbus.name, stringify!(MessageBus));
    }

    #[rstest]
    fn test_endpoints_when_no_endpoints() {
        let msgbus = get_message_bus();
        assert!(msgbus.borrow().endpoints().is_empty());
    }

    #[rstest]
    fn test_topics_when_no_subscriptions() {
        let msgbus = get_message_bus();
        assert!(msgbus.borrow().patterns().is_empty());
        assert!(!msgbus.borrow().has_subscribers("my-topic"));
    }

    #[rstest]
    fn test_is_subscribed_when_no_subscriptions() {
        let msgbus = get_message_bus();
        let handler = get_stub_shareable_handler(None);

        assert!(!msgbus.borrow().is_subscribed("my-topic", handler));
    }

    #[rstest]
    fn test_get_response_handler_when_no_handler() {
        let msgbus = get_message_bus();
        let msgbus_ref = msgbus.borrow();
        let handler = msgbus_ref.get_response_handler(&UUID4::new());
        assert!(handler.is_none());
    }

    #[rstest]
    fn test_get_response_handler_when_already_registered() {
        let msgbus = get_message_bus();
        let mut msgbus_ref = msgbus.borrow_mut();
        let handler = get_stub_shareable_handler(None);

        let request_id = UUID4::new();
        msgbus_ref
            .register_response_handler(&request_id, handler.clone())
            .unwrap();

        let result = msgbus_ref.register_response_handler(&request_id, handler);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_get_response_handler_when_registered() {
        let msgbus = get_message_bus();
        let mut msgbus_ref = msgbus.borrow_mut();
        let handler = get_stub_shareable_handler(None);

        let request_id = UUID4::new();
        msgbus_ref
            .register_response_handler(&request_id, handler)
            .unwrap();

        let handler = msgbus_ref.get_response_handler(&request_id).unwrap();
        assert_eq!(handler.id(), handler.id());
    }

    #[rstest]
    fn test_is_registered_when_no_registrations() {
        let msgbus = get_message_bus();
        assert!(!msgbus.borrow().is_registered("MyEndpoint"));
    }

    #[rstest]
    fn test_register_endpoint() {
        let msgbus = get_message_bus();
        let endpoint = "MyEndpoint".into();
        let handler = get_stub_shareable_handler(None);

        msgbus::register_any(endpoint, handler);

        assert_eq!(msgbus.borrow().endpoints(), vec![endpoint.to_string()]);
        assert!(msgbus.borrow().get_endpoint(endpoint).is_some());
    }

    #[rstest]
    fn test_endpoint_send() {
        let msgbus = get_message_bus();
        let endpoint = "MyEndpoint".into();
        let (handler, checker) = get_call_check_handler(None);

        msgbus::register_any(endpoint, handler);
        assert!(msgbus.borrow().get_endpoint(endpoint).is_some());
        assert!(!checker.was_called());

        // Send a message to the endpoint
        msgbus::send_any(endpoint, &"Test Message");
        assert!(checker.was_called());
    }

    #[rstest]
    fn test_deregsiter_endpoint() {
        let msgbus = get_message_bus();
        let endpoint = "MyEndpoint".into();
        let handler = get_stub_shareable_handler(None);

        msgbus::register_any(endpoint, handler);
        msgbus::deregister_any(endpoint);

        assert!(msgbus.borrow().endpoints().is_empty());
    }

    #[rstest]
    fn test_subscribe() {
        let msgbus = get_message_bus();
        let topic = "my-topic";
        let handler = get_stub_shareable_handler(None);

        msgbus::subscribe_any(topic.into(), handler, Some(1));

        assert!(msgbus.borrow().has_subscribers(topic));
        assert_eq!(msgbus.borrow().patterns(), vec![topic]);
    }

    #[rstest]
    fn test_unsubscribe() {
        let msgbus = get_message_bus();
        let topic = "my-topic";
        let handler = get_stub_shareable_handler(None);

        msgbus::subscribe_any(topic.into(), handler.clone(), None);
        msgbus::unsubscribe_any(topic.into(), handler);

        assert!(!msgbus.borrow().has_subscribers(topic));
        assert!(msgbus.borrow().patterns().is_empty());
    }

    #[rstest]
    fn test_matching_subscriptions() {
        let msgbus = get_message_bus();
        let pattern = "my-pattern";

        let handler_id1 = Ustr::from("1");
        let handler1 = get_stub_shareable_handler(Some(handler_id1));

        let handler_id2 = Ustr::from("2");
        let handler2 = get_stub_shareable_handler(Some(handler_id2));

        let handler_id3 = Ustr::from("3");
        let handler3 = get_stub_shareable_handler(Some(handler_id3));

        let handler_id4 = Ustr::from("4");
        let handler4 = get_stub_shareable_handler(Some(handler_id4));

        msgbus::subscribe_any(pattern.into(), handler1, None);
        msgbus::subscribe_any(pattern.into(), handler2, None);
        msgbus::subscribe_any(pattern.into(), handler3, Some(1));
        msgbus::subscribe_any(pattern.into(), handler4, Some(2));

        assert_eq!(
            msgbus.borrow().patterns(),
            vec![pattern, pattern, pattern, pattern]
        );
        assert_eq!(subscriptions_count_any(pattern), 4);

        let topic = pattern;
        let subs = msgbus.borrow_mut().matching_subscriptions(topic);
        assert_eq!(subs.len(), 4);
        assert_eq!(subs[0].handler_id, handler_id4);
        assert_eq!(subs[1].handler_id, handler_id3);
        assert_eq!(subs[2].handler_id, handler_id1);
        assert_eq!(subs[3].handler_id, handler_id2);
    }

    #[rstest]
    fn test_subscription_pattern_matching() {
        let msgbus = get_message_bus();
        let handler1 = get_stub_shareable_handler(Some(Ustr::from("1")));
        let handler2 = get_stub_shareable_handler(Some(Ustr::from("2")));
        let handler3 = get_stub_shareable_handler(Some(Ustr::from("3")));

        msgbus::subscribe_any("data.quotes.*".into(), handler1, None);
        msgbus::subscribe_any("data.trades.*".into(), handler2, None);
        msgbus::subscribe_any("data.*.BINANCE.*".into(), handler3, None);
        assert_eq!(msgbus.borrow().subscriptions().len(), 3);

        let topic = "data.quotes.BINANCE.ETHUSDT";
        assert_eq!(msgbus.borrow().find_topic_matches(topic.into()).len(), 2);

        let matches = msgbus.borrow_mut().matching_subscriptions(topic);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].handler_id, Ustr::from("3"));
        assert_eq!(matches[1].handler_id, Ustr::from("1"));
    }

    /// A simple reference model for subscription behavior.
    struct SimpleSubscriptionModel {
        /// Stores (pattern, handler_id) tuples for active subscriptions.
        subscriptions: Vec<(String, String)>,
    }

    impl SimpleSubscriptionModel {
        fn new() -> Self {
            Self {
                subscriptions: Vec::new(),
            }
        }

        fn subscribe(&mut self, pattern: &str, handler_id: &str) {
            let subscription = (pattern.to_string(), handler_id.to_string());
            if !self.subscriptions.contains(&subscription) {
                self.subscriptions.push(subscription);
            }
        }

        fn unsubscribe(&mut self, pattern: &str, handler_id: &str) -> bool {
            let subscription = (pattern.to_string(), handler_id.to_string());
            if let Some(idx) = self.subscriptions.iter().position(|s| s == &subscription) {
                self.subscriptions.remove(idx);
                true
            } else {
                false
            }
        }

        fn is_subscribed(&self, pattern: &str, handler_id: &str) -> bool {
            self.subscriptions
                .contains(&(pattern.to_string(), handler_id.to_string()))
        }

        fn matching_subscriptions(&self, topic: &str) -> Vec<(String, String)> {
            let topic = topic.into();

            self.subscriptions
                .iter()
                .filter(|(pat, _)| is_matching_backtracking(topic, pat.into()))
                .map(|(pat, id)| (pat.clone(), id.clone()))
                .collect()
        }

        fn subscription_count(&self) -> usize {
            self.subscriptions.len()
        }
    }

    #[rstest]
    fn subscription_model_fuzz_testing() {
        let mut rng = StdRng::seed_from_u64(42);

        let msgbus = get_message_bus();
        let mut model = SimpleSubscriptionModel::new();

        // Map from handler_id to handler
        let mut handlers: Vec<(String, ShareableMessageHandler)> = Vec::new();

        // Generate some patterns
        let patterns = generate_test_patterns(&mut rng);

        // Generate some handler IDs
        let handler_ids: Vec<String> = (0..50).map(|i| format!("handler_{i}")).collect();

        // Initialize handlers
        for id in &handler_ids {
            let handler = get_stub_shareable_handler(Some(Ustr::from(id)));
            handlers.push((id.clone(), handler));
        }

        let num_operations = 50_000;
        for op_num in 0..num_operations {
            let operation = rng.random_range(0..4);

            match operation {
                // Subscribe
                0 => {
                    let pattern_idx = rng.random_range(0..patterns.len());
                    let handler_idx = rng.random_range(0..handlers.len());
                    let pattern = &patterns[pattern_idx];
                    let (handler_id, handler) = &handlers[handler_idx];

                    // Apply to reference model
                    model.subscribe(pattern, handler_id);

                    // Apply to message bus
                    msgbus::subscribe_any(pattern.as_str().into(), handler.clone(), None);

                    assert_eq!(
                        model.subscription_count(),
                        msgbus.borrow().subscriptions().len()
                    );

                    assert!(
                        msgbus.borrow().is_subscribed(pattern, handler.clone()),
                        "Op {op_num}: is_subscribed should return true after subscribe"
                    );
                }

                // Unsubscribe
                1 => {
                    if model.subscription_count() > 0 {
                        let sub_idx = rng.random_range(0..model.subscription_count());
                        let (pattern, handler_id) = model.subscriptions[sub_idx].clone();

                        // Apply to reference model
                        model.unsubscribe(&pattern, &handler_id);

                        // Find handler
                        let handler = handlers
                            .iter()
                            .find(|(id, _)| id == &handler_id)
                            .map(|(_, h)| h.clone())
                            .unwrap();

                        // Apply to message bus
                        msgbus::unsubscribe_any(pattern.as_str().into(), handler.clone());

                        assert_eq!(
                            model.subscription_count(),
                            msgbus.borrow().subscriptions().len()
                        );
                        assert!(
                            !msgbus.borrow().is_subscribed(pattern, handler.clone()),
                            "Op {op_num}: is_subscribed should return false after unsubscribe"
                        );
                    }
                }

                // Check is_subscribed
                2 => {
                    // Get a random pattern and handler
                    let pattern_idx = rng.random_range(0..patterns.len());
                    let handler_idx = rng.random_range(0..handlers.len());
                    let pattern = &patterns[pattern_idx];
                    let (handler_id, handler) = &handlers[handler_idx];

                    let expected = model.is_subscribed(pattern, handler_id);
                    let actual = msgbus.borrow().is_subscribed(pattern, handler.clone());

                    assert_eq!(
                        expected, actual,
                        "Op {op_num}: Subscription state mismatch for pattern '{pattern}', handler '{handler_id}': expected={expected}, actual={actual}"
                    );
                }

                // Check matching_subscriptions
                3 => {
                    // Generate a topic
                    let topic = create_topic(&mut rng);

                    let actual_matches = msgbus.borrow_mut().matching_subscriptions(topic);
                    let expected_matches = model.matching_subscriptions(&topic);

                    assert_eq!(
                        expected_matches.len(),
                        actual_matches.len(),
                        "Op {}: Match count mismatch for topic '{}': expected={}, actual={}",
                        op_num,
                        topic,
                        expected_matches.len(),
                        actual_matches.len()
                    );

                    for sub in &actual_matches {
                        assert!(
                            expected_matches
                                .contains(&(sub.pattern.to_string(), sub.handler_id.to_string())),
                            "Op {}: Expected match not found: pattern='{}', handler_id='{}'",
                            op_num,
                            sub.pattern,
                            sub.handler_id
                        );
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    fn generate_pattern_from_topic(topic: &str, rng: &mut StdRng) -> String {
        let mut pattern = String::new();

        for c in topic.chars() {
            let val: f64 = rng.random();
            if val < 0.1 {
                pattern.push('*');
            } else if val < 0.3 {
                pattern.push('?');
            } else if val < 0.5 {
                continue;
            } else {
                pattern.push(c);
            };
        }

        pattern
    }

    fn generate_test_patterns(rng: &mut StdRng) -> Vec<String> {
        let mut patterns = vec![
            "data.*.*.*".to_string(),
            "*.*.BINANCE.*".to_string(),
            "events.order.*".to_string(),
            "data.*.*.?USDT".to_string(),
            "*.trades.*.BTC*".to_string(),
            "*.*.*.*".to_string(),
        ];

        // Add some random patterns
        for _ in 0..50 {
            match rng.random_range(0..10) {
                // Use existing pattern
                0..=1 => {
                    let idx = rng.random_range(0..patterns.len());
                    patterns.push(patterns[idx].clone());
                }
                // Generate new pattern from topic
                _ => {
                    let topic = create_topic(rng);
                    let pattern = generate_pattern_from_topic(&topic, rng);
                    patterns.push(pattern);
                }
            }
        }

        patterns
    }

    fn create_topic(rng: &mut StdRng) -> Ustr {
        let cat = ["data", "info", "order"];
        let model = ["quotes", "trades", "orderbooks", "depths"];
        let venue = ["BINANCE", "BYBIT", "OKX", "FTX", "KRAKEN"];
        let instrument = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

        let cat = cat[rng.random_range(0..cat.len())];
        let model = model[rng.random_range(0..model.len())];
        let venue = venue[rng.random_range(0..venue.len())];
        let instrument = instrument[rng.random_range(0..instrument.len())];
        Ustr::from(&format!("{cat}.{model}.{venue}.{instrument}"))
    }
}
