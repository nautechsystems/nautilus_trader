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

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.generators cimport ClientOrderIdGenerator
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.base cimport PassiveOrder
from nautilus_trader.model.orders.bracket cimport BracketOrder
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder


cdef class OrderFactory:
    """
    A factory class which provides different order types.

    The `TraderId` tag and `StrategyId` tag will be inserted into all
    IDs generated.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        Clock clock=None,
        int initial_count=0,
    ):
        """
        Initialize a new instance of the ``OrderFactory`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID (only numerical tag sent to venue).
        strategy_id : StrategyId
            The strategy ID (only numerical tag sent to venue).
        clock : Clock
            The clock for the component.
        initial_count : int, optional
            The initial order count for the factory.

        Raises
        ------
        ValueError
            If initial_count is negative (< 0).

        """
        if clock is None:
            clock = LiveClock()
        Condition.not_negative_int(initial_count, "initial_count")

        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self.trader_id = trader_id
        self.strategy_id = strategy_id

        self._id_generator = ClientOrderIdGenerator(
            trader_id=trader_id,
            strategy_id=strategy_id,
            clock=clock,
            initial_count=initial_count,
        )

    cdef int count_c(self):
        return self._id_generator.count

    @property
    def count(self):
        """
        The count of IDs generated.

        Returns
        -------
        int

        """
        return self.count_c()

    cpdef void set_count(self, int count) except *:
        """
        System Method: Set the internal order ID generator count to the
        given count.

        Parameters
        ----------
        count : int
            The count to set.

        """
        self._id_generator.set_count(count)

    cpdef void reset(self) except *:
        """
        Reset the order factory.

        All stateful fields are reset to their initial value.
        """
        self._id_generator.reset()

    cpdef MarketOrder market(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        TimeInForce time_in_force=TimeInForce.GTC,
    ):
        """
        Create a new market order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        time_in_force : TimeInForce, optional
            The orders time-in-force. Often not applicable for market orders.

        Returns
        -------
        MarketOrder

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is other than GTC, IOC or FOK.

        """
        return MarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            time_in_force=time_in_force,
            init_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cpdef LimitOrder limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expire_time=None,
        bint post_only=False,
        bint reduce_only=False,
        bint hidden=False,
    ):
        """
        Create a new limit order.

        If the time-in-force is GTD then a valid expire time must be given.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        price : Price
            The orders price.
        time_in_force : TimeInForce, optional
            The orders time-in-force.
        expire_time : datetime, optional
            The order expire time (for GTD orders).
        post_only : bool, optional
            If the order will only make a market.
        reduce_only : bool, optional
            If the order will only reduce an open position.
        hidden : bool, optional
            If the order should be hidden from the public book.

        Returns
        -------
        LimitOrder

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is GTD expire_time is None.
        ValueError
            If post_only and hidden.
        ValueError
            If hidden and post_only.

        """
        return LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
            post_only=post_only,
            reduce_only=reduce_only,
            hidden=hidden,
        )

    cpdef StopMarketOrder stop_market(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expire_time=None,
        bint reduce_only=False,
    ):
        """
        Create a new stop-market order.

        If the time-in-force is GTD then a valid expire time must be given.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        price : Price
            The orders price.
        time_in_force : TimeInForce, optional
            The orders time-in-force.
        expire_time : datetime, optional
            The order expire time (for GTD orders).
        reduce_only : bool,
            If the order will only reduce an open position.

        Returns
        -------
        StopMarketOrder

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is GTD expire_time is None.

        """
        return StopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
            reduce_only=reduce_only,
        )

    cpdef StopLimitOrder stop_limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        Price trigger,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expire_time=None,
        bint post_only=False,
        bint reduce_only=False,
        bint hidden=False,
    ):
        """
        Create a new stop-limit order.

        If the time-in-force is GTD then a valid expire time must be given.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        price : Price
            The orders limit price.
        trigger : Price
            The orders stop trigger price.
        time_in_force : TimeInForce, optional
            The orders time-in-force.
        expire_time : datetime, optional
            The order expire time (for GTD orders).
        post_only : bool, optional
            If the order will only make a market.
        reduce_only : bool, optional
            If the order will only reduce an open position.
        hidden : bool, optional
            If the order should be hidden from the public book.

        Returns
        -------
        StopLimitOrder

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is GTD and expire_time is None.
        ValueError
            If post_only and hidden.
        ValueError
            If hidden and post_only.

        """
        return StopLimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            price=price,
            trigger=trigger,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
            post_only=post_only,
            reduce_only=reduce_only,
            hidden=hidden,
        )

    cpdef BracketOrder bracket(
        self,
        Order entry_order,
        Price stop_loss,
        Price take_profit,
        TimeInForce sl_tif=TimeInForce.GTC,
        TimeInForce tp_tif=TimeInForce.GTC,
    ):
        """
        Create a bracket order from the given entry order, stop-loss price and
        take-profit price.

        Parameters
        ----------
        entry_order : Order
            The entry parent order for the bracket.
        stop_loss : Price
            The stop-loss child order stop price.
        take_profit : Price
            The take-profit child order limit price.
        sl_tif : TimeInForce, optional
            The stop-loss orders time-in-force (DAY or GTC).
        tp_tif : TimeInForce, optional
            The take-profit orders time-in-force (DAY or GTC).

        Returns
        -------
        BracketOrder

        Raises
        ------
        ValueError
            If sl_tif is not either DAY or GTC.
        ValueError
            If tp_tif is not either DAY or GTC.
        ValueError
            If entry_order.side is BUY and entry_order.price <= stop_loss.price.
        ValueError
            If entry_order.side is BUY and entry_order.price >= take_profit.price.
        ValueError
            If entry_order.side is SELL and entry_order.price >= stop_loss.price.
        ValueError
            If entry_order.side is SELL and entry_order.price <= take_profit.price.

        """
        Condition.true(sl_tif == TimeInForce.DAY or sl_tif == TimeInForce.GTC, "sl_tif is unsupported")
        Condition.true(tp_tif == TimeInForce.DAY or sl_tif == TimeInForce.GTC, "tp_tif is unsupported")

        # Validate prices
        if entry_order.side == OrderSide.BUY:
            Condition.true(stop_loss < take_profit, "stop_loss was >= take_profit")
            if isinstance(entry_order, PassiveOrder):
                Condition.true(entry_order.price > stop_loss, "entry_order.price was <= stop_loss")
                Condition.true(entry_order.price < take_profit, "entry_order.price was > take_profit")
        else:  # entry_order.side == OrderSide.SELL
            Condition.true(stop_loss > take_profit, "stop_loss was <= take_profit")
            if isinstance(entry_order, PassiveOrder):
                Condition.true(entry_order.price < stop_loss, "entry_order.price < stop_loss")
                Condition.true(entry_order.price > take_profit, "entry_order.price > take_profit")

        cdef StopMarketOrder stop_loss_order = self.stop_market(
            instrument_id=entry_order.instrument_id,
            order_side=Order.opposite_side_c(entry_order.side),
            quantity=entry_order.quantity,
            price=stop_loss,
            time_in_force=sl_tif,
            expire_time=None,
            reduce_only=True,
        )

        cdef LimitOrder take_profit_order = self.limit(
            instrument_id=entry_order.instrument_id,
            order_side=Order.opposite_side_c(entry_order.side),
            quantity=entry_order.quantity,
            price=take_profit,
            time_in_force=tp_tif,
            expire_time=None,
            reduce_only=True,
        )

        return BracketOrder(entry_order, stop_loss_order, take_profit_order)
