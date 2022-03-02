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
from nautilus_trader.model.c_enums.trailing_offset_type cimport TrailingOffsetType
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.limit_if_touched cimport LimitIfTouchedOrder
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.orders.market_if_touched cimport MarketIfTouchedOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.model.orders.trailing_stop_limit cimport TrailingStopLimitOrder
from nautilus_trader.model.orders.trailing_stop_market cimport TrailingStopMarketOrder


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
        Set the internal order ID generator count to the given count.

        Parameters
        ----------
        count : int
            The count to set.

        Warnings
        --------
        System method (not intended to be called by user code).

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
        Create a new `market` order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``ON_OPEN``, ``ON_CLOSE``}, default ``GTC``
            The orders time-in-force. Often not applicable for market orders.
        reduce_only : bool, default False
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
            If `time_in_force` is other than ``GTC``, ``IOC``, ``FOK``, ``ON_OPEN`` or ``ON_CLOSE``.

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
            contingency_type=ContingencyType.NONE,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
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
        Quantity display_qty=None,
        str tags=None,
    ):
        """
        Create a new `limit` order.

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
        time_in_force : TimeInForce, default ``GTC``
            The orders time-in-force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
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
            If `time_in_force` is ``GTD`` and `expire_time` is ``None`` or <= UNIX epoch.
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
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            order_list_id=None,
            contingency_type=ContingencyType.NONE,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
        )

    cpdef StopMarketOrder stop_market(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price trigger_price,
        TriggerType trigger_type=TriggerType.DEFAULT,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expire_time=None,
        bint reduce_only=False,
        str tags=None,
    ):
        """
        Create a new `stop-market` conditional order.

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
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        time_in_force : TimeInForce, default ``GTC``
            The orders time-in-force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
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
            If `time_in_force` is ``GTD`` and `expire_time` is ``None`` or <= UNIX epoch.

        """
        return StopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            reduce_only=reduce_only,
            order_list_id=None,
            contingency_type=ContingencyType.NONE,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
        )

    cpdef StopLimitOrder stop_limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        Price trigger_price,
        TriggerType trigger_type=TriggerType.DEFAULT,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expire_time=None,
        bint post_only=False,
        bint reduce_only=False,
        Quantity display_qty=None,
        str tags=None,
    ):
        """
        Create a new `stop-limit` conditional order.

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
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        time_in_force : TimeInForce, default ``GTC``
            The orders time-in-force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
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
            If `time_in_force` is ``GTD`` and `expire_time` is ``None`` or <= UNIX epoch.
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
            trigger_type=trigger_type,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            order_list_id=None,
            contingency_type=ContingencyType.NONE,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
        )

    cpdef MarketToLimitOrder market_to_limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expire_time=None,
        bint reduce_only=False,
        Quantity display_qty=None,
        str tags=None,
    ):
        """
        Create a new `market` order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        time_in_force : TimeInForce {``GTC``, ``GTD``, ``IOC``, ``FOK``}, default ``GTC``
            The orders time-in-force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the limit order to display on the public book (iceberg).
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        MarketToLimitOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is other than ``GTC``, ``GTD``, ``IOC`` or ``FOK``.

        """
        return MarketToLimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            time_in_force=time_in_force,
            expire_time=expire_time,
            reduce_only=reduce_only,
            display_qty=display_qty,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            order_list_id=None,
            contingency_type=ContingencyType.NONE,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
        )

    cpdef MarketIfTouchedOrder market_if_touched(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price trigger_price,
        TriggerType trigger_type=TriggerType.DEFAULT,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expire_time=None,
        bint reduce_only=False,
        str tags=None,
    ):
        """
        Create a new `market-if-touched` (MIT) conditional order.

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
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        time_in_force : TimeInForce, default ``GTC``
            The orders time-in-force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        MarketIfTouchedOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` is ``None`` or <= UNIX epoch.

        """
        return MarketIfTouchedOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            reduce_only=reduce_only,
            order_list_id=None,
            contingency_type=ContingencyType.NONE,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
        )

    cpdef LimitIfTouchedOrder limit_if_touched(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price price,
        Price trigger_price,
        TriggerType trigger_type=TriggerType.DEFAULT,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expire_time=None,
        bint post_only=False,
        bint reduce_only=False,
        Quantity display_qty=None,
        str tags=None,
    ):
        """
        Create a new `limit-if-touched` (LIT) conditional order.

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
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        time_in_force : TimeInForce, default ``GTC``
            The orders time-in-force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        LimitIfTouchedOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` is ``None`` or <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        return LimitIfTouchedOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            order_list_id=None,
            contingency_type=ContingencyType.NONE,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
        )

    cpdef TrailingStopMarketOrder trailing_stop_market(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        trailing_offset: Decimal,
        Price trigger_price=None,
        TriggerType trigger_type=TriggerType.DEFAULT,
        TrailingOffsetType offset_type=TrailingOffsetType.PRICE,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expire_time=None,
        bint reduce_only=False,
        str tags=None,
    ):
        """
        Create a new `trailing-stop-market` conditional order.

        If the time-in-force is ``GTD`` then a valid expire time must be given.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        trailing_offset : Decimal
            The trailing offset for the trigger price (STOP).
        trigger_price : Price, optional
            The order trigger price (STOP). If ``None`` then will typically default
            to the delta of market price and `trailing_offset`.
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        offset_type : TrailingOffsetType, default ``PRICE``
            The order trailing offset type.
        time_in_force : TimeInForce, default ``GTC``
            The orders time-in-force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        TrailingStopMarketOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` is ``None`` or <= UNIX epoch.

        """
        return TrailingStopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            trailing_offset=trailing_offset,
            offset_type=offset_type,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            reduce_only=reduce_only,
            order_list_id=None,
            contingency_type=ContingencyType.NONE,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
        )

    cpdef TrailingStopLimitOrder trailing_stop_limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        limit_offset: Decimal,
        trailing_offset: Decimal,
        Price price=None,
        Price trigger_price=None,
        TriggerType trigger_type=TriggerType.DEFAULT,
        TrailingOffsetType offset_type=TrailingOffsetType.PRICE,
        TimeInForce time_in_force=TimeInForce.GTC,
        datetime expire_time=None,
        bint post_only=False,
        bint reduce_only=False,
        Quantity display_qty=None,
        str tags=None,
    ):
        """
        Create a new `trailing-stop-limit` conditional order.

        If the time-in-force is ``GTD`` then a valid expire time must be given.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        trailing_offset : Decimal
            The trailing offset for the trigger price (STOP).
        limit_offset : Decimal
            The trailing offset for the order price (LIMIT).
        price : Price, optional
            The order price (LIMIT). If ``None`` then will typically default to the
            delta of market price and `limit_offset`.
        trigger_price : Price, optional
            The order trigger price (STOP). If ``None`` then will typically default
            to the delta of market price and `trailing_offset`.
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        offset_type : TrailingOffsetType, default ``PRICE``
            The order trailing offset type.
        time_in_force : TimeInForce, default ``GTC``
            The orders time-in-force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        TrailingStopLimitOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` is ``None`` or <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        return TrailingStopLimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            limit_offset=limit_offset,
            trailing_offset=trailing_offset,
            offset_type=offset_type,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            order_list_id=None,
            contingency_type=ContingencyType.NONE,
            linked_order_ids=None,
            parent_order_id=None,
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
            The stop-loss child order trigger price (STOP).
        take_profit : Price
            The take-profit child order price (LIMIT).
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
            contingency_type=ContingencyType.OTO,
            linked_order_ids=[stop_loss_client_order_id, take_profit_client_order_id],
            parent_order_id=None,
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
            trigger_type=TriggerType.DEFAULT,
            time_in_force=tif_bracket,
            expire_time=None,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            reduce_only=True,
            order_list_id=order_list_id,
            contingency_type=ContingencyType.OCO,
            linked_order_ids=[take_profit_client_order_id],
            parent_order_id=entry_client_order_id,
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
            expire_time=None,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=True,
            reduce_only=True,
            order_list_id=order_list_id,
            contingency_type=ContingencyType.OCO,
            linked_order_ids=[stop_loss_client_order_id],
            parent_order_id=entry_client_order_id,
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
        datetime expire_time=None,
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
            The stop-loss child order trigger price (STOP).
        take_profit : Price
            The take-profit child order price (LIMIT).
        tif : TimeInForce {``DAY``, ``GTC``}, optional
            The entry orders time-in-force .
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        tif_bracket : TimeInForce {``DAY``, ``GTC``}, optional
            The bracket orders time-in-force.
        post_only : bool, default False
            If the entry order will only provide liquidity (make a market).

        Returns
        -------
        OrderList

        Raises
        ------
        ValueError
            If `tif` is ``GTD`` and `expire_time` is ``None``.
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
            expire_time=expire_time,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=post_only,
            order_list_id=order_list_id,
            contingency_type=ContingencyType.OTO,
            linked_order_ids=[stop_loss_client_order_id, take_profit_client_order_id],
            parent_order_id=None,
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
            trigger_type=TriggerType.DEFAULT,
            time_in_force=tif_bracket,
            expire_time=None,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            reduce_only=True,
            order_list_id=order_list_id,
            contingency_type=ContingencyType.OCO,
            linked_order_ids=[take_profit_client_order_id],
            parent_order_id=entry_client_order_id,
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
            expire_time=None,
            init_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
            post_only=True,
            reduce_only=True,
            display_qty=None,
            order_list_id=order_list_id,
            contingency_type=ContingencyType.OCO,
            linked_order_ids=[stop_loss_client_order_id],
            parent_order_id=entry_client_order_id,
            tags="TAKE_PROFIT",
        )

        return OrderList(
            list_id=order_list_id,
            orders=[entry_order, stop_loss_order, take_profit_order],
        )
