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
        return self._is_subclass(type(other)) and self.value == other.value

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
        return hash((type(self), self.value))

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"

    cdef inline bint _is_subclass(self, type other) except *:
        cdef type type_self = type(self)
        return issubclass(other, type_self) or issubclass(type_self, other)


cdef class Symbol(Identifier):
    """
    Represents the symbol for a financial market tradeable instrument.

    The code and venue combination identifier value must be unique at the
    fund level.
    """

    def __init__(self, str code, Venue venue not None):
        """
        Initialize a new instance of the `Symbol` class.

        Parameters
        ----------
        code : str
            The symbols code identifier value.
        venue : Venue
            The symbols venue.

        Raises
        ------
        ValueError
            If code is not a valid string.

        """
        Condition.valid_string(code, "code")
        super().__init__(f"{code}.{venue.value}")

        self.code = code
        self.venue = venue

    @staticmethod
    cdef Symbol from_str_c(str value):
        Condition.valid_string(value, "value")

        cdef tuple pieces = value.partition('.')
        return Symbol(pieces[0], Venue(pieces[2]))

    @staticmethod
    def from_str(value: str) -> Symbol:
        """
        Return a symbol parsed from the given string value. Must be correctly
        formatted with two valid strings either side of a period.

        Example: "AUD/USD.IDEALPRO".

        Parameters
        ----------
        value : str
            The symbol string value to parse.

        Returns
        -------
        Symbol

        """
        return Symbol.from_str_c(value)


cdef class Venue(Identifier):
    """
    Represents a trading venue for a financial market tradeable instrument.

    The identifier value must be unique at the fund level.
    """

    def __init__(self, str name):
        """
        Initialize a new instance of the `Venue` class.

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


cdef class Exchange(Venue):
    """
    Represents an exchange that financial market instruments are traded on.

    The identifier value must be unique at the fund level.
    """

    def __init__(self, str name):
        """
        Initialize a new instance of the `Exchange` class.

        Parameters
        ----------
        name : str
            The exchange name identifier value.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        super().__init__(name)


cdef class Brokerage(Identifier):
    """
    Represents a brokerage intermediary.
    """

    def __init__(self, str name):
        """
        Initialize a new instance of the `Brokerage` class.

        Parameters
        ----------
        name : str
            The broker name identifier value.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        super().__init__(name)


cdef class IdTag(Identifier):
    """
    Represents a generic identifier tag.
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

        Its is expected a trader identifier  is the abbreviated name of the
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
        an order_id tag number separated by a hyphen.

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
        Return a strategy identifier with an `NULL` value.

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


cdef class BracketOrderId(Identifier):
    """
    Represents a valid bracket order identifier.

    The identifier value must be unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the `OrderId` class.

        Parameters
        ----------
        value : str
            The value of the order_id (should be unique).

        """
        super().__init__(value)


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

    The identifier value must be unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the `ClientOrderId` class.

        Parameters
        ----------
        value : str
            The client order link identifier value.

        """
        super().__init__(value)


cdef OrderId _NULL_ORDER_ID = OrderId(_NULL_ID)

cdef class OrderId(Identifier):
    """
    Represents a valid order identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the `OrderId` class.

        Parameters
        ----------
        value : str
            The exchange/broker assigned order identifier value.

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
    cdef OrderId null_c():
        return _NULL_ORDER_ID

    cdef bint is_null(self) except *:
        return self.value == _NULL_ID

    cdef bint not_null(self) except *:
        return self.value != _NULL_ID

    @staticmethod
    def null():
        """
        Return an order identifier with an `NULL` value.

        Returns
        -------
        OrderId

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
            The exchange/broker assigned position identifier value.

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
        Return a position identifier with a `NULL` value.

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
