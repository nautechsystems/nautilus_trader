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

from nautilus_trader.common.component import Subscription


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
            f"Subscription(topic=*, handler={handler_str}, priority=0)",
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
