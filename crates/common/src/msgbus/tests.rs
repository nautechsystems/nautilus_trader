// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::UUID4;
use nautilus_model::identifiers::TraderId;
use rand::{Rng, SeedableRng, rngs::StdRng};
use regex::Regex;
use rstest::rstest;
use ustr::Ustr;

use crate::msgbus::{
    self, MessageBus, get_message_bus,
    handler::ShareableMessageHandler,
    matching::is_matching_backtracking,
    stubs::{
        check_handler_was_called, get_call_check_shareable_handler, get_stub_shareable_handler,
    },
    subscriptions_count,
};

#[rstest]
fn test_new() {
    let trader_id = TraderId::from("trader-001");
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

    msgbus::register(endpoint, handler);

    assert_eq!(msgbus.borrow().endpoints(), vec![endpoint.to_string()]);
    assert!(msgbus.borrow().get_endpoint(endpoint).is_some());
}

#[rstest]
fn test_endpoint_send() {
    let msgbus = get_message_bus();
    let endpoint = "MyEndpoint".into();
    let handler = get_call_check_shareable_handler(None);

    msgbus::register(endpoint, handler.clone());
    assert!(msgbus.borrow().get_endpoint(endpoint).is_some());
    assert!(!check_handler_was_called(handler.clone()));

    // Send a message to the endpoint
    msgbus::send_any(endpoint, &"Test Message");
    assert!(check_handler_was_called(handler));
}

#[rstest]
fn test_deregsiter_endpoint() {
    let msgbus = get_message_bus();
    let endpoint = "MyEndpoint".into();
    let handler = get_stub_shareable_handler(None);

    msgbus::register(endpoint, handler);
    msgbus::deregister(endpoint);

    assert!(msgbus.borrow().endpoints().is_empty());
}

#[rstest]
fn test_subscribe() {
    let msgbus = get_message_bus();
    let topic = "my-topic";
    let handler = get_stub_shareable_handler(None);

    msgbus::subscribe_str(topic, handler, Some(1));

    assert!(msgbus.borrow().has_subscribers(topic));
    assert_eq!(msgbus.borrow().patterns(), vec![topic]);
}

#[rstest]
fn test_unsubscribe() {
    let msgbus = get_message_bus();
    let topic = "my-topic";
    let handler = get_stub_shareable_handler(None);

    msgbus::subscribe_str(topic, handler.clone(), None);
    msgbus::unsubscribe_str(topic, handler);

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

    msgbus::subscribe_str(pattern, handler1, None);
    msgbus::subscribe_str(pattern, handler2, None);
    msgbus::subscribe_str(pattern, handler3, Some(1));
    msgbus::subscribe_str(pattern, handler4, Some(2));

    assert_eq!(
        msgbus.borrow().patterns(),
        vec![pattern, pattern, pattern, pattern]
    );
    assert_eq!(subscriptions_count(pattern), 4);

    let topic = pattern;
    let subs = msgbus.borrow_mut().matching_subscriptions(topic);
    assert_eq!(subs.len(), 4);
    assert_eq!(subs[0].handler_id, handler_id4);
    assert_eq!(subs[1].handler_id, handler_id3);
    assert_eq!(subs[2].handler_id, handler_id1);
    assert_eq!(subs[3].handler_id, handler_id2);
}

#[rstest]
#[case("a", "*", true)]
#[case("a", "a", true)]
#[case("a", "b", false)]
#[case("data.quotes.BINANCE", "data.*", true)]
#[case("data.quotes.BINANCE", "data.quotes*", true)]
#[case("data.quotes.BINANCE", "data.*.BINANCE", true)]
#[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.*", true)]
#[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ETH*", true)]
#[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ETH???", false)]
#[case("data.trades.BINANCE.ETHUSD", "data.*.BINANCE.ETH???", true)]
// We don't support [seq] style pattern
#[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[HC]USDT", false)]
// We don't support [!seq] style pattern
#[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[!ABC]USDT", false)]
// We don't support [^seq] style pattern
#[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[^ABC]USDT", false)]
fn test_is_matching(#[case] topic: &str, #[case] pattern: &str, #[case] expected: bool) {
    assert_eq!(
        is_matching_backtracking(topic.into(), pattern.into()),
        expected
    );
}

fn convert_pattern_to_regex(pattern: &str) -> String {
    let mut regex = String::new();
    regex.push('^');

    for c in pattern.chars() {
        match c {
            '.' => regex.push_str("\\."),
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            _ => regex.push(c),
        }
    }

    regex.push('$');
    regex
}

