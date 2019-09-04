# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport Identifier
from nautilus_trader.model.c_enums.account_type cimport (
    AccountType,
    account_type_to_string,
    account_type_from_string)


cdef class Symbol(Identifier):
    """
    Represents the symbol for a financial market tradeable instrument.
    The code and and venue combination identifier value must be unique at the
    fund level.
    """

    def __init__(self,
                 str code,
                 Venue venue):
        """
        Initializes a new instance of the Symbol class.

        :param code: The symbols code identifier value.
        :param venue: The symbols venue.
        :raises ConditionFailed: If the code is not a valid string.
        """
        super().__init__(f'{code.upper()}.{venue.value}')

        Condition.valid_string(code, 'code')
        assert code.isupper()

        self.code = code.upper()
        self.venue = venue

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return self.value

    @staticmethod
    cdef Symbol from_string(str value):
        """
        Return a symbol parsed from the given string value. Must be correctly 
        formatted with two valid strings either side of a period '.'.
        
        Example: 'AUDUSD.FXCM'.
        
        :param value: The symbol string value to parse.
        :return Symbol.
        """
        cdef tuple partitioned = value.partition('.')
        return Symbol(partitioned[0], Venue(partitioned[2]))

    @staticmethod
    def py_from_string(value: str) -> Symbol:
        """
        Python wrapper for the from_string method.

        Return a symbol parsed from the given string value. Must be correctly
        formatted with two valid strings either side of a period '.'.

        Example: 'AUDUSD.FXCM'.

        :param value: The symbol string value to parse.
        :return Symbol.
        """
        return Symbol.from_string(value)


cdef class Venue(Identifier):
    """
    Represents a trading venue for a financial market tradeable instrument.
    The identifier value must be unique at the fund level.
    """

    def __init__(self, str name):
        """
        Initializes a new instance of the Venue class.

        :param name: The venue name identifier value.
        :raises ConditionFailed: If the name is not a valid string.
        """
        super().__init__(name.upper())


cdef class Exchange(Venue):
    """
    Represents an exchange that financial market instruments are traded on.
    The identifier value must be unique at the fund level.
    """

    def __init__(self, str name):
        """
        Initializes a new instance of the Exchange class.

        :param name: The exchange name identifier value.
        :raises ConditionFailed: If the name is not a valid string.
        """
        super().__init__(name.upper())


cdef class Brokerage(Identifier):
    """
    Represents a brokerage. The identifier value must be unique at the fund
    level.
    """

    def __init__(self, str name):
        """
        Initializes a new instance of the Brokerage class.

        :param name: The brokerage name identifier value.
        :raises ConditionFailed: If the name is not a valid string.
        """
        super().__init__(name.upper())


cdef class Label(Identifier):
    """
    Represents a valid label.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the Label class.

        :param value: The label identifier value.
        :raises ConditionFailed: If the value is not a valid string.
        """
        super().__init__(value)


cdef class IdTag(Identifier):
    """
    Represents an identifier tag.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the IdTag class.

        :param value: The identifier tag value.
        :raises ConditionFailed: If the value is not a valid string.
        """
        super().__init__(value)


cdef class TraderId(Identifier):
    """
    Represents a valid trader identifier. The name and order_id_tag combination
    identifier value must be unique at the fund level.
    """

    def __init__(self, str name, str order_id_tag):
        """
        Initializes a new instance of the TraderId class.

        :param name: The trader name identifier value.
        :param order_id_tag: The trader order_id tag value.
        :raises ConditionFailed: If the name is not a valid string.
        :raises ConditionFailed: If the order_id_tag is not a valid string.
        """
        super().__init__(f'{name}-{order_id_tag}')

        Condition.valid_string(name, 'name')

        self.name = name
        self.order_id_tag = IdTag(order_id_tag)

    @staticmethod
    cdef TraderId from_string(str value):
        """
        Return a trader_id parsed from the given string value. Must be 
        correctly formatted with two valid strings either side of a hyphen '-'.
        
        Its is expected a trader_id is the abbreviated name of the trader with
        an order_id tag number separated by a hyphen '-'.
        
        Example: 'TESTER-001'.

        :param value: The value for the strategy_id.
        :return TraderId.
        """
        cdef tuple partitioned = value.partition('-')

        return TraderId(name=partitioned[0], order_id_tag=partitioned[2])

    @staticmethod
    def py_from_string(value: str) -> TraderId:
        """
        Python wrapper for the from_string method.

        Return a trader_id parsed from the given string value. Must be
        correctly formatted with two valid strings either side of a hyphen '-'.

        Its is expected a trader_id is the abbreviated name of the trader with
        an order_id tag number separated by a hyphen '-'.

        Example: 'TESTER-001'.

        :param value: The value for the trader_id.
        :return TraderId.
        """
        return TraderId.from_string(value)


