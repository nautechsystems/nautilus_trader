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

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.live.clock cimport LiveClock
from nautilus_trader.live.factories cimport LiveUUIDFactory
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.generators cimport OrderIdGenerator
from nautilus_trader.model.identifiers cimport IdTag
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport PassiveOrder


cdef class OrderFactory:
    """
    A factory class which provides different order types.
    """

    def __init__(self,
                 IdTag id_tag_trader not None,
                 IdTag id_tag_strategy not None,
                 Clock clock=None,
                 UUIDFactory uuid_factory=None,
                 int initial_count=0):
        """
        Initialize a new instance of the OrderFactory class.

        :param id_tag_trader: The identifier tag for the trader.
        :param id_tag_strategy: The identifier tag for the strategy.
        :param clock: The clock for the component.
        :raises ValueError: If initial_count is negative (< 0).
        """
        if clock is None:
            clock = LiveClock()
        if uuid_factory is None:
            uuid_factory = LiveUUIDFactory()
        Condition.not_negative_int(initial_count, "initial_count")

        self._clock = clock
        self._uuid_factory = uuid_factory
        self._id_generator = OrderIdGenerator(
            id_tag_trader=id_tag_trader,
            id_tag_strategy=id_tag_strategy,
            clock=clock,
            initial_count=initial_count)

    cpdef int count(self):
        """
        Return the internal order_id generator count.

        :return: int.
        """
        return self._id_generator.count

    cpdef void set_count(self, int count) except *:
        """
        System Method: Set the internal order_id generator count to the given count.

        :param count: The count to set.
        """
        self._id_generator.set_count(count)

    cpdef void reset(self) except *:
        """
        Reset the order factory by clearing all stateful values.
        """
        self._id_generator.reset()

    cpdef MarketOrder market(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            TimeInForce time_in_force=TimeInForce.DAY):
        """
        Return a market order.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param time_in_force: The orders time in force (default=DAY).
        :raises ValueError: If quantity is not positive (> 0).
        :return Order.
        """
        return MarketOrder(
            self._id_generator.generate(),
            symbol,
            order_side,
            quantity,
            time_in_force,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.utc_now())

    cpdef LimitOrder limit(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None,
            bint is_post_only=False,
            bint is_hidden=False):
        """
        Returns a limit order.
        Note: If the time in force is GTD then a valid expire time must be given.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price.
        :param time_in_force: The orders time in force (default=DAY).
        :param expire_time: The optional order expire time (for GTD orders).
        :param is_post_only: If the order can only make liquidity.
        :param is_hidden: If the order is hidden from the public book.
        :raises ValueError: If quantity is not positive (> 0).
        :raises ValueError: If time_in_force is GTD and the expire_time is None.
        :return Order.
        """
        return LimitOrder(
            self._id_generator.generate(),
            symbol,
            order_side,
            quantity,
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.utc_now(),
            is_post_only=is_post_only,
            is_hidden=is_hidden)

    cpdef StopOrder stop(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Returns a stop-market order.
        Note: If the time in force is GTD then a valid expire time must be given.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price.
        :param time_in_force: The orders time in force (default=DAY).
        :param expire_time: The optional order expire time (for GTD orders).
        :raises ValueError: If quantity is not positive (> 0).
        :raises ValueError: If time_in_force is GTD and the expire_time is None.
        :return Order.
        """
        return StopOrder(
            self._id_generator.generate(),
            symbol,
            order_side,
            quantity,
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.utc_now())

    cpdef BracketOrder bracket(
            self,
            Order entry_order,
            Price stop_loss,
            Price take_profit=None):
        """
        Return a bracket order from the given entry.

        :param entry_order: The entry order for the bracket.
        :param stop_loss: The stop-loss order price.
        :param take_profit: The optional take-profit order price.
        :return BracketOrder.
        """
        # Validate prices
        if entry_order.side == OrderSide.BUY:
            Condition.true(take_profit is None or stop_loss.lt(take_profit), "stop_loss < take_profit")
            if isinstance(entry_order, PassiveOrder):
                Condition.true(entry_order.price.gt(stop_loss), "entry_order.price > stop_loss")
                Condition.true(take_profit is None or entry_order.price.lt(take_profit), "entry_order.price < take_profit")
        else:  # entry_order.side == OrderSide.SELL
            Condition.true(take_profit is None or stop_loss.gt(take_profit), "stop_loss > take_profit")
            if isinstance(entry_order, PassiveOrder):
                Condition.true(entry_order.price.lt(stop_loss), "entry_order.price < stop_loss")
                Condition.true(take_profit is None or entry_order.price.gt(take_profit), "entry_order.price > take_profit")

        cdef OrderSide child_order_side = OrderSide.BUY if entry_order.side == OrderSide.SELL else OrderSide.SELL

        cdef Order stop_loss_order = self.stop(
            entry_order.symbol,
            child_order_side,
            entry_order.quantity,
            stop_loss,
            TimeInForce.GTC,
            expire_time=None)

        cdef Order take_profit_order = None
        if take_profit is not None:
            take_profit_order = self.limit(
                entry_order.symbol,
                child_order_side,
                entry_order.quantity,
                take_profit,
                TimeInForce.GTC,
                expire_time=None)

        return BracketOrder(entry_order, stop_loss_order, take_profit_order)
