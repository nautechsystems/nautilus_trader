# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Test identifiers behavior.
"""

import pickle
import re

import pytest

from nautilus_trader.model import AccountId
from nautilus_trader.model import ActorId
from nautilus_trader.model import ClientId
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ComponentId
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OptionSeriesId
from nautilus_trader.model import OrderListId
from nautilus_trader.model import PositionId
from nautilus_trader.model import StrategyId
from nautilus_trader.model import Symbol
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.model import VenueOrderId


GENERATED_IDENTIFIER_CASES = (
    (ActorId, "Actor-001"),
    (AccountId, "SIM-001"),
    (ClientId, "Client-001"),
    (ClientOrderId, "Order-001"),
    (ComponentId, "Component-001"),
    (ExecAlgorithmId, "Algorithm-001"),
    (OrderListId, "OrderList-001"),
    (PositionId, "Position-001"),
    (StrategyId, "Strategy-001"),
    (TraderId, "Trader-001"),
    (Venue, "Venue-001"),
    (VenueOrderId, "VenueOrder-001"),
)


def test_trader_id_equality_and_value() -> None:
    """
    Test trader id equality and value.
    """
    tid1 = TraderId("TESTER-000")
    tid2 = TraderId("TESTER-001")

    assert tid1 == tid1
    assert tid1 != tid2
    assert tid1.value == "TESTER-000"


def test_account_id_equality_and_value() -> None:
    """
    Test account id equality and value.
    """
    aid1 = AccountId("SIM-02851908")
    aid2 = AccountId("SIM-09999999")

    assert aid1 == aid1
    assert aid1 != aid2
    assert aid1.value == "SIM-02851908"
    assert aid1 == AccountId("SIM-02851908")


def test_actor_id_equality_and_value() -> None:
    """
    Test actor id equality and value.
    """
    actor_id = ActorId("actor-001")
    restored = ActorId.from_str("actor-001")

    assert actor_id == restored
    assert hash(actor_id) == hash(restored)
    assert actor_id.value == "actor-001"


def test_symbol_equality() -> None:
    """
    Test symbol equality.
    """
    s1 = Symbol("AUD/USD")
    s2 = Symbol("ETH/USD")
    s3 = Symbol("AUD/USD")

    assert s1 == s1
    assert s1 != s2
    assert s1 == s3


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("AUDUSD", False),
        ("AUD/USD", False),
        ("CL.FUT", True),
        ("LO.OPT", True),
        ("ES.c.0", True),
    ],
)
def test_symbol_is_composite(value: object, expected: object) -> None:
    """
    Test symbol is composite.
    """
    assert Symbol(value).is_composite == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("AUDUSD", "AUDUSD"),
        ("AUD/USD", "AUD/USD"),
        ("CL.FUT", "CL"),
        ("LO.OPT", "LO"),
        ("ES.c.0", "ES"),
    ],
)
def test_symbol_root(value: object, expected: object) -> None:
    """
    Test symbol root.
    """
    assert Symbol(value).root == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("AUDUSD", "AUDUSD"),
        ("AUD/USD", "AUD/USD"),
        ("CL.FUT", "CL*"),
        ("LO.OPT", "LO*"),
        ("ES.c.0", "ES*"),
    ],
)
def test_symbol_topic(value: object, expected: object) -> None:
    """
    Test symbol topic.
    """
    assert Symbol(value).topic == expected


def test_symbol_str_and_repr() -> None:
    """
    Test symbol str and repr.
    """
    symbol = Symbol("AUD/USD")
    assert str(symbol) == "AUD/USD"
    assert repr(symbol) == "Symbol('AUD/USD')"


def test_symbol_pickle() -> None:
    """
    Test symbol pickle.
    """
    symbol = Symbol("AUD/USD")
    pickled = pickle.dumps(symbol)
    unpickled = pickle.loads(pickled)
    assert unpickled == symbol


def test_venue_equality() -> None:
    """
    Test venue equality.
    """
    v1 = Venue("SIM")
    v2 = Venue("IDEALPRO")
    v3 = Venue("SIM")

    assert v1 == v1
    assert v1 != v2
    assert v1 == v3


def test_venue_str_and_repr() -> None:
    """
    Test venue str and repr.
    """
    venue = Venue("NYMEX")
    assert str(venue) == "NYMEX"
    assert repr(venue) == "Venue('NYMEX')"


def test_venue_pickle() -> None:
    """
    Test venue pickle.
    """
    venue = Venue("NYMEX")
    pickled = pickle.dumps(venue)
    unpickled = pickle.loads(pickled)
    assert unpickled == venue


def test_instrument_id_equality() -> None:
    """
    Test instrument id equality.
    """
    id1 = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
    id2 = InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO"))
    id3 = InstrumentId(Symbol("GBP/USD"), Venue("SIM"))

    assert id1 == id1
    assert id1 != id2
    assert id1 != id3


def test_instrument_id_str_and_repr() -> None:
    """
    Test instrument id str and repr.
    """
    iid = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
    assert str(iid) == "AUD/USD.SIM"
    assert repr(iid) == "InstrumentId('AUD/USD.SIM')"


def test_instrument_id_from_str() -> None:
    """
    Test instrument id from str.
    """
    iid = InstrumentId.from_str("AUD/USD.SIM")
    assert str(iid.symbol) == "AUD/USD"
    assert str(iid.venue) == "SIM"


def test_instrument_id_from_str_with_utf8() -> None:
    """
    Test instrument id from str with utf8.
    """
    iid = InstrumentId.from_str("TËST-PÉRP.BINANCE")
    assert str(iid.symbol) == "TËST-PÉRP"
    assert str(iid.venue) == "BINANCE"


@pytest.mark.parametrize(
    ("value", "expected_err"),
    [
        (
            "BTCUSDT",
            "invalid `InstrumentId` value 'BTCUSDT': "
            "missing '.' separator between symbol and venue components",
        ),
        (
            ".USDT",
            "invalid `InstrumentId` value '.USDT': "
            "invalid symbol: invalid string for 'value', was empty",
        ),
        (
            "BTC.",
            "invalid `InstrumentId` value 'BTC.': "
            "invalid venue: invalid string for 'value', was empty",
        ),
    ],
)
def test_instrument_id_from_str_invalid(value: object, expected_err: object) -> None:
    """
    Test instrument id from str invalid.
    """
    with pytest.raises(ValueError, match=re.escape(expected_err)) as exc_info:
        InstrumentId.from_str(value)

    assert str(exc_info.value) == expected_err


def test_instrument_id_pickle() -> None:
    """
    Test instrument id pickle.
    """
    iid = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
    pickled = pickle.dumps(iid)
    unpickled = pickle.loads(pickled)
    assert unpickled == iid


def test_option_series_id_construction() -> None:
    """
    Test option series id construction.
    """
    series_id = OptionSeriesId("DERIBIT", "ETH", "USDC", 1_700_000_000_000_000_000)

    assert series_id.venue == Venue("DERIBIT")
    assert series_id.underlying == "ETH"
    assert series_id.settlement_currency == "USDC"
    assert series_id.expiration_ns == 1_700_000_000_000_000_000
    assert series_id.value == "DERIBIT:ETH:USDC:2023-11-14T22:13:20Z"


@pytest.mark.parametrize(
    ("venue", "expected_err"),
    [
        (
            "",
            "invalid `OptionSeriesId` value ':ETH:USDC:1700000000000000000': "
            "invalid venue: invalid string for 'value', was empty",
        ),
        (
            "DÉRIBIT",
            "invalid `OptionSeriesId` value 'DÉRIBIT:ETH:USDC:1700000000000000000': "
            "invalid venue: invalid string for 'value' contained a non-ASCII char, was 'DÉRIBIT'",
        ),
    ],
)
def test_option_series_id_invalid_venue_raises(venue: Venue, expected_err: object) -> None:
    """
    Test option series id invalid venue raises.
    """
    with pytest.raises(ValueError, match=re.escape(expected_err)) as exc_info:
        OptionSeriesId(venue, "ETH", "USDC", 1_700_000_000_000_000_000)

    assert type(exc_info.value) is ValueError
    assert str(exc_info.value) == expected_err


def test_exec_algorithm_id() -> None:
    """
    Test exec algorithm id.
    """
    ea1 = ExecAlgorithmId("VWAP")
    ea2 = ExecAlgorithmId("TWAP")

    assert ea1 == ea1
    assert ea1 != ea2
    assert isinstance(hash(ea1), int)
    assert str(ea1) == "VWAP"
    assert repr(ea1) == "ExecAlgorithmId('VWAP')"


def test_client_id() -> None:
    """
    Test client id.
    """
    c1 = ClientId("MyClient")
    c2 = ClientId("OtherClient")
    c3 = ClientId("MyClient")

    assert c1 == c1
    assert c1 != c2
    assert c1 == c3
    assert c1.value == "MyClient"
    assert str(c1) == "MyClient"
    assert repr(c1) == "ClientId('MyClient')"


def test_client_order_id() -> None:
    """
    Test client order id.
    """
    co1 = ClientOrderId("O-123456")
    co2 = ClientOrderId("O-789012")
    co3 = ClientOrderId("O-123456")

    assert co1 == co1
    assert co1 != co2
    assert co1 == co3
    assert co1.value == "O-123456"
    assert str(co1) == "O-123456"
    assert repr(co1) == "ClientOrderId('O-123456')"


def test_component_id() -> None:
    """
    Test component id.
    """
    comp1 = ComponentId("MyComponent")
    comp2 = ComponentId("OtherComponent")

    assert comp1 == comp1
    assert comp1 != comp2
    assert comp1.value == "MyComponent"
    assert str(comp1) == "MyComponent"
    assert repr(comp1) == "ComponentId('MyComponent')"


def test_strategy_id() -> None:
    """
    Test strategy id.
    """
    s1 = StrategyId("S-001")
    s2 = StrategyId("S-002")

    assert s1 == s1
    assert s1 != s2
    assert s1.value == "S-001"
    assert str(s1) == "S-001"
    assert repr(s1) == "StrategyId('S-001')"


def test_venue_order_id() -> None:
    """
    Test venue order id.
    """
    vo1 = VenueOrderId("V-123456")
    vo2 = VenueOrderId("V-789012")

    assert vo1 == vo1
    assert vo1 != vo2
    assert vo1.value == "V-123456"
    assert str(vo1) == "V-123456"
    assert repr(vo1) == "VenueOrderId('V-123456')"


def test_order_list_id() -> None:
    """
    Test order list id.
    """
    ol1 = OrderListId("OL-123456")
    ol2 = OrderListId("OL-789012")

    assert ol1 == ol1
    assert ol1 != ol2
    assert ol1.value == "OL-123456"
    assert str(ol1) == "OL-123456"
    assert repr(ol1) == "OrderListId('OL-123456')"


def test_position_id() -> None:
    """
    Test position id.
    """
    p1 = PositionId("P-123456")
    p2 = PositionId("P-789012")

    assert p1 == p1
    assert p1 != p2
    assert p1.value == "P-123456"
    assert str(p1) == "P-123456"
    assert repr(p1) == "PositionId('P-123456')"


def test_trade_id() -> None:
    """
    Test trade id.
    """
    t1 = TradeId("T-123456")
    t2 = TradeId("T-789012")

    assert t1 == t1
    assert t1 != t2
    assert t1.value == "T-123456"
    assert str(t1) == "T-123456"
    assert repr(t1) == "TradeId('T-123456')"


def test_trade_id_pickle() -> None:
    """
    Test trade id pickle.
    """
    trade_id = TradeId("T-123456")
    pickled = pickle.dumps(trade_id)
    unpickled = pickle.loads(pickled)

    assert type(unpickled) is TradeId
    assert unpickled == trade_id
    assert unpickled.value == "T-123456"


def test_trade_id_maximum_length() -> None:
    """
    Test trade id maximum length.
    """
    with pytest.raises(ValueError, match="exceeds maximum length"):
        TradeId("A" * 37)


@pytest.mark.parametrize(("identifier_type", "value"), GENERATED_IDENTIFIER_CASES)
def test_generated_identifier_from_str_invalid(identifier_type: object, value: object) -> None:
    """
    Test generated identifier from str invalid.
    """
    with pytest.raises(
        ValueError,
        match=re.escape("invalid string for 'value', was empty"),
    ) as exc_info:
        identifier_type.from_str("")

    assert type(exc_info.value) is ValueError
    assert str(exc_info.value) == "invalid string for 'value', was empty"


@pytest.mark.parametrize(("identifier_type", "value"), GENERATED_IDENTIFIER_CASES)
def test_generated_identifier_pickle_roundtrip(identifier_type: object, value: object) -> None:
    """
    Test generated identifier pickle roundtrip.
    """
    identifier = identifier_type(value)
    pickled = pickle.dumps(identifier)
    unpickled = pickle.loads(pickled)

    assert type(unpickled) is identifier_type
    assert unpickled == identifier
    assert unpickled.value == value


@pytest.mark.parametrize(("identifier_type", "value"), GENERATED_IDENTIFIER_CASES)
def test_generated_identifier_invalid_pickle_state(identifier_type: object, value: object) -> None:
    """
    Test generated identifier invalid pickle state.
    """
    identifier = identifier_type(value)

    with pytest.raises(
        ValueError,
        match=re.escape("invalid string for 'value', was empty"),
    ) as exc_info:
        identifier.__setstate__(("",))

    assert type(exc_info.value) is ValueError
    assert str(exc_info.value) == "invalid string for 'value', was empty"
    assert identifier.value == value


@pytest.mark.parametrize(
    ("identifier_type", "value", "expected_error"),
    [
        (Symbol, "BTC/USD", "invalid string for 'value', was empty"),
        (TradeId, "Trade-001", "String is empty"),
    ],
)
def test_custom_identifier_invalid_pickle_state(
    identifier_type: object,
    value: object,
    expected_error: object,
) -> None:
    """
    Test custom identifier invalid pickle state.
    """
    identifier = identifier_type(value)

    with pytest.raises(ValueError, match=re.escape(expected_error)) as exc_info:
        identifier.__setstate__(("",))

    assert type(exc_info.value) is ValueError
    assert str(exc_info.value) == expected_error
    assert identifier.value == value


def test_instrument_id_invalid_pickle_state() -> None:
    """
    Test instrument id invalid pickle state.
    """
    instrument_id = InstrumentId.from_str("BTC/USD.BINANCE")

    with pytest.raises(
        ValueError,
        match=re.escape("invalid string for 'value', was empty"),
    ) as exc_info:
        instrument_id.__setstate__(("ETH/USD", ""))

    assert type(exc_info.value) is ValueError
    assert str(exc_info.value) == "invalid string for 'value', was empty"
    assert instrument_id.value == "BTC/USD.BINANCE"


@pytest.mark.parametrize(
    "identifier",
    [
        Symbol("AUD/USD"),
        Venue("BINANCE"),
        InstrumentId(Symbol("BTC/USD"), Venue("BINANCE")),
        ComponentId("MyComponent"),
        ClientId("MyClient"),
        TraderId("TRADER-001"),
        StrategyId("Strategy-001"),
        ExecAlgorithmId("TWAP"),
        AccountId("SIM-001"),
        ClientOrderId("O-123456"),
        VenueOrderId("V-123456"),
        OrderListId("OL-123456"),
        PositionId("P-123456"),
        TradeId("T-123456"),
    ],
)
def test_identifier_equality_with_none(identifier: object) -> None:
    """
    Test identifier equality with none.
    """
    assert (identifier == None) is False
    assert (identifier != None) is True


@pytest.mark.parametrize(
    "identifier",
    [
        Symbol("AUD/USD"),
        Venue("BINANCE"),
        InstrumentId(Symbol("BTC/USD"), Venue("BINANCE")),
        ComponentId("MyComponent"),
        ClientId("MyClient"),
        TraderId("TRADER-001"),
        StrategyId("Strategy-001"),
        ExecAlgorithmId("TWAP"),
        AccountId("SIM-001"),
        ClientOrderId("O-123456"),
        VenueOrderId("V-123456"),
        OrderListId("OL-123456"),
        PositionId("P-123456"),
        TradeId("T-123456"),
    ],
)
def test_identifier_ordering_with_none_raises(identifier: object) -> None:
    """
    Test identifier ordering with none raises.
    """
    with pytest.raises(TypeError):
        _ = identifier < None
    with pytest.raises(TypeError):
        _ = identifier <= None
    with pytest.raises(TypeError):
        _ = identifier > None
    with pytest.raises(TypeError):
        _ = identifier >= None
