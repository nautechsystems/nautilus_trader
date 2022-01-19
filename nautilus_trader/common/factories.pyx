# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.common.generators cimport ClientOrderIdGenerator
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.contingency_type cimport ContingencyType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder


cdef class OrderFactory:
    """
    A factory class which provides different order types.

    The `TraderId` tag and `StrategyId` tag will be inserted into all
    IDs generated.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID (only numerical tag sent to venue).
    strategy_id : StrategyId
        The strategy ID (only numerical tag sent to venue).
    clock : Clock
        The clock for the factory.
    initial_count : int, optional
        The initial order count for the factory.

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

        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self.trader_id = trader_id
        self.strategy_id = strategy_id

        self._order_list_id = 1  # TODO(cs): Improve this
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
        bint reduce_only=False,
        str tags=None,
    ):
        """
        Create a new market order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        time_in_force : TimeInForce, optional
            The orders time-in-force. Often not applicable for market orders.
        reduce_only : bool, optional
            If the order carries the 'reduce-only' execution instruction.
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        MarketOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is other than ``GTC``, ``IOC``, ``FOK`` or ``OC``.

        """
        return MarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            time_in_force=time_in_force,
            reduce_only=reduce_only,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            order_list_id=None,
            parent_order_id=None,
            child_order_ids=None,
            contingency=ContingencyType.NONE,
            contingency_ids=None,
            tags=tags,
        )

    cpdef LimitOrder limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expiration=None,
        bint post_only=False,
        bint reduce_only=False,
        Quantity display_qty=None,
        str tags=None,
    ):
        """
        Create a new limit order.

        If the time-in-force is ``GTD`` then a valid expire time must be given.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        price : Price
            The orders price.
        time_in_force : TimeInForce, optional
            The orders time-in-force.
        expiration : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, optional
            If the order will only provide liquidity (make a market).
        reduce_only : bool, optional
            If the order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        LimitOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expiration` is ``None`` or <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

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
            expiration=expiration,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            order_list_id=None,
            parent_order_id=None,
            child_order_ids=None,
            contingency=ContingencyType.NONE,
            contingency_ids=None,
            tags=tags,
        )

    cpdef StopMarketOrder stop_market(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price trigger_price,
        TriggerMethod trigger=TriggerMethod.DEFAULT,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expiration=None,
        bint reduce_only=False,
        str tags=None,
    ):
        """
        Create a new stop-market order.

        If the time-in-force is ``GTD`` then a valid expire time must be given.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        trigger_price : Price
            The orders trigger price (STOP).
        trigger : TriggerMethod
            The order trigger method.
        time_in_force : TimeInForce, optional
            The orders time-in-force.
        expiration : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, optional
            If the order carries the 'reduce-only' execution instruction.
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        StopMarketOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expiration` is ``None`` or <= UNIX epoch.

        """
        return StopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            trigger_price=trigger_price,
            trigger=trigger,
            time_in_force=time_in_force,
            expiration=expiration,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            reduce_only=reduce_only,
            order_list_id=None,
            parent_order_id=None,
            child_order_ids=None,
            contingency=ContingencyType.NONE,
            contingency_ids=None,
            tags=tags,
        )

    cpdef StopLimitOrder stop_limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        Price trigger_price,
        TriggerMethod trigger=TriggerMethod.DEFAULT,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expiration=None,
        bint post_only=False,
        bint reduce_only=False,
        Quantity display_qty=None,
        str tags=None,
    ):
        """
        Create a new stop-limit order.

        If the time-in-force is ``GTD`` then a valid expire time must be given.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        price : Price
            The orders limit price.
        trigger_price : Price
            The orders trigger stop price.
        trigger : TriggerMethod
            The order trigger method.
        time_in_force : TimeInForce, optional
            The orders time-in-force.
        expiration : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, optional
            If the order will only provide liquidity (make a market).
        reduce_only : bool, optional
            If the order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        StopLimitOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expiration` is ``None`` or <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        return StopLimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            trigger=trigger,
            time_in_force=time_in_force,
            expiration=expiration,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            order_list_id=None,
            parent_order_id=None,
            child_order_ids=None,
            contingency=ContingencyType.NONE,
            contingency_ids=None,
            tags=tags,
        )

    cpdef OrderList bracket_market(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price stop_loss,
        Price take_profit,
        TimeInForce tif_bracket=TimeInForce.GTC,
    ):
        """
        Create a bracket order with a MARKET entry from the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The entry orders side.
        quantity : Quantity
            The entry orders quantity (> 0).
        stop_loss : Price
            The stop-loss child order stop price.
        take_profit : Price
            The take-profit child order limit price.
        tif_bracket : TimeInForce {``DAY``, ``GTC``}, optional
            The bracket orders time-in-force .

        Returns
        -------
        OrderList

        Raises
        ------
        ValueError
            If `tif_bracket` is not either ``DAY`` or ``GTC``.
        ValueError
            If `entry_order.side` is ``BUY`` and `entry_order.price` <= `stop_loss.price`.
        ValueError
            If `entry_order.side` is ``BUY`` and `entry_order.price` >= `take_profit.price`.
        ValueError
            If `entry_order.side` is ``SELL`` and `entry_order.price` >= `stop_loss.price`.
        ValueError
            If `entry_order.side` is ``SELL`` and `entry_order.price` <= `take_profit.price`.

        """
        Condition.true(tif_bracket == TimeInForce.DAY or tif_bracket == TimeInForce.GTC, "tif_bracket is unsupported")

        # Validate prices
        if order_side == OrderSide.BUY:
            Condition.true(stop_loss < take_profit, "stop_loss was >= take_profit")
        elif order_side == OrderSide.SELL:
            Condition.true(stop_loss > take_profit, "stop_loss was <= take_profit")

        cdef OrderListId order_list_id = OrderListId(str(self._order_list_id))
        self._order_list_id += 1
        cdef ClientOrderId entry_client_order_id = self._id_generator.generate()
        cdef ClientOrderId stop_loss_client_order_id = self._id_generator.generate()
        cdef ClientOrderId take_profit_client_order_id = self._id_generator.generate()

        cdef MarketOrder entry_order = MarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=entry_client_order_id,
            order_side=order_side,
            quantity=quantity,
            time_in_force=TimeInForce.GTC,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            order_list_id=order_list_id,
            parent_order_id=None,
            child_order_ids=[stop_loss_client_order_id, take_profit_client_order_id],
            contingency=ContingencyType.OTO,
            contingency_ids=[stop_loss_client_order_id, take_profit_client_order_id],
            tags="ENTRY",
        )

        cdef StopMarketOrder stop_loss_order = StopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=entry_order.instrument_id,
            client_order_id=stop_loss_client_order_id,
            order_side=Order.opposite_side_c(entry_order.side),
            quantity=quantity,
            trigger_price=stop_loss,
            trigger=TriggerMethod.DEFAULT,
            time_in_force=tif_bracket,
            expiration=None,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            reduce_only=True,
            order_list_id=order_list_id,
            parent_order_id=entry_client_order_id,
            child_order_ids=None,
            contingency=ContingencyType.OCO,
            contingency_ids=[take_profit_client_order_id],
            tags="STOP_LOSS",
        )

        cdef LimitOrder take_profit_order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=entry_order.instrument_id,
            client_order_id=take_profit_client_order_id,
            order_side=Order.opposite_side_c(entry_order.side),
            quantity=quantity,
            price=take_profit,
            time_in_force=tif_bracket,
            expiration=None,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=True,
            reduce_only=True,
            order_list_id=order_list_id,
            parent_order_id=entry_client_order_id,
            child_order_ids=None,
            contingency=ContingencyType.OCO,
            contingency_ids=[stop_loss_client_order_id],
            tags="TAKE_PROFIT",
        )

        return OrderList(
            list_id=order_list_id,
            orders=[entry_order, stop_loss_order, take_profit_order],
        )

    cpdef OrderList bracket_limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price entry,
        Price stop_loss,
        Price take_profit,
        TimeInForce tif=TimeInForce.GTC,
        datetime expiration=None,
        TimeInForce tif_bracket=TimeInForce.GTC,
        bint post_only=False,
    ):
        """
        Create a bracket order with a LIMIT entry from the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The entry orders side.
        quantity : Quantity
            The entry orders quantity (> 0).
        entry : Price
            The entry LIMIT order price.
        stop_loss : Price
            The stop-loss child order stop price.
        take_profit : Price
            The take-profit child order limit price.
        tif : TimeInForce {``DAY``, ``GTC``}, optional
            The entry orders time-in-force .
        expiration : datetime, optional
            The order expiration (for ``GTD`` orders).
        tif_bracket : TimeInForce {``DAY``, ``GTC``}, optional
            The bracket orders time-in-force.
        post_only : bool, optional
            If the entry order will only provide liquidity (make a market).

        Returns
        -------
        OrderList

        Raises
        ------
        ValueError
            If `tif` is ``GTD`` and `expiration` is ``None``.
        ValueError
            If `tif_bracket` is not either ``DAY`` or ``GTC``.
        ValueError
            If `entry_order.side` is ``BUY`` and `entry_order.price` <= `stop_loss.price`.
        ValueError
            If `entry_order.side` is ``BUY`` and `entry_order.price` >= `take_profit.price`.
        ValueError
            If `entry_order.side` is ``SELL`` and `entry_order.price` >= `stop_loss.price`.
        ValueError
            If `entry_order.side` is ``SELL`` and `entry_order.price` <= `take_profit.price`.

        """
        Condition.true(tif_bracket == TimeInForce.DAY or tif_bracket == TimeInForce.GTC, "tif_bracket is unsupported")

        # Validate prices
        if order_side == OrderSide.BUY:
            Condition.true(stop_loss < take_profit, "stop_loss was >= take_profit")
            Condition.true(entry > stop_loss, "BUY entry was <= stop_loss")
            Condition.true(entry < take_profit, "BUY entry was >= take_profit")
        elif order_side == OrderSide.SELL:
            Condition.true(stop_loss > take_profit, "stop_loss was <= take_profit")
            Condition.true(entry < stop_loss, "SELL entry was >= stop_loss")
            Condition.true(entry > take_profit, "SELL entry was <= take_profit")

        cdef OrderListId order_list_id = OrderListId(str(self._order_list_id))
        self._order_list_id += 1
        cdef ClientOrderId entry_client_order_id = self._id_generator.generate()
        cdef ClientOrderId stop_loss_client_order_id = self._id_generator.generate()
        cdef ClientOrderId take_profit_client_order_id = self._id_generator.generate()

        cdef LimitOrder entry_order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=entry_client_order_id,
            order_side=order_side,
            quantity=quantity,
            price=entry,
            time_in_force=tif,
            expiration=expiration,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=post_only,
            order_list_id=order_list_id,
            parent_order_id=None,
            child_order_ids=[stop_loss_client_order_id, take_profit_client_order_id],
            contingency=ContingencyType.OTO,
            contingency_ids=[stop_loss_client_order_id, take_profit_client_order_id],
            tags="ENTRY",
        )

        cdef StopMarketOrder stop_loss_order = StopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=entry_order.instrument_id,
            client_order_id=stop_loss_client_order_id,
            order_side=Order.opposite_side_c(entry_order.side),
            quantity=quantity,
            trigger_price=stop_loss,
            trigger=TriggerMethod.DEFAULT,
            time_in_force=tif_bracket,
            expiration=None,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            reduce_only=True,
            order_list_id=order_list_id,
            parent_order_id=entry_client_order_id,
            child_order_ids=None,
            contingency=ContingencyType.OCO,
            contingency_ids=[take_profit_client_order_id],
            tags="STOP_LOSS",
        )

        cdef LimitOrder take_profit_order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=entry_order.instrument_id,
            client_order_id=take_profit_client_order_id,
            order_side=Order.opposite_side_c(entry_order.side),
            quantity=quantity,
            price=take_profit,
            time_in_force=tif_bracket,
            expiration=None,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=True,
            reduce_only=True,
            display_qty=None,
            order_list_id=order_list_id,
            parent_order_id=entry_client_order_id,
            contingency=ContingencyType.OCO,
            contingency_ids=[stop_loss_client_order_id],
            tags="TAKE_PROFIT",
        )

        return OrderList(
            list_id=order_list_id,
            orders=[entry_order, stop_loss_order, take_profit_order],
        )
