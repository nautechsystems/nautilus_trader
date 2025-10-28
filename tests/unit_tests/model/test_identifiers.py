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

import pickle

import pytest

from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue


def test_trader_identifier() -> None:
    # Arrange, Act
    trader_id1 = TraderId("TESTER-000")
    trader_id2 = TraderId("TESTER-001")

    # Assert
    assert trader_id1 == trader_id1
    assert trader_id1 != trader_id2
    assert trader_id1.value == "TESTER-000"


def test_account_identifier() -> None:
    # Arrange, Act
    account_id1 = AccountId("SIM-02851908")
    account_id2 = AccountId("SIM-09999999")

    # Assert
    assert account_id1 == account_id1
    assert account_id1 != account_id2
    assert "SIM-02851908", account_id1.value
    assert account_id1 == AccountId("SIM-02851908")


def test_symbol_equality() -> None:
    # Arrange
    symbol1 = Symbol("AUD/USD")
    symbol2 = Symbol("ETH/USD")
    symbol3 = Symbol("AUD/USD")

    # Act, Assert
    assert symbol1 == symbol1
    assert symbol1 != symbol2
    assert symbol1 == symbol3


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ["AUDUSD", False],
        ["AUD/USD", False],
        ["CL.FUT", True],
        ["LO.OPT", True],
        ["ES.c.0", True],
    ],
)
def test_symbol_is_composite(value: str, expected: str) -> None:
    # Arrange
    symbol = Symbol(value)

    # Act, Assert
    assert symbol.is_composite() == expected


@pytest.mark.parametrize(
    ("value", "expected_root"),
    [
        ["AUDUSD", "AUDUSD"],
        ["AUD/USD", "AUD/USD"],
        ["CL.FUT", "CL"],
        ["LO.OPT", "LO"],
        ["ES.c.0", "ES"],
    ],
)
def test_symbol_root(value: str, expected_root: str) -> None:
    # Arrange
    symbol = Symbol(value)

    # Act, Assert
    assert symbol.root() == expected_root


@pytest.mark.parametrize(
    ("value", "expected_topic"),
    [
        ["AUDUSD", "AUDUSD"],
        ["AUD/USD", "AUD/USD"],
        ["CL.FUT", "CL*"],
        ["LO.OPT", "LO*"],
        ["ES.c.0", "ES*"],
    ],
)
def test_symbol_topic(value: str, expected_topic: str) -> None:
    # Arrange
    symbol = Symbol(value)

    # Act, Assert
    assert symbol.topic() == expected_topic


def test_symbol_str() -> None:
    # Arrange
    symbol = Symbol("AUD/USD")

    # Act, Assert
    assert str(symbol) == "AUD/USD"


def test_symbol_repr() -> None:
    # Arrange
    symbol = Symbol("AUD/USD")

    # Act, Assert
    assert repr(symbol) == "Symbol('AUD/USD')"


def test_symbol_pickling() -> None:
    # Arrange
    symbol = Symbol("AUD/USD")

    # Act
    pickled = pickle.dumps(symbol)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

    # Act, Assert
    assert symbol == unpickled


def test_venue_equality() -> None:
    # Arrange
    venue1 = Venue("SIM")
    venue2 = Venue("IDEALPRO")
    venue3 = Venue("SIM")

    # Act, Assert
    assert venue1 == venue1
    assert venue1 != venue2
    assert venue1 == venue3


def test_venue_str() -> None:
    # Arrange
    venue = Venue("NYMEX")

    # Act, Assert
    assert str(venue) == "NYMEX"


def test_venue_from_code_when_not_found() -> None:
    # Arrange, Act
    result = Venue.from_code("UNKNOWN")

    # Assert
    assert result is None


def test_venue_from_code() -> None:
    # Arrange, Act
    result = Venue.from_code("XCME")

    # Assert
    assert isinstance(result, Venue)
    assert result.value == "XCME"


def test_venue_repr() -> None:
    # Arrange
    venue = Venue("NYMEX")

    # Act, Assert
    assert repr(venue) == "Venue('NYMEX')"


def test_venue_pickling() -> None:
    # Arrange
    venue = Venue("NYMEX")

    # Act
    pickled = pickle.dumps(venue)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

    # Act, Assert
    assert venue == unpickled


