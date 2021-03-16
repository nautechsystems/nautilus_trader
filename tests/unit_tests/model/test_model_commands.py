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

import pytest

from nautilus_trader.model.commands import Routing
from nautilus_trader.model.identifiers import Venue


class TestRouting:
    def test_instantiate_when_all_venues_none_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            Routing(None, None, None)

    def test_first(self):
        # Arrange
        routing1 = Routing(
            broker=Venue("IB"),
            intermediary=Venue("SMART"),
            exchange=Venue("NYSE"),
        )

        routing2 = Routing(
            broker=None,
            intermediary=Venue("JPMX"),
            exchange=Venue("NYSE"),
        )

        routing3 = Routing(
            broker=None,
            intermediary=None,
            exchange=Venue("BITMEX"),
        )

        # Act
        # Assert
        assert routing1.first() == Venue("IB")
        assert routing2.first() == Venue("JPMX")
        assert routing3.first() == Venue("BITMEX")

    def test_equality(self):
        # Arrange
        routing1 = Routing(
            broker=Venue("IB"),
            intermediary=Venue("SMART"),
            exchange=Venue("NYSE"),
        )

        routing2 = Routing(
            broker=Venue("IB"),
            intermediary=None,
            exchange=Venue("NYSE"),
        )

        routing3 = Routing(
            broker=None,
            intermediary=None,
            exchange=Venue("BITMEX"),
        )

        # Act
        # Assert
        assert routing1 == routing1
        assert routing1 != routing2
        assert routing1 != routing3

    def test_hash_str_and_repr(self):
        # Arrange
        routing1 = Routing(
            broker=Venue("IB"),
            intermediary=Venue("SMART"),
            exchange=Venue("NYSE"),
        )

        routing2 = Routing(
            broker=Venue("IB"),
            intermediary=None,
            exchange=Venue("NYSE"),
        )

        routing3 = Routing(
            broker=None,
            intermediary=None,
            exchange=Venue("BITMEX"),
        )

        # Act
        # Assert
        assert isinstance(hash(routing1), int)
        assert hash(routing1) == hash(routing1)
        assert str(routing1) == "IB->SMART->NYSE"
        assert str(routing2) == "IB->NYSE"
        assert str(routing3) == "BITMEX"
        assert repr(routing1) == "Routing('IB->SMART->NYSE')"

    def test_serialization(self):
        # Arrange
        routing1 = Routing(
            broker=Venue("IB"),
            intermediary=Venue("SMART"),
            exchange=Venue("NYSE"),
        )

        routing2 = Routing(
            broker=Venue("IB"),
            intermediary=None,
            exchange=Venue("NYSE"),
        )

        routing3 = Routing(
            broker=None,
            intermediary=None,
            exchange=Venue("BITMEX"),
        )

        # Act
        # Assert
        assert routing1.to_serializable_str() == "IB,SMART,NYSE"
        assert routing2.to_serializable_str() == "IB,,NYSE"
        assert routing3.to_serializable_str() == ",,BITMEX"
        assert (
            routing1.from_serializable_str(routing1.to_serializable_str()) == routing1
        )
        assert (
            routing2.from_serializable_str(routing2.to_serializable_str()) == routing2
        )
        assert (
            routing3.from_serializable_str(routing3.to_serializable_str()) == routing3
        )