cdef class StrategyId(Identifier):
    """
    Represents a valid strategy identifier. The name and order_id_tag combination
    must be unique at the trader level.
    """

    def __init__(self, str name, str order_id_tag):
        """
        Initializes a new instance of the StrategyId class.

        :param name: The strategy name identifier value.
        :param order_id_tag: The strategy order_id tag value.
        :raises ConditionFailed: If the name is not a valid string.
        :raises ConditionFailed: If the order_id_tag is not a valid string.
        """
        super().__init__(f'{name}-{order_id_tag}')
        Condition.valid_string(name, 'name')

        self.name = name
        self.order_id_tag = IdTag(order_id_tag)

    @staticmethod
    cdef StrategyId from_string(str value):
        """
        Return a strategy_id parsed from the given string value. Must be 
        correctly formatted with two valid strings either side of a hyphen '-'.
        
        Is is expected a strategy_id is the class name of the strategy with
        an order_id tag number separated by a hyphen '-'.
        
        Example: 'EMACross-001'.

        :param value: The value for the strategy_id.
        :return StrategyId.
        """
        cdef tuple partitioned = value.partition('-')
        return StrategyId(name=partitioned[0], order_id_tag=partitioned[2])

    @staticmethod
    def py_from_string(value: str) -> StrategyId:
        """
        Python wrapper for the from_string method.

        Return a strategy_id parsed from the given string value. Must be
        correctly formatted with two valid strings either side of a hyphen '-'.

        Is is expected a strategy_id is the class name of the strategy with
        an order_id tag number separated by a hyphen '-'.

        Example: 'EMACross-001'.

        :param value: The value for the strategy_id.
        :return StrategyId.
        """
        return StrategyId.from_string(value)


cdef class AccountId(Identifier):
    """
    Represents a valid account identifier. The broker and account_number
    combination must be unique at the fund level.
    """

    def __init__(self, str broker, str account_number, AccountType account_type):
        """
        Initializes a new instance of the AccountId class.

        :param broker: The broker identifier value.
        :param account_number: The account number identifier value.
        :param account_number: The account type.
        :raises ConditionFailed: If the broker is not a valid string.
        :raises ConditionFailed: If the account_number is not a valid string.
        """
        super().__init__(f'{broker}-{account_number}-{account_type_to_string(account_type)}')

        self.broker = Brokerage(broker)
        self.account_number = AccountNumber(account_number)
        self.account_type = account_type

    @staticmethod
    cdef AccountId from_string(str value):
        """
        Return an account_id from the given string value. Must be correctly
        formatted with two valid strings either side of a hyphen '-'.
        
        Example: 'FXCM-02851908'.

        :param value: The value for the account_id.
        :return AccountId.
        """
        cdef list split = value.split('-', maxsplit=3)
        return AccountId(
            broker=split[0],
            account_number=split[1],
            account_type=account_type_from_string(split[2]))

    @staticmethod
    def py_from_string(value: str) -> AccountId:
        """
        Python wrapper for the from_string method.

        Return an account_id from the given string value. Must be correctly
        formatted with two valid strings either side of a hyphen '-'.

        Example: 'FXCM-02851908'.

        :param value: The value for the account_id.
        :return AccountId.
        """
        return AccountId.from_string(value)


cdef class AccountNumber(Identifier):
    """
    Represents a valid account number.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the AccountNumber class.

        :param value: The value of the account number.
        """
        super().__init__(value)


cdef class AtomicOrderId(Identifier):
    """
    Represents a valid atomic order identifier. The identifier value must be
    unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the OrderId class.

        :param value: The value of the order_id (should be unique).
        """
        super().__init__(value)

        Condition.true(value.startswith('AO-'), f'value must begin with \'AO-\', was {value}.')


cdef class OrderId(Identifier):
    """
    Represents a valid order identifier. The identifier value must be unique at
    the fund level.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the OrderId class.

        :param value: The value of the order_id (should be unique).
        """
        super().__init__(value)

        Condition.true(value.startswith('O-'), f'value must begin with \'O-\', was {value}.')


cdef class OrderIdBroker(Identifier):
    """
    Represents a valid broker order identifier.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the OrderId class.

        :param value: The broker order identifier value.
        """
        super().__init__(value)


cdef class PositionId(Identifier):
    """
    Represents a valid position identifier. The identifier value must be unique
    at the fund level.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the PositionId class.

        :param value: The position identifier value.
        """
        super().__init__(value)

        Condition.true(value.startswith('P-'), f' value must begin with \'P-\', was {value}.')


cdef class ExecutionId(Identifier):
    """
    Represents a valid execution identifier.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the ExecutionId class.

        :param value: The execution identifier value.
        """
        super().__init__(value)


cdef class ExecutionTicket(Identifier):
    """
    Represents a valid execution ticket.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the ExecutionTicket class.

        :param value: The execution ticket identifier value.
        """
        super().__init__(value)


cdef class InstrumentId(Identifier):
    """
    Represents a valid instrument identifier. The identifier value must be
    unique at the fund level.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the InstrumentId class.

        :param value: The instrument identifier value.
        """
        super().__init__(value)
