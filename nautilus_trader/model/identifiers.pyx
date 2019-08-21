# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.common.clock cimport Clock, LiveClock


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
    should be unique at the fund level.
    """

    def __init__(self, str name, str order_id_tag):
        """
        Initializes a new instance of the TraderId class.

        :param name: The name of the trader.
        :param name: The order identifier tag for the trader.
        :raises ConditionFailed: If the name is not a valid string.
        :raises ConditionFailed: If the order_id_tag is not a valid string.
        """
        Condition.valid_string(name, 'name')

        super().__init__(f'{name}-{order_id_tag}')
        self.order_id_tag = IdTag(order_id_tag)

    @staticmethod
    cdef from_string(str value):
        """
        Return a trader identifier from the given string value. Must be correctly
        formatted with two valid strings either side of a hyphen '-'.
        
        Example: 'Trader1-001'.

        :param value: The value for the strategy identifier.
        :return: StrategyId.
        """
        cdef tuple partitioned = value.partition('-')

        return TraderId(name=partitioned[0], order_id_tag=partitioned[2])


cdef class StrategyId(Identifier):
    """
    Represents a valid strategy identifier, the name and order_id_tag combination
    should be unique at the trader level.
    """

    def __init__(self, str name, str order_id_tag):
        """
        Initializes a new instance of the StrategyId class.

        :param name: The name of the strategy.
        :param order_id_tag: The order identifier tag for the strategy.
        :raises ConditionFailed: If the name is not a valid string.
        :raises ConditionFailed: If the order_id_tag is not a valid string.
        """
        Condition.valid_string(name, 'name')

        super().__init__(f'{name}-{order_id_tag}')
        self.order_id_tag = IdTag(order_id_tag)

    @staticmethod
    cdef from_string(str value):
        """
        Return a strategy identifier from the given string value. Must be correctly
        formatted with two valid strings either side of a hyphen '-'.
        
        Example: 'Strategy1-001'.

        :param value: The value for the strategy identifier.
        :return: StrategyId.
        """
        cdef tuple partitioned = value.partition('-')

        return StrategyId(name=partitioned[0], order_id_tag=partitioned[2])


cdef class AccountId(Identifier):
    """
    Represents a valid account identifier.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the AccountId class.

        :param value: The value of the account identifier.
        """
        super().__init__(value)


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

        :param value: The value of the order identifier (should be unique).
        """
        super().__init__(value)


cdef class PositionId(Identifier):
    """
    Represents a valid position identifier (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the PositionId class.

        :param value: The value of the position identifier (should be unique).
        """
        super().__init__(value)


cdef class ExecutionId(Identifier):
    """
    Represents a valid execution identifier (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the ExecutionId class.

        :param value: The value of the execution identifier (should be unique).
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


cdef class IdentifierGenerator:
    """
    Provides a generator for unique identifier strings.
    """

    def __init__(self,
                 str prefix,
                 IdTag id_tag_trader,
                 IdTag id_tag_strategy,
                 Clock clock):
        """
        Initializes a new instance of the IdentifierGenerator class.

        :param prefix: The prefix for each generated identifier.
        :param id_tag_trader: The identifier tag for the trader.
        :param id_tag_strategy: The identifier tag for the strategy.
        :param clock: The internal clock.
        :raises ConditionFailed: If the prefix is not a valid string.
        :raises ConditionFailed: If the id_tag_trader is not a valid string.
        :raises ConditionFailed: If the id_tag_strategy is not a valid string.
        """
        Condition.valid_string(prefix, 'prefix')

        self._clock = clock
        self.prefix = prefix
        self.id_tag_trader = id_tag_trader
        self.id_tag_strategy = id_tag_strategy
        self.counter = 0

    cpdef void reset(self):
        """
        Reset the identifier generator by returning all stateful internal values
        to their initial value.
        """
        self.counter = 0

    cdef str _generate(self):
        """
        Return a unique identifier string.

        :return: str.
        """
        self.counter += 1

        return (f'{self.prefix}-'
                f'{self._get_datetime_tag()}-'
                f'{self.id_tag_trader.value}-'
                f'{self.id_tag_strategy.value}-'
                f'{self.counter}')

    cdef str _get_datetime_tag(self):
        """
        Return the datetime tag string for the current time.

        :return: str.
        """
        cdef datetime time_now = self._clock.time_now()
        return (f'{time_now.year}'
                f'{time_now.month:02d}'
                f'{time_now.day:02d}'
                f'-'
                f'{time_now.hour:02d}'
                f'{time_now.minute:02d}'
                f'{time_now.second:02d}')


cdef class OrderIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique OrderId(s).
    """

    def __init__(self,
                 IdTag id_tag_trader,
                 IdTag id_tag_strategy,
                 Clock clock=LiveClock()):
        """
        Initializes a new instance of the OrderIdGenerator class.

        :param id_tag_trader: The order identifier tag for the trader.
        :param id_tag_strategy: The order identifier tag for the strategy.
        :param clock: The clock for the component.
        """
        super().__init__('O',
                         id_tag_trader,
                         id_tag_strategy,
                         clock)

    cpdef OrderId generate(self):
        """
        Return a unique order identifier.

        :return: OrderId.
        """
        return OrderId(self._generate())


cdef class PositionIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique PositionId(s).
    """

    def __init__(self,
                 IdTag id_tag_trader,
                 IdTag id_tag_strategy,
                 Clock clock=LiveClock()):
        """
        Initializes a new instance of the PositionIdGenerator class.

        :param id_tag_trader: The position identifier tag for the trader.
        :param id_tag_strategy: The position identifier tag for the strategy.
        :param clock: The clock for the component.
        """
        super().__init__('P',
                         id_tag_trader,
                         id_tag_strategy,
                         clock)

    cpdef PositionId generate(self):
        """
        Return a unique position identifier.

        :return: PositionId.
        """
        return PositionId(self._generate())
