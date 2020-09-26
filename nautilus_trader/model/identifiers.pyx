# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.core.types cimport Identifier
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.account_type cimport account_type_from_string
from nautilus_trader.model.c_enums.account_type cimport account_type_to_string


cdef str _NONE_ID = 'NoneId'


cdef class Symbol(Identifier):
    """
    Represents the symbol for a financial market tradeable instrument.
    The code and venue combination identifier value must be unique at the
    fund level.
    """

    def __init__(
            self,
            str code,
            Venue venue not None,
    ):
        """
        Initialize a new instance of the Symbol class.

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
    cdef Symbol from_string(str value):
        """
        Return a symbol parsed from the given string value. Must be correctly
        formatted with two valid strings either side of a period '.'.

        Example: "AUD/USD.FXCM".

        Parameters
        ----------
        value : str
            The symbol string value to parse.

        Returns
        -------
        Symbol

        """
        Condition.valid_string(value, "value")

        cdef tuple partitioned = value.partition('.')
        return Symbol(partitioned[0], Venue(partitioned[2]))

    @staticmethod
    def py_from_string(value: str) -> Symbol:
        """
        Python wrapper for the from_string method.

        Return a symbol parsed from the given string value. Must be correctly
        formatted with two valid strings either side of a period '.'.

        Example: "AUD/USD.FXCM".

        Parameters
        ----------
        value : str
            The symbol string value to parse.

        Returns
        -------
        Symbol

        """
        return Symbol.from_string(value)


cdef class Venue(Identifier):
    """
    Represents a trading venue for a financial market tradeable instrument.
    The identifier value must be unique at the fund level.
    """

    def __init__(self, str name):
        """
        Initialize a new instance of the Venue class.

        Parameters
        ----------
        name : str
            The venue name identifier value.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        super().__init__(name.upper())


cdef class Exchange(Venue):
    """
    Represents an exchange that financial market instruments are traded on.
    The identifier value must be unique at the fund level.
    """

    def __init__(self, str name):
        """
        Initialize a new instance of the Exchange class.

        Parameters
        ----------
        name : str
            The exchange name identifier value.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        super().__init__(name.upper())


cdef class Brokerage(Identifier):
    """
    Represents a brokerage. The identifier value must be unique at the fund
    level.
    """

    def __init__(self, str name):
        """
        Initialize a new instance of the Brokerage class.

        Parameters
        ----------
        name : str
            The brokerage name identifier value.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        super().__init__(name.upper())


cdef class IdTag(Identifier):
    """
    Represents an identifier tag.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the IdTag class.

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
    Represents a valid trader identifier. The name and order_id_tag combination
    identifier value must be unique at the fund level.
    """

    def __init__(self, str name, str order_id_tag):
        """
        Initialize a new instance of the TraderId class.

        Parameters
        ----------
        name : str
            The trader name identifier value.
        order_id_tag : str
            The trader order identifier tag value.

        Raises
        ------
        ValueError
            If name is not a valid string.
        ValueError
            If order_id_tag is not a valid string.

        """
        Condition.valid_string(name, "name")
        Condition.valid_string(order_id_tag, "order_id_tag")
        super().__init__(f"{name}-{order_id_tag}")

        self.name = name
        self.order_id_tag = IdTag(order_id_tag)

    @staticmethod
    cdef TraderId from_string(str value):
        """
        Return a trader identifier parsed from the given string value. Must be
        correctly formatted with two valid strings either side of a hyphen '-'.

        Its is expected a trader identifier  is the abbreviated name of the
        trader with an order identifier tag number separated by a hyphen '-'.

        Example: "TESTER-001".

        Parameters
        ----------
        value : str
            The value for the strategy identifier.

        Returns
        -------
        TraderId

        """
        Condition.valid_string(value, "value")

        cdef tuple partitioned = value.partition('-')

        return TraderId(name=partitioned[0], order_id_tag=partitioned[2])

    @staticmethod
    def py_from_string(value: str) -> TraderId:
        """
        Return a trader identifier parsed from the given string value. Must be
        correctly formatted with two valid strings either side of a hyphen '-'.

        Its is expected a trader identifier  is the abbreviated name of the
        trader with an order identifier tag number separated by a hyphen '-'.

        Example: "TESTER-001".

        Parameters
        ----------
        value : str
            The value for the strategy identifier.

        Returns
        -------
        TraderId

        """
        return TraderId.from_string(value)


cdef class StrategyId(Identifier):
    """
    Represents a valid strategy identifier. The name and order_id_tag combination
    must be unique at the trader level.
    """

    def __init__(self, str name, str order_id_tag):
        """
        Initialize a new instance of the StrategyId class.

        Parameters
        ----------
        name : str
            The strategy name identifier value.
        order_id_tag : str
            The strategy order identifier tag value.

        Raises
        ------
        ValueError
            If name is not a valid string.
        ValueError
            If order_id_tag is not a valid string.

        """
        Condition.valid_string(name, "name")
        Condition.valid_string(order_id_tag, "order_id_tag")
        super().__init__(f"{name}-{order_id_tag}")

        self.name = name
        self.order_id_tag = IdTag(order_id_tag)

    @staticmethod
    cdef StrategyId from_string(str value):
        """
        Return a strategy identifier parsed from the given string value. Must be
        correctly formatted with two valid strings either side of a hyphen '-'.

        Is is expected a strategy identifier is the class name of the strategy with
        an order_id tag number separated by a hyphen '-'.

        Example: "EMACross-001".

        Parameters
        ----------
        value : str
            The value for the strategy identifier.

        Returns
        -------
        StrategyId

        """
        Condition.valid_string(value, "value")

        cdef tuple partitioned = value.partition('-')
        return StrategyId(name=partitioned[0], order_id_tag=partitioned[2])

    @staticmethod
    def py_from_string(value: str) -> StrategyId:
        """
        Return a strategy identifier parsed from the given string value. Must be
        correctly formatted with two valid strings either side of a hyphen '-'.

        Is is expected a strategy identifier is the class name of the strategy with
        an order_id tag number separated by a hyphen '-'.

        Example: "EMACross-001".

        Parameters
        ----------
        value : str
            The value for the strategy identifier.

        Returns
        -------
        StrategyId

        """
        return StrategyId.from_string(value)


