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
from nautilus_trader.msgbus.message_bus import Subscription


class TestSubscription:
    def test_comparisons_returns_expected(self):
        # Arrange
        subscriber = []

        subscription1 = Subscription(
            topic="*",
            handler=subscriber.append,
            priority=0,
        )

        subscription2 = Subscription(
            topic="*",
            handler=subscriber.append,
            priority=1,
        )

        # Act, Assert
        assert subscription1 == subscription2
        assert subscription1 < subscription2
        assert subscription1 <= subscription2
        assert subscription2 > subscription1
        assert subscription2 >= subscription1

    def test_equality_when_equal_returns_true(self):
        # Arrange
        subscriber = []

        subscription1 = Subscription(
            topic="*",
            handler=subscriber.append,
            priority=1,
        )

        subscription2 = Subscription(
            topic="*",
            handler=subscriber.append,
            priority=2,  # <-- priority does not affect equality
        )

        # Act, Assert
        assert subscription1 == subscription2

    def test_equality_when_not_equal_returns_false(self):
        # Arrange
        subscriber = []

        subscription1 = Subscription(
            topic="*",
            handler=subscriber.append,
            priority=1,
        )

        subscription2 = Subscription(
            topic="something",
            handler=subscriber.append,
            priority=2,  # <-- priority does not affect equality
        )

        # Act, Assert
        assert subscription1 != subscription2

    def test_reverse_sorting_list_of_subscribers_returns_expected_ordered_list(self):
        # Arrange
        subscriber = []

        subscription1 = Subscription(
            topic="*",
            handler=subscriber.append,
        )

        subscription2 = Subscription(
            topic="*",
            handler=subscriber.append,
            priority=5,  # <-- priority does not affect equality
        )

        subscription3 = Subscription(
            topic="*",
            handler=subscriber.append,
            priority=2,  # <-- priority does not affect equality
        )

        subscription4 = Subscription(
            topic="*",
            handler=subscriber.append,
            priority=10,  # <-- priority does not affect equality
        )

        # Act
        sorted_list = sorted(
            [
                subscription1,
                subscription2,
                subscription3,
                subscription4,
            ],
            reverse=True,
        )

        # Assert
        assert sorted_list == [subscription4, subscription2, subscription3, subscription1]
        assert sorted_list[0] == subscription4
        assert sorted_list[1] == subscription2
        assert sorted_list[2] == subscription3
        assert sorted_list[3] == subscription1

    def test_subscription_for_all(self):
        # Arrange
        subscriber = []
        handler_str = str(subscriber.append)

        # Act
        subscription = Subscription(
            topic="*",
            handler=subscriber.append,
        )

        # Assert
        assert str(subscription).startswith(
            f"Subscription(topic=*, handler={handler_str}, priority=0)"
        )

    def test_str_repr(self):
        # Arrange
        subscriber = []
        handler_str = str(subscriber.append)

        # Act
        subscription = Subscription(
            topic="system_status",
            handler=subscriber.append,
        )

        # Assert
        assert (
            str(subscription)
            == f"Subscription(topic=system_status, handler={handler_str}, priority=0)"
        )
        assert (
            repr(subscription)
            == f"Subscription(topic=system_status, handler={handler_str}, priority=0)"
        )


class TestMessageBus:
    def setup(self):
        # Fixture setup
        self.clock = TestClock()
        self.logger = Logger(self.clock)

        self.handler = []
        self.msgbus = MessageBus(
            name="TestBus",
            clock=self.clock,
            logger=self.logger,
        )

    def test_channels_with_no_subscribers_returns_empty_list(self):
        # Arrange, Act
        result = self.msgbus.channels()

        assert result == []

    def test_subscriptions_with_no_subscribers_returns_empty_list(self):
        # Arrange, Act
        result = self.msgbus.subscriptions("*")

        # Assert
        assert result == []

    def test_subscribe_to_msg_type_returns_channels_list_including_msg_type(self):
        # Arrange
        handler = [].append

        # Act
        self.msgbus.subscribe(topic="*", handler=handler)
        self.msgbus.subscribe(topic="system", handler=handler)

        result = self.msgbus.channels()

        # Assert
        assert result == ["system", "*"]

    def test_subscribe_when_handler_already_subscribed_does_not_add_subscription(self):
        # Arrange
        handler = [].append

        self.msgbus.subscribe(topic="a", handler=handler)

        # Act
        self.msgbus.subscribe(topic="a", handler=handler)

        result = self.msgbus.channels()

        # Assert
        assert result == ["a"]

    def test_subscribe_to_channel_returns_subscriptions_list_including_handler(self):
        # Arrange
        handler = [].append

        # Act
        self.msgbus.subscribe(topic="system", handler=handler)

        result = self.msgbus.subscriptions("system")

        # Assert
        assert len(result) == 1
        assert result[0].handler == handler

    def test_subscribe_to_all_returns_subscriptions_list_including_handler(self):
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

    def test_unsubscribe_from_msg_type_returns_subscriptions_list_without_handler(self):
        # Arrange
        handler = [].append

        self.msgbus.subscribe(topic="orders:*", handler=handler)

        # Act
        self.msgbus.unsubscribe(topic="orders:*", handler=handler)

        result = self.msgbus.subscriptions("orders:*")

        # Assert
        assert result == []

    def test_unsubscribe_from_msg_type_when_no_subscription_does_nothing(self):
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

        self.msgbus.subscribe(topic="Event:OrderEvent*", handler=subscriber.append)

        # Act
        self.msgbus.publish("Event:OrderEvent*", "OK!")

        # Assert
        assert "OK!" in subscriber

    def test_publish_with_none_matching_header_then_filters_from_subscriber(self):
        # Arrange
        subscriber = []

        self.msgbus.subscribe(
            topic="Event:PositionEvent:*",
            handler=subscriber.append,
        )

        # Act
        self.msgbus.publish("Event:OrderEvent*", "OK!")

        # Assert
        assert "OK!" not in subscriber

    def test_publish_with_matching_subset_header_then_sends_to_subscriber(self):
        # Arrange
        subscriber = []

        self.msgbus.subscribe(
            topic="order*",
            handler=subscriber.append,
        )

        # Act
        self.msgbus.publish("order.S-001", "OK!")

        # Assert
        assert "OK!" in subscriber

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
