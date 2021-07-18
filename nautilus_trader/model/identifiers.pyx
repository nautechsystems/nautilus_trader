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

from nautilus_trader.core.correctness cimport Condition


cdef str _NULL_ID = "NULL"

cdef class Identifier:
    """
    The abstract base class for all identifiers.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``Identifier`` class.

        Parameters
        ----------
        value : str
            The value of the ID.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        Condition.valid_string(value, "value")

        self.value = value

    def __eq__(self, Identifier other) -> bool:
        return isinstance(other, type(self)) and self.value == other.value

    def __lt__(self, Identifier other) -> bool:
        return self.value < other.value

    def __le__(self, Identifier other) -> bool:
        return self.value <= other.value

    def __gt__(self, Identifier other) -> bool:
        return self.value > other.value

    def __ge__(self, Identifier other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"

    cdef bint is_null(self) except *:
        return self.value == _NULL_ID

    cdef bint not_null(self) except *:
        return self.value != _NULL_ID


cdef class Symbol(Identifier):
    """
    Represents a valid ticker symbol ID for a tradeable financial market
    instrument.

    The ID value must be unique for a trading venue.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``Symbol`` class.

        Parameters
        ----------
        value : str
            The ticker symbol ID value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)


cdef class Venue(Identifier):
    """
    Represents a valid trading venue ID for a tradeable financial market
    instrument.
    """

    def __init__(self, str name):
        """
        Initialize a new instance of the ``Venue`` class.

        Parameters
        ----------
        name : str
            The venue name ID value.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        super().__init__(name)


cdef class InstrumentId(Identifier):
    """
    Represents a valid instrument ID.

    The symbol and venue combination should uniquely identify the instrument.
    """

    def __init__(self, Symbol symbol not None, Venue venue not None):
        """
        Initialize a new instance of the ``InstrumentId`` class.

        Parameters
        ----------
        symbol : Symbol
            The instruments ticker symbol.
        venue : Venue
            The instruments trading venue.

        """
        super().__init__(f"{symbol.value}.{venue.value}")

        self.symbol = symbol
        self.venue = venue

    @staticmethod
    cdef InstrumentId from_str_c(str value):
        Condition.valid_string(value, "value")

        cdef list pieces = value.rsplit('.', maxsplit=1)

        if len(pieces) != 2:
            raise ValueError(f"The InstrumentId string value was malformed, was {value}")

        return InstrumentId(symbol=Symbol(pieces[0]), venue=Venue(pieces[1]))

    @staticmethod
    def from_str(value: str) -> InstrumentId:
        """
        Return an instrument ID parsed from the given string value.
        Must be correctly formatted including characters either side of a single
        period.

        Examples: "AUD/USD.IDEALPRO", "BTC/USDT.BINANCE"

        Parameters
        ----------
        value : str
            The instrument ID string value to parse.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_str_c(value)


cdef class TraderId(Identifier):
    """
    Represents a valid trader ID.

    The name and tag combination ID value must be unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``TraderId`` class.

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
            If value is not a valid string containing a hyphen.

        """
        Condition.true(
            value == _NULL_ID or "-" in value,
            "ID incorrectly formatted (did not contain '-' hyphen)",
        )
        super().__init__(value)

    cpdef str get_tag(self):
        """
        Return the order ID tag value for this ID.

        Returns
        -------
        str

        """
        return self.value.partition("-")[2]


cdef class StrategyId(Identifier):
    """
    Represents a valid strategy ID.

    The name and tag combination must be unique at the trader level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``StrategyId`` class.

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
            If value is not a valid string containing a hyphen.

        """
        Condition.true(
            value == _NULL_ID or "-" in value,
            "ID incorrectly formatted (did not contain '-' hyphen)",
        )
        super().__init__(value)

    cpdef str get_tag(self):
        """
        Return the order ID tag value for this ID.

        Returns
        -------
        str

        """
        return self.value.partition("-")[2]


cdef class AccountId(Identifier):
    """
    Represents a valid account ID.

    The issuer and ID combination must be unique at the fund level.
    """

    def __init__(self, str issuer, str number):
        """
        Initialize a new instance of the ``AccountId`` class.

        Parameters
        ----------
        issuer : str
            The account issuer (exchange/broker) ID value.
        number : str
            The account 'number' ID value.

        Raises
        ------
        ValueError
            If issuer is not a valid string.
        ValueError
            If number is not a valid string.

        """
        Condition.valid_string(issuer, "issuer")
        Condition.valid_string(number, "number")
        super().__init__(f"{issuer}-{number}")

        self.issuer = issuer
        self.number = number

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


cdef class ClientId(Identifier):
    """
    Represents a system client ID.

    The ID value must be unique per data or execution engine.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``ClientId`` class.

        Parameters
        ----------
        value : str
            The client ID value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)


cdef class ClientOrderId(Identifier):
    """
    Represents a valid client order ID.

    The ID value must be unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``ClientOrderId`` class.

        Parameters
        ----------
        value : str
            The client order ID value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)


cdef class ClientOrderLinkId(Identifier):
    """
    Represents a valid client order link ID.

    The ID value must be unique at the account level.

    Permits order originators to tie together groups of orders in which trades
    resulting from orders are associated for a specific purpose, for example the
    calculation of average execution price for a customer or to associate lists
    submitted to a broker as waves of a larger program trade.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.sp2/tagnum_583.html
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``ClientOrderLinkId`` class.

        Parameters
        ----------
        value : str
            The client order link ID value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)


cdef VenueOrderId _NULL_ORDER_ID = VenueOrderId(_NULL_ID)

cdef class VenueOrderId(Identifier):
    """
    Represents a valid venue order ID.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``VenueOrderId`` class.

        Parameters
        ----------
        value : str
            The venue assigned order ID value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        References
        ----------
        Null Object Pattern
        https://deviq.com/null-object-pattern/

        """
        super().__init__(value)

    @staticmethod
    cdef VenueOrderId null_c():
        return _NULL_ORDER_ID

    @staticmethod
    def null():
        """
        Return an order ID with a 'NULL' value.

        Returns
        -------
        VenueOrderId

        """
        return _NULL_ORDER_ID


cdef PositionId _NULL_POSITION_ID = PositionId(_NULL_ID)

cdef class PositionId(Identifier):
    """
    Represents a valid position ID.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``PositionId`` class.

        Parameters
        ----------
        value : str
            The position ID value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        References
        ----------
        Null Object Pattern
        https://deviq.com/null-object-pattern/

        """
        super().__init__(value)

    @staticmethod
    cdef PositionId null_c():
        return _NULL_POSITION_ID

    @staticmethod
    def null():
        """
        Return a position ID with a 'NULL' value.

        Returns
        -------
        PositionId

        """
        return _NULL_POSITION_ID


cdef class ExecutionId(Identifier):
    """
    Represents a valid execution ID.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``ExecutionId`` class.

        Parameters
        ----------
        value : str
            The execution ID value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)
