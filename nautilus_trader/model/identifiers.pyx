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

from libc.string cimport strcmp

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport account_id_hash
from nautilus_trader.core.rust.model cimport account_id_new
from nautilus_trader.core.rust.model cimport client_id_hash
from nautilus_trader.core.rust.model cimport client_id_new
from nautilus_trader.core.rust.model cimport client_order_id_hash
from nautilus_trader.core.rust.model cimport client_order_id_new
from nautilus_trader.core.rust.model cimport component_id_hash
from nautilus_trader.core.rust.model cimport component_id_new
from nautilus_trader.core.rust.model cimport exec_algorithm_id_hash
from nautilus_trader.core.rust.model cimport exec_algorithm_id_new
from nautilus_trader.core.rust.model cimport instrument_id_check_parsing
from nautilus_trader.core.rust.model cimport instrument_id_from_cstr
from nautilus_trader.core.rust.model cimport instrument_id_hash
from nautilus_trader.core.rust.model cimport instrument_id_is_synthetic
from nautilus_trader.core.rust.model cimport instrument_id_new
from nautilus_trader.core.rust.model cimport instrument_id_to_cstr
from nautilus_trader.core.rust.model cimport interned_string_stats
from nautilus_trader.core.rust.model cimport order_list_id_hash
from nautilus_trader.core.rust.model cimport order_list_id_new
from nautilus_trader.core.rust.model cimport position_id_hash
from nautilus_trader.core.rust.model cimport position_id_new
from nautilus_trader.core.rust.model cimport strategy_id_hash
from nautilus_trader.core.rust.model cimport strategy_id_new
from nautilus_trader.core.rust.model cimport symbol_hash
from nautilus_trader.core.rust.model cimport symbol_is_composite
from nautilus_trader.core.rust.model cimport symbol_new
from nautilus_trader.core.rust.model cimport symbol_root
from nautilus_trader.core.rust.model cimport symbol_topic
from nautilus_trader.core.rust.model cimport trade_id_hash
from nautilus_trader.core.rust.model cimport trade_id_new
from nautilus_trader.core.rust.model cimport trade_id_to_cstr
from nautilus_trader.core.rust.model cimport trader_id_hash
from nautilus_trader.core.rust.model cimport trader_id_new
from nautilus_trader.core.rust.model cimport venue_code_exists
from nautilus_trader.core.rust.model cimport venue_from_cstr_code
from nautilus_trader.core.rust.model cimport venue_hash
from nautilus_trader.core.rust.model cimport venue_is_synthetic
from nautilus_trader.core.rust.model cimport venue_new
from nautilus_trader.core.rust.model cimport venue_order_id_hash
from nautilus_trader.core.rust.model cimport venue_order_id_new
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.string cimport ustr_to_pystr


cdef class Identifier:
    """
    The abstract base class for all identifiers.
    """

    def __getstate__(self):
        raise NotImplementedError("method `__getstate__` must be implemented in the subclass")  # pragma: no cover

    def __setstate__(self, state):
        raise NotImplementedError("method `__setstate__` must be implemented in the subclass")  # pragma: no cover

    def __lt__(self, Identifier other) -> bool:
        return self.to_str() < other.to_str()

    def __le__(self, Identifier other) -> bool:
        return self.to_str() <= other.to_str()

    def __gt__(self, Identifier other) -> bool:
        return self.to_str() > other.to_str()

    def __ge__(self, Identifier other) -> bool:
        return self.to_str() >= other.to_str()

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.to_str()}')"

    cdef str to_str(self):
        raise NotImplementedError("method `to_str` must be implemented in the subclass")  # pragma: no cover

    @property
    def value(self) -> str:
        """
        Return the identifier (ID) value.

        Returns
        -------
        str

        """
        return self.to_str()