def test_instrument_id_equality() -> None:
    # Arrange
    instrument_id1 = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
    instrument_id2 = InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO"))
    instrument_id3 = InstrumentId(Symbol("GBP/USD"), Venue("SIM"))

    # Act, Assert
    assert instrument_id1 == instrument_id1
    assert instrument_id1 != instrument_id2
    assert instrument_id1 != instrument_id3


def test_instrument_id_str() -> None:
    # Arrange
    instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

    # Act, Assert
    assert str(instrument_id) == "AUD/USD.SIM"


def test_instrument_id_pickling() -> None:
    # Arrange
    instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

    # Act
    pickled = pickle.dumps(instrument_id)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

    # Act, Assert
    assert unpickled == instrument_id


def test_instrument_id_repr() -> None:
    # Arrange
    instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

    # Act, Assert
    assert repr(instrument_id) == "InstrumentId('AUD/USD.SIM')"


def test_instrument_id_from_str() -> None:
    # Arrange
    instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

    # Act
    result = InstrumentId.from_str(str(instrument_id))

    # Assert
    assert str(result.symbol) == "AUD/USD"
    assert str(result.venue) == "SIM"
    assert result == instrument_id


def test_instrument_id_from_str_with_utf8_symbol() -> None:
    # Arrange
    non_ascii_symbol = "TËST-PÉRP"
    non_ascii_instrument = "TËST-PÉRP.BINANCE"

    # Act
    result = InstrumentId.from_str(non_ascii_instrument)

    # Assert
    assert str(result.symbol) == non_ascii_symbol
    assert str(result.venue) == "BINANCE"
    assert str(result) == non_ascii_instrument


@pytest.mark.parametrize(
    ("input", "expected_err"),
    [
        [
            "BTCUSDT",
            "Error parsing `InstrumentId` from 'BTCUSDT': missing '.' separator between symbol and venue components",
        ],
        [
            ".USDT",
            "invalid string for 'value', was empty",  # TODO: Improve error message
        ],
        [
            "BTC.",
            "invalid string for 'value', was empty",  # TODO: Improve error message
        ],
    ],
)
def test_instrument_id_from_str_when_invalid(input: str, expected_err: str) -> None:
    # Arrange, Act
    with pytest.raises(ValueError) as exc_info:
        InstrumentId.from_str(input)

    # Assert
    assert str(exc_info.value) == expected_err


def test_exec_algorithm_id() -> None:
    # Arrange
    exec_algorithm_id1 = ExecAlgorithmId("VWAP")
    exec_algorithm_id2 = ExecAlgorithmId("TWAP")

    # Act, Assert
    assert exec_algorithm_id1 == exec_algorithm_id1
    assert exec_algorithm_id1 != exec_algorithm_id2
    assert isinstance(hash(exec_algorithm_id1), int)
    assert str(exec_algorithm_id1) == "VWAP"
    assert repr(exec_algorithm_id1) == "ExecAlgorithmId('VWAP')"


def test_trade_id_maximum_length() -> None:
    # Arrange, Act, Assert
    with pytest.raises(ValueError):
        TradeId("A" * 37)


def test_instrument_id_new_spread_single_positive_ratio() -> None:
    # Arrange
    id1 = InstrumentId(Symbol("AAPL"), Venue("NASDAQ"))

    # Act, Assert - single leg spreads should raise an error
    with pytest.raises(ValueError, match="instrument_ratios list needs to have at least 2 legs"):
        InstrumentId.new_spread([(id1, 1)])


def test_instrument_id_new_spread_single_negative_ratio() -> None:
    # Arrange
    id1 = InstrumentId(Symbol("AAPL"), Venue("NASDAQ"))

    # Act, Assert - single leg spreads should raise an error
    with pytest.raises(ValueError, match="instrument_ratios list needs to have at least 2 legs"):
        InstrumentId.new_spread([(id1, -2)])


def test_instrument_id_new_spread_multiple_instruments_sorted() -> None:
    # Arrange - note the order: MSFT, AAPL, GOOGL (not alphabetical)
    id1 = InstrumentId(Symbol("MSFT"), Venue("NASDAQ"))
    id2 = InstrumentId(Symbol("AAPL"), Venue("NASDAQ"))
    id3 = InstrumentId(Symbol("GOOGL"), Venue("NASDAQ"))

    # Act
    result = InstrumentId.new_spread([(id1, 1), (id2, -2), (id3, 3)])

    # Assert - should be sorted alphabetically: AAPL, GOOGL, MSFT
    assert result.symbol.value == "((2))AAPL_(3)GOOGL_(1)MSFT"
    assert result.venue == Venue("NASDAQ")