#[rstest]
#[case("a??.quo*es.?I?AN*ET?US*T", "^a..\\.quo.*es\\..I.AN.*ET.US.*T$")]
#[case("da?*.?u*?s??*NC**ETH?", "^da..*\\..u.*.s...*NC.*.*ETH.$")]
fn test_convert_pattern_to_regex(#[case] pat: &str, #[case] regex: &str) {
    assert_eq!(convert_pattern_to_regex(pat), regex);
}

fn generate_pattern_from_topic(topic: &str, rng: &mut StdRng) -> String {
    let mut pattern = String::new();

    for c in topic.chars() {
        let val: f64 = rng.random();
        // 10% chance of wildcard
        if val < 0.1 {
            pattern.push('*');
        }
        // 20% chance of question mark
        else if val < 0.3 {
            pattern.push('?');
        }
        // 20% chance of skipping
        else if val < 0.5 {
            continue;
        }
        // 50% chance of keeping the character
        else {
            pattern.push(c);
        };
    }

    pattern
}

#[rstest]
fn test_matching_backtracking() {
    let topic = "data.quotes.BINANCE.ETHUSDT";
    let mut rng = StdRng::seed_from_u64(42);

    for i in 0..1000 {
        let pattern = generate_pattern_from_topic(topic, &mut rng);
        let regex_pattern = convert_pattern_to_regex(&pattern);
        let regex = Regex::new(&regex_pattern).unwrap();
        assert_eq!(
            is_matching_backtracking(topic.into(), pattern.as_str().into()),
            regex.is_match(topic),
            "Failed to match on iteration: {i}, pattern: \"{pattern}\", topic: {topic}, regex: \"{regex_pattern}\""
        );
    }
}

#[rstest]
fn test_subscription_pattern_matching() {
    let msgbus = get_message_bus();
    let handler1 = get_stub_shareable_handler(Some(Ustr::from("1")));
    let handler2 = get_stub_shareable_handler(Some(Ustr::from("2")));
    let handler3 = get_stub_shareable_handler(Some(Ustr::from("3")));

    msgbus::subscribe_str("data.quotes.*", handler1, None);
    msgbus::subscribe_str("data.trades.*", handler2, None);
    msgbus::subscribe_str("data.*.BINANCE.*", handler3, None);
    assert_eq!(msgbus.borrow().subscriptions().len(), 3);

    let topic = "data.quotes.BINANCE.ETHUSDT";
    assert_eq!(msgbus.borrow().find_topic_matches(topic.into()).len(), 2);

    let matches = msgbus.borrow_mut().matching_subscriptions(topic);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].handler_id, Ustr::from("3"));
    assert_eq!(matches[1].handler_id, Ustr::from("1"));
}

/// A simple reference model for subscription behavior
struct SimpleSubscriptionModel {
    /// Stores (pattern, handler_id) tuples for active subscriptions
    subscriptions: Vec<(String, String)>,
}

impl SimpleSubscriptionModel {
    /// Create a new empty model
    fn new() -> Self {
        Self {
            subscriptions: Vec::new(),
        }
    }

    /// Subscribe a handler to a pattern in the model
    fn subscribe(&mut self, pattern: &str, handler_id: &str) {
        let subscription = (pattern.to_string(), handler_id.to_string());
        if !self.subscriptions.contains(&subscription) {
            self.subscriptions.push(subscription);
        }
    }

    /// Unsubscribe a handler from a pattern in the model
    fn unsubscribe(&mut self, pattern: &str, handler_id: &str) -> bool {
        let subscription = (pattern.to_string(), handler_id.to_string());
        if let Some(idx) = self.subscriptions.iter().position(|s| s == &subscription) {
            self.subscriptions.remove(idx);
            true
        } else {
            false
        }
    }

    /// Check if a handler is subscribed to a pattern in the model
    fn is_subscribed(&self, pattern: &str, handler_id: &str) -> bool {
        self.subscriptions
            .contains(&(pattern.to_string(), handler_id.to_string()))
    }

    /// Get all subscriptions that match a topic according to the matching rules
    fn matching_subscriptions(&self, topic: &str) -> Vec<(String, String)> {
        let topic = topic.into();

        self.subscriptions
            .iter()
            .filter(|(pat, _)| is_matching_backtracking(topic, pat.into()))
            .map(|(pat, id)| (pat.clone(), id.clone()))
            .collect()
    }

    /// Count of active subscriptions
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
                msgbus::subscribe(pattern.as_str().into(), handler.clone(), None);

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
                    msgbus::unsubscribe(pattern.as_str().into(), handler.clone());

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

// Helper function to generate diverse test patterns
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
