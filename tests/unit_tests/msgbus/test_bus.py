# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import pytest

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.component import is_matching_py
from nautilus_trader.core.message import Request
from nautilus_trader.core.message import Response
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


@pytest.fixture
def clock():
    return TestClock()


@pytest.fixture
def trader_id():
    return TestIdStubs.trader_id()


@pytest.fixture
def bus(clock, trader_id):
    return MessageBus(trader_id=trader_id, clock=clock)


def test_instantiate_message_bus(bus, trader_id):
    # Arrange, Act, Assert
    assert bus.trader_id == trader_id
    assert bus.sent_count == 0
    assert bus.req_count == 0
    assert bus.res_count == 0
    assert bus.pub_count == 0


def test_endpoints_with_none_registered_returns_empty_list(bus):
    # Arrange, Act
    result = bus.endpoints()

    # Assert
    assert result == []


def test_topics_with_no_subscribers_returns_empty_list(bus):
    # Arrange, Act
    result = bus.topics()

    # Assert
    assert result == []


def test_subscriptions_with_no_subscribers_returns_empty_list(bus):
    # Arrange, Act
    result = bus.subscriptions()

    # Assert
    assert result == []


def test_has_subscribers_with_no_subscribers_returns_false(bus):
    # Arrange, Act, Assert
    assert not bus.has_subscribers()


def test_register_adds_endpoint(bus):
    # Arrange
    endpoint = []

    # Act
    bus.register("mailbox", endpoint.append)

    # Assert
    assert bus.endpoints() == ["mailbox"]


def test_deregister_removes_endpoint(bus):
    # Arrange
    endpoint = []
    bus.register("mailbox", endpoint.append)

    # Act
    bus.deregister("mailbox", endpoint.append)

    # Assert
    assert bus.endpoints() == []


def test_send_when_no_endpoint_at_address_logs_error(bus):
    # Arrange
    endpoint = []

    # Act
    bus.send("mailbox", "message")

    # Assert
    assert "message" not in endpoint
    assert bus.sent_count == 0


def test_send_when_endpoint_at_address_sends_message_to_handler(bus):
    # Arrange
    endpoint = []
    bus.register("mailbox", endpoint.append)

    # Act
    bus.send("mailbox", "message")

    # Assert
    assert "message" in endpoint
    assert bus.sent_count == 1


def test_request_when_endpoint_not_registered_logs_error(bus, clock):
    # Arrange
    handler = []
    request = Request(
        callback=handler.append,
        request_id=UUID4(),
        ts_init=clock.timestamp_ns(),
    )

    # Act
    bus.request(endpoint="mailbox", request=request)

    # Assert
    assert len(handler) == 0
    assert bus.req_count == 0


def test_response_when_no_correlation_id_logs_error(bus, clock):
    # Arrange
    handler = []
    response = Response(
        correlation_id=UUID4(),
        response_id=UUID4(),
        ts_init=clock.timestamp_ns(),
    )

    # Act
    bus.response(response)

    # Assert
    assert response not in handler
    assert bus.res_count == 0


def test_request_response_when_correlation_id_registered_handles_response(bus, clock):
    # Arrange
    endpoint = []
    handler = []
    bus.register(endpoint="mailbox", handler=endpoint.append)

    request_id = UUID4()
    request = Request(
        callback=handler.append,
        request_id=request_id,
        ts_init=clock.timestamp_ns(),
    )

    # Act
    bus.request(endpoint="mailbox", request=request)
    assert bus.is_pending_request(request_id)

    response = Response(
        correlation_id=request_id,
        response_id=UUID4(),
        ts_init=clock.timestamp_ns(),
    )
    bus.response(response)

    # Assert
    assert request in endpoint
    assert response in handler
    assert bus.req_count == 1
    assert bus.res_count == 1
    assert not bus.is_pending_request(request_id)


def test_subscribe_then_returns_topics_list_including_topic(bus):
    # Arrange
    handler = [].append

    # Act
    bus.subscribe(topic="*", handler=handler)
    bus.subscribe(topic="system", handler=handler)
    result = bus.topics()

    # Assert
    assert result == ["*", "system"]


def test_has_subscribers_when_subscribers_returns_true(bus):
    # Arrange, Act
    bus.subscribe(topic="*", handler=[].append)
    bus.subscribe(topic="system", handler=[].append)

    # Assert
    assert bus.has_subscribers()
    assert bus.has_subscribers(pattern="system")


def test_subscribe_when_handler_already_subscribed_does_not_add_subscription(bus):
    # Arrange
    handler = [].append
    bus.subscribe(topic="a", handler=handler)

    # Act
    bus.subscribe(topic="a", handler=handler)
    result = bus.topics()

    # Assert
    assert result == ["a"]


