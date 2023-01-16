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

from nautilus_trader.core.rust.model cimport account_id_eq
from nautilus_trader.core.rust.model cimport account_id_free
from nautilus_trader.core.rust.model cimport account_id_hash
from nautilus_trader.core.rust.model cimport account_id_new
from nautilus_trader.core.rust.model cimport account_id_to_cstr
from nautilus_trader.core.rust.model cimport client_order_id_eq
from nautilus_trader.core.rust.model cimport client_order_id_free
from nautilus_trader.core.rust.model cimport client_order_id_hash
from nautilus_trader.core.rust.model cimport client_order_id_new
from nautilus_trader.core.rust.model cimport client_order_id_to_cstr
from nautilus_trader.core.rust.model cimport component_id_eq
from nautilus_trader.core.rust.model cimport component_id_free
from nautilus_trader.core.rust.model cimport component_id_hash
from nautilus_trader.core.rust.model cimport component_id_new
from nautilus_trader.core.rust.model cimport component_id_to_cstr
from nautilus_trader.core.rust.model cimport instrument_id_clone
from nautilus_trader.core.rust.model cimport instrument_id_eq
from nautilus_trader.core.rust.model cimport instrument_id_free
from nautilus_trader.core.rust.model cimport instrument_id_hash
from nautilus_trader.core.rust.model cimport instrument_id_new
from nautilus_trader.core.rust.model cimport instrument_id_new_from_cstr
from nautilus_trader.core.rust.model cimport instrument_id_to_cstr
from nautilus_trader.core.rust.model cimport order_list_id_eq
from nautilus_trader.core.rust.model cimport order_list_id_free
from nautilus_trader.core.rust.model cimport order_list_id_hash
from nautilus_trader.core.rust.model cimport order_list_id_new
from nautilus_trader.core.rust.model cimport order_list_id_to_cstr
from nautilus_trader.core.rust.model cimport position_id_eq
from nautilus_trader.core.rust.model cimport position_id_free
from nautilus_trader.core.rust.model cimport position_id_hash
from nautilus_trader.core.rust.model cimport position_id_new
from nautilus_trader.core.rust.model cimport position_id_to_cstr
from nautilus_trader.core.rust.model cimport symbol_clone
from nautilus_trader.core.rust.model cimport symbol_eq
from nautilus_trader.core.rust.model cimport symbol_free
from nautilus_trader.core.rust.model cimport symbol_hash
from nautilus_trader.core.rust.model cimport symbol_new
from nautilus_trader.core.rust.model cimport symbol_to_cstr
from nautilus_trader.core.rust.model cimport trade_id_clone
from nautilus_trader.core.rust.model cimport trade_id_eq
from nautilus_trader.core.rust.model cimport trade_id_free
from nautilus_trader.core.rust.model cimport trade_id_hash
from nautilus_trader.core.rust.model cimport trade_id_new
from nautilus_trader.core.rust.model cimport trade_id_to_cstr
from nautilus_trader.core.rust.model cimport venue_clone
from nautilus_trader.core.rust.model cimport venue_eq
from nautilus_trader.core.rust.model cimport venue_free
from nautilus_trader.core.rust.model cimport venue_hash
from nautilus_trader.core.rust.model cimport venue_new
from nautilus_trader.core.rust.model cimport venue_order_id_eq
from nautilus_trader.core.rust.model cimport venue_order_id_free
from nautilus_trader.core.rust.model cimport venue_order_id_hash
from nautilus_trader.core.rust.model cimport venue_order_id_new
from nautilus_trader.core.rust.model cimport venue_order_id_to_cstr
from nautilus_trader.core.rust.model cimport venue_to_cstr
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr


