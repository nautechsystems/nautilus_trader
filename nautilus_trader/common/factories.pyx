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

from nautilus_trader.core.types cimport Label
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_purpose cimport OrderPurpose
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.objects cimport Quantity, Price
from nautilus_trader.model.identifiers cimport Symbol, IdTag
from nautilus_trader.model.generators cimport OrderIdGenerator
from nautilus_trader.model.order cimport Order, BracketOrder
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.live.clock cimport LiveClock
from nautilus_trader.live.factories cimport LiveUUIDFactory


cdef class OrderFactory:
    """
    A factory class which provides different order types.
    """

    def __init__(self,
                 IdTag id_tag_trader not None,
                 IdTag id_tag_strategy not None,
                 Clock clock not None=LiveClock(),
                 UUIDFactory uuid_factory not None=LiveUUIDFactory(),
                 int initial_count=0):
        """
        Initialize a new instance of the OrderFactory class.

        :param id_tag_trader: The identifier tag for the trader.
        :param id_tag_strategy: The identifier tag for the strategy.
        :param clock: The clock for the component.
        :raises ValueError: If the initial count is negative (< 0).
        """
        Condition.not_negative_int(initial_count, 'initial_count')

        self._clock = clock
        self._uuid_factory = uuid_factory
        self._id_generator = OrderIdGenerator(
            id_tag_trader=id_tag_trader,
            id_tag_strategy=id_tag_strategy,
            clock=clock,
            initial_count=initial_count)

    cpdef int count(self):
        """
        System Method: Return the internal order_id generator count.

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

    cpdef Order market(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Label label=None,
            OrderPurpose order_purpose=OrderPurpose.NONE):
        """
        Return a market order.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param label: The optional order label / secondary identifier.
        :param order_purpose: The orders specified purpose (default=None).
        :raises ValueError: If the quantity is not positive (> 0).
        :return Order.
        """
        return Order(
            self._id_generator.generate(),
            symbol,
            order_side,
            OrderType.MARKET,
            quantity,
            price=None,
            label=label,
            order_purpose=order_purpose,
            time_in_force=TimeInForce.DAY,
            expire_time=None,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.time_now())

    cpdef Order limit(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=None,
            OrderPurpose order_purpose=OrderPurpose.NONE,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Returns a limit order.
        Note: If the time in force is GTD then a valid expire time must be given.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price.
        :param label: The optional order label / secondary identifier.
        :param order_purpose: The orders specified purpose (default=NONE).
        :param time_in_force: The orders time in force (default=DAY).
        :param expire_time: The optional order expire time (for GTD orders).
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        :return Order.
        """
        return Order(
            self._id_generator.generate(),
            symbol,
            order_side,
            OrderType.LIMIT,
            quantity,
            price=price,
            label=label,
            order_purpose=order_purpose,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.time_now())

    cpdef Order stop(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=None,
            OrderPurpose order_purpose=OrderPurpose.NONE,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Returns a stop-market order.
        Note: If the time in force is GTD then a valid expire time must be given.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price.
        :param label: The optional order label / secondary identifier.
        :param order_purpose: The orders specified purpose (default=NONE).
        :param time_in_force: The orders time in force (default=DAY).
        :param expire_time: The optional order expire time (for GTD orders).
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        :return Order.
        """
        return Order(
            self._id_generator.generate(),
            symbol,
            order_side,
            OrderType.STOP,
            quantity,
            price=price,
            label=label,
            order_purpose=order_purpose,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.time_now())

    cpdef Order stop_limit(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=None,
            OrderPurpose order_purpose=OrderPurpose.NONE,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Return a stop-limit order.
        Note: If the time in force is GTD then a valid expire time must be given.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price.
        :param label: The optional order label / secondary identifier.
        :param order_purpose: The orders specified purpose (default=NONE).
        :param time_in_force: The orders time in force (default=DAY).
        :param expire_time: The optional order expire time (for GTD orders).
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        :return Order.
        """
        return Order(
            self._id_generator.generate(),
            symbol,
            order_side,
            OrderType.STOP_LIMIT,
            quantity,
            price=price,
            label=label,
            order_purpose=order_purpose,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.time_now())

    cpdef Order market_if_touched(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=None,
            OrderPurpose order_purpose=OrderPurpose.NONE,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Return a market-if-touched order.
        Note: If the time in force is GTD then a valid expire time must be given.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price.
        :param label: The optional order label / secondary identifier.
        :param order_purpose: The orders specified purpose (default=NONE).
        :param time_in_force: The orders time in force (default=DAY).
        :param expire_time: The optional order expire time (for GTD orders).
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        :return Order.
        """
        return Order(
            self._id_generator.generate(),
            symbol,
            order_side,
            OrderType.MIT,
            quantity,
            price=price,
            label=label,
            order_purpose=order_purpose,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.time_now())

    cpdef Order fill_or_kill(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Label label=None,
            OrderPurpose order_purpose=OrderPurpose.NONE):
        """
        Return a fill-or-kill order.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param label: The optional order label / secondary identifier.
        :param order_purpose: The orders specified purpose (default=NONE).
        :raises ValueError: If the quantity is not positive (> 0).
        :return Order.
        """
        return Order(
            self._id_generator.generate(),
            symbol,
            order_side,
            OrderType.MARKET,
            quantity,
            price=None,
            label=label,
            order_purpose=order_purpose,
            time_in_force=TimeInForce.FOC,
            expire_time=None,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.time_now())

    cpdef Order immediate_or_cancel(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Label label=None,
            OrderPurpose order_purpose=OrderPurpose.NONE):
        """
        Return an immediate-or-cancel order.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param label: The optional order label / secondary identifier.
        :param order_purpose: The orders specified purpose (default=NONE).
        :raises ValueError: If the quantity is not positive (> 0).
        :return Order.
        """
        return Order(
            self._id_generator.generate(),
            symbol,
            order_side,
            OrderType.MARKET,
            quantity,
            price=None,
            label=label,
            order_purpose=order_purpose,
            time_in_force=TimeInForce.IOC,
            expire_time=None,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.time_now())

    cpdef BracketOrder bracket_market(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price stop_loss,
            Price take_profit=None,
            Label label=None):
        """
        Return a bracket order with a market entry.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param stop_loss: The stop-loss order price.
        :param take_profit: The optional take-profit order price.
        :param label: The optional order label / secondary identifier.
        :raises ValueError: If the quantity is not positive (> 0).
        :return BracketOrder.
        """
        cdef Label entry_label = None
        if label is not None:
            entry_label = Label(label.value + '_E')

        cdef Order entry_order = self.market(
            symbol,
            order_side,
            quantity,
            entry_label,
            OrderPurpose.ENTRY)

        return self._create_bracket_order(
            entry_order,
            stop_loss,
            take_profit,
            label)

    cpdef BracketOrder bracket_limit(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price entry,
            Price stop_loss,
            Price take_profit=None,
            Label label=None,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Return a bracket order with a limit entry.


        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param entry: The parent orders entry price.
        :param stop_loss: The stop-loss order price.
        :param take_profit: The optional take-profit order price.
        :param label: The optional order label / secondary identifier.
        :param time_in_force: The orders time in force (default=DAY).
        :param expire_time: The optional order expire time (for GTD orders).
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        :return BracketOrder.
        """
        cdef Label entry_label = None
        if label is not None:
            entry_label = Label(label.value + '_E')

        cdef Order entry_order = self.limit(
            symbol,
            order_side,
            quantity,
            entry,
            label,
            OrderPurpose.ENTRY,
            time_in_force,
            expire_time)

        return self._create_bracket_order(
            entry_order,
            stop_loss,
            take_profit,
            label)

    cpdef BracketOrder bracket_stop(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price entry,
            Price stop_loss,
            Price take_profit=None,
            Label label=None,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Return a bracket order with a stop entry.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param entry: The parent orders entry price.
        :param stop_loss: The stop-loss order price.
        :param take_profit: The optional take-profit order price.
        :param label: The orders The optional order label / secondary identifier.
        :param time_in_force: The orders time in force (default=DAY).
        :param expire_time: The optional order expire time (for GTD orders).
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        :return BracketOrder.
        """
        cdef Label entry_label = None
        if label is not None:
            entry_label = Label(label.value + '_E')

        cdef Order entry_order = self.stop(
            symbol,
            order_side,
            quantity,
            entry,
            label,
            OrderPurpose.ENTRY,
            time_in_force,
            expire_time)

        return self._create_bracket_order(
            entry_order,
            stop_loss,
            take_profit,
            label)

    cdef BracketOrder _create_bracket_order(
            self,
            Order entry_order,
            Price stop_loss,
            Price take_profit,
            Label original_label):
        cdef OrderSide child_order_side = OrderSide.BUY if entry_order.side == OrderSide.SELL else OrderSide.SELL

        cdef Label label_stop_loss = None
        cdef Label label_take_profit = None
        if original_label is not None:
            label_stop_loss = Label(original_label.value + "_SL")
            label_take_profit = Label(original_label.value + "_TP")

        cdef Order stop_loss_order = self.stop(
            entry_order.symbol,
            child_order_side,
            entry_order.quantity,
            stop_loss,
            label_stop_loss,
            OrderPurpose.STOP_LOSS,
            TimeInForce.GTC,
            expire_time=None)

        cdef Order take_profit_order = None
        if take_profit is not None:
            take_profit_order = self.limit(
                entry_order.symbol,
                child_order_side,
                entry_order.quantity,
                take_profit,
                label_take_profit,
                OrderPurpose.TAKE_PROFIT,
                TimeInForce.GTC,
                expire_time=None)

        return BracketOrder(entry_order, stop_loss_order, take_profit_order)