cdef class Symbol(Identifier):
    """
    Represents a valid ticker symbol ID for a tradable instrument.

    Parameters
    ----------
    value : str
        The ticker symbol ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.

    Warnings
    --------
    The ID value must be unique for a trading venue.

    References
    ----------
    https://en.wikipedia.org/wiki/Ticker_symbol
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        self._mem = symbol_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = symbol_new(pystr_to_cstr(state))

    def __eq__(self, Symbol other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef Symbol from_mem_c(Symbol_t mem):
        cdef Symbol symbol = Symbol.__new__(Symbol)
        symbol._mem = mem
        return symbol

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)

    cpdef bint is_composite(self):
        """
        Returns true if the symbol string contains a period ('.').

        Returns
        -------
        str

        """
        return <bint>symbol_is_composite(&self._mem)

    cpdef str root(self):
        """
        Return the symbol root.

        The symbol root is the substring that appears before the first period ('.')
        in the full symbol string. It typically represents the underlying asset for
        futures and options contracts. If no period is found, the entire symbol
        string is considered the root.

        Returns
        -------
        str

        """
        return cstr_to_pystr(symbol_root(&self._mem))

    cpdef str topic(self):
        """
        Return the symbol topic.

        The symbol topic is the root symbol with a wildcard '*' appended if the symbol has a root,
        otherwise returns the full symbol string.

        Returns
        -------
        str

        """
        return cstr_to_pystr(symbol_topic(&self._mem))


cdef class Venue(Identifier):
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
    """

    def __init__(self, str name not None) -> None:
        Condition.valid_string(name, "name")
        self._mem = venue_new(pystr_to_cstr(name))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = venue_new(pystr_to_cstr(state))

    def __eq__(self, Venue other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)

    @staticmethod
    cdef Venue from_mem_c(Venue_t mem):
        cdef Venue venue = Venue.__new__(Venue)
        venue._mem = mem
        return venue

    @staticmethod
    cdef Venue from_code_c(str code):
        cdef const char* code_ptr = pystr_to_cstr(code)
        if not venue_code_exists(code_ptr):
            return None
        cdef Venue venue = Venue.__new__(Venue)
        venue._mem = venue_from_cstr_code(code_ptr)
        return venue

    cpdef bint is_synthetic(self):
        """
        Return whether the venue is synthetic ('SYNTH').

        Returns
        -------
        bool

        """
        return <bint>venue_is_synthetic(&self._mem)

    @staticmethod
    def from_code(str code):
        """
        Return the venue with the given `code` from the built-in internal map (if found).

        Currency only supports CME Globex exchange ISO 10383 MIC codes.

        Parameters
        ----------
        code : str
            The code of the venue.

        Returns
        -------
        Venue or ``None``

        """
        Condition.not_none(code, "code")

        return Venue.from_code_c(code)



