# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.msgbus.message_bus import MessageBus
from tests.test_kit.stubs import TestStubs


class TestMessageBus:
    def setup(self):
        # Fixture setup
        self.clock = TestClock()
        self.logger = Logger(self.clock)

        self.trader_id = TestStubs.trader_id()

        self.handler = []
        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

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

    def test_send_when_endpoint_at_address_sends_message_to_handler(self):
        # Arrange
        endpoint = []
        self.msgbus.register("mailbox", endpoint.append)

        # Act
        self.msgbus.send("mailbox", "message")

        # Assert
        assert "message" in endpoint

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
        assert self.msgbus.has_subscribers(topic="system")

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

        result = self.msgbus.subscriptions(topic="*")

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

    def test_publish_with_header_sends_to_handler(self):
        # Arrange
        subscriber = []

        self.msgbus.subscribe(topic="events.order*", handler=subscriber.append)

        # Act
        self.msgbus.publish("events.order.SCALPER-001", "ORDER")

        # Assert
        assert "ORDER" in subscriber

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
