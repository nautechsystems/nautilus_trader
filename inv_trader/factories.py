#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="factories.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import datetime

from decimal import Decimal

from inv_trader.enums import OrderSide, OrderType, TimeInForce
from inv_trader.objects import Symbol, Order


class OrderFactory:
    """
    A static factory class which provides different order types.
    """

    @staticmethod
    def market_order(
            symbol: Symbol,
            identifier: str,
            label: str,
            order_side: OrderSide,
            quantity: int) -> Order:
        """
        Creates and returns a new market order with the given parameters.

        :param symbol: The market orders symbol.
        :param identifier: The market orders identifier.
        :param label: The market orders label.
        :param order_side: The market orders side.
        :param quantity: The market orders quantity (> 0).
        :return: The market order.
        """
        return Order(symbol,
                     identifier,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     datetime.datetime.utcnow())

    @staticmethod
    def limit_order(
            symbol: Symbol,
            identifier: str,
            label: str,
            order_side: OrderSide,
            quantity: int,
            price: Decimal,
            time_in_force: TimeInForce=None,
            expire_time: datetime.datetime=None) -> Order:
        """
        Creates and returns a new limit order with the given parameters.

        :param symbol: The limit orders symbol.
        :param identifier: The limit orders identifier.
        :param label: The limit orders label.
        :param order_side: The limit orders side.
        :param quantity: The limit orders quantity (> 0).
        :param price: The limit orders price (> 0).
        :param time_in_force: The limit orders time in force (optional can be None).
        :param expire_time: The limit orders expire time (optional can be None unless GTD).
        :return: The limit order.
        """
        return Order(symbol,
                     identifier,
                     label,
                     order_side,
                     OrderType.LIMIT,
                     quantity,
                     datetime.datetime.utcnow(),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def stop_order(
            symbol: Symbol,
            identifier: str,
            label: str,
            order_side: OrderSide,
            quantity: int,
            price: Decimal,
            time_in_force: TimeInForce=None,
            expire_time: datetime.datetime=None) -> Order:
        """
        Creates and returns a new stop order with the given parameters.

        :param symbol: The stop orders symbol.
        :param identifier: The stop orders identifier.
        :param label: The stop orders label.
        :param order_side: The stop orders side.
        :param quantity: The stop orders quantity (> 0).
        :param price: The stop orders price (> 0).
        :param time_in_force: The stop orders time in force (optional can be None).
        :param expire_time: The stop orders expire time (optional can be None unless GTD).
        :return: The stop order.
        """
        return Order(symbol,
                     identifier,
                     label,
                     order_side,
                     OrderType.STOP,
                     quantity,
                     datetime.datetime.utcnow(),
                     price,
                     time_in_force,
                     expire_time)
