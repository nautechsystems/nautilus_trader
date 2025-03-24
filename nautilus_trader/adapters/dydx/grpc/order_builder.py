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
"""
Construct order objects to be sent as GRPC messages to the dYdX chain.
"""

import math
from enum import Enum
from enum import IntEnum
from enum import unique

from v4_proto.dydxprotocol.clob.order_pb2 import Order
from v4_proto.dydxprotocol.clob.order_pb2 import OrderId
from v4_proto.dydxprotocol.subaccounts.subaccount_pb2 import SubaccountId


QUOTE_QUANTUMS_ATOMIC_RESOLUTION = -6
MAX_CLIENT_ID = 2**32 - 1


def round_down(input_value: float, base: int) -> int:
    """
    Round a value down.
    """
    return math.floor(input_value / base) * base


@unique
class OrderFlags(IntEnum):
    """
    Define the order flags.
    """

    SHORT_TERM = 0
    LONG_TERM = 64
    CONDITIONAL = 32


@unique
class OrderExecution(Enum):
    """
    Define the order execution types.
    """

    DEFAULT = "DEFAULT"
    IOC = "IOC"
    FOK = "FOK"
    POST_ONLY = "POST_ONLY"


@unique
class DYDXGRPCOrderType(Enum):
    """
    Define the order types.
    """

    LIMIT = "LIMIT"
    MARKET = "MARKET"
    STOP_LIMIT = "STOP_LIMIT"
    TAKE_PROFIT_LIMIT = "TAKE_PROFIT"
    STOP_MARKET = "STOP_MARKET"
    TAKE_PROFIT_MARKET = "TAKE_PROFIT_MARKET"


class OrderHelper:
    """
    Define helper functions to construct an order object.

    Ref: https://github.com/dydxprotocol/v4-clients/blob/64a9e637e9997f8ec27aa6bbe6ff23ce728143b1/v4-client-py-v2/dydx_v4_client/node/chain_helpers.py#L9

    """

    @staticmethod
    def calculate_time_in_force(  # noqa: C901
        order_type: DYDXGRPCOrderType,
        time_in_force: Order.TimeInForce,
        post_only: bool = False,
        execution: OrderExecution = OrderExecution.DEFAULT,
    ) -> Order.TimeInForce:
        """
        Calculate the time in force.
        """
        if order_type == DYDXGRPCOrderType.MARKET:
            return Order.TimeInForce.TIME_IN_FORCE_IOC

        if order_type == DYDXGRPCOrderType.LIMIT:
            if post_only:
                return Order.TimeInForce.TIME_IN_FORCE_POST_ONLY

            return time_in_force

        if order_type in [DYDXGRPCOrderType.STOP_LIMIT, DYDXGRPCOrderType.TAKE_PROFIT_LIMIT]:
            if execution == OrderExecution.DEFAULT:
                return Order.TimeInForce.TIME_IN_FORCE_UNSPECIFIED

            if execution == OrderExecution.POST_ONLY:
                return Order.TimeInForce.TIME_IN_FORCE_POST_ONLY

            if execution == OrderExecution.FOK:
                return Order.TimeInForce.TIME_IN_FORCE_FILL_OR_KILL

            if execution == OrderExecution.IOC:
                return Order.TimeInForce.TIME_IN_FORCE_IOC
        elif order_type in [DYDXGRPCOrderType.STOP_MARKET, DYDXGRPCOrderType.TAKE_PROFIT_MARKET]:
            if execution in [OrderExecution.DEFAULT, OrderExecution.POST_ONLY]:
                message = f"Execution value {execution.value} not supported for {order_type.value}"
                raise ValueError(message)

            if execution == OrderExecution.FOK:
                return Order.TimeInForce.TIME_IN_FORCE_FILL_OR_KILL

            if execution == OrderExecution.IOC:
                return Order.TimeInForce.TIME_IN_FORCE_IOC

        message = f"Invalid combination of order type `{order_type.value}`, time in force `{time_in_force.value}`, and execution `{execution.value}`"
        raise ValueError(message)

    @staticmethod
    def calculate_client_metadata(order_type: DYDXGRPCOrderType) -> int:
        """
        Calculate the client metadata.
        """
        return (
            1
            if order_type
            in [
                DYDXGRPCOrderType.MARKET,
                DYDXGRPCOrderType.STOP_MARKET,
                DYDXGRPCOrderType.TAKE_PROFIT_MARKET,
            ]
            else 0
        )

    @staticmethod
    def calculate_condition_type(order_type: DYDXGRPCOrderType) -> Order.ConditionType:
        """
        Calculate the condition type.
        """
        if order_type in [DYDXGRPCOrderType.LIMIT, DYDXGRPCOrderType.MARKET]:
            return Order.ConditionType.CONDITION_TYPE_UNSPECIFIED

        if order_type in [DYDXGRPCOrderType.STOP_LIMIT, DYDXGRPCOrderType.STOP_MARKET]:
            return Order.ConditionType.CONDITION_TYPE_STOP_LOSS

        if order_type in [
            DYDXGRPCOrderType.TAKE_PROFIT_LIMIT,
            DYDXGRPCOrderType.TAKE_PROFIT_MARKET,
        ]:
            return Order.ConditionType.CONDITION_TYPE_TAKE_PROFIT

        message = f"Invalid order type `{order_type}`"
        raise ValueError(message)