cdef class InstrumentId(Identifier):
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

    def __init__(self, Symbol symbol not None, Venue venue not None) -> None:
        self._mem = instrument_id_new(
            symbol._mem,
            venue._mem,
        )

    @property
    def symbol(self) -> Symbol:
        """
        Returns the instrument ticker symbol.

        Returns
        -------
        Symbol

        """
        return Symbol.from_mem_c(self._mem.symbol)

    @property
    def venue(self) -> Venue:
        """
        Returns the instrument trading venue.

        Returns
        -------
        Venue

        """
        return Venue.from_mem_c(self._mem.venue)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = instrument_id_from_cstr(
            pystr_to_cstr(state),
        )

    def __eq__(self, InstrumentId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem.symbol._0, other._mem.symbol._0) == 0 and strcmp(self._mem.venue._0, other._mem.venue._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef InstrumentId from_mem_c(InstrumentId_t mem):
        cdef InstrumentId instrument_id = InstrumentId.__new__(InstrumentId)
        instrument_id._mem = mem
        return instrument_id

    @staticmethod
    cdef InstrumentId from_str_c(str value):
        Condition.valid_string(value, "value")

        cdef str parse_err = cstr_to_pystr(instrument_id_check_parsing(pystr_to_cstr(value)))
        if parse_err:
            raise ValueError(parse_err)

        cdef InstrumentId instrument_id = InstrumentId.__new__(InstrumentId)
        instrument_id._mem = instrument_id_from_cstr(pystr_to_cstr(value))
        return instrument_id

    cdef str to_str(self):
        return cstr_to_pystr(instrument_id_to_cstr(&self._mem))

    @staticmethod
    def from_str(value: str) -> InstrumentId:
        """
        Return an instrument ID parsed from the given string value.
        Must be correctly formatted including symbol and venue components either side of a single
        period.

        Examples: 'AUD/USD.IDEALPRO', 'BTCUSDT.BINANCE'

        Parameters
        ----------
        value : str
            The instrument ID string value to parse.

        Returns
        -------
        InstrumentId

        Raises
        ------
        ValueError
            If `value` is not a valid instrument ID string.

        """
        return InstrumentId.from_str_c(value)

    cpdef bint is_synthetic(self):
        """
        Return whether the instrument ID is a synthetic instrument (with venue of 'SYNTH').

        Returns
        -------
        bool

        """
        return <bint>instrument_id_is_synthetic(&self._mem)

    @staticmethod
    def from_pyo3(pyo3_instrument_id) -> InstrumentId:
        """
        Return an instrument ID from the given PyO3 instance.

        Parameters
        ----------
        value : nautilus_pyo3.InstrumentId
            The PyO3 instrument ID instance.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_str_c(pyo3_instrument_id.value)

    cpdef to_pyo3(self):
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.InstrumentId

        """
        return nautilus_pyo3.InstrumentId.from_str(self.to_str())


cdef class ComponentId(Identifier):
    """
    Represents a valid component ID.

    Parameters
    ----------
    value : str
        The component ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.

    Warnings
    --------
    The ID value must be unique at the trader level.
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        self._mem = component_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = component_id_new(pystr_to_cstr(state))

    def __eq__(self, ComponentId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef ComponentId from_mem_c(ComponentId_t mem):
        cdef ComponentId component_id = ComponentId.__new__(ComponentId)
        component_id._mem = mem
        return component_id

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)


cdef class ClientId(Identifier):
    """
    Represents a system client ID.

    Parameters
    ----------
    value : str
        The client ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.

    Warnings
    --------
    The ID value must be unique at the trader level.
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        self._mem = client_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = client_id_new(pystr_to_cstr(state))

    def __eq__(self, ClientId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef ClientId from_mem_c(ClientId_t mem):
        cdef ClientId client_id = ClientId.__new__(ClientId)
        client_id._mem = mem
        return client_id

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)


cdef class TraderId(Identifier):
    """
    Represents a valid trader ID.

    Must be correctly formatted with two valid strings either side of a hyphen.
    It is expected a trader ID is the abbreviated name of the trader
    with an order ID tag number separated by a hyphen.

    Example: "TESTER-001".

    The reason for the numerical component of the ID is so that order and position IDs
    do not collide with those from another node instance.

    Parameters
    ----------
    value : str
        The trader ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string containing a hyphen.

    Warnings
    --------
    The name and tag combination ID value must be unique at the firm level.
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        self._mem = trader_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = trader_id_new(pystr_to_cstr(state))

    def __eq__(self, TraderId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef TraderId from_mem_c(TraderId_t mem):
        cdef TraderId trader_id = TraderId.__new__(TraderId)
        trader_id._mem = mem
        return trader_id

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)

    cpdef str get_tag(self):
        """
        Return the order ID tag value for this ID.

        Returns
        -------
        str

        """
        return self.to_str().split("-")[-1]


# External strategy ID constant
cdef StrategyId EXTERNAL_STRATEGY_ID = StrategyId("EXTERNAL")


cdef class StrategyId(Identifier):
    """
    Represents a valid strategy ID.

    Must be correctly formatted with two valid strings either side of a hyphen.
    It is expected a strategy ID is the class name of the strategy,
    with an order ID tag number separated by a hyphen.

    Example: "EMACross-001".

    The reason for the numerical component of the ID is so that order and position IDs
    do not collide with those from another strategy within the node instance.

    Parameters
    ----------
    value : str
        The strategy ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string containing a hyphen.

    Warnings
    --------
    The name and tag combination must be unique at the trader level.
    """

    def __init__(self, str value) -> None:
        Condition.valid_string(value, "value")
        Condition.is_true(value == "EXTERNAL" or "-" in value, "value was malformed: did not contain a hyphen '-'")

        self._mem = strategy_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = strategy_id_new(pystr_to_cstr(state))

    def __eq__(self, StrategyId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef StrategyId from_mem_c(StrategyId_t mem):
        cdef StrategyId strategy_id = StrategyId.__new__(StrategyId)
        strategy_id._mem = mem
        return strategy_id

    @staticmethod
    cdef StrategyId external_c():
        return EXTERNAL_STRATEGY_ID

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)

    cpdef str get_tag(self):
        """
        Return the order ID tag value for this ID.

        Returns
        -------
        str

        """
        return self.to_str().split("-")[-1]

    cpdef bint is_external(self):
        """
        If the strategy ID is the global 'external' strategy. This represents
        the strategy for all orders interacting with this instance of the system
        which did not originate from any strategy being managed by the system.

        Returns
        -------
        bool

        """
        return self == EXTERNAL_STRATEGY_ID


cdef class ExecAlgorithmId(Identifier):
    """
    Represents a valid execution algorithm ID.

    Parameters
    ----------
    value : str
        The execution algorithm ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        self._mem = exec_algorithm_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = exec_algorithm_id_new(pystr_to_cstr(state))

    def __eq__(self, ExecAlgorithmId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef ExecAlgorithmId from_mem_c(ExecAlgorithmId_t mem):
        cdef ExecAlgorithmId exec_algorithm_id = ExecAlgorithmId.__new__(ExecAlgorithmId)
        exec_algorithm_id._mem = mem
        return exec_algorithm_id

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)



cdef class AccountId(Identifier):
    """
    Represents a valid account ID.

    Must be correctly formatted with two valid strings either side of a hyphen.
    It is expected an account ID is the name of the issuer with an account number
    separated by a hyphen.

    Example: "IB-D02851908".

    Parameters
    ----------
    value : str
        The account ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string containing a hyphen.

    Warnings
    --------
    The issuer and number ID combination must be unique at the firm level.
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        Condition.is_true("-" in value, "value was malformed: did not contain a hyphen '-'")
        self._mem = account_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = account_id_new(pystr_to_cstr(state))

    def __eq__(self, AccountId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef AccountId from_mem_c(AccountId_t mem):
        cdef AccountId account_id = AccountId.__new__(AccountId)
        account_id._mem = mem
        return account_id

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)

    cpdef str get_issuer(self):
        """
        Return the account issuer for this ID.

        Returns
        -------
        str

        """
        return self.to_str().split("-")[0]

    cpdef str get_id(self):
        """
        Return the account ID without issuer name.

        Returns
        -------
        str

        """
        return self.to_str().split("-")[1]


cdef class ClientOrderId(Identifier):
    """
    Represents a valid client order ID (assigned by the Nautilus system).

    Parameters
    ----------
    value : str
        The client order ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.

    Warnings
    --------
    The ID value must be unique at the firm level.
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        self._mem = client_order_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = client_order_id_new(pystr_to_cstr(state))

    def __eq__(self, ClientOrderId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef ClientOrderId from_mem_c(ClientOrderId_t mem):
        cdef ClientOrderId client_order_id = ClientOrderId.__new__(ClientOrderId)
        client_order_id._mem = mem
        return client_order_id

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)


cdef class VenueOrderId(Identifier):
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
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        self._mem = venue_order_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = venue_order_id_new(pystr_to_cstr(state))

    def __eq__(self, VenueOrderId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef VenueOrderId from_mem_c(VenueOrderId_t mem):
        cdef VenueOrderId venue_order_id = VenueOrderId.__new__(VenueOrderId)
        venue_order_id._mem = mem
        return venue_order_id

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)


cdef class OrderListId(Identifier):
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
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        self._mem = order_list_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = order_list_id_new(pystr_to_cstr(state))

    def __eq__(self, OrderListId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef OrderListId from_mem_c(OrderListId_t mem):
        cdef OrderListId order_list_id = OrderListId.__new__(OrderListId)
        order_list_id._mem = mem
        return order_list_id

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)


cdef class PositionId(Identifier):
    """
    Represents a valid position ID.

    Parameters
    ----------
    value : str
        The position ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string containing a hyphen.
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        self._mem = position_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = position_id_new(pystr_to_cstr(state))

    def __eq__(self, PositionId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem._0, other._mem._0) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef PositionId from_mem_c(PositionId_t mem):
        cdef PositionId position_id = PositionId.__new__(PositionId)
        position_id._mem = mem
        return position_id

    cdef str to_str(self):
        return ustr_to_pystr(self._mem._0)

    cdef bint is_virtual_c(self):
        return self.to_str().startswith("P-")


cdef class TradeId(Identifier):
    """
    Represents a valid trade match ID (assigned by a trading venue).

    Maximum length is 36 characters.
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
    ValueError
        If `value` length exceeds maximum 36 characters.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0/tagnum_1003.html
    """

    def __init__(self, str value not None) -> None:
        Condition.valid_string(value, "value")
        if len(value) > 36:
            Condition.in_range_int(len(value), 1, 36, "value")

        self._mem = trade_id_new(pystr_to_cstr(value))

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = trade_id_new(pystr_to_cstr(state))

    def __eq__(self, TradeId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(trade_id_to_cstr(&self._mem), trade_id_to_cstr(&other._mem)) == 0

    def __hash__(self) -> int:
        return hash(self.to_str())

    @staticmethod
    cdef TradeId from_mem_c(TradeId_t mem):
        cdef TradeId trade_id = TradeId.__new__(TradeId)
        trade_id._mem = mem
        return trade_id

    cdef str to_str(self):
        return cstr_to_pystr(trade_id_to_cstr(&self._mem), False)
