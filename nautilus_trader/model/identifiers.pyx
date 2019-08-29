# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport Identifier


cdef class Symbol(Identifier):
    """
    Represents the symbol for a financial market tradeable instrument.
    """

    def __init__(self,
                 str code,
                 Venue venue):
        """
        Initializes a new instance of the Symbol class.

        :param code: The symbols code.
        :param venue: The symbols venue.
        :raises ConditionFailed: If the code is not a valid string.
        """
        Condition.valid_string(code, 'code')

        self.code = code.upper()
        self.venue = venue
        # Super class initialization last because of .upper()
        super().__init__(f'{self.code}.{self.venue.value}')

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
    """

    def __init__(self, str name):
        """
        Initializes a new instance of the Venue class.

        :param name: The venues name.
        :raises ConditionFailed: If the name is not a valid string.
        """
        super().__init__(name.upper())


cdef class Exchange(Venue):
    """
    Represents an exchange that financial market instruments are traded on.
    """

    def __init__(self, str name):
        """
        Initializes a new instance of the Exchange class.

        :param name: The exchanges name.
        :raises ConditionFailed: If the name is not a valid string.
        """
        super().__init__(name.upper())


cdef class Brokerage(Identifier):
    """
    Represents a brokerage.
    """

    def __init__(self, str name):
        """
        Initializes a new instance of the Brokerage class.

        :param name: The brokerages name.
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

        :param value: The value of the label.
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

        :param value: The value of the identifier tag.
        :raises ConditionFailed: If the value is not a valid string.
        """
        super().__init__(value)


cdef class TraderId(Identifier):
    """
    Represents a valid trader identifier, the name and order_id_tag combination
    must be unique at the fund level.
    """

    def __init__(self, str name, str order_id_tag):
        """
        Initializes a new instance of the TraderId class.

        :param name: The name of the trader.
        :param name: The order_id tag for the trader.
        :raises ConditionFailed: If the name is not a valid string.
        :raises ConditionFailed: If the order_id_tag is not a valid string.
        """
        Condition.valid_string(name, 'name')

        super().__init__(f'{name}-{order_id_tag}')
        self.name = name
        self.order_id_tag = IdTag(order_id_tag)

    @staticmethod
    cdef TraderId from_string(str value):
        """
        Return a trader_id parsed from the given string value. Must be 
        correctly formatted with two valid strings either side of a hyphen '-'.
        
        Normally a trader_id is the abbreviated name of the trader and
        an order_id tag number separated by a hyphen '-'.
        
        Example: 'Trader1-001'.

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

        Normally a trader_id is the abbreviated name of the trader and
        an order_id tag number separated by a hyphen '-'.

        Example: 'Trader1-001'.

        :param value: The value for the trader_id.
        :return TraderId.
        """
        return TraderId.from_string(value)


cdef class StrategyId(Identifier):
    """
    Represents a valid strategy identifier, the name and order_id_tag combination
    must be unique at the trader level.
    """

    def __init__(self, str name, str order_id_tag):
        """
        Initializes a new instance of the StrategyId class.

        :param name: The name of the strategy.
        :param order_id_tag: The order_id tag for the strategy.
        :raises ConditionFailed: If the name is not a valid string.
        :raises ConditionFailed: If the order_id_tag is not a valid string.
        """
        Condition.valid_string(name, 'name')

        super().__init__(f'{name}-{order_id_tag}')
        self.name = name
        self.order_id_tag = IdTag(order_id_tag)

    @staticmethod
    cdef StrategyId from_string(str value):
        """
        Return a strategy_id parsed from the given string value. Must be 
        correctly formatted with two valid strings either side of a hyphen '-'.
        
        Normally a strategy_id is the class name of the strategy and
        an order_id tag number separated by a hyphen '-'.
        
        Example: 'MyStrategy-001'.

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

        Normally a strategy_id is the class name of the strategy and
        an order_id tag number separated by a hyphen '-'.

        Example: 'MyStrategy-001'.

        :param value: The value for the strategy_id.
        :return StrategyId.
        """
        return StrategyId.from_string(value)


cdef class AccountId(Identifier):
    """
    Represents a valid account identifier.
    """

    def __init__(self, str broker, str account_number):
        """
        Initializes a new instance of the AccountId class.

        :param broker: The broker for the account_id.
        :param account_number: The account number for the account_id.
        :raises ConditionFailed: If the broker is not a valid string.
        :raises ConditionFailed: If the account_number is not a valid string.
        """
        super().__init__(f'{broker}-{account_number}')

        self.broker = Brokerage(broker)
        self.number = AccountNumber(account_number)

    @staticmethod
    cdef AccountId from_string(str value):
        """
        Return an account_id from the given string value. Must be correctly
        formatted with two valid strings either side of a hyphen '-'.
        
        Example: 'FXCM-02851908'.

        :param value: The value for the account_id.
        :return AccountId.
        """
        cdef tuple partitioned = value.partition('-')
        return AccountId(broker=partitioned[0], account_number=partitioned[2])

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


cdef class OrderId(Identifier):
    """
    Represents a valid order identifier (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the OrderId class.

        :param value: The value of the order_id (should be unique).
        """
        super().__init__(value)


cdef class PositionId(Identifier):
    """
    Represents a valid position identifier (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the PositionId class.

        :param value: The value of the position_id (should be unique).
        """
        super().__init__(value)


cdef class ExecutionId(Identifier):
    """
    Represents a valid execution identifier (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the ExecutionId class.

        :param value: The value of the execution_id (should be unique).
        """
        super().__init__(value)


cdef class ExecutionTicket(Identifier):
    """
    Represents a valid execution ticket (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the ExecutionTicket class.

        :param value: The value of the execution ticket.
        """
        super().__init__(value)


cdef class InstrumentId(Identifier):
    """
    Represents a valid instrument identifier (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the InstrumentId class.

        :param value: The value of the instrument identifier.
        """
        super().__init__(value)