class OrderBuilder:
    """
    Construct order objects to be sent as GRPC messages to the dYdX chain.

    Ref: https://github.com/dydxprotocol/v4-clients/blob/64a9e637e9997f8ec27aa6bbe6ff23ce728143b1/v4-client-py-v2/dydx_v4_client/node/market.py#L21

    """

    def __init__(
        self,
        atomic_resolution: int,
        step_base_quantums: int,
        subticks_per_tick: int,
        quantum_conversion_exponent: int,
        clob_pair_id: int,
    ) -> None:
        """
        Construct order objects to be sent as GRPC messages to the dYdX chain.
        """
        self._atomic_resolution = atomic_resolution
        self._step_base_quantums = step_base_quantums
        self._subticks_per_tick = subticks_per_tick
        self._quantum_conversion_exponent = quantum_conversion_exponent
        self._clob_pair_id = clob_pair_id

    def calculate_quantums(self, size: float) -> int:
        """
        Convert the order size to quantums.
        """
        raw_quantums = size * 10 ** (-self._atomic_resolution)
        quantums = round_down(raw_quantums, self._step_base_quantums)
        return max(quantums, self._step_base_quantums)

    def calculate_subticks(self, price: float) -> int:
        """
        Convert the order price to subticks.
        """
        exponent = (
            self._atomic_resolution
            - self._quantum_conversion_exponent
            - QUOTE_QUANTUMS_ATOMIC_RESOLUTION
        )
        raw_subticks = price * 10**exponent
        subticks = round_down(raw_subticks, self._subticks_per_tick)
        return max(subticks, self._subticks_per_tick)

    def create_order_id(
        self,
        address: str,
        subaccount_number: int,
        client_id: int,
        order_flags: int,
    ) -> OrderId:
        """
        Create a new OrderId instance.
        """
        return OrderId(
            subaccount_id=SubaccountId(owner=address, number=subaccount_number),
            client_id=client_id,
            order_flags=order_flags,
            clob_pair_id=self._clob_pair_id,
        )

    def create_order(
        self,
        order_id: OrderId,
        order_type: DYDXGRPCOrderType,
        side: Order.Side,
        size: float,
        price: float,
        time_in_force: Order.TimeInForce,
        reduce_only: bool,
        post_only: bool = False,
        good_til_block: int | None = None,
        good_til_block_time: int | None = None,
        execution: OrderExecution = OrderExecution.DEFAULT,
        trigger_price: float | None = None,
    ) -> Order:
        """
        Create a new Order instance.

        order_id : OrderId
            OrderId protobuf message.
        order_type: DYDXGRPCOrderType
            Order type enum: LIMIT, MARKET, STOP_LIMIT, TAKE_PROFIT_LIMIT,
            STOP_MARKET or TAKE_PROFIT_MARKET.
        side : Order.Side
            The side of the order.
        size : float
            The size of the order.
        price : float
            The price of the limit order. Set to 0 for market orders.
        time_in_force : Order.TimeInForce
            Time in force setting for the order.
            Options: GTT (Good Till Time), FOK (Fill or Kill), IOC (Immediate or Cancel)
        post_only : bool, default False
            Ensures that the order will only be added to the order book if it does
            not immediately fill against an existing order in the order book.
            In other words, a post-only limit order will only be placed if it can
            be added as a maker order and not as a taker order.
        good_til_block : int, optional
            The block height when the order expires if it is not yet filled.
        good_til_block_time : int, optional
            The time in seconds since the epoch when the order expired if it is
            not yet filled.
        execution : OrderExecution, default OrderExecution.DEFAULT
            OrderExecution enum: DEFAULT, IOC, FOK or POST_ONLY
        trigger_price : float, optional.
            The price of the conditional limit order. Only applicable to STOP_LIMIT,
            STOP_MARKET, TAKE_PROFIT_MARKET or TAKE_PROFIT_LIMIT orders.

        """
        order_time_in_force = OrderHelper.calculate_time_in_force(
            order_type,
            time_in_force,
            post_only,
            execution,
        )
        client_metadata = OrderHelper.calculate_client_metadata(order_type)
        condition_type = OrderHelper.calculate_condition_type(order_type)
        conditional_order_trigger_subticks = 0

        if trigger_price is not None:
            conditional_order_trigger_subticks = self.calculate_subticks(trigger_price)

        return Order(
            order_id=order_id,
            side=side,
            quantums=self.calculate_quantums(size),
            subticks=self.calculate_subticks(price),
            good_til_block=good_til_block,
            good_til_block_time=good_til_block_time,
            time_in_force=order_time_in_force,
            reduce_only=reduce_only,
            client_metadata=client_metadata,
            condition_type=condition_type,
            conditional_order_trigger_subticks=conditional_order_trigger_subticks,
        )
