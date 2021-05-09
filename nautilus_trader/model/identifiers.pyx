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

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the `Identifier` class.

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


cdef class Symbol(Identifier):
    """
    Represents a valid ticker symbol identifier for a financial market tradeable
    instrument.

    The identifier value must be unique for a trading venue.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the `Symbol` class.

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
    Represents a valid trading venue identifier for a financial market tradeable
    instrument.

    The identifier value must be unique at the fund level.
    """

    def __init__(self, str name, str broker=None):
        """
        Initialize a new instance of the `Venue` class.

        Parameters
        ----------
        name : str
            The venue name identifier value.
        broker : str, optional
            The broker name identifier value.

        Raises
        ------
        ValueError
            If name is not a valid string.
        ValueError
            If broker is not None and not a valid string.

        """
        if broker is not None:
            Condition.valid_string(broker, "broker")
            name = f"{broker}-{name}"
        super().__init__(name)

        self.client_id = ClientId(name)


cdef class InstrumentId(Identifier):
    """
    Represents a valid instrument identifier.

    The symbol and venue combination should uniquely identify the instrument.
    """

    def __init__(
        self,
        Symbol symbol not None,
        Venue venue not None,
    ):
        """
        Initialize a new instance of the `InstrumentId` class.

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

        cdef tuple venue_pieces = pieces[2].partition('-')
        if venue_pieces[2] != '':  # Venue contains a dash therefore a broker
            return InstrumentId(
                symbol=Symbol(pieces[0]),
                venue=Venue(venue_pieces[0], broker=venue_pieces[2]),
            )
        else:
            return InstrumentId(
                symbol=Symbol(pieces[0]),
                venue=Venue(pieces[2]),
            )

    @staticmethod
    def from_str(value: str) -> InstrumentId:
        """
        Return an instrument identifier parsed from the given string value.
        Must be correctly formatted including a single period and optional dash.

        Examples: "AUD/USD.IB-IDEALPRO", "BTC/USDT.BINANCE"

        Parameters
        ----------
        value : str
            The instrument identifier string value to parse.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_str_c(value)


cdef class IdTag(Identifier):
    """
    Represents a valid identifier tag.

    Can be used as part of a more complex identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the `IdTag` class.

        Parameters
        ----------
        value : str
            The identifier tag value.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        super().__init__(value)


cdef class TraderId(Identifier):
    """
    Represents a valid trader identifier.

    The name and tag combination identifier value must be unique at the fund level.
    """

    def __init__(self, str name, str tag):
        """
        Initialize a new instance of the `TraderId` class.

        Parameters
        ----------
        name : str
            The trader name identifier value. Used for internal system
            identification, it is never used for identifiers which may
            be sent outside of the Nautilus stack, such as on order identifiers.
        tag : str
            The trader identifier tag value. Used to tag client order identifiers
            which relate to a particular trader.

        Raises
        ------
        ValueError
            If name is not a valid string.
        ValueError
            If tag is not a valid string.

        """
        Condition.valid_string(name, "name")
        Condition.valid_string(tag, "tag")
        super().__init__(f"{name}-{tag}")

        self.name = name
        self.tag = IdTag(tag)

    @staticmethod
    cdef TraderId from_str_c(str value):
        Condition.valid_string(value, "value")

        cdef tuple pieces = value.partition('-')

        if len(pieces) != 3:
            raise ValueError(f"The TraderId string value was malformed, was {value}")

        return TraderId(name=pieces[0], tag=pieces[2])

    @staticmethod
    def from_str(value: str) -> TraderId:
        """
        Return a trader identifier parsed from the given string value. Must be
        correctly formatted with two valid strings either side of a hyphen.

        It is expected a trader identifier is the abbreviated name of the
        trader with an order identifier tag number separated by a hyphen.

        Example: "TESTER-001".

        Parameters
        ----------
        value : str
            The value for the strategy identifier.

        Returns
        -------
        TraderId

        """
        return TraderId.from_str_c(value)


cdef StrategyId _NULL_STRATEGY_ID = StrategyId(_NULL_ID, _NULL_ID)

cdef class StrategyId(Identifier):
    """
    Represents a valid strategy identifier.

    The name and tag combination must be unique at the trader level.
    """

    def __init__(self, str name, str tag):
        """
        Initialize a new instance of the `StrategyId` class.

        Parameters
        ----------
        name : str
            The strategy name identifier value.
        tag : str
            The strategy identifier tag value. Used to tag client order
            identifiers which relate to a particular strategy.

        Raises
        ------
        ValueError
            If name is not a valid string.
        ValueError
            If tag is not a valid string.

        """
        Condition.valid_string(name, "name")
        Condition.valid_string(tag, "tag")
        super().__init__(f"{name}-{tag}")

        self.name = name
        self.tag = IdTag(tag)

    @staticmethod
    cdef inline StrategyId null_c():
        return _NULL_STRATEGY_ID

    cdef inline bint is_null(self) except *:
        return self.value == "NULL-NULL"

    cdef inline bint not_null(self) except *:
        return self.value != "NULL-NULL"

    @staticmethod
    cdef StrategyId from_str_c(str value):
        Condition.valid_string(value, "value")

        cdef tuple pieces = value.partition('-')

        if len(pieces) != 3:
            raise ValueError(f"The StrategyId string value was malformed, was {value}")

        return StrategyId(name=pieces[0], tag=pieces[2])

    @staticmethod
    def from_str(value: str) -> StrategyId:
        """
        Return a strategy identifier parsed from the given string value.

        Must be correctly formatted with two valid strings either side of a hyphen.
        Is is expected a strategy identifier is the class name of the strategy with
        an order identifier tag number separated by a hyphen.

        Example: "EMACross-001".

        Parameters
        ----------
        value : str
            The value for the strategy identifier.

        Returns
        -------
        StrategyId

        """
        return StrategyId.from_str_c(value)

    @staticmethod
    def null():
        """
        Return a strategy identifier with a 'NULL' value.

        Returns
        -------
        StrategyId

        """
        return _NULL_STRATEGY_ID


cdef class Issuer(Identifier):
    """
    Represents an account issuer, may be a brokerage or exchange.
    """

    def __init__(self, str name):
        """
        Initialize a new instance of the `Issuer` class.

        Parameters
        ----------
        name : str
            The issuer identifier value.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        super().__init__(name)