def test_subscribe_then_subscriptions_list_includes_handler(bus):
    # Arrange
    handler = [].append

    # Act
    bus.subscribe(topic="system", handler=handler)
    result = bus.subscriptions("system")

    # Assert
    assert len(result) == 1
    assert result[0].handler == handler


def test_subscribe_to_all_then_subscriptions_list_includes_handler(bus):
    # Arrange
    handler = [].append

    # Act
    bus.subscribe(topic="*", handler=handler)
    result = bus.subscriptions("*")

    # Assert
    assert len(result) == 1
    assert result[0].handler == handler


def test_subscribe_all_when_handler_already_subscribed_does_not_add_subscription(bus):
    # Arrange
    handler = [].append
    bus.subscribe(topic="a*", handler=handler)

    # Act
    bus.subscribe(topic="a*", handler=handler)
    result = bus.subscriptions("a*")

    # Assert
    assert len(result) == 1
    assert result[0].handler == handler


def test_unsubscribe_then_handler_not_in_subscriptions_list(bus):
    # Arrange
    handler = [].append
    bus.subscribe(topic="events.order*", handler=handler)

    # Act
    bus.unsubscribe(topic="events.order*", handler=handler)
    result = bus.subscriptions("events.order*")

    # Assert
    assert result == []


def test_unsubscribe_when_no_subscription_does_nothing(bus):
    # Arrange
    handler = [].append

    # Act
    bus.unsubscribe(topic="*", handler=handler)
    result = bus.subscriptions(pattern="*")

    # Assert
    assert result == []


def test_unsubscribe_from_all_returns_subscriptions_list_without_handler(bus):
    # Arrange
    handler = [].append
    bus.subscribe(topic="*", handler=handler)

    # Act
    bus.unsubscribe(topic="*", handler=handler)
    result = bus.subscriptions("*")

    # Assert
    assert result == []


def test_unsubscribe_from_all_when_no_subscription_does_nothing(bus):
    # Arrange
    handler = [].append

    # Act
    bus.unsubscribe(topic="*", handler=handler)
    result = bus.subscriptions("*")

    # Assert
    assert result == []


def test_is_subscribed_lifecycle(bus):
    def handler(msg):
        return msg

    assert not bus.is_subscribed("topic.test", handler)

    bus.subscribe(topic="topic.test", handler=handler)
    assert bus.is_subscribed("topic.test", handler)

    bus.unsubscribe(topic="topic.test", handler=handler)
    assert not bus.is_subscribed("topic.test", handler)


def test_publish_with_no_subscribers_does_nothing(bus):
    # Arrange, Act
    bus.publish("*", "hello world")

    # Assert
    assert True  # No exceptions raised


def test_publish_with_subscriber_sends_to_handler(bus):
    # Arrange
    subscriber = []
    bus.subscribe(topic="system", handler=subscriber.append)

    # Act
    bus.publish("system", "hello world")

    # Assert
    assert "hello world" in subscriber
    assert bus.pub_count == 1


def test_publish_with_multiple_subscribers_sends_to_handlers(bus):
    # Arrange
    subscriber1 = []
    subscriber2 = []
    subscriber3 = []
    bus.subscribe(topic="system", handler=subscriber1.append)
    bus.subscribe(topic="system", handler=subscriber2.append)
    bus.subscribe(topic="system", handler=subscriber3.append)

    # Act
    bus.publish("system", "hello world")

    # Assert
    assert "hello world" in subscriber1
    assert "hello world" in subscriber2
    assert "hello world" in subscriber3
    assert bus.pub_count == 1


def test_publish_with_header_sends_to_handler(bus):
    # Arrange
    subscriber = []
    bus.subscribe(topic="events.order*", handler=subscriber.append)

    # Act
    bus.publish("events.order.SCALPER-001", "ORDER")

    # Assert
    assert "ORDER" in subscriber
    assert bus.pub_count == 1


def test_publish_with_header_sends_to_handler_after_published(bus):
    # Arrange
    subscriber = []

    # Act
    bus.publish("events.order.SCALPER-001", "ORDER")
    bus.subscribe(topic="events.order*", handler=subscriber.append)
    bus.publish("events.order.SCALPER-001", "ORDER")

    # Assert
    assert "ORDER" in subscriber
    assert bus.pub_count == 2


def test_publish_with_none_matching_header_then_filters_from_subscriber(bus):
    # Arrange
    subscriber = []
    bus.subscribe(topic="events.position*", handler=subscriber.append)

    # Act
    bus.publish("events.order*", "ORDER")

    # Assert
    assert "ORDER" not in subscriber
    assert bus.pub_count == 1


def test_publish_with_matching_subset_header_then_sends_to_subscriber(bus):
    # Arrange
    subscriber = []
    bus.subscribe(topic="events.order.*", handler=subscriber.append)

    # Act
    bus.publish("events.order.S-001", "ORDER")

    # Assert
    assert "ORDER" in subscriber
    assert bus.pub_count == 1


