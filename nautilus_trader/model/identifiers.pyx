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
            The value of the identifier.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        Condition.valid_string(value, "value")

        self.value = value

    def __eq__(self, Identifier other) -> bool:
        return isinstance(other, type(self)) and self.value == other.value

    def __ne__(self, Identifier other) -> bool:
        return not self == other

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
    Represents a valid ticker symbol identifier for a tradeable financial market
    instrument.

    The identifier value must be unique for a trading venue.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``Symbol`` class.

        Parameters
        ----------
        value : str
            The ticker symbol identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)


cdef class Venue(Identifier):
    """
    Represents a valid trading venue identifier for a tradeable financial market
    instrument.
    """

    def __init__(self, str name):
        """
        Initialize a new instance of the ``Venue`` class.

        Parameters
        ----------
        name : str
            The venue name identifier value.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        super().__init__(name)


cdef class InstrumentId(Identifier):
    """
    Represents a valid instrument identifier.

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

        cdef tuple pieces = value.partition('.')

        if len(pieces) != 3:
            raise ValueError(f"The InstrumentId string value was malformed, was {value}")

        return InstrumentId(symbol=Symbol(pieces[0]), venue=Venue(pieces[2]))

    @staticmethod
    def from_str(value: str) -> InstrumentId:
        """
        Return an instrument identifier parsed from the given string value.
        Must be correctly formatted including characters either side of a single
        period.

        Examples: "AUD/USD.IDEALPRO", "BTC/USDT.BINANCE"

        Parameters
        ----------
        value : str
            The instrument identifier string value to parse.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_str_c(value)


cdef class TraderId(Identifier):
    """
    Represents a valid trader identifier.

    The name and tag combination identifier value must be unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``TraderId`` class.

        Must be correctly formatted with two valid strings either side of a hyphen.
        It is expected a trader identifier is the abbreviated name of the trader
        with an order identifier tag number separated by a hyphen.

        Example: "TESTER-001".

        Parameters
        ----------
        value : str
            The trader identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string containing a hyphen.

        """
        Condition.true(
            value == _NULL_ID or "-" in value,
            "identifier incorrectly formatted (did not contain '-' hyphen)",
        )
        super().__init__(value)

    cpdef str get_tag(self):
        """
        Return the order identifier tag value for this identifier.

        Returns
        -------
        str

        """
        return self.value.partition("-")[2]


cdef StrategyId _NULL_STRATEGY_ID = StrategyId(_NULL_ID)

cdef class StrategyId(Identifier):
    """
    Represents a valid strategy identifier.

    The name and tag combination must be unique at the trader level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``StrategyId`` class.

        Must be correctly formatted with two valid strings either side of a hyphen.
        Is is expected a strategy identifier is the class name of the strategy with
        an order identifier tag number separated by a hyphen.

        Example: "EMACross-001".

        Parameters
        ----------
        value : str
            The strategy identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string containing a hyphen.

        """
        Condition.true(
            value == _NULL_ID or "-" in value,
            "identifier incorrectly formatted (did not contain '-' hyphen)",
        )
        super().__init__(value)

    cpdef str get_tag(self):
        """
        Return the order identifier tag value for this identifier.

        Returns
        -------
        str

        """
        return self.value.partition("-")[2]

    @staticmethod
    cdef StrategyId null_c():
        return _NULL_STRATEGY_ID

    @staticmethod
    def null():
        """
        Return a strategy identifier with a 'NULL' value.

        Returns
        -------
        StrategyId

        """
        return _NULL_STRATEGY_ID


cdef class AccountId(Identifier):
    """
    Represents a valid account identifier.

    The issuer and identifier combination must be unique at the fund level.
    """

    def __init__(self, str issuer, str number):
        """
        Initialize a new instance of the ``AccountId`` class.

        Parameters
        ----------
        issuer : str
            The account issuer (exchange/broker) identifier value.
        number : str
            The account 'number' identifier value.

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
        Return an account identifier from the given string value. Must be
        correctly formatted with two valid strings either side of a hyphen.

        Example: "IB-D02851908".

        Parameters
        ----------
        value : str
            The value for the account identifier.

        Returns
        -------
        AccountId

        """
        return AccountId.from_str_c(value)


cdef class ClientId(Identifier):
    """
    Represents a system client identifier.

    The identifier value must be unique per data or execution engine.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``ClientId`` class.

        Parameters
        ----------
        value : str
            The client identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)


cdef class ClientOrderId(Identifier):
    """
    Represents a valid client order identifier.

    The identifier value must be unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``ClientOrderId`` class.

        Parameters
        ----------
        value : str
            The client order identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)


cdef class ClientOrderLinkId(Identifier):
    """
    Represents a valid client order link identifier.

    The identifier value must be unique at the account level.

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
            The client order link identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)


cdef VenueOrderId _NULL_ORDER_ID = VenueOrderId(_NULL_ID)

cdef class VenueOrderId(Identifier):
    """
    Represents a valid venue order identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``VenueOrderId`` class.

        Parameters
        ----------
        value : str
            The venue assigned order identifier value.

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
        Return an order identifier with a 'NULL' value.

        Returns
        -------
        VenueOrderId

        """
        return _NULL_ORDER_ID


cdef PositionId _NULL_POSITION_ID = PositionId(_NULL_ID)

cdef class PositionId(Identifier):
    """
    Represents a valid position identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``PositionId`` class.

        Parameters
        ----------
        value : str
            The position identifier value.

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
        Return a position identifier with a 'NULL' value.

        Returns
        -------
        PositionId

        """
        return _NULL_POSITION_ID


cdef class ExecutionId(Identifier):
    """
    Represents a valid execution identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ``ExecutionId`` class.

        Parameters
        ----------
        value : str
            The execution identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)
