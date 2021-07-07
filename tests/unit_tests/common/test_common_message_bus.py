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
from nautilus_trader.core.message import MessageType


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

    def test_subscribe_to_msg_type_returns_handlers_list_including_handler(self):
        # Arrange
        all_strings = MessageType(type=str)
        handler = [].append

        # Act
        self.msg_bus.subscribe(msg_type=all_strings, handler=handler)

        result = self.msg_bus.subscriptions(all_strings)

        # Assert
        assert len(result) == 1
        assert result[0].handler == handler

    def test_process_with_no_subscribers_does_nothing(self):
        # Arrange
        string_msg = MessageType(type=str)

        # Act
        self.msg_bus.publish(string_msg, "hello world")

        # Assert
        assert True  # No exceptions raised

    def test_process_with_subscriber_sends_to_handler(self):
        # Arrange
        subscriber = []
        string_msg = MessageType(type=str)

        self.msg_bus.subscribe(msg_type=string_msg, handler=subscriber.append)

        # Act
        self.msg_bus.publish(string_msg, "hello world")

        # Assert
        assert "hello world" in subscriber

    def test_process_with_sub_and_header_sends_to_handler(self):
        # Arrange
        subscriber = []
        status_msgs = MessageType(type=str, header={"topic": "status"})

        self.msg_bus.subscribe(msg_type=status_msgs, handler=subscriber.append)

        # Act
        self.msg_bus.publish(status_msgs, "OK!")

        # Assert
        assert "OK!" in subscriber

    def test_process_with_sub_and_header_then_filters_from_sending_to_handler(self):
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