cdef class AccountId(Identifier):
    """
    Represents a valid account identifier.

    The issuer and identifier combination must be unique at the fund level.
    """

    def __init__(self, str issuer, str identifier):
        """
        Initialize a new instance of the `AccountId` class.

        Parameters
        ----------
        issuer : str
            The issuer identifier value (exchange/broker).
        identifier : str
            The account identifier value.

        Raises
        ------
        ValueError
            If issuer is not a valid string.
        ValueError
            If identifier is not a valid string.

        """
        super().__init__(f"{issuer}-{identifier}")

        self.issuer = Issuer(issuer)
        self.identifier = Identifier(identifier)

    cdef Venue issuer_as_venue(self):
        return Venue(self.issuer.value)

    @staticmethod
    cdef AccountId from_str_c(str value):
        Condition.valid_string(value, "value")

        cdef list pieces = value.split('-', maxsplit=1)

        if len(pieces) != 2:
            raise ValueError(f"The AccountId string value was malformed, was {value}")

        return AccountId(issuer=pieces[0], identifier=pieces[1])

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

    The identifier value must be unique per running `Trader` instance.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the `ClientId` class.

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

    @staticmethod
    cdef ClientId from_str_c(str value):
        Condition.valid_string(value, "value")

        return ClientId(value)

    @staticmethod
    def from_str(value: str) -> ClientId:
        """
        Return a client identifier parsed from the given string value.

        Parameters
        ----------
        value : str
            The client identifier string value to parse.

        Returns
        -------
        ClientId

        """
        return ClientId.from_str_c(value)


cdef class ClientOrderId(Identifier):
    """
    Represents a valid client order identifier.

    The identifier value must be unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the `ClientOrderId` class.

        Parameters
        ----------
        value : str
            The client order identifier value.

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
        Initialize a new instance of the `ClientOrderLinkId` class.

        Parameters
        ----------
        value : str
            The client order link identifier value.

        """
        super().__init__(value)


cdef VenueOrderId _NULL_ORDER_ID = VenueOrderId(_NULL_ID)

cdef class VenueOrderId(Identifier):
    """
    Represents a valid venue order identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the `VenueOrderId` class.

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

    cdef bint is_null(self) except *:
        return self.value == _NULL_ID

    cdef bint not_null(self) except *:
        return self.value != _NULL_ID

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
        Initialize a new instance of the `PositionId` class.

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

    cdef bint is_null(self) except *:
        return self.value == _NULL_ID

    cdef bint not_null(self) except *:
        return self.value != _NULL_ID

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
        Initialize a new instance of the `ExecutionId` class.

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


cdef class TradeMatchId(Identifier):
    """
    Represents a valid and unique trade match identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the `TradeMatchId` class.

        Parameters
        ----------
        value : str
            The trade match identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)
