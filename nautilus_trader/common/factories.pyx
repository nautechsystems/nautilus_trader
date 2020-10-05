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
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.generators cimport OrderIdGenerator
from nautilus_trader.common.uuid cimport LiveUUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.identifiers cimport IdTag
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport PassiveOrder
from nautilus_trader.model.order cimport StopMarketOrder


cdef class OrderFactory:
    """
    A factory class which provides different order types.
    """

    def __init__(
            self,
            StrategyId strategy_id not None,
            IdTag id_tag_trader not None,
            IdTag id_tag_strategy not None,
            Clock clock=None,
            UUIDFactory uuid_factory=None,
            int initial_count=0,
    ):
        """
        Initialize a new instance of the OrderFactory class.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy identifier (not sent to broker/exchange).
        id_tag_trader : IdTag
            The order identifier tag for the trader.
        id_tag_strategy : IdTag
            The order identifier tag for the strategy.
        clock : Clock
            The clock for the component.
        uuid_factory : UUIDFactory
            The UUID factory for the component.
        initial_count : int, optional
            The initial order count for the factory (default=0).

        Raises
        ------
        ValueError
            If initial_count is negative (< 0).

        """
        if clock is None:
            clock = LiveClock()
        if uuid_factory is None:
            uuid_factory = LiveUUIDFactory()
        Condition.not_negative_int(initial_count, "initial_count")

        self.strategy_id = strategy_id
        self._clock = clock
        self._uuid_factory = uuid_factory
        self._id_generator = OrderIdGenerator(
            id_tag_trader=id_tag_trader,
            id_tag_strategy=id_tag_strategy,
            clock=clock,
            initial_count=initial_count,
        )

    cpdef int count(self):
        """
        Return the internal order identifier generator count.

        Returns
        -------
        int

        """
        return self._id_generator.count

    cpdef void set_count(self, int count) except *:
        """
        System Method: Set the internal order identifier generator count to the
        given count.

        Parameters
        ----------
        count : int
            The count to set.

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
            TimeInForce time_in_force=TimeInForce.DAY,
    ):
        """
        Create a new market order.

        Parameters
        ----------
        symbol : Symbol
            The orders symbol.
        order_side : OrderSide
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        time_in_force : TimeInForce, optional
            The orders time in force (default=DAY).

        Returns
        -------
        MarketOrder

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is UNDEFINED.
        ValueError
            If time_in_force is other than DAY, IOC or FOC.

        """
        return MarketOrder(
            self._id_generator.generate(),
            self.strategy_id,
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
            bint post_only=True,
            bint hidden=False,
    ):
        """
        Create a new limit order.

        If the time in force is GTD then a valid expire time must be given.

        Parameters
        ----------
        symbol : Symbol
            The orders symbol.
        order_side : OrderSide
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        price : Price
            The orders price.
        time_in_force : TimeInForce, optional
            The orders time in force (default=DAY).
        expire_time : datetime, optional
            The order expire time (for GTD orders, default=None).
        post_only : bool, optional
            If the order will only make a market (default=True).
        hidden : bool, optional
            If the order should be hidden from the public book (default=False).

        Returns
        -------
        MarketOrder

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is UNDEFINED.
        ValueError
            If time_in_force is GTD and expire_time is None.

        """
        return LimitOrder(
            self._id_generator.generate(),
            self.strategy_id,
            symbol,
            order_side,
            quantity,
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.utc_now(),
            post_only=post_only,
            hidden=hidden)

    cpdef StopMarketOrder stop(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None,
    ):
        """
        Create a new stop-market order.

        If the time in force is GTD then a valid expire time must be given.

        Parameters
        ----------
        symbol : Symbol
            The orders symbol.
        order_side : OrderSide
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        price : Price
            The orders price.
        time_in_force : TimeInForce, optional
            The orders time in force (default=DAY).
        expire_time : datetime, optional
            The order expire time (for GTD orders, default=None).

        Returns
        -------
        StopMarketOrder

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is UNDEFINED.
        ValueError
            If time_in_force is GTD and expire_time is None.

        """
        return StopMarketOrder(
            self._id_generator.generate(),
            self.strategy_id,
            symbol,
            order_side,
            quantity,
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp=self._clock.utc_now(),
        )

    cpdef BracketOrder bracket(
            self,
            Order entry_order,
            Price stop_loss,
            Price take_profit=None,
    ):
        """
        Create a bracket order from the given entry.

        Parameters
        ----------
        entry_order : Order
            The entry parent order for the bracket.
        stop_loss : Price
            The stop-loss child order price.
        take_profit : Price, optional
            The take-profit child order price (default=None).

        Returns
        -------
        BracketOrder

        Raises
        ------
        ValueError
            If entry_order.side is BUY and entry_order.price <= stop_loss.price.
        ValueError
            If entry_order.side is BUY and entry_order.price >= take_profit.price.
        ValueError
            If entry_order.side is SELL and entry_order.price >= stop_loss.price.
        ValueError
            If entry_order.side is SELL and entry_order.price <= take_profit.price.

        """
        # Validate prices
        if entry_order.side == OrderSide.BUY:
            Condition.true(take_profit is None or stop_loss < take_profit, "stop_loss < take_profit")
            if isinstance(entry_order, PassiveOrder):
                Condition.true(entry_order.price > stop_loss, "entry_order.price > stop_loss")
                Condition.true(take_profit is None or entry_order.price < take_profit, "entry_order.price < take_profit")
        else:  # entry_order.side == OrderSide.SELL
            Condition.true(take_profit is None or stop_loss > take_profit, "stop_loss > take_profit")
            if isinstance(entry_order, PassiveOrder):
                Condition.true(entry_order.price < stop_loss, "entry_order.price < stop_loss")
                Condition.true(take_profit is None or entry_order.price > take_profit, "entry_order.price > take_profit")

        cdef OrderSide child_order_side = OrderSide.BUY if entry_order.side == OrderSide.SELL else OrderSide.SELL

        cdef Order stop_loss_order = self.stop(
            entry_order.symbol,
            child_order_side,
            entry_order.quantity,
            stop_loss,
            TimeInForce.GTC,
            expire_time=None,
        )

        cdef Order take_profit_order = None
        if take_profit is not None:
            take_profit_order = self.limit(
                entry_order.symbol,
                child_order_side,
                entry_order.quantity,
                take_profit,
                TimeInForce.GTC,
                expire_time=None,
            )

        return BracketOrder(entry_order, stop_loss_order, take_profit_order)
