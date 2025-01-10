# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime

from nautilus_trader.common.component cimport Clock
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId


cdef class IdentifierGenerator:
    """
    Provides a generator for unique ID strings.

    Parameters
    ----------
    trader_id : TraderId
        The ID tag for the trader.
    clock : Clock
        The internal clock.
    """

    def __init__(self, TraderId trader_id not None, Clock clock not None):
        self._clock = clock
        self._id_tag_trader = trader_id.get_tag()

    cdef str _get_datetime_tag(self):
        """
        Return the tag string for the current timestamp (UTC).

        Returns
        -------
        str

        """
        cdef datetime now = self._clock.utc_now()
        return (
            f"{now.year}"
            f"{now.month:02d}"
            f"{now.day:02d}-"
            f"{now.hour:02d}"
            f"{now.minute:02d}"
            f"{now.second:02d}"
        )


cdef class ClientOrderIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique `ClientOrderId`(s).

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the generator.
    strategy_id : StrategyId
        The strategy ID for the generator.
    clock : Clock
        The clock for the generator.
    initial_count : int
        The initial count for the generator.

    Raises
    ------
    ValueError
        If `initial_count` is negative (< 0).
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        Clock clock not None,
        int initial_count=0,
    ):
        Condition.not_negative_int(initial_count, "initial_count")
        super().__init__(trader_id, clock)

        self._id_tag_strategy = strategy_id.get_tag()
        self.count = initial_count

    cpdef void set_count(self, int count):
        """
        Set the internal counter to the given count.

        Parameters
        ----------
        count : int
            The count to set.

        """
        self.count = count

    cpdef ClientOrderId generate(self):
        """
        Return a unique client order ID.

        Returns
        -------
        ClientOrderId

        """
        self.count += 1

        return ClientOrderId(
            f"O-"
            f"{self._get_datetime_tag()}-"
            f"{self._id_tag_trader}-"
            f"{self._id_tag_strategy}-"
            f"{self.count}",
        )

    cpdef void reset(self):
        """
        Reset the ID generator.

        All stateful fields are reset to their initial value.
        """
        self.count = 0


cdef class OrderListIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique `OrderListId`(s).

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the generator.
    strategy_id : StrategyId
        The strategy ID for the generator.
    clock : Clock
        The clock for the generator.
    initial_count : int
        The initial count for the generator.

    Raises
    ------
    ValueError
        If `initial_count` is negative (< 0).
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        Clock clock not None,
        int initial_count=0,
    ):
        Condition.not_negative_int(initial_count, "initial_count")
        super().__init__(trader_id, clock)

        self._id_tag_strategy = strategy_id.get_tag()
        self.count = initial_count

    cpdef void set_count(self, int count):
        """
        Set the internal counter to the given count.

        Parameters
        ----------
        count : int
            The count to set.

        """
        self.count = count

    cpdef OrderListId generate(self):
        """
        Return a unique order list ID.

        Returns
        -------
        OrderListId

        """
        self.count += 1

        return OrderListId(
            f"OL-"
            f"{self._get_datetime_tag()}-"
            f"{self._id_tag_trader}-"
            f"{self._id_tag_strategy}-"
            f"{self.count}",
        )

    cpdef void reset(self):
        """
        Reset the ID generator.

        All stateful fields are reset to their initial value.
        """
        self.count = 0


cdef class PositionIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique PositionId(s).

    Parameters
    ----------
    trader_id : TraderId
        The trader ID tag for the generator.
    """

    def __init__(self, TraderId trader_id not None, Clock clock not None):
        super().__init__(trader_id, clock)

        self._counts: dict[StrategyId, int] = {}

    cpdef void set_count(self, StrategyId strategy_id, int count):
        """
        Set the internal position count for the given strategy ID.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the count.
        count : int
            The count to set.

        Raises
        ------
        ValueError
            If `count` is negative (< 0).

        """
        Condition.not_none(strategy_id, "strategy_id")
        Condition.not_negative_int(count, "count")

        self._counts[strategy_id] = count

    cpdef int get_count(self, StrategyId strategy_id):
        """
        Return the internal position count for the given strategy ID.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the count.

        Returns
        -------
        int

        """
        Condition.not_none(strategy_id, "strategy_id")

        return self._counts.get(strategy_id, 0)

    cpdef PositionId generate(self, StrategyId strategy_id, bint flipped=False):
        """
        Return a unique position ID.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the position.
        flipped : bool
            If the position is being flipped. If True, then the generated id
            will be appended with 'F'.

        Returns
        -------
        PositionId

        """
        Condition.not_none(strategy_id, "strategy_id")

        cdef int count = self._counts.get(strategy_id, 0)
        count += 1
        self._counts[strategy_id] = count

        return PositionId(
            f"P-"
            f"{self._get_datetime_tag()}-"
            f"{self._id_tag_trader}-"
            f"{strategy_id.get_tag()}-"
            f"{count}{'F' if flipped else ''}",
        )

    cpdef void reset(self):
        """
        Reset the ID generator.

        All stateful fields are reset to their initial value.
        """
        self._counts.clear()
