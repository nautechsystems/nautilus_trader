# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import pickle

import pytest

from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue


class TestIdentifiers:
    def test_equality(self):
        # Arrange
        id1 = Symbol("abc123")
        id2 = Symbol("abc123")
        id3 = Symbol("def456")

        # Act, Assert
        assert id1.value == "abc123"
        assert id1 == id1
        assert id1 == id2
        assert id1 != id3

    def test_comparison(self):
        # Arrange
        string1 = Symbol("123")
        string2 = Symbol("456")
        string3 = Symbol("abc")
        string4 = Symbol("def")

        # Act, Assert
        assert string1 <= string1
        assert string1 <= string2
        assert string1 < string2
        assert string2 > string1
        assert string2 >= string1
        assert string2 >= string2
        assert string3 <= string4

    def test_hash(self):
        # Arrange
        identifier1 = Symbol("abc")
        identifier2 = Symbol("abc")

        # Act, Assert
        assert isinstance(hash(identifier1), int)
        assert hash(identifier1) == hash(identifier2)

    def test_identifier_equality(self):
        # Arrange
        id1 = Symbol("some-id-1")
        id2 = Symbol("some-id-2")

        # Act, Assert
        assert id1 == id1
        assert id1 != id2

    def test_identifier_to_str(self):
        # Arrange
        identifier = Symbol("some-id")

        # Act
        result = str(identifier)

        # Assert
        assert result == "some-id"

    def test_identifier_repr(self):
        # Arrange
        identifier = Symbol("some-id")

        # Act
        result = repr(identifier)

        # Assert
        assert result == "Symbol('some-id')"

    def test_trader_identifier(self):
        # Arrange, Act
        trader_id1 = TraderId("TESTER-000")
        trader_id2 = TraderId("TESTER-001")

        # Assert
        assert trader_id1 == trader_id1
        assert trader_id1 != trader_id2
        assert trader_id1.value == "TESTER-000"
        assert trader_id1.get_tag() == "000"

    def test_account_identifier(self):
        # Arrange, Act
        account_id1 = AccountId("SIM-02851908")
        account_id2 = AccountId("SIM-09999999")

        # Assert
        assert account_id1 == account_id1
        assert account_id1 != account_id2
        assert "SIM-02851908", account_id1.value
        assert account_id1 == AccountId("SIM-02851908")


class TestSymbol:
    def test_symbol_equality(self):
        # Arrange
        symbol1 = Symbol("AUD/USD")
        symbol2 = Symbol("ETH/USD")
        symbol3 = Symbol("AUD/USD")

        # Act, Assert
        assert symbol1 == symbol1
        assert symbol1 != symbol2
        assert symbol1 == symbol3

    def test_symbol_str(self):
        # Arrange
        symbol = Symbol("AUD/USD")

        # Act, Assert
        assert str(symbol) == "AUD/USD"

    def test_symbol_repr(self):
        # Arrange
        symbol = Symbol("AUD/USD")

        # Act, Assert
        assert repr(symbol) == "Symbol('AUD/USD')"

    def test_symbol_pickling(self):
        # Arrange
        symbol = Symbol("AUD/USD")

        # Act
        pickled = pickle.dumps(symbol)
        unpickled = pickle.loads(pickled)  # noqa S301 (pickle is safe here)

        # Act, Assert
        assert symbol == unpickled


class TestVenue:
    def test_venue_equality(self):
        # Arrange
        venue1 = Venue("SIM")
        venue2 = Venue("IDEALPRO")
        venue3 = Venue("SIM")

        # Act, Assert
        assert venue1 == venue1
        assert venue1 != venue2
        assert venue1 == venue3

    def test_venue_is_synthetic(self):
        # Arrange
        venue1 = Venue("SYNTH")
        venue2 = Venue("SIM")

        # Act, Assert
        assert venue1.is_synthetic()
        assert not venue2.is_synthetic()

    def test_venue_str(self):
        # Arrange
        venue = Venue("NYMEX")

        # Act, Assert
        assert str(venue) == "NYMEX"

    def test_venue_repr(self):
        # Arrange
        venue = Venue("NYMEX")

        # Act, Assert
        assert repr(venue) == "Venue('NYMEX')"

    def test_venue_pickling(self):
        # Arrange
        venue = Venue("NYMEX")

        # Act
        pickled = pickle.dumps(venue)
        unpickled = pickle.loads(pickled)  # noqa S301 (pickle is safe here)

        # Act, Assert
        assert venue == unpickled


class TestInstrumentId:
    def test_instrument_id_equality(self):
        # Arrange
        instrument_id1 = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        instrument_id2 = InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO"))
        instrument_id3 = InstrumentId(Symbol("GBP/USD"), Venue("SIM"))

        # Act, Assert
        assert instrument_id1 == instrument_id1
        assert instrument_id1 != instrument_id2
        assert instrument_id1 != instrument_id3

    def test_instrument_id_is_synthetic(self):
        # Arrange
        instrument_id1 = InstrumentId(Symbol("BTC-ETH"), Venue("SYNTH"))
        instrument_id2 = InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO"))

        # Act, Assert
        assert instrument_id1.is_synthetic()
        assert not instrument_id2.is_synthetic()

    def test_instrument_id_str(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

        # Act, Assert
        assert str(instrument_id) == "AUD/USD.SIM"

    def test_pickling(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

        # Act
        pickled = pickle.dumps(instrument_id)
        unpickled = pickle.loads(pickled)  # noqa S301 (pickle is safe here)

        # Act, Assert
        assert unpickled == instrument_id

    def test_instrument_id_repr(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

        # Act, Assert
        assert repr(instrument_id) == "InstrumentId('AUD/USD.SIM')"

    def test_parse_instrument_id_from_str(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

        # Act
        result = InstrumentId.from_str(str(instrument_id))

        # Assert
        assert str(result.symbol) == "AUD/USD"
        assert str(result.venue) == "SIM"
        assert result == instrument_id


class TestStrategyId:
    def test_is_external(self):
        # Arrange
        strategy1 = StrategyId("EXTERNAL")
        strategy2 = StrategyId("MyStrategy-001")

        # Act, Assert
        assert strategy1.is_external()
        assert not strategy2.is_external()


class TestExecAlgorithmId:
    def test_exec_algorithm_id(self):
        # Arrange
        exec_algorithm_id1 = ExecAlgorithmId("VWAP")
        exec_algorithm_id2 = ExecAlgorithmId("TWAP")

        # Act, Assert
        assert exec_algorithm_id1 == exec_algorithm_id1
        assert exec_algorithm_id1 != exec_algorithm_id2
        assert isinstance(hash(exec_algorithm_id1), int)
        assert str(exec_algorithm_id1) == "VWAP"
        assert repr(exec_algorithm_id1) == "ExecAlgorithmId('VWAP')"


@pytest.mark.parametrize(
    ("client_order_id", "trader_id", "expected"),
    [
        [
            ClientOrderId("O-20210410-022422-001-001-001"),
            TraderId("TRADER-001"),
            True,
        ],
        [
            ClientOrderId("O-20210410-022422-001-001-001"),
            TraderId("TRADER-000"),  # <-- Different trader ID
            False,
        ],
        [
            ClientOrderId("O-001"),  # <-- Some custom ID without enough components
            TraderId("TRADER-001"),
            False,
        ],
    ],
)
def test_client_order_id_is_this_trader(
    client_order_id: ClientOrderId,
    trader_id: TraderId,
    expected: bool,
) -> None:
    # Arrange, Act, Assert
    assert client_order_id.is_this_trader(trader_id) == expected
