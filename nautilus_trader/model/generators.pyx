# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.live.clock cimport LiveClock


cdef class IdentifierGenerator:
    """
    Provides a generator for unique identifier strings.
    """

    def __init__(self,
                 str prefix not None,
                 IdTag id_tag_trader not None,
                 IdTag id_tag_strategy not None,
                 Clock clock not None,
                 int initial_count=0):
        """
        Initializes a new instance of the IdentifierGenerator class.

        :param prefix: The prefix for each generated identifier.
        :param id_tag_trader: The identifier tag for the trader.
        :param id_tag_strategy: The identifier tag for the strategy.
        :param clock: The internal clock.
        :param initial_count: The initial count for the generator.
        :raises ValueError: If the prefix is not a valid string.
        :raises ValueError: If the initial count is negative (< 0).
        """
        Condition.valid_string(prefix, 'prefix')
        Condition.not_negative_int(initial_count, 'initial_count')

        self._clock = clock
        self.prefix = prefix
        self.id_tag_trader = id_tag_trader
        self.id_tag_strategy = id_tag_strategy
        self.count = initial_count

    cpdef void set_count(self, int count) except *:
        """
        Set the internal counter to the given count.
        
        :param count: The count to set.
        """
        self.count = count

    cpdef void reset(self) except *:
        """
        Reset the identifier generator by setting all stateful values to their 
        default value.
        """
        self.count = 0

    cdef str _generate(self):
        """
        Return a unique identifier string.

        :return str.
        """
        self.count += 1

        return (f'{self.prefix}-'
                f'{self._get_datetime_tag()}-'
                f'{self.id_tag_trader.value}-'
                f'{self.id_tag_strategy.value}-'
                f'{self.count}')

    cdef str _get_datetime_tag(self):
        """
        Return the datetime tag string for the current time.

        :return str.
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
                 IdTag id_tag_trader not None,
                 IdTag id_tag_strategy not None,
                 Clock clock not None=LiveClock(),
                 int initial_count=0):
        """
        Initializes a new instance of the OrderIdGenerator class.

        :param id_tag_trader: The order_id tag for the trader.
        :param id_tag_strategy: The order_id tag for the strategy.
        :param clock: The clock for the component.
        :param initial_count: The initial count for the generator.
        :raises ValueError: If the initial count is negative (< 0).
        """
        super().__init__('O',
                         id_tag_trader,
                         id_tag_strategy,
                         clock,
                         initial_count)

    cpdef OrderId generate(self):
        """
        Return a unique order_id.

        :return OrderId.
        """
        return OrderId(self._generate())


cdef class PositionIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique PositionId(s).
    """

    def __init__(self,
                 IdTag id_tag_trader not None,
                 IdTag id_tag_strategy not None,
                 Clock clock not None=LiveClock(),
                 int initial_count=0):
        """
        Initializes a new instance of the PositionIdGenerator class.

        :param id_tag_trader: The position_id tag for the trader.
        :param id_tag_strategy: The position_id tag for the strategy.
        :param clock: The clock for the component.
        :param initial_count: The initial count for the generator.
        :raises ValueError: If the initial count is negative (< 0).
        """
        super().__init__('P',
                         id_tag_trader,
                         id_tag_strategy,
                         clock,
                         initial_count)

    cpdef PositionId generate(self):
        """
        Return a unique position_id.

        :return PositionId.
        """
        return PositionId(self._generate())
