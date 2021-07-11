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

from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import Identifier
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId


class TestIdentifiers:
    @pytest.mark.parametrize(
        "value, ex",
        [
            [None, TypeError],
            ["", ValueError],
            [" ", ValueError],
            ["  ", ValueError],
            [1234, TypeError],
        ],
    )
    def test_instantiate_given_various_invalid_values_raises_exception(self, value, ex):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ex):
            Identifier(value)

    def test_equality(self):
        # Arrange
        id1 = Identifier("abc123")
        id2 = Identifier("abc123")
        id3 = Identifier("def456")

        # Act
        # Assert
        assert "abc123" == id1.value
        assert id1 == id1
        assert id1 == id2
        assert id1 != id3

    def test_equality_of_subclass(self):
        # Arrange
        id1 = Venue("BINANCE")
        id2 = Venue("BINANCE")
        id3 = InstrumentId(Symbol("BINANCE"), Venue("BINANCE"))  # Invalid
        id4 = Identifier("BINANCE")

        # Act
        # Assert
        assert id1 == id1
        assert id2 == id2
        assert id1 == id2
        assert id2 == id1
        assert id1 != id3
        assert id2 != id3
        assert id2 != id4
        assert id4 != id1

    def test_comparison(self):
        # Arrange
        string1 = Identifier("123")
        string2 = Identifier("456")
        string3 = Identifier("abc")
        string4 = Identifier("def")

        # Act
        # Assert
        assert string1 <= string1
        assert string1 <= string2
        assert string1 < string2
        assert string2 > string1
        assert string2 >= string1
        assert string2 >= string2
        assert string3 <= string4

    def test_hash(self):
        # Arrange
        identifier1 = Identifier("abc")
        identifier2 = Identifier("abc")

        # Act
        # Assert
        assert isinstance(hash(identifier1), int)
        assert hash(identifier1) == hash(identifier2)

    def test_identifier_equality(self):
        # Arrange
        id1 = Identifier("some-id-1")
        id2 = Identifier("some-id-2")

        # Act
        # Assert
        assert id1 == id1
        assert id1 != id2

    def test_identifier_to_str(self):
        # Arrange
        identifier = Identifier("some-id")

        # Act
        result = str(identifier)

        # Assert
        assert "some-id" == result

    def test_identifier_repr(self):
        # Arrange
        identifier = Identifier("some-id")

        # Act
        result = repr(identifier)

        # Assert
        assert "Identifier('some-id')" == result

    def test_mixed_identifier_equality(self):
        # Arrange
        id1 = ClientOrderId("O-123456")
        id2 = PositionId("P-123456")

        # Act
        # Assert
        assert id1 == id1
        assert id1 != id2

    def test_account_id_given_malformed_string_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            AccountId.from_str("BAD_STRING")

    def test_strategy_id_given_malformed_string_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            StrategyId("BAD_STRING")

    def test_trader_id_given_malformed_string_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            TraderId("BAD_STRING")

    def test_trader_identifier(self):
        # Arrange
        # Act
        trader_id1 = TraderId("TESTER-000")
        trader_id2 = TraderId("TESTER-001")

        # Assert
        assert trader_id1 == trader_id1
        assert trader_id1 != trader_id2
        assert "TESTER-000" == trader_id1.value
        assert trader_id1.get_tag() == "000"

    def test_account_identifier(self):
        # Arrange
        # Act
        account_id1 = AccountId("SIM", "02851908")
        account_id2 = AccountId("SIM", "09999999")

        # Assert
        assert account_id1 == account_id1
        assert account_id1 != account_id2
        assert "SIM-02851908", account_id1.value
        assert account_id1 == AccountId("SIM", "02851908")

    def test_position_identifier(self):
        # Arrange
        # Act
        position_id0 = PositionId.null()

        # Assert
        assert "NULL" == position_id0.value

    def test_order_identifier(self):
        # Arrange
        # Act
        order_id = VenueOrderId.null()

        # Assert
        assert "NULL" == order_id.value


class TestVenue:
    def test_instrument_id_equality(self):
        # Arrange
        venue1 = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        venue2 = InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO"))
        venue3 = InstrumentId(Symbol("GBP/USD"), Venue("SIM"))

        # Act
        # Assert
        assert venue1 == venue1
        assert venue1 != venue2
        assert venue1 != venue3

    def test_instrument_id_str(self):
        # Arrange
        venue = Venue("NYMEX")

        # Act
        # Assert
        assert str(venue) == "NYMEX"

    def test_venue_repr(self):
        # Arrange
        venue = Venue("NYMEX")

        # Act
        # Assert
        assert repr(venue) == "Venue('NYMEX')"


class TestInstrumentId:
    def test_instrument_id_equality(self):
        # Arrange
        instrument_id1 = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        instrument_id2 = InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO"))
        instrument_id3 = InstrumentId(Symbol("GBP/USD"), Venue("SIM"))

        # Act
        # Assert
        assert instrument_id1 == instrument_id1
        assert instrument_id1 != instrument_id2
        assert instrument_id1 != instrument_id3

    def test_instrument_id_str(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

        # Act
        # Assert
        assert "AUD/USD.SIM" == str(instrument_id)

    def test_instrument_id_repr(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

        # Act
        # Assert
        assert "InstrumentId('AUD/USD.SIM')" == repr(instrument_id)

    def test_parse_instrument_id_from_str(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

        # Act
        result = InstrumentId.from_str(str(instrument_id))

        # Assert
        assert instrument_id == result
