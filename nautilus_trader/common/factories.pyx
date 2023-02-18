# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.common.generators cimport OrderListIdGenerator
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.enums_c cimport ContingencyType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport TimeInForce
from nautilus_trader.model.enums_c cimport TrailingOffsetType
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.enums_c cimport order_type_to_str
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

    The `TraderId` tag and `StrategyId` tag will be inserted into all IDs generated.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID (only numerical tag sent to venue).
    strategy_id : StrategyId
        The strategy ID (only numerical tag sent to venue).
    clock : Clock
        The clock for the factory.
    initial_order_id_count : int, optional
        The initial order ID count for the factory.
    initial_order_list_id_count : int, optional
        The initial order list ID count for the factory.

    Raises
    ------
    ValueError
        If `initial_order_id_count` is negative (< 0).
    ValueError
        If `initial_order_list_id_count` is negative (< 0).
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        Clock clock not None,
        int initial_order_id_count=0,
        int initial_order_list_id_count=0,
    ):
        self._clock = clock
        self.trader_id = trader_id
        self.strategy_id = strategy_id

        self._order_id_generator = ClientOrderIdGenerator(
            trader_id=trader_id,
            strategy_id=strategy_id,
            clock=clock,
            initial_count=initial_order_id_count,
        )
        self._order_list_id_generator = OrderListIdGenerator(
            trader_id=trader_id,
            strategy_id=strategy_id,
            clock=clock,
            initial_count=initial_order_list_id_count,
        )

    cpdef void set_client_order_id_count(self, int count) except *:
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
        self._order_id_generator.set_count(count)

    cpdef void set_order_list_id_count(self, int count) except *:
        """
        Set the internal order list ID generator count to the given count.

        Parameters
        ----------
        count : int
            The count to set.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        self._order_list_id_generator.set_count(count)

    cpdef ClientOrderId generate_client_order_id(self) except *:
        """
        Generate and return a new client order ID.

        The identifier will be the next in the logical sequence.

        Returns
        -------
        ClientOrderId

        """
        return self._order_id_generator.generate()

    cpdef OrderListId generate_order_list_id(self) except *:
        """
        Generate and return a new order list ID.

        The identifier will be the next in the logical sequence.

        Returns
        -------
        OrderListId

        """
        return self._order_list_id_generator.generate()

    cpdef void reset(self) except *:
        """
        Reset the order factory.

        All stateful fields are reset to their initial value.
        """
        self._order_id_generator.reset()
        self._order_list_id_generator.reset()

    cpdef OrderList create_list(self, list orders):
        """
        Return a new order list containing the given `orders`.

        Parameters
        ----------
        orders : list[Order]
            The orders for the list.

        Returns
        -------
        OrderList

        Raises
        ------
        ValueError
            If `orders` is empty.

        Notes
        -----
        The order at index 0 in the list will be considered the 'first' order.

        """
        Condition.not_empty(orders, "orders")

        return OrderList(
            order_list_id=self._order_list_id_generator.generate(),
            orders=orders,
        )

    cpdef MarketOrder market(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        TimeInForce time_in_force = TimeInForce.GTC,
        bint reduce_only = False,
        str tags = None,
    ):
        """
        Create a new ``MARKET`` order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
            The orders time in force. Often not applicable for market orders.
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
            If `time_in_force` is ``GTD``.

        """
        return MarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._order_id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            time_in_force=time_in_force,
            reduce_only=reduce_only,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
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
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint post_only = False,
        bint reduce_only = False,
        Quantity display_qty = None,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        str tags = None,
    ):
        """
        Create a new ``LIMIT`` order.

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
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The orders emulation trigger.
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
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        return LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._order_id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            price=price,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=time_in_force,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            emulation_trigger=emulation_trigger,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
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
        TriggerType trigger_type = TriggerType.DEFAULT,
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint reduce_only = False,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        str tags = None,
    ):
        """
        Create a new ``STOP_MARKET`` conditional order.

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
        time_in_force : TimeInForce  {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The orders emulation trigger.
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
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.

        """
        return StopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._order_id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=time_in_force,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            reduce_only=reduce_only,
            emulation_trigger=emulation_trigger,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
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
        TriggerType trigger_type = TriggerType.DEFAULT,
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint post_only = False,
        bint reduce_only = False,
        Quantity display_qty = None,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        str tags = None,
    ):
        """
        Create a new ``STOP_LIMIT`` conditional order.

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
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The orders emulation trigger.
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
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        return StopLimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._order_id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=time_in_force,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            emulation_trigger=emulation_trigger,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
        )

    cpdef MarketToLimitOrder market_to_limit(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint reduce_only = False,
        Quantity display_qty = None,
        str tags = None,
    ):
        """
        Create a new ``MARKET`` order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        time_in_force : TimeInForce {``GTC``, ``GTD``, ``IOC``, ``FOK``}, default ``GTC``
            The orders time in force.
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
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.

        """
        return MarketToLimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._order_id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            reduce_only=reduce_only,
            display_qty=display_qty,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=time_in_force,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
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
        TriggerType trigger_type = TriggerType.DEFAULT,
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint reduce_only = False,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        str tags = None,
    ):
        """
        Create a new ``MARKET_IF_TOUCHED`` (MIT) conditional order.

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
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The orders emulation trigger.
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
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.

        """
        return MarketIfTouchedOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._order_id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=time_in_force,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            reduce_only=reduce_only,
            emulation_trigger=emulation_trigger,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
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
        TriggerType trigger_type = TriggerType.DEFAULT,
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint post_only = False,
        bint reduce_only = False,
        Quantity display_qty = None,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        str tags = None,
    ):
        """
        Create a new ``LIMIT_IF_TOUCHED`` (LIT) conditional order.

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
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The orders emulation trigger.
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
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        return LimitIfTouchedOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._order_id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=time_in_force,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            emulation_trigger=emulation_trigger,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
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
        Price trigger_price = None,
        TriggerType trigger_type = TriggerType.DEFAULT,
        TrailingOffsetType trailing_offset_type = TrailingOffsetType.PRICE,
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint reduce_only = False,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        str tags = None,
    ):
        """
        Create a new ``TRAILING_STOP_MARKET`` conditional order.

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
        trailing_offset_type : TrailingOffsetType, default ``PRICE``
            The order trailing offset type.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The orders emulation trigger.
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
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `trailing_offset_type` is ``NO_TRAILING_OFFSET``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.

        """
        return TrailingStopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._order_id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            trailing_offset=trailing_offset,
            trailing_offset_type=trailing_offset_type,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=time_in_force,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            reduce_only=reduce_only,
            emulation_trigger=emulation_trigger,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
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
        Price price = None,
        Price trigger_price = None,
        TriggerType trigger_type = TriggerType.DEFAULT,
        TrailingOffsetType trailing_offset_type = TrailingOffsetType.PRICE,
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint post_only = False,
        bint reduce_only = False,
        Quantity display_qty = None,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        str tags = None,
    ):
        """
        Create a new ``TRAILING_STOP_LIMIT`` conditional order.

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
        trailing_offset_type : TrailingOffsetType, default ``PRICE``
            The order trailing offset type.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The orders emulation trigger.
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
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `trailing_offset_type` is ``NO_TRAILING_OFFSET``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        return TrailingStopLimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=instrument_id,
            client_order_id=self._order_id_generator.generate(),
            order_side=order_side,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            limit_offset=limit_offset,
            trailing_offset=trailing_offset,
            trailing_offset_type=trailing_offset_type,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=time_in_force,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            emulation_trigger=emulation_trigger,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
        )

    cpdef OrderList bracket(
        self,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity quantity,
        Price entry_trigger_price = None,
        Price entry_price = None,
        Price sl_trigger_price = None,
        Price tp_trigger_price = None,
        Price tp_price = None,
        OrderType entry_order_type = OrderType.MARKET,
        OrderType tp_order_type = OrderType.LIMIT,
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint entry_post_only = False,
        bint tp_post_only = True,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        ContingencyType contingency_type = ContingencyType.OUO,
    ):
        """
        Create a bracket order with optional entry of take-profit order types.

        The stop-loss order will always be ``STOP_MARKET``.
        The bracketing stop-loss and take-profit orders will have a time in force
        of ``GTC``.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The entry orders side.
        quantity : Quantity
            The entry orders quantity (> 0).
        entry_trigger_price : Price, optional
            The entry order trigger price (STOP).
        entry_price : Price, optional
            The entry order price (LIMIT).
        sl_trigger_price : Price, optional
            The stop-loss child order trigger price (STOP).
        tp_trigger_price : Price, optional
            The take-profit child order trigger price (STOP).
        tp_price : Price, optional
            The take-profit child order price (LIMIT).
        entry_order_type : OrderType {``MARKET``, ``LIMIT``, ``LIMIT_IF_TOUCHED``, ``MARKET_IF_TOUCHED``}, default ``MARKET``
            The entry order type.
        tp_order_type : OrderType {``LIMIT``, ``LIMIT_IF_TOUCHED``, ``MARKET_IF_TOUCHED``}, default ``LIMIT``
            The take-profit order type.
        time_in_force : TimeInForce {``DAY``, ``GTC``}, optional
            The entry orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        entry_post_only : bool, default False
            If the entry order will only provide liquidity (make a market).
        tp_post_only : bool, default False
            If the take-profit order will only provide liquidity (make a market).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The emulation trigger type for the entry, as well as the TP and SL bracket orders.
        contingency_type : ContingencyType, default ``OUO``
            The contingency type for the TP and SL bracket orders.

        Returns
        -------
        OrderList

        """
        cdef OrderListId order_list_id = self._order_list_id_generator.generate()
        cdef ClientOrderId entry_client_order_id = self._order_id_generator.generate()
        cdef ClientOrderId sl_client_order_id = self._order_id_generator.generate()
        cdef ClientOrderId tp_client_order_id = self._order_id_generator.generate()

        ########################################################################
        # ENTRY ORDER
        ########################################################################
        if entry_order_type == OrderType.MARKET:
            entry_order = MarketOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=instrument_id,
                client_order_id=entry_client_order_id,
                order_side=order_side,
                quantity=quantity,
                init_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
                time_in_force=TimeInForce.GTC,
                contingency_type=ContingencyType.OTO,
                order_list_id=order_list_id,
                linked_order_ids=[sl_client_order_id, tp_client_order_id],
                parent_order_id=None,
                tags="ENTRY",
            )
        elif entry_order_type == OrderType.LIMIT:
            entry_order = LimitOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=instrument_id,
                client_order_id=entry_client_order_id,
                order_side=order_side,
                quantity=quantity,
                price=entry_price,
                init_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
                time_in_force=time_in_force,
                expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
                post_only=entry_post_only,
                emulation_trigger=emulation_trigger,
                contingency_type=ContingencyType.OTO,
                order_list_id=order_list_id,
                linked_order_ids=[sl_client_order_id, tp_client_order_id],
                parent_order_id=None,
                tags="ENTRY",
            )
        elif entry_order_type == OrderType.MARKET_IF_TOUCHED:
            entry_order = MarketIfTouchedOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=instrument_id,
                client_order_id=entry_client_order_id,
                order_side=order_side,
                quantity=quantity,
                trigger_price=entry_trigger_price,
                trigger_type=TriggerType.DEFAULT,
                init_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
                time_in_force=time_in_force,
                expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
                emulation_trigger=emulation_trigger,
                contingency_type=ContingencyType.OTO,
                order_list_id=order_list_id,
                linked_order_ids=[sl_client_order_id, tp_client_order_id],
                parent_order_id=None,
                tags="ENTRY",
            )
        elif entry_order_type == OrderType.LIMIT_IF_TOUCHED:
            entry_order = LimitIfTouchedOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=instrument_id,
                client_order_id=entry_client_order_id,
                order_side=order_side,
                quantity=quantity,
                price=entry_price,
                trigger_price=entry_trigger_price,
                trigger_type=TriggerType.DEFAULT,
                init_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
                time_in_force=time_in_force,
                expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
                post_only=entry_post_only,
                emulation_trigger=emulation_trigger,
                contingency_type=ContingencyType.OTO,
                order_list_id=order_list_id,
                linked_order_ids=[sl_client_order_id, tp_client_order_id],
                parent_order_id=None,
                tags="ENTRY",
            )
        else:
            raise ValueError(f"invalid `entry_order_type`, was {order_type_to_str(entry_order_type)}")

        ########################################################################
        # TAKE-PROFIT ORDER
        ########################################################################
        if tp_order_type == OrderType.LIMIT:
            tp_order = LimitOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=entry_order.instrument_id,
                client_order_id=tp_client_order_id,
                order_side=Order.opposite_side_c(entry_order.side),
                quantity=quantity,
                price=tp_price,
                init_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
                time_in_force=TimeInForce.GTC,
                post_only=tp_post_only,
                reduce_only=True,
                display_qty=None,
                emulation_trigger=emulation_trigger,
                contingency_type=contingency_type,
                order_list_id=order_list_id,
                linked_order_ids=[sl_client_order_id],
                parent_order_id=entry_client_order_id,
                tags="TAKE_PROFIT",
            )
        elif tp_order_type == OrderType.LIMIT_IF_TOUCHED:
            tp_order = LimitIfTouchedOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=entry_order.instrument_id,
                client_order_id=tp_client_order_id,
                order_side=Order.opposite_side_c(entry_order.side),
                quantity=quantity,
                price=tp_price,
                trigger_price=tp_trigger_price,
                trigger_type=TriggerType.DEFAULT,
                init_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
                time_in_force=TimeInForce.GTC,
                post_only=tp_post_only,
                reduce_only=True,
                display_qty=None,
                emulation_trigger=emulation_trigger,
                contingency_type=contingency_type,
                order_list_id=order_list_id,
                linked_order_ids=[sl_client_order_id],
                parent_order_id=entry_client_order_id,
                tags="TAKE_PROFIT",
            )
        elif tp_order_type == OrderType.MARKET_IF_TOUCHED:
            tp_order = MarketIfTouchedOrder(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                instrument_id=entry_order.instrument_id,
                client_order_id=tp_client_order_id,
                order_side=Order.opposite_side_c(entry_order.side),
                quantity=quantity,
                trigger_price=tp_trigger_price,
                trigger_type=TriggerType.DEFAULT,
                init_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
                time_in_force=TimeInForce.GTC,
                reduce_only=True,
                emulation_trigger=emulation_trigger,
                contingency_type=contingency_type,
                order_list_id=order_list_id,
                linked_order_ids=[sl_client_order_id],
                parent_order_id=entry_client_order_id,
                tags="TAKE_PROFIT",
            )
        else:
            raise ValueError(f"invalid `tp_order_type`, was {order_type_to_str(entry_order_type)}")

        ########################################################################
        # STOP-LOSS ORDER
        ########################################################################
        sl_order = StopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=entry_order.instrument_id,
            client_order_id=sl_client_order_id,
            order_side=Order.opposite_side_c(entry_order.side),
            quantity=quantity,
            trigger_price=sl_trigger_price,
            trigger_type=TriggerType.DEFAULT,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            reduce_only=True,
            emulation_trigger=emulation_trigger,
            contingency_type=contingency_type,
            order_list_id=order_list_id,
            linked_order_ids=[tp_client_order_id],
            parent_order_id=entry_client_order_id,
            tags="STOP_LOSS",
        )

        return OrderList(
            order_list_id=order_list_id,
            orders=[entry_order, sl_order, tp_order],
        )
