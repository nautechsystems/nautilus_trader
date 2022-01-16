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


cdef class Identifier:
    """
    The abstract base class for all identifiers.

    Parameters
    ----------
    value : str
        The value of the ID.

    Raises
    ------
    ValueError
        If `value` is not a valid string.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, str value):
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


cdef class Symbol(Identifier):
    """
    Represents a valid ticker symbol ID for a tradeable financial market
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

    References
    ----------
    https://en.wikipedia.org/wiki/Ticker_symbol
    """

    def __init__(self, str value):
        super().__init__(value)


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

    def __init__(self, str name):
        super().__init__(name)


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


cdef class ComponentId(Identifier):
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
    """

    def __init__(self, str value):
        super().__init__(value)


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
    """

    def __init__(self, str value):
        super().__init__(value)


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
    """

    def __init__(self, str value):
        Condition.true("-" in value, "ID incorrectly formatted (did not contain '-' hyphen)")
        super().__init__(value)

    cpdef str get_tag(self):
        """
        Return the order ID tag value for this ID.

        Returns
        -------
        str

        """
        return self.value.partition("-")[2]


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
    """

    def __init__(self, str value):
        Condition.true("-" in value, "ID incorrectly formatted (did not contain '-' hyphen)")
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
    """

    def __init__(self, str issuer, str number):
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


cdef class ClientOrderId(Identifier):
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
    """

    def __init__(self, str value):
        super().__init__(value)


cdef class ClientOrderLinkId(Identifier):
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

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.sp2/tagnum_583.html
    """

    def __init__(self, str value):
        super().__init__(value)


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

    def __init__(self, str value):
        super().__init__(value)


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

    def __init__(self, str value):
        super().__init__(value)


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
        If `value` is not a valid string.
    """

    def __init__(self, str value):
        super().__init__(value)


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

    Raises
    ------
    ValueError
        If `value` is not a valid string.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0/tagnum_1003.html
    """

    def __init__(self, str value):
        super().__init__(value)
