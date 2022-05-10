# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport account_id_free
from nautilus_trader.core.rust.model cimport account_id_from_buffer
from nautilus_trader.core.rust.model cimport client_order_id_free
from nautilus_trader.core.rust.model cimport client_order_id_from_buffer
from nautilus_trader.core.rust.model cimport client_order_link_id_free
from nautilus_trader.core.rust.model cimport client_order_link_id_from_buffer
from nautilus_trader.core.rust.model cimport component_id_free
from nautilus_trader.core.rust.model cimport component_id_from_buffer
from nautilus_trader.core.rust.model cimport instrument_id_free
from nautilus_trader.core.rust.model cimport instrument_id_from_buffers
from nautilus_trader.core.rust.model cimport order_list_id_free
from nautilus_trader.core.rust.model cimport order_list_id_from_buffer
from nautilus_trader.core.rust.model cimport position_id_free
from nautilus_trader.core.rust.model cimport position_id_from_buffer
from nautilus_trader.core.rust.model cimport symbol_free
from nautilus_trader.core.rust.model cimport symbol_from_buffer
from nautilus_trader.core.rust.model cimport trade_id_free
from nautilus_trader.core.rust.model cimport trade_id_from_buffer
from nautilus_trader.core.rust.model cimport venue_free
from nautilus_trader.core.rust.model cimport venue_from_buffer
from nautilus_trader.core.rust.model cimport venue_order_id_free
from nautilus_trader.core.rust.model cimport venue_order_id_from_buffer
from nautilus_trader.core.string cimport buffer32_to_pystr
from nautilus_trader.core.string cimport buffer64_to_pystr
from nautilus_trader.core.string cimport pystr_to_buffer32
from nautilus_trader.core.string cimport pystr_to_buffer36
from nautilus_trader.core.string cimport pystr_to_buffer64
from nautilus_trader.core.string cimport pystr_to_buffer128