def test_instrument_id_new_spread_empty_list_raises_error() -> None:
    # Arrange, Act, Assert
    with pytest.raises(ValueError, match="instrument_ratios list needs to have at least 2 legs"):
        InstrumentId.new_spread([])


def test_instrument_id_new_spread_zero_ratio_raises_error() -> None:
    # Arrange
    id1 = InstrumentId(Symbol("AAPL"), Venue("NASDAQ"))
    id2 = InstrumentId(Symbol("MSFT"), Venue("NASDAQ"))

    # Act, Assert - need at least 2 legs, but one has zero ratio
    with pytest.raises(ValueError, match="ratio cannot be zero"):
        InstrumentId.new_spread([(id1, 1), (id2, 0)])


def test_instrument_id_new_spread_mismatched_venues_raises_error() -> None:
    # Arrange
    id1 = InstrumentId(Symbol("AAPL"), Venue("NASDAQ"))
    id2 = InstrumentId(Symbol("MSFT"), Venue("NYSE"))

    # Act, Assert
    with pytest.raises(ValueError, match="All venues must match"):
        InstrumentId.new_spread([(id1, 1), (id2, -1)])


def test_instrument_id_to_list_single_positive_ratio() -> None:
    # Arrange
    combo = InstrumentId(Symbol("(1)AAPL"), Venue("NASDAQ"))

    # Act
    result = combo.to_list()

    # Assert
    assert len(result) == 1
    assert result[0][0] == InstrumentId(Symbol("AAPL"), Venue("NASDAQ"))
    assert result[0][1] == 1


def test_instrument_id_to_list_single_negative_ratio() -> None:
    # Arrange
    combo = InstrumentId(Symbol("((2))AAPL"), Venue("NASDAQ"))

    # Act
    result = combo.to_list()

    # Assert
    assert len(result) == 1
    assert result[0][0] == InstrumentId(Symbol("AAPL"), Venue("NASDAQ"))
    assert result[0][1] == -2


def test_instrument_id_to_list_multiple_instruments() -> None:
    # Arrange
    combo = InstrumentId(Symbol("(1)AAPL_((2))MSFT_(3)GOOGL"), Venue("NASDAQ"))

    # Act
    result = combo.to_list()

    # Assert - should be sorted alphabetically
    assert len(result) == 3
    assert result[0] == (InstrumentId(Symbol("AAPL"), Venue("NASDAQ")), 1)
    assert result[1] == (InstrumentId(Symbol("GOOGL"), Venue("NASDAQ")), 3)
    assert result[2] == (InstrumentId(Symbol("MSFT"), Venue("NASDAQ")), -2)


def test_instrument_id_to_list_invalid_format_raises_error() -> None:
    # Arrange
    combo = InstrumentId(Symbol("INVALID_FORMAT"), Venue("NASDAQ"))

    # Act, Assert
    with pytest.raises(ValueError, match="Invalid symbol format for component"):
        combo.to_list()


def test_instrument_id_new_spread_to_list_roundtrip() -> None:
    # Arrange - provide in alphabetical order since that's how they'll be returned
    id1 = InstrumentId(Symbol("AAPL"), Venue("NASDAQ"))
    id2 = InstrumentId(Symbol("GOOGL"), Venue("NASDAQ"))
    id3 = InstrumentId(Symbol("MSFT"), Venue("NASDAQ"))
    original_list = [(id1, 1), (id2, 3), (id3, -2)]  # Alphabetical order

    # Act
    spread = InstrumentId.new_spread(original_list)
    result_list = spread.to_list()

    # Assert
    assert result_list == original_list


def test_instrument_id_is_spread_false_for_simple_symbol() -> None:
    # Arrange
    instrument_id = InstrumentId(Symbol("AAPL"), Venue("NASDAQ"))

    # Act, Assert
    assert not instrument_id.is_spread()


def test_instrument_id_is_spread_true_for_spread_symbol() -> None:
    # Arrange
    instrument_id = InstrumentId(Symbol("(1)AAPL_((2))MSFT"), Venue("NASDAQ"))

    # Act, Assert
    assert instrument_id.is_spread()


def test_instrument_id_is_spread_true_for_symbol_with_underscore() -> None:
    # Arrange
    instrument_id = InstrumentId(Symbol("SOME_SYMBOL"), Venue("NASDAQ"))

    # Act, Assert
    assert instrument_id.is_spread()
