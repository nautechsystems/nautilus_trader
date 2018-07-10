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

from inv_trader.model.enums import OrderSide, OrderType, TimeInForce
from inv_trader.model.objects import Symbol
from inv_trader.model.order import Order


class OrderFactory:
    """
    A static factory class which provides different order types.
    If the time in force is GTD then a valid expire time must be given.
    """

    @staticmethod
    def market(
            symbol: Symbol,
            identifier: str,
            label: str,
            order_side: OrderSide,
            quantity: int) -> Order:
        """
        Creates and returns a new market order with the given parameters.

        :param symbol: The orders symbol.
        :param identifier: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
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
    def limit(
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

        :param symbol: The orders symbol.
        :param identifier: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (optional can be None).
        :param expire_time: The orders expire time (optional can be None unless GTD).
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
    def stop_market(
            symbol: Symbol,
            identifier: str,
            label: str,
            order_side: OrderSide,
            quantity: int,
            price: Decimal,
            time_in_force: TimeInForce=None,
            expire_time: datetime.datetime=None) -> Order:
        """
        Creates and returns a new stop-market order with the given parameters.

        :param symbol: The orders symbol.
        :param identifier: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (optional can be None).
        :param expire_time: The orders expire time (optional can be None unless GTD).
        :return: The stop-market order.
        """
        return Order(symbol,
                     identifier,
                     label,
                     order_side,
                     OrderType.STOP_MARKET,
                     quantity,
                     datetime.datetime.utcnow(),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def stop_limit(
            symbol: Symbol,
            identifier: str,
            label: str,
            order_side: OrderSide,
            quantity: int,
            price: Decimal,
            time_in_force: TimeInForce=None,
            expire_time: datetime.datetime=None) -> Order:
        """
        Creates and returns a new stop-limit order with the given parameters.

        :param symbol: The orders symbol.
        :param identifier: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (optional can be None).
        :param expire_time: The orders expire time (optional can be None unless GTD).
        :return: The stop-limit order.
        """
        return Order(symbol,
                     identifier,
                     label,
                     order_side,
                     OrderType.STOP_LIMIT,
                     quantity,
                     datetime.datetime.utcnow(),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def market_if_touched(
            symbol: Symbol,
            identifier: str,
            label: str,
            order_side: OrderSide,
            quantity: int,
            price: Decimal,
            time_in_force: TimeInForce=None,
            expire_time: datetime.datetime=None) -> Order:
        """
        Creates and returns a new market-if-touched order with the given parameters.

        :param symbol: The orders symbol.
        :param identifier: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (optional can be None).
        :param expire_time: The orders expire time (optional can be None unless GTD).
        :return: The market-if-touched order.
        """
        return Order(symbol,
                     identifier,
                     label,
                     order_side,
                     OrderType.MIT,
                     quantity,
                     datetime.datetime.utcnow(),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def fill_or_kill(
            symbol: Symbol,
            identifier: str,
            label: str,
            order_side: OrderSide,
            quantity: int) -> Order:
        """
        Creates and returns a new fill-or-kill order with the given parameters.

        :param symbol: The orders symbol.
        :param identifier: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The market order.
        """
        return Order(symbol,
                     identifier,
                     label,
                     order_side,
                     OrderType.FOC,
                     quantity,
                     datetime.datetime.utcnow())

    @staticmethod
    def immediate_or_cancel(
            symbol: Symbol,
            identifier: str,
            label: str,
            order_side: OrderSide,
            quantity: int) -> Order:
        """
        Creates and returns a new immediate-or-cancel order with the given parameters.

        :param symbol: The orders symbol.
        :param identifier: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The market order.
        """
        return Order(symbol,
                     identifier,
                     label,
                     order_side,
                     OrderType.IOC,
                     quantity,
                     datetime.datetime.utcnow())
