#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import datetime
import uuid

from decimal import Decimal

from inv_trader.core.checks import typechecking
from inv_trader.model.enums import Venue, Resolution, QuoteType, OrderSide, OrderType, OrderStatus
from inv_trader.model.objects import Symbol, BarType, Bar
from inv_trader.execution import ExecutionClient
from inv_trader.model.order import Order
from inv_trader.model.events import Event, OrderEvent
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.strategy import TradeStrategy

StrategyId = str
OrderId = str


class MockExecClient(ExecutionClient):
    """
    Provides a mock execution client for trading strategies.
    """

    def connect(self):
        """
        Connect to the execution service.
        """
        self._log("MockExecClient connected.")

    def disconnect(self):
        """
        Disconnect from the execution service.
        """
        self._log("MockExecClient disconnected.")

    @typechecking
    def submit_order(
            self,
            order: Order,
            strategy_id: StrategyId):
        """
        Send a submit order request to the execution service.
        """
        super()._register_order(order, strategy_id)

        order_submitted = OrderSubmitted(
            order.symbol,
            order.id,
            datetime.datetime.utcnow(),
            uuid.uuid4(),
            datetime.datetime.utcnow())

        order_accepted = OrderAccepted(
            order.symbol,
            order.id,
            datetime.datetime.utcnow(),
            uuid.uuid4(),
            datetime.datetime.utcnow())

        order_working = OrderWorking(
            order.symbol,
            order.id,
            'B' + order.id,
            order.label,
            order.side,
            order.type,
            order.quantity,
            Decimal('1'),
            order.time_in_force,
            datetime.datetime.utcnow(),
            uuid.uuid4(),
            datetime.datetime.utcnow(),
            order.expire_time)

        super()._on_event(order_submitted)
        super()._on_event(order_accepted)
        super()._on_event(order_working)

    @typechecking
    def cancel_order(self, order: Order):
        """
        Send a cancel order request to the execution service.
        """
        order_cancelled = OrderCancelled(
            order.symbol,
            order.id,
            datetime.datetime.utcnow(),
            uuid.uuid4(),
            datetime.datetime.utcnow())

        super()._on_event(order_cancelled)

    @typechecking
    def modify_order(self, order: Order, new_price: Decimal):
        """
        Send a modify order request to the execution service.
        """
        order_modified = OrderModified(
            order.symbol,
            order.id,
            'B' + order.id,
            new_price,
            datetime.datetime.utcnow(),
            uuid.uuid4(),
            datetime.datetime.utcnow())

        super()._on_event(order_modified)
