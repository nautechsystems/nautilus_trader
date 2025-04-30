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
use rstest::rstest;
use ustr::Ustr;

use crate::msgbus::{
    self, MessageBus, get_message_bus, is_matching,
    stubs::{
        check_handler_was_called, get_call_check_shareable_handler, get_stub_shareable_handler,
    },
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
    assert!(msgbus.borrow().topics().is_empty());
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

    let result = msgbus_ref.register_response_handler(&request_id, handler.clone());
    assert!(result.is_err());
}

#[rstest]
fn test_get_response_handler_when_registered() {
    let msgbus = get_message_bus();
    let mut msgbus_ref = msgbus.borrow_mut();
    let handler = get_stub_shareable_handler(None);

    let request_id = UUID4::new();
    msgbus_ref
        .register_response_handler(&request_id, handler.clone())
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
fn test_regsiter_endpoint() {
    let msgbus = get_message_bus();
    let endpoint = "MyEndpoint";
    let handler = get_stub_shareable_handler(None);

    msgbus::register(endpoint, handler);

    assert_eq!(msgbus.borrow().endpoints(), vec![endpoint.to_string()]);
    assert!(msgbus.borrow().get_endpoint(endpoint).is_some());
}

#[rstest]
fn test_endpoint_send() {
    let msgbus = get_message_bus();
    let endpoint = Ustr::from("MyEndpoint");
    let handler = get_call_check_shareable_handler(None);

    msgbus::register(endpoint, handler.clone());
    assert!(msgbus.borrow().get_endpoint(endpoint).is_some());
    assert!(!check_handler_was_called(handler.clone()));

    // Send a message to the endpoint
    msgbus::send(&endpoint, &"Test Message");
    assert!(check_handler_was_called(handler));
}

#[rstest]
fn test_deregsiter_endpoint() {
    let msgbus = get_message_bus();
    let endpoint = Ustr::from("MyEndpoint");
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

    msgbus::subscribe(topic, handler, Some(1));

    assert!(msgbus.borrow().has_subscribers(topic));
    assert_eq!(msgbus.borrow().topics(), vec![topic]);
}

#[rstest]
fn test_unsubscribe() {
    let msgbus = get_message_bus();
    let topic = "my-topic";
    let handler = get_stub_shareable_handler(None);

    msgbus::subscribe(topic, handler.clone(), None);
    msgbus::unsubscribe(topic, handler);

    assert!(!msgbus.borrow().has_subscribers(topic));
    assert!(msgbus.borrow().topics().is_empty());
}

#[rstest]
fn test_matching_subscriptions() {
    let msgbus = get_message_bus();
    let topic = "my-topic";

    let handler_id1 = Ustr::from("1");
    let handler1 = get_stub_shareable_handler(Some(handler_id1));

    let handler_id2 = Ustr::from("2");
    let handler2 = get_stub_shareable_handler(Some(handler_id2));

    let handler_id3 = Ustr::from("3");
    let handler3 = get_stub_shareable_handler(Some(handler_id3));

    let handler_id4 = Ustr::from("4");
    let handler4 = get_stub_shareable_handler(Some(handler_id4));

    msgbus::subscribe(topic, handler1, None);
    msgbus::subscribe(topic, handler2, None);
    msgbus::subscribe(topic, handler3, Some(1));
    msgbus::subscribe(topic, handler4, Some(2));
    let topic = Ustr::from(topic);

    let subs = msgbus.borrow().matching_subscriptions(&topic);
    assert_eq!(subs.len(), 4);
    assert_eq!(subs[0].handler_id, handler_id4);
    assert_eq!(subs[1].handler_id, handler_id3);
    assert_eq!(subs[2].handler_id, handler_id1);
    assert_eq!(subs[3].handler_id, handler_id2);
}

#[rstest]
#[case("*", "*", true)]
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
        is_matching(&Ustr::from(topic), &Ustr::from(pattern)),
        expected
    );
}