cdef class Identifier:
    """
    The base class for all identifiers.
    """

    def __getstate__(self):
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def __setstate__(self, state):
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

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
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

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
    Represents a valid ticker symbol ID for a tradable financial market
    instrument.

    Parameters
    ----------
    value : str
        The ticker symbol ID value.

    Warnings
    --------
    - The ID value must be unique for a trading venue.
    - Panics at runtime if `value` is not a valid string.

    References
    ----------
    https://en.wikipedia.org/wiki/Ticker_symbol
    """

    def __init__(self, str value not None):
        self._mem = symbol_new(pystr_to_cstr(value))

    def __del__(self) -> None:
        if self._mem.value != NULL:
            symbol_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = symbol_new(pystr_to_cstr(state))

    def __eq__(self, Symbol other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return symbol_eq(&self._mem, &other._mem)

    def __hash__ (self) -> int:
        return symbol_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(symbol_to_cstr(&self._mem))


cdef class Venue(Identifier):
    """
    Represents a valid trading venue ID.

    Parameters
    ----------
    name : str
        The venue ID value.

    Warnings
    --------
    - Panics at runtime if `value` is not a valid string.
    """

    def __init__(self, str name not None):
        self._mem = venue_new(pystr_to_cstr(name))

    def __del__(self) -> None:
        if self._mem.value != NULL:
            venue_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = venue_new(pystr_to_cstr(state))

    def __eq__(self, Venue other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return venue_eq(&self._mem, &other._mem)

    def __hash__ (self) -> int:
        return venue_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(venue_to_cstr(&self._mem))


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

    def __init__(self, Symbol symbol not None, Venue venue not None):
        self._mem = instrument_id_new(
            <Symbol_t *>&symbol._mem,
            <Venue_t *>&venue._mem,
        )
        self.symbol = symbol
        self.venue = venue

    def __del__(self) -> None:
        if self._mem.symbol.value != NULL:
            instrument_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        cdef list pieces = state.rsplit('.', maxsplit=1)

        self._mem = instrument_id_new_from_cstr(
            pystr_to_cstr(state),
        )
        self.symbol = Symbol(pieces[0])
        self.venue = Venue(pieces[1])

    def __eq__(self, InstrumentId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return instrument_id_eq(&self._mem, &other._mem)

    def __hash__ (self) -> int:
        return instrument_id_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(instrument_id_to_cstr(&self._mem))

    @staticmethod
    cdef InstrumentId from_mem_c(InstrumentId_t mem):
        cdef Symbol symbol = Symbol.__new__(Symbol)
        symbol._mem = symbol_clone(&mem.symbol)

        cdef Venue venue = Venue.__new__(Venue)
        venue._mem = venue_clone(&mem.venue)

        cdef InstrumentId instrument_id = InstrumentId.__new__(InstrumentId)
        instrument_id._mem = instrument_id_clone(&mem)
        instrument_id.symbol = symbol
        instrument_id.venue = venue

        return instrument_id

    @staticmethod
    cdef InstrumentId from_str_c(str value):
        cdef list pieces = value.rsplit('.', maxsplit=1)

        cdef InstrumentId instrument_id = InstrumentId.__new__(InstrumentId)
        instrument_id._mem = instrument_id_new_from_cstr(pystr_to_cstr(value))
        instrument_id.symbol = Symbol(pieces[0])
        instrument_id.venue = Venue(pieces[1])

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
    - The ID value must be unique at the trader level.
    - Panics at runtime if `value` is not a valid string.
    """

    def __init__(self, str value not None):
        self._mem = component_id_new(pystr_to_cstr(value))

    def __del__(self) -> None:
        if self._mem.value != NULL:
            component_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = component_id_new(pystr_to_cstr(state))

    def __eq__(self, ComponentId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return component_id_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return component_id_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(component_id_to_cstr(&self._mem))


cdef class ClientId(ComponentId):
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
    - The ID value must be unique at the trader level.
    - Panics at runtime if `value` is not a valid string.
    """

    def __init__(self, str value not None):
        super().__init__(value)


cdef class TraderId(ComponentId):
    """
    Represents a valid trader ID.

    Must be correctly formatted with two valid strings either side of a hyphen.
    It is expected a trader ID is the abbreviated name of the trader
    with an order ID tag number separated by a hyphen.

    Example: "TESTER-001".

    Parameters
    ----------
    value : str
        The trader ID value.

    Warnings
    --------
    - The name and tag combination ID value must be unique at the firm level.
    - Panics at runtime if `value` is not a valid string containing a hyphen.
    """

    def __init__(self, str value not None):
        super().__init__(value)

    cpdef str get_tag(self):
        """
        Return the order ID tag value for this ID.

        Returns
        -------
        str

        """
        return self.to_str().split("-")[-1]


# External strategy ID constant
cdef StrategyId EXTERNAL_STRATEGY = StrategyId("EXTERNAL")


cdef class StrategyId(ComponentId):
    """
    Represents a valid strategy ID.

    Must be correctly formatted with two valid strings either side of a hyphen.
    It is expected a strategy ID is the class name of the strategy,
    with an order ID tag number separated by a hyphen.

    Example: "EMACross-001".

    Parameters
    ----------
    value : str
        The strategy ID value.

    Warnings
    --------
    - The name and tag combination must be unique at the trader level.
    - Panics at runtime if `value` is not a valid string containing a hyphen.
    """

    def __init__(self, str value):
        super().__init__(value)

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
        return self == EXTERNAL_STRATEGY

    @staticmethod
    cdef StrategyId external_c():
        return EXTERNAL_STRATEGY


cdef class ExecAlgorithmId(ComponentId):
    """
    Represents a valid execution algorithm ID.

    Parameters
    ----------
    value : str
        The execution algorithm ID value.

    Warnings
    --------
    - Panics at runtime if `value` is not a valid string.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0/tagnum_1003.html
    """

    def __init__(self, str value not None):
        super().__init__(value)



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

    Warnings
    --------
    - The issuer and number ID combination must be unique at the firm level.
    - Panics at runtime if `value` is not a valid string containing a hyphen.
    """

    def __init__(self, str value not None):
        self._mem = account_id_new(pystr_to_cstr(value))

    def __del__(self) -> None:
        if self._mem.value != NULL:
            account_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = account_id_new(pystr_to_cstr(state))

    def __eq__(self, AccountId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return account_id_eq(&self._mem, &other._mem)

    def __hash__ (self) -> int:
        return account_id_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(account_id_to_cstr(&self._mem))

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

    Warnings
    --------
    - The ID value must be unique at the firm level.
    - Panics at runtime if `value` is not a valid string.
    """

    def __init__(self, str value not None):
        self._mem = client_order_id_new(pystr_to_cstr(value))

    def __del__(self) -> None:
        if self._mem.value != NULL:
            client_order_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = client_order_id_new(pystr_to_cstr(state))

    def __eq__(self, ClientOrderId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return client_order_id_eq(&self._mem, &other._mem)

    def __hash__ (self) -> int:
        return client_order_id_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(client_order_id_to_cstr(&self._mem))


cdef class VenueOrderId(Identifier):
    """
    Represents a valid venue order ID (assigned by a trading venue).

    Parameters
    ----------
    value : str
        The venue assigned order ID value.

    Warnings
    --------
    - Panics at runtime if `value` is not a valid string.
    """

    def __init__(self, str value not None):
        self._mem = venue_order_id_new(pystr_to_cstr(value))

    def __del__(self) -> None:
        if self._mem.value != NULL:
            venue_order_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = venue_order_id_new(pystr_to_cstr(state))

    def __eq__(self, VenueOrderId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return venue_order_id_eq(&self._mem, &other._mem)

    def __hash__ (self) -> int:
        return venue_order_id_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(venue_order_id_to_cstr(&self._mem))


cdef class OrderListId(Identifier):
    """
    Represents a valid order list ID (assigned by the Nautilus system).

    Parameters
    ----------
    value : str
        The order list ID value.

    Warnings
    --------
    - Panics at runtime if `value` is not a valid string.
    """

    def __init__(self, str value not None):
        self._mem = order_list_id_new(pystr_to_cstr(value))

    def __del__(self) -> None:
        if self._mem.value != NULL:
            order_list_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = order_list_id_new(pystr_to_cstr(state))

    def __eq__(self, OrderListId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return order_list_id_eq(&self._mem, &other._mem)

    def __hash__ (self) -> int:
        return order_list_id_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(order_list_id_to_cstr(&self._mem))


cdef class PositionId(Identifier):
    """
    Represents a valid position ID.

    Parameters
    ----------
    value : str
        The position ID value.

    Warnings
    --------
    - Panics at runtime if `value` is not a valid string.
    """

    def __init__(self, str value not None):
        self._mem = position_id_new(pystr_to_cstr(value))

    def __del__(self) -> None:
        if self._mem.value != NULL:
            position_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = position_id_new(pystr_to_cstr(state))

    def __eq__(self, PositionId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return position_id_eq(&self._mem, &other._mem)

    def __hash__ (self) -> int:
        return position_id_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(position_id_to_cstr(&self._mem))

    cdef bint is_virtual_c(self) except *:
        return self.to_str().startswith("P-")

    @staticmethod
    cdef PositionId from_mem_c(PositionId_t mem):
        cdef PositionId position_id = PositionId.__new__(PositionId)
        position_id._mem = mem
        return position_id


cdef class TradeId(Identifier):
    """
    Represents a valid trade match ID (assigned by a trading venue).

    Can correspond to the `TradeID <1003> field` of the FIX protocol.

    The unique ID assigned to the trade entity once it is received or matched by
    the exchange or central counterparty.

    Parameters
    ----------
    value : str
        The trade match ID value.

    Warnings
    --------
    - Panics at runtime if `value` is not a valid string.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0/tagnum_1003.html
    """

    def __init__(self, str value not None):
        self._mem = trade_id_new(pystr_to_cstr(value))

    def __del__(self) -> None:
        if self._mem.value != NULL:
            trade_id_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = trade_id_new(pystr_to_cstr(state))

    def __eq__(self, TradeId other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return trade_id_eq(&self._mem, &other._mem)

    def __hash__ (self) -> int:
        return trade_id_hash(&self._mem)

    cdef str to_str(self):
        return cstr_to_pystr(trade_id_to_cstr(&self._mem))

    @staticmethod
    cdef TradeId from_mem_c(TradeId_t mem):
        cdef TradeId trade_id = TradeId.__new__(TradeId)
        trade_id._mem = trade_id_clone(&mem)
        return trade_id
