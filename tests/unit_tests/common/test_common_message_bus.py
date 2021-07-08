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
from nautilus_trader.common.message_bus import MessageBus
from nautilus_trader.common.message_bus import Subscription
from nautilus_trader.core.type import MessageType


class TestSubscription:
    def test_comparisons_returns_expected(self):
        # Arrange
        subscriber = []
        string_msg = MessageType(type=str)

        subscription1 = Subscription(
            msg_type=string_msg,
            handler=subscriber.append,
            priority=0,
        )

        subscription2 = Subscription(
            msg_type=string_msg,
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
        string_msg = MessageType(type=str)

        subscription1 = Subscription(
            msg_type=string_msg,
            handler=subscriber.append,
            priority=1,
        )

        subscription2 = Subscription(
            msg_type=string_msg,
            handler=subscriber.append,
            priority=2,  # <-- priority does not affect equality
        )

        # Act, Assert
        assert subscription1 == subscription2

    def test_equality_when_not_equal_returns_false(self):
        # Arrange
        subscriber = []
        string_msg1 = MessageType(type=str)
        string_msg2 = MessageType(type=str, header={"topic": "status"})

        subscription1 = Subscription(
            msg_type=string_msg1,
            handler=subscriber.append,
            priority=1,
        )

        subscription2 = Subscription(
            msg_type=string_msg2,
            handler=subscriber.append,
            priority=2,  # <-- priority does not affect equality
        )

        # Act, Assert
        assert subscription1 != subscription2

    def test_reverse_sorting_list_of_subscribers_returns_expected_ordered_list(self):
        # Arrange
        subscriber = []
        string_msg = MessageType(type=str)

        subscription1 = Subscription(
            msg_type=string_msg,
            handler=subscriber.append,
        )

        subscription2 = Subscription(
            msg_type=string_msg,
            handler=subscriber.append,
            priority=5,  # <-- priority does not affect equality
        )

        subscription3 = Subscription(
            msg_type=string_msg,
            handler=subscriber.append,
            priority=2,  # <-- priority does not affect equality
        )

        subscription4 = Subscription(
            msg_type=string_msg,
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
            msg_type=None,
            handler=subscriber.append,
        )

        # Assert
        assert subscription.msg_type is None
        assert str(subscription).startswith(
            f"Subscription(msg_type=*, handler={handler_str}, priority=0)"
        )

    def test_str_repr(self):
        # Arrange
        subscriber = []
        string_msg = MessageType(type=str, header={"topic": "status"})
        handler_str = str(subscriber.append)

        # Act
        subscription = Subscription(
            msg_type=string_msg,
            handler=subscriber.append,
        )

        # Assert
        assert (
            str(subscription)
            == f"Subscription(msg_type=<str> {{'topic': 'status'}}, handler={handler_str}, priority=0)"
        )
        assert (
            repr(subscription)
            == f"Subscription(msg_type=<str> {{'topic': 'status'}}, handler={handler_str}, priority=0)"
        )


class TestMessageBus:
    def setup(self):
        # Fixture setup
        self.clock = TestClock()
        self.logger = Logger(self.clock)

        self.handler = []
        self.msg_bus = MessageBus(
            name="TestBus",
            clock=self.clock,
            logger=self.logger,
        )

    def test_channels_with_no_subscribers_returns_empty_list(self):
        # Arrange, Act
        result = self.msg_bus.channels()

        assert result == []

    def test_subscriptions_with_no_subscribers_returns_empty_list(self):
        # Arrange
        all_strings = MessageType(type=str)

        # Act
        result = self.msg_bus.subscriptions(all_strings)

        # Assert
        assert result == []

    def test_subscribe_to_msg_type_returns_channels_list_including_msg_type(self):
        # Arrange
        all_strings = MessageType(type=str)
        handler = [].append

        # Act
        self.msg_bus.subscribe(msg_type=all_strings, handler=handler)

        result = self.msg_bus.channels()

        # Assert
        assert result == [str]

    def test_subscribe_when_handler_already_subscribed_does_not_add_subscription(self):
        # Arrange
        all_strings = MessageType(type=str)
        handler = [].append

        self.msg_bus.subscribe(msg_type=all_strings, handler=handler)

        # Act
        self.msg_bus.subscribe(msg_type=all_strings, handler=handler)

        result = self.msg_bus.channels()

        # Assert
        assert result == [str]

    def test_subscribe_to_all_returns_channels_list_including_none(self):
        # Arrange
        handler = [].append

        # Act
        self.msg_bus.subscribe(msg_type=None, handler=handler)

        result = self.msg_bus.channels()

        # Assert
        assert result == [None]

    def test_subscribe_to_msg_type_returns_subscriptions_list_including_handler(self):
        # Arrange
        all_strings = MessageType(type=str)
        handler = [].append

        # Act
        self.msg_bus.subscribe(msg_type=all_strings, handler=handler)

        result = self.msg_bus.subscriptions(all_strings)

        # Assert
        assert len(result) == 1
        assert result[0].handler == handler

    def test_subscribe_to_all_returns_subscriptions_list_including_handler(self):
        # Arrange
        handler = [].append

        # Act
        self.msg_bus.subscribe(msg_type=None, handler=handler)

        result = self.msg_bus.subscriptions()

        # Assert
        assert len(result) == 1
        assert result[0].handler == handler

    def test_subscribe_all_when_handler_already_subscribed_does_not_add_subscription(self):
        # Arrange
        handler = [].append

        self.msg_bus.subscribe(msg_type=None, handler=handler)

        # Act
        self.msg_bus.subscribe(msg_type=None, handler=handler)

        result = self.msg_bus.subscriptions()

        # Assert
        assert len(result) == 1
        assert result[0].handler == handler

    def test_unsubscribe_from_msg_type_returns_subscriptions_list_without_handler(self):
        # Arrange
        all_strings = MessageType(type=str)
        handler = [].append

        self.msg_bus.subscribe(msg_type=all_strings, handler=handler)

        # Act
        self.msg_bus.unsubscribe(msg_type=all_strings, handler=handler)

        result = self.msg_bus.subscriptions(all_strings)

        # Assert
        assert result == []

    def test_unsubscribe_from_msg_type_when_no_subscription_does_nothing(self):
        # Arrange
        all_strings = MessageType(type=str)
        handler = [].append

        # Act
        self.msg_bus.unsubscribe(msg_type=all_strings, handler=handler)

        result = self.msg_bus.subscriptions(all_strings)

        # Assert
        assert result == []

    def test_unsubscribe_from_all_returns_subscriptions_list_without_handler(self):
        # Arrange
        handler = [].append

        self.msg_bus.subscribe(msg_type=None, handler=handler)

        # Act
        self.msg_bus.unsubscribe(msg_type=None, handler=handler)

        result = self.msg_bus.subscriptions()

        # Assert
        assert result == []

    def test_unsubscribe_from_all_when_no_subscription_does_nothing(self):
        # Arrange
        handler = [].append

        # Act
        self.msg_bus.unsubscribe(msg_type=None, handler=handler)

        result = self.msg_bus.subscriptions()

        # Assert
        assert result == []

    def test_publish_with_no_subscribers_does_nothing(self):
        # Arrange
        string_msg = MessageType(type=str)

        # Act
        self.msg_bus.publish(string_msg, "hello world")

        # Assert
        assert True  # No exceptions raised

    def test_publish_with_subscriber_sends_to_handler(self):
        # Arrange
        subscriber = []
        string_msg = MessageType(type=str)

        self.msg_bus.subscribe(msg_type=string_msg, handler=subscriber.append)

        # Act
        self.msg_bus.publish(string_msg, "hello world")

        # Assert
        assert "hello world" in subscriber

    def test_publish_with_multiple_subscribers_sends_to_handlers(self):
        # Arrange
        subscriber1 = []
        subscriber2 = []
        subscriber3 = []
        string_msg = MessageType(type=str)

        self.msg_bus.subscribe(msg_type=string_msg, handler=subscriber1.append)
        self.msg_bus.subscribe(msg_type=string_msg, handler=subscriber2.append)
        self.msg_bus.subscribe(msg_type=string_msg, handler=subscriber3.append)

        # Act
        self.msg_bus.publish(string_msg, "hello world")

        # Assert
        assert "hello world" in subscriber1
        assert "hello world" in subscriber2
        assert "hello world" in subscriber3

    def test_publish_with_header_sends_to_handler(self):
        # Arrange
        subscriber = []
        status_msgs = MessageType(type=str, header={"topic": "status"})

        self.msg_bus.subscribe(msg_type=status_msgs, handler=subscriber.append)

        # Act
        self.msg_bus.publish(status_msgs, "OK!")

        # Assert
        assert "OK!" in subscriber

    def test_publish_with_none_matching_header_then_filters_from_subscriber(self):
        # Arrange
        subscriber = []
        string_msgs = MessageType(type=str)
        status_msgs = MessageType(type=str, header={"topic": "status"})

        self.msg_bus.subscribe(
            msg_type=status_msgs,
            handler=subscriber.append,
        )

        # Act
        self.msg_bus.publish(string_msgs, "OK!")

        # Assert
        assert "OK!" not in subscriber

    def test_publish_with_matching_subset_header_then_sends_to_subscriber(self):
        # Arrange
        subscriber = []
        status_msgs1 = MessageType(type=str, header={"topic": "status"})
        status_msgs2 = MessageType(type=str, header={"topic": "status", "extra": 0})

        self.msg_bus.subscribe(
            msg_type=status_msgs1,
            handler=subscriber.append,
        )

        # Act
        self.msg_bus.publish(status_msgs2, "OK!")

        # Assert
        assert "OK!" in subscriber

    def test_publish_with_both_channel_and_all_sub_sends_to_subscribers(self):
        # Arrange
        subscriber1 = []
        subscriber2 = []
        status_msgs = MessageType(type=str, header={"topic": "status"})

        self.msg_bus.subscribe(
            msg_type=status_msgs,
            handler=subscriber1.append,
        )

        self.msg_bus.subscribe(
            msg_type=None,  # <-- subscribe ALL
            handler=subscriber2.append,
        )

        # Act
        self.msg_bus.publish(status_msgs, "OK!")

        # Assert
        assert "OK!" in subscriber1
        assert "OK!" in subscriber2