cdef class Symbol:
    """
    Represents a valid ticker symbol ID for a tradable financial market
    instrument.

    The ID value must be unique for a trading venue.

    Parameters
    ----------
    value : str
        The ticker symbol ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    AssertionError
        If `value` has length greater than 32 chars.

    References
    ----------
    https://en.wikipedia.org/wiki/Ticker_symbol
    """

    def __init__(self, str value):
        Condition.valid_string(value, "value")

        self.value = value
        self._mem = symbol_from_buffer(pystr_to_buffer32(value))

    def __del__(self) -> None:
        symbol_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.value

    def __setstate__(self, state):
        self.value = state
        self._mem = symbol_from_buffer(pystr_to_buffer32(state))

    def __eq__(self, Symbol other) -> bool:
        return self.value == other.value

    def __lt__(self, Symbol other) -> bool:
        return self.value < other.value

    def __le__(self, Symbol other) -> bool:
        return self.value <= other.value

    def __gt__(self, Symbol other) -> bool:
        return self.value > other.value

    def __ge__(self, Symbol other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"


cdef class Venue:
    """
    Represents a valid trading venue ID.

    Parameters
    ----------
    name : str
        The venue ID value.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    AssertionError
        If `value` has length greater than 32 chars.
    """

    def __init__(self, str name):
        Condition.valid_string(name, "name")

        self.value = name
        self._mem = venue_from_buffer(pystr_to_buffer32(name))

    def __del__(self) -> None:
        venue_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.value

    def __setstate__(self, state):
        self.value = state
        self._mem = venue_from_buffer(pystr_to_buffer32(state))

    def __eq__(self, Venue other) -> bool:
        return self.value == other.value

    def __lt__(self, Venue other) -> bool:
        return self.value < other.value

    def __le__(self, Venue other) -> bool:
        return self.value <= other.value

    def __gt__(self, Venue other) -> bool:
        return self.value > other.value

    def __ge__(self, Venue other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"


cdef class InstrumentId:
    """
    Represents a valid instrument ID.

    The symbol and venue combination should uniquely identify the instrument.

    Parameters
    ----------
    symbol : Symbol
        The instruments ticker symbol.
    venue : Venue
        The instruments trading venue.
    """

    def __init__(self, Symbol symbol not None, Venue venue not None):
        Condition.not_none(symbol, "symbol")
        Condition.not_none(venue, "venue")

        self.symbol = symbol
        self.venue = venue
        self.value = f"{symbol}.{venue}"
        self._mem = instrument_id_from_buffers(symbol._mem.value, venue._mem.value)

    def __del__(self) -> None:
        instrument_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.symbol.value, self.venue.value

    def __setstate__(self, state):
        self.symbol = Symbol(state[0])
        self.venue = Venue(state[1])
        self.value = f"{self.symbol}.{self.venue}"
        self._mem = instrument_id_from_buffers(pystr_to_buffer32(state[0]), pystr_to_buffer32(state[1]))

    def __eq__(self, InstrumentId other) -> bool:
        return self.value == other.value

    def __lt__(self, InstrumentId other) -> bool:
        return self.value < other.value

    def __le__(self, InstrumentId other) -> bool:
        return self.value <= other.value

    def __gt__(self, InstrumentId other) -> bool:
        return self.value > other.value

    def __ge__(self, InstrumentId other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"

    @staticmethod
    cdef InstrumentId from_raw_c(InstrumentId_t raw):
        cdef Symbol symbol = Symbol.__new__(Symbol)
        symbol._mem = raw.symbol
        symbol.value = buffer32_to_pystr(raw.symbol.value)

        cdef Venue venue = Venue.__new__(Venue)
        venue._mem = raw.venue
        venue.value = buffer32_to_pystr(raw.venue.value)

        cdef InstrumentId instrument_id = InstrumentId.__new__(InstrumentId)
        instrument_id._mem = raw
        instrument_id.symbol = symbol
        instrument_id.venue = venue
        instrument_id.value = symbol.value + "." + venue.value

        return instrument_id

    @staticmethod
    cdef InstrumentId from_str_c(str value):
        Condition.valid_string(value, "value")

        cdef list pieces = value.rsplit('.', maxsplit=1)

        if len(pieces) != 2:
            raise ValueError(f"The InstrumentId string value was malformed, was {value}")

        cdef Symbol symbol = Symbol(pieces[0])
        cdef Venue venue = Venue(pieces[1])

        cdef InstrumentId instrument_id = InstrumentId.__new__(InstrumentId)
        instrument_id._mem = instrument_id_from_buffers(
            symbol._mem.value,
            venue._mem.value,
        )
        instrument_id.symbol = symbol
        instrument_id.venue = venue
        instrument_id.value = f"{symbol}.{venue}"

        return instrument_id

    @staticmethod
    def from_str(value: str) -> InstrumentId:
        """
        Return an instrument ID parsed from the given string value.
        Must be correctly formatted including characters either side of a single
        period.

        Examples: "AUD/USD.IDEALPRO", "BTCUSDT.BINANCE"

        Parameters
        ----------
        value : str
            The instrument ID string value to parse.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_str_c(value)


cdef class ComponentId:
    """
    Represents a valid component ID.

    The ID value must be unique at the trader level.

    Parameters
    ----------
    value : str
        The component ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    AssertionError
        If `value` has length greater than 32 chars.
    """

    def __init__(self, str value):
        Condition.valid_string(value, "value")

        self.value = value
        self._mem = component_id_from_buffer(pystr_to_buffer32(value))

    def __del__(self) -> None:
        component_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.value

    def __setstate__(self, state):
        self.value = state
        self._mem = component_id_from_buffer(pystr_to_buffer32(state))

    def __eq__(self, ComponentId other) -> bool:
        return self.value == other.value

    def __lt__(self, ComponentId other) -> bool:
        return self.value < other.value

    def __le__(self, ComponentId other) -> bool:
        return self.value <= other.value

    def __gt__(self, ComponentId other) -> bool:
        return self.value > other.value

    def __ge__(self, ComponentId other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"


cdef class ClientId(ComponentId):
    """
    Represents a system client ID.

    The ID value must be unique at the trader level.

    Parameters
    ----------
    value : str
        The client ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    AssertionError
        If `value` has length greater than 32 chars.
    """

    def __init__(self, str value):
        super().__init__(value)

    def __eq__(self, ClientId other) -> bool:
        return self.value == other.value

    def __hash__(self) -> int:
        return hash(self.value)


cdef class TraderId(ComponentId):
    """
    Represents a valid trader ID.

    The name and tag combination ID value must be unique at the firm level.

    Must be correctly formatted with two valid strings either side of a hyphen.
    It is expected a trader ID is the abbreviated name of the trader
    with an order ID tag number separated by a hyphen.

    Example: "TESTER-001".

    Parameters
    ----------
    value : str
        The trader ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string containing a hyphen.
    AssertionError
        If `value` has length greater than 32 chars.
    """

    def __init__(self, str value):
        Condition.true("-" in value, "ID incorrectly formatted (did not contain '-' hyphen)")
        super().__init__(value)

    def __eq__(self, TraderId other) -> bool:
        return self.value == other.value

    def __hash__(self) -> int:
        return hash(self.value)

    cpdef str get_tag(self):
        """
        Return the order ID tag value for this ID.

        Returns
        -------
        str

        """
        return self.value.rsplit("-", maxsplit=1)[-1]


# External strategy ID constant
cdef StrategyId EXTERNAL_STRATEGY = StrategyId("EXTERNAL")


cdef class StrategyId(ComponentId):
    """
    Represents a valid strategy ID.

    The name and tag combination must be unique at the trader level.

    Must be correctly formatted with two valid strings either side of a hyphen.
    It is expected a strategy ID is the class name of the strategy,
    with an order ID tag number separated by a hyphen.

    Example: "EMACross-001".

    Parameters
    ----------
    value : str
        The strategy ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string containing a hyphen.
    AssertionError
        If `value` has length greater than 32 chars.
    """


    def __init__(self, str value):
        if value != "EXTERNAL":
            Condition.true(
                value.__contains__("-"),
                "ID incorrectly formatted (did not contain '-' hyphen)",
            )
        Condition.valid_string(value, "value")

        self.value = value

    def __eq__(self, StrategyId other) -> bool:
        return self.value == other.value

    def __hash__(self) -> int:
        return hash(self.value)

    cpdef str get_tag(self):
        """
        Return the order ID tag value for this ID.

        Returns
        -------
        str

        """
        return self.value.rsplit("-", maxsplit=1)[-1]

    cpdef bint is_external(self):
        """
        If the strategy ID is the global 'external' strategy. This represents
        the strategy for all orders interacting with this instance of the system
        which did not originate from any strategy being managed by the system.

        Returns
        -------
        bool

        """
        return self.value == EXTERNAL_STRATEGY.value

    @staticmethod
    cdef StrategyId external_c():
        return EXTERNAL_STRATEGY


cdef class AccountId:
    """
    Represents a valid account ID.

    The issuer and number ID combination must be unique at the firm level.

    Parameters
    ----------
    issuer : str
        The account issuer (trading venue) ID value.
    number : str
        The account 'number' ID value.

    Raises
    ------
    ValueError
        If `issuer` is not a valid string.
    ValueError
        If `number` is not a valid string.
    AssertionError
        If `issuer` and `number` combinaed has length greater than 35 chars.
    """

    def __init__(self, str issuer, str number):
        Condition.valid_string(issuer, "issuer")
        Condition.valid_string(number, "number")

        self.issuer = issuer
        self.number = number
        self.value = f"{issuer}-{number}"
        self._mem = account_id_from_buffer(pystr_to_buffer36(self.value))

    def __del__(self) -> None:
        account_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.value

    def __setstate__(self, state):
        cdef list pieces = state.split('-', maxsplit=1)
        self.issuer = pieces[0]
        self.number = pieces[1]
        self.value = state
        self._mem = account_id_from_buffer(pystr_to_buffer36(state))

    def __eq__(self, AccountId other) -> bool:
        return self.value == other.value

    def __lt__(self, AccountId other) -> bool:
        return self.value < other.value

    def __le__(self, AccountId other) -> bool:
        return self.value <= other.value

    def __gt__(self, AccountId other) -> bool:
        return self.value > other.value

    def __ge__(self, AccountId other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"

    @staticmethod
    cdef AccountId from_str_c(str value):
        Condition.valid_string(value, "value")

        cdef list pieces = value.split('-', maxsplit=1)

        if len(pieces) != 2:
            raise ValueError(f"The AccountId string value was malformed, was {value}")

        return AccountId(issuer=pieces[0], number=pieces[1])

    @staticmethod
    def from_str(value: str) -> AccountId:
        """
        Return an account ID from the given string value. Must be
        correctly formatted with two valid strings either side of a hyphen.

        Example: "IB-D02851908".

        Parameters
        ----------
        value : str
            The value for the account ID.

        Returns
        -------
        AccountId

        """
        return AccountId.from_str_c(value)


cdef class ClientOrderId:
    """
    Represents a valid client order ID (assigned by the Nautilus system).

    The ID value must be unique at the firm level.

    Parameters
    ----------
    value : str
        The client order ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    AssertionError
        If `value` has length greater than 36 chars.
    """

    def __init__(self, str value):
        Condition.valid_string(value, "value")

        self.value = value
        self._mem = client_order_id_from_buffer(pystr_to_buffer36(value))

    def __del__(self) -> None:
        client_order_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.value

    def __setstate__(self, state):
        self._mem = client_order_id_from_buffer(pystr_to_buffer36(state))

    def __eq__(self, ClientOrderId other) -> bool:
        return self.value == other.value

    def __lt__(self, ClientOrderId other) -> bool:
        return self.value < other.value

    def __le__(self, ClientOrderId other) -> bool:
        return self.value <= other.value

    def __gt__(self, ClientOrderId other) -> bool:
        return self.value > other.value

    def __ge__(self, ClientOrderId other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"


cdef class ClientOrderLinkId:
    """
    Represents a valid client order link ID (assigned by the Nautilus system).

    The ID value must be unique for a trading venue.

    Can correspond to the `ClOrdLinkID <583> field` of the FIX protocol.

    Permits order originators to tie together groups of orders in which trades
    resulting from orders are associated for a specific purpose, for example the
    calculation of average execution price for a customer or to associate lists
    submitted to a broker as waves of a larger program trade.

    Parameters
    ----------
    value : str
        The client order link ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    AssertionError
        If `value` has length greater than 36 chars.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.sp2/tagnum_583.html
    """

    def __init__(self, str value):
        Condition.valid_string(value, "value")

        self.value = value
        self._mem = client_order_link_id_from_buffer(pystr_to_buffer36(value))

    def __del__(self) -> None:
        client_order_link_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.value

    def __setstate__(self, state):
        self.value = state
        self._mem = client_order_link_id_from_buffer(pystr_to_buffer36(state))

    def __eq__(self, ClientOrderLinkId other) -> bool:
        return self.value == other.value

    def __lt__(self, ClientOrderLinkId other) -> bool:
        return self.value < other.value

    def __le__(self, ClientOrderLinkId other) -> bool:
        return self.value <= other.value

    def __gt__(self, ClientOrderLinkId other) -> bool:
        return self.value > other.value

    def __ge__(self, ClientOrderLinkId other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"


cdef class VenueOrderId:
    """
    Represents a valid venue order ID (assigned by a trading venue).

    Parameters
    ----------
    value : str
        The venue assigned order ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    AssertionError
        If `value` has length greater than 36 chars.
    """

    def __init__(self, str value):
        Condition.valid_string(value, "value")

        self.value = value
        self._mem = venue_order_id_from_buffer(pystr_to_buffer36(value))

    def __del__(self) -> None:
        venue_order_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.value

    def __setstate__(self, state):
        self.value = state
        self._mem = venue_order_id_from_buffer(pystr_to_buffer36(state))

    def __eq__(self, VenueOrderId other) -> bool:
        return self.value == other.value

    def __lt__(self, VenueOrderId other) -> bool:
        return self.value < other.value

    def __le__(self, VenueOrderId other) -> bool:
        return self.value <= other.value

    def __gt__(self, VenueOrderId other) -> bool:
        return self.value > other.value

    def __ge__(self, VenueOrderId other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"


cdef class OrderListId:
    """
    Represents a valid order list ID (assigned by the Nautilus system).

    Parameters
    ----------
    value : str
        The order list ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    AssertionError
        If `value` has length greater than 32 chars.
    """

    def __init__(self, str value):
        Condition.valid_string(value, "value")

        self.value = value
        self._mem = order_list_id_from_buffer(pystr_to_buffer32(value))

    def __del__(self) -> None:
        order_list_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.value

    def __setstate__(self, state):
        self.value = state
        self._mem = order_list_id_from_buffer(pystr_to_buffer32(state))

    def __eq__(self, OrderListId other) -> bool:
        return self.value == other.value

    def __lt__(self, OrderListId other) -> bool:
        return self.value < other.value

    def __le__(self, OrderListId other) -> bool:
        return self.value <= other.value

    def __gt__(self, OrderListId other) -> bool:
        return self.value > other.value

    def __ge__(self, OrderListId other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"


cdef class PositionId:
    """
    Represents a valid position ID.

    Parameters
    ----------
    value : str
        The position ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    AssertionError
        If `value` has length greater than 128 chars.
    """

    def __init__(self, str value):
        Condition.valid_string(value, "value")

        self.value = value
        self._mem = position_id_from_buffer(pystr_to_buffer128(value))

    def __del__(self) -> None:
        position_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.value

    def __setstate__(self, state):
        self.value = state
        self._mem = position_id_from_buffer(pystr_to_buffer128(state))

    def __eq__(self, PositionId other) -> bool:
        return self.value == other.value

    def __lt__(self, PositionId other) -> bool:
        return self.value < other.value

    def __le__(self, PositionId other) -> bool:
        return self.value <= other.value

    def __gt__(self, PositionId other) -> bool:
        return self.value > other.value

    def __ge__(self, PositionId other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"

    cdef bint is_virtual_c(self) except *:
        return self.value.startswith("P-")


cdef class TradeId:
    """
    Represents a valid trade match ID (assigned by a trading venue).

    Can correspond to the `TradeID <1003> field` of the FIX protocol.

    The unique ID assigned to the trade entity once it is received or matched by
    the exchange or central counterparty.

    Parameters
    ----------
    value : str
        The trade match ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    AssertionError
        If `value` has length greater than 64 chars.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0/tagnum_1003.html
    """

    def __init__(self, str value):
        Condition.valid_string(value, "value")

        self.value = value
        self._mem = trade_id_from_buffer(pystr_to_buffer64(value))

    def __del__(self) -> None:
        trade_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.value

    def __setstate__(self, state):
        self.value = state
        self._mem = trade_id_from_buffer(pystr_to_buffer64(state))

    def __eq__(self, TradeId other) -> bool:
        return self.value == other.value

    def __lt__(self, TradeId other) -> bool:
        return self.value < other.value

    def __le__(self, TradeId other) -> bool:
        return self.value <= other.value

    def __gt__(self, TradeId other) -> bool:
        return self.value > other.value

    def __ge__(self, TradeId other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"

    @staticmethod
    cdef TradeId from_raw_c(TradeId_t raw):
        cdef TradeId trade_id = TradeId.__new__(TradeId)
        trade_id.value = buffer64_to_pystr(raw.value)
        trade_id._mem = raw
        return trade_id