def test_publish_with_both_channel_and_all_sub_sends_to_subscribers(bus):
    # Arrange
    subscriber1 = []
    subscriber2 = []
    bus.subscribe(topic="MyMessages", handler=subscriber1.append)
    bus.subscribe(topic="*", handler=subscriber2.append)

    # Act
    bus.publish("MyMessages", "OK!")

    # Assert
    assert "OK!" in subscriber1
    assert "OK!" in subscriber2
    assert bus.pub_count == 1


def test_subscribe_prior_to_publish_then_receives_message_on_topic(bus):
    # Arrange
    handler1 = []
    handler2 = []
    bus.subscribe(topic="data.signal.my_signal", handler=handler1.append)
    bus.subscribe(topic="data.signal.*", handler=handler2.append)

    # Act
    bus.publish("data.signal.my_signal", "message1")
    bus.publish("data.signal.*", "message2")
    bus.publish("data.signal.another_signal", "message3")

    # Assert
    assert handler1 == ["message1"]
    assert handler2 == ["message1", "message2", "message3"]


def test_msgbus_for_system_events_using_component_id(bus):
    # Arrange
    subscriber = []
    bus.subscribe(topic="events.system.*", handler=subscriber.append)

    topic = f"events.system.{TestIdStubs.trader_id()!s}"

    # Act
    bus.publish("events.system.DUMMY", "DUMMY EVENT")
    bus.publish(topic, "TRADER EVENT")

    # Assert
    assert bus.pub_count == 2
    assert len(subscriber) == 2
    assert subscriber == ["DUMMY EVENT", "TRADER EVENT"]


@pytest.mark.parametrize(
    ("topic", "pattern", "expected"),
    [
        ["*", "*", True],
        ["a", "*", True],
        ["a", "a", True],
        ["a", "b", False],
        ["data.quotes.BINANCE", "data.*", True],
        ["data.quotes.BINANCE", "data.quotes*", True],
        ["data.quotes.BINANCE", "data.*.BINANCE", True],
        ["data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.*", True],
        ["data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ETH*", True],
        ["data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ETH???", False],
        ["data.trades.BINANCE.ETHUSD", "data.*.BINANCE.ETH???", True],
        # We don't support [seq] style pattern
        ["data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[HC]USDT", False],
        # We don't support [!seq] style pattern
        ["data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[!ABC]USDT", False],
        # We don't support [^seq] style pattern
        ["data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[^ABC]USDT", False],
    ],
)
def test_is_matching_given_various_topic_pattern_combos(topic, pattern, expected):
    # Arrange, Act, Assert
    assert is_matching_py(topic=topic, pattern=pattern) == expected


def test_duplicate_request_id_not_processed(bus, clock):
    endpoint_msgs = []
    callback_msgs = []

    bus.register("mailbox", endpoint_msgs.append)
    req_id = UUID4()
    req = Request(
        callback=callback_msgs.append,
        request_id=req_id,
        ts_init=clock.timestamp_ns(),
    )

    bus.request(endpoint="mailbox", request=req)

    assert bus.req_count == 1
    assert endpoint_msgs == [req]
    assert bus.is_pending_request(req_id)

    bus.request(endpoint="mailbox", request=req)

    assert bus.req_count == 1
    assert endpoint_msgs == [req]


def test_publish_question_mark_pattern(bus):
    # Arrange
    received = []
    bus.subscribe(topic="test.?", handler=received.append)

    # Act
    bus.publish("test.a", "ok1")
    bus.publish("test.1", "ok2")
    bus.publish("test.", "nope")
    bus.publish("test.12", "nope2")

    # Assert
    assert received == ["ok1", "ok2"]


def test_publish_invokes_handlers_in_priority_order(bus):
    # Arrange
    order = []

    def low(msg):
        order.append(f"low-{msg}")

    def high(msg):
        order.append(f"high-{msg}")

    bus.subscribe(topic="orders", handler=low, priority=0)
    bus.subscribe(topic="orders", handler=high, priority=10)

    # Act
    bus.publish("orders", "123")

    # Assert
    assert order == ["high-123", "low-123"]


def test_streaming_type_registration(bus):
    # Arrange
    assert not bus.is_streaming_type(int)

    # Act
    bus.add_streaming_type(int)

    # Assert
    assert bus.is_streaming_type(int)


def test_add_listener_receives_byte_messages(bus):
    # Arrange
    events = []

    class DummyListener:
        def __init__(self, closed=False):
            self._closed = closed

        def is_closed(self):
            return self._closed

        def publish(self, topic, payload):
            events.append((topic, payload))

    listener_open = DummyListener(closed=False)
    listener_closed = DummyListener(closed=True)
    bus.add_listener(listener_open)
    bus.add_listener(listener_closed)

    # Act
    payload = b"data"
    bus.publish("any.topic", payload)

    # Assert
    assert events == [("any.topic", payload)]
