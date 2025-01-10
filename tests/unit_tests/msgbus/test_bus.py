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


class TestMessageBus:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.handler = []
        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

    def test_instantiate_message_bus(self):
        # Arrange, Act, Assert
        assert self.msgbus.trader_id == self.trader_id
        assert self.msgbus.sent_count == 0
        assert self.msgbus.req_count == 0
        assert self.msgbus.res_count == 0
        assert self.msgbus.pub_count == 0

    def test_endpoints_with_none_registered_returns_empty_list(self):
        # Arrange, Act
        result = self.msgbus.endpoints()

        assert result == []

    def test_topics_with_no_subscribers_returns_empty_list(self):
        # Arrange, Act
        result = self.msgbus.topics()

        assert result == []

    def test_subscriptions_with_no_subscribers_returns_empty_list(self):
        # Arrange, Act
        result = self.msgbus.subscriptions()

        # Assert
        assert result == []

    def test_has_subscribers_with_no_subscribers_returns_false(self):
        # Arrange, Act, Assert
        assert not self.msgbus.has_subscribers()

    def test_register_adds_endpoint(self):
        # Arrange
        endpoint = []

        # Act
        self.msgbus.register("mailbox", endpoint.append)

        # Assert
        assert self.msgbus.endpoints() == ["mailbox"]

    def test_deregister_removes_endpoint(self):
        # Arrange
        endpoint = []
        self.msgbus.register("mailbox", endpoint.append)

        # Act
        self.msgbus.deregister("mailbox", endpoint.append)

        # Assert
        assert self.msgbus.endpoints() == []

    def test_send_when_no_endpoint_at_address_logs_error(self):
        # Arrange, Act
        endpoint = []
        self.msgbus.send("mailbox", "message")

        # Assert
        assert "message" not in endpoint
        assert self.msgbus.sent_count == 0

    def test_send_when_endpoint_at_address_sends_message_to_handler(self):
        # Arrange
        endpoint = []
        self.msgbus.register("mailbox", endpoint.append)

        # Act
        self.msgbus.send("mailbox", "message")

        # Assert
        assert "message" in endpoint
        assert self.msgbus.sent_count == 1

    def test_request_when_endpoint_not_registered_logs_error(self):
        # Arrange, Act
        handler = []

        request = Request(
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.msgbus.request(endpoint="mailbox", request=request)

        # Assert
        assert len(handler) == 0
        assert self.msgbus.req_count == 0

    def test_response_when_no_correlation_id_logs_error(self):
        # Arrange, Act
        handler = []

        response = Response(
            correlation_id=UUID4(),
            response_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.msgbus.response(response)

        # Assert
        assert response not in handler
        assert self.msgbus.res_count == 0

    def test_request_response_when_correlation_id_registered_handles_response(self):
        # Arrange, Act
        endpoint = []
        handler = []

        self.msgbus.register(endpoint="mailbox", handler=endpoint.append)

        request_id = UUID4()
        request = Request(
            callback=handler.append,
            request_id=request_id,
            ts_init=self.clock.timestamp_ns(),
        )

        self.msgbus.request(endpoint="mailbox", request=request)
        assert self.msgbus.is_pending_request(request_id)

        response = Response(
            correlation_id=request_id,
            response_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.msgbus.response(response)

        # Assert
        assert request in endpoint
        assert response in handler
        assert self.msgbus.req_count == 1
        assert self.msgbus.res_count == 1
        assert not self.msgbus.is_pending_request(request_id)  # Already responded

    def test_subscribe_then_returns_topics_list_including_topic(self):
        # Arrange
        handler = [].append

        # Act
        self.msgbus.subscribe(topic="*", handler=handler)
        self.msgbus.subscribe(topic="system", handler=handler)

        result = self.msgbus.topics()

        # Assert
        assert result == ["*", "system"]

    def test_has_subscribers_when_subscribers_returns_true(self):
        # Arrange, Act
        self.msgbus.subscribe(topic="*", handler=[].append)
        self.msgbus.subscribe(topic="system", handler=[].append)

        # Assert
        assert self.msgbus.has_subscribers()
        assert self.msgbus.has_subscribers(pattern="system")

    def test_subscribe_when_handler_already_subscribed_does_not_add_subscription(self):
        # Arrange
        handler = [].append

        self.msgbus.subscribe(topic="a", handler=handler)

        # Act
        self.msgbus.subscribe(topic="a", handler=handler)

        result = self.msgbus.topics()

        # Assert
        assert result == ["a"]

    def test_subscribe_then_subscriptions_list_includes_handler(self):
        # Arrange
        handler = [].append

        # Act
        self.msgbus.subscribe(topic="system", handler=handler)

        result = self.msgbus.subscriptions("system")

        # Assert
        assert len(result) == 1
        assert result[0].handler == handler

    def test_subscribe_to_all_then_subscriptions_list_includes_handler(self):
        # Arrange
        handler = [].append

        # Act
        self.msgbus.subscribe(topic="*", handler=handler)

        result = self.msgbus.subscriptions("*")

        # Assert
        assert len(result) == 1
        assert result[0].handler == handler

    def test_subscribe_all_when_handler_already_subscribed_does_not_add_subscription(self):
        # Arrange
        handler = [].append

        self.msgbus.subscribe(topic="a*", handler=handler)

        # Act
        self.msgbus.subscribe(topic="a*", handler=handler)

        result = self.msgbus.subscriptions("a*")

        # Assert
        assert len(result) == 1
        assert result[0].handler == handler

    def test_unsubscribe_then_handler_not_in_subscriptions_list(self):
        # Arrange
        handler = [].append

        self.msgbus.subscribe(topic="events.order*", handler=handler)

        # Act
        self.msgbus.unsubscribe(topic="events.order*", handler=handler)

        result = self.msgbus.subscriptions("events.order*")

        # Assert
        assert result == []

    def test_unsubscribe_when_no_subscription_does_nothing(self):
        # Arrange
        handler = [].append

        # Act
        self.msgbus.unsubscribe(topic="*", handler=handler)

        result = self.msgbus.subscriptions(pattern="*")

        # Assert
        assert result == []

    def test_unsubscribe_from_all_returns_subscriptions_list_without_handler(self):
        # Arrange
        handler = [].append

        self.msgbus.subscribe(topic="*", handler=handler)

        # Act
        self.msgbus.unsubscribe(topic="*", handler=handler)

        result = self.msgbus.subscriptions("*")

        # Assert
        assert result == []

    def test_unsubscribe_from_all_when_no_subscription_does_nothing(self):
        # Arrange
        handler = [].append

        # Act
        self.msgbus.unsubscribe(topic="*", handler=handler)

        result = self.msgbus.subscriptions("*")

        # Assert
        assert result == []

    def test_publish_with_no_subscribers_does_nothing(self):
        # Arrange, Act
        self.msgbus.publish("*", "hello world")

        # Assert
        assert True  # No exceptions raised

    def test_publish_with_subscriber_sends_to_handler(self):
        # Arrange
        subscriber = []

        self.msgbus.subscribe(topic="system", handler=subscriber.append)

        # Act
        self.msgbus.publish("system", "hello world")

        # Assert
        assert "hello world" in subscriber
        assert self.msgbus.pub_count == 1

    def test_publish_with_multiple_subscribers_sends_to_handlers(self):
        # Arrange
        subscriber1 = []
        subscriber2 = []
        subscriber3 = []

        self.msgbus.subscribe(topic="system", handler=subscriber1.append)
        self.msgbus.subscribe(topic="system", handler=subscriber2.append)
        self.msgbus.subscribe(topic="system", handler=subscriber3.append)

        # Act
        self.msgbus.publish("system", "hello world")

        # Assert
        assert "hello world" in subscriber1
        assert "hello world" in subscriber2
        assert "hello world" in subscriber3
        assert self.msgbus.pub_count == 1

    def test_publish_with_header_sends_to_handler(self):
        # Arrange
        subscriber = []

        self.msgbus.subscribe(topic="events.order*", handler=subscriber.append)

        # Act
        self.msgbus.publish("events.order.SCALPER-001", "ORDER")

        # Assert
        assert "ORDER" in subscriber
        assert self.msgbus.pub_count == 1

    def test_publish_with_header_sends_to_handler_after_published(self):
        # Arrange
        subscriber = []
        self.msgbus.publish("events.order.SCALPER-001", "ORDER")

        self.msgbus.subscribe(topic="events.order*", handler=subscriber.append)

        # Act
        self.msgbus.publish("events.order.SCALPER-001", "ORDER")

        # Assert
        assert "ORDER" in subscriber
        assert self.msgbus.pub_count == 2

    def test_publish_with_none_matching_header_then_filters_from_subscriber(self):
        # Arrange
        subscriber = []

        self.msgbus.subscribe(
            topic="events.position*",
            handler=subscriber.append,
        )

        # Act
        self.msgbus.publish("events.order*", "ORDER")

        # Assert
        assert "ORDER" not in subscriber
        assert self.msgbus.pub_count == 1

    def test_publish_with_matching_subset_header_then_sends_to_subscriber(self):
        # Arrange
        subscriber = []

        self.msgbus.subscribe(
            topic="events.order.*",
            handler=subscriber.append,
        )

        # Act
        self.msgbus.publish("events.order.S-001", "ORDER")

        # Assert
        assert "ORDER" in subscriber
        assert self.msgbus.pub_count == 1

    def test_publish_with_both_channel_and_all_sub_sends_to_subscribers(self):
        # Arrange
        subscriber1 = []
        subscriber2 = []

        self.msgbus.subscribe(
            topic="MyMessages",
            handler=subscriber1.append,
        )

        self.msgbus.subscribe(
            topic="*",  # <-- subscribe ALL
            handler=subscriber2.append,
        )

        # Act
        self.msgbus.publish("MyMessages", "OK!")

        # Assert
        assert "OK!" in subscriber1
        assert "OK!" in subscriber2
        assert self.msgbus.pub_count == 1

    def test_subscribe_prior_to_publish_then_receives_message_on_topic(self):
        # Arrange
        handler1 = []
        handler2 = []

        self.msgbus.subscribe(topic="data.signal.my_signal", handler=handler1.append)
        self.msgbus.subscribe(topic="data.signal.*", handler=handler2.append)

        # Act
        self.msgbus.publish("data.signal.my_signal", "message1")
        self.msgbus.publish("data.signal.*", "message2")
        self.msgbus.publish("data.signal.another_signal", "message3")

        # Assert
        assert handler1 == ["message1"]
        assert handler2 == ["message1", "message2", "message3"]

    def test_msgbus_for_system_events_using_component_id(self):
        # Arrange
        subscriber = []
        self.msgbus.subscribe(topic="events.system.*", handler=subscriber.append)

        topic = f"events.system.{TestIdStubs.trader_id()!s}"

        # Act
        self.msgbus.publish("events.system.DUMMY", "DUMMY EVENT")
        self.msgbus.publish(topic, "TRADER EVENT")

        # Assert
        assert self.msgbus.pub_count == 2
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