cdef class AccountId(Identifier):
    """
    Represents a valid account identifier. The broker and account_number
    combination must be unique at the fund level.
    """

    def __init__(
            self,
            str broker,
            str account_number,
            AccountType account_type,
    ):
        """
        Initialize a new instance of the AccountId class.

        Parameters
        ----------
        broker : str
            The broker identifier value.
        account_number : str
            The account number identifier value.
        account_type : AccountType
            The account type.

        Raises
        ------
        ValueError
            If broker is not a valid string.
        ValueError
            If account_number is not a valid string.

        """
        Condition.valid_string(broker, "broker")
        Condition.valid_string(account_number, "account_number")
        super().__init__(f"{broker}-{account_number}-{account_type_to_string(account_type)}")

        self.broker = Brokerage(broker)
        self.account_number = AccountNumber(account_number)
        self.account_type = account_type

    @staticmethod
    cdef AccountId from_string(str value):
        """
        Return an account identifier from the given string value. Must be correctly
        formatted with two valid strings either side of a hyphen '-'.

        Example: "FXCM-02851908-DEMO".

        Parameters
        ----------
        value : str
            The value for the account identifier.

        Returns
        -------
        AccountId

        """
        Condition.valid_string(value, "value")

        cdef list split = value.split('-', maxsplit=2)
        return AccountId(
            broker=split[0],
            account_number=split[1],
            account_type=account_type_from_string(split[2]))

    @staticmethod
    def py_from_string(value: str) -> AccountId:
        """
        Return an account identifier from the given string value. Must be correctly
        formatted with two valid strings either side of a hyphen '-'.

        Example: "FXCM-02851908-DEMO".

        Parameters
        ----------
        value : str
            The value for the account identifier.

        Returns
        -------
        AccountId

        """
        return AccountId.from_string(value)


cdef class AccountNumber(Identifier):
    """
    Represents a valid account number.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the AccountNumber class.

        Parameters
        ----------
        value : str
            The value of the account number.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)


cdef class BracketOrderId(Identifier):
    """
    Represents a valid bracket order identifier. The identifier value must be
    unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the OrderId class.

        Parameters
        ----------
        value : str
            The value of the order_id (should be unique).

        Raises
        ------
        ValueError
            If value is not a valid string or does not start with 'BO-'.

        """
        Condition.true(value.startswith("BO-"), f"value must begin with \"BO-\", was {value}.")
        super().__init__(value)


cdef class ClientOrderId(Identifier):
    """
    Represents a valid client order identifier. The identifier value must be unique at
    the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ClientOrderId class.

        Parameters
        ----------
        value : str
            The client order identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string, or does not start with 'O-'.

        """
        Condition.true(value.startswith("O-"), f"value must begin with \"O-\", was {value}.")
        super().__init__(value)


cdef class OrderId(Identifier):
    """
    Represents a valid order identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the OrderId class.

        Parameters
        ----------
        value : str
            The broker/exchange assigned order identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)


cdef class ClientPositionId(Identifier):
    """
    Represents a valid client position identifier. The identifier value must be unique
    at the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ClientPositionId class.

        Parameters
        ----------
        value : str
            The client position identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string, or does not start with 'P-'.

        """
        if value != _NONE_ID:
            Condition.true(value.startswith("P-"), f" value must begin with \"P-\", was {value}.")
        super().__init__(value)

    @staticmethod
    cdef ClientPositionId none():
        """
        Returns a client position identifier with a `None` value.

        Returns
        -------
        ClientPositionId

        """
        return ClientPositionId(_NONE_ID)

    cdef bint is_none_value(self):
        """
        Return a value indicating whether the identifier value is 'None'.

        Returns
        -------
        bool

        """
        return self.value == _NONE_ID


cdef class PositionId(Identifier):
    """
    Represents a valid position identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the PositionId class.

        Parameters
        ----------
        value : str
            The broker/exchange assigned position identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)

    @staticmethod
    cdef PositionId none():
        """
        Returns a position identifier with a `None` value.

        Returns
        -------
        PositionId

        """
        return PositionId(_NONE_ID)

    cdef bint is_none_value(self):
        """
        Return a value indicating whether the identifier value is 'None'.

        Returns
        -------
        bool

        """
        return self.value == _NONE_ID


cdef class ExecutionId(Identifier):
    """
    Represents a valid execution identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the ExecutionId class.

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


cdef class MatchId(Identifier):
    """
    Represents a valid trade match identifier.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the MatchId class.

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


cdef class InstrumentId(Identifier):
    """
    Represents a valid instrument identifier. The identifier value must be
    unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initialize a new instance of the InstrumentId class.

        Parameters
        ----------
        value : str
            The instrument identifier value.

        Raises
        ------
        ValueError
            If value is not a valid string.

        """
        super().__init__(value)
