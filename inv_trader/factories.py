#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="factories.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from datetime import datetime
from decimal import Decimal
from typing import Optional

from inv_trader.core.checks import typechecking
from inv_trader.model.enums import OrderSide, OrderType, TimeInForce
from inv_trader.model.objects import Symbol
from inv_trader.model.order import Order


class OrderFactory:
    """
    A static factory class which provides different order types.
    """

    @staticmethod
    @typechecking
    def market(
            symbol: Symbol,
            order_id: str,
            label: str,
            order_side: OrderSide,
            quantity: int) -> Order:
        """
        Creates and returns a new market order with the given parameters.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The market order.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     datetime.utcnow(),
                     price=None,
                     time_in_force=None,
                     expire_time=None)

    @staticmethod
    @typechecking
    def limit(
            symbol: Symbol,
            order_id: str,
            label: str,
            order_side: OrderSide,
            quantity: int,
            price: Decimal,
            time_in_force: Optional[TimeInForce]=None,
            expire_time: Optional[datetime]=None) -> Order:
        """
        Creates and returns a new limit order with the given parameters.
        If the time in force is GTD then a valid expire time must be given.
        
        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (optional can be None).
        :param expire_time: The orders expire time (optional can be None unless GTD).
        :return: The limit order.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.LIMIT,
                     quantity,
                     datetime.utcnow(),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    @typechecking
    def stop_market(
            symbol: Symbol,
            order_id: str,
            label: str,
            order_side: OrderSide,
            quantity: int,
            price: Decimal,
            time_in_force: Optional[TimeInForce]=None,
            expire_time: Optional[datetime]=None) -> Order:
        """
        Creates and returns a new stop-market order with the given parameters.
        If the time in force is GTD then a valid expire time must be given.
        
        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (optional can be None).
        :param expire_time: The orders expire time (optional can be None unless GTD).
        :return: The stop-market order.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.STOP_MARKET,
                     quantity,
                     datetime.utcnow(),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    @typechecking
    def stop_limit(
            symbol: Symbol,
            order_id: str,
            label: str,
            order_side: OrderSide,
            quantity: int,
            price: Decimal,
            time_in_force: Optional[TimeInForce]=None,
            expire_time: Optional[datetime]=None) -> Order:
        """
        Creates and returns a new stop-limit order with the given parameters.
        If the time in force is GTD then a valid expire time must be given.
        
        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (optional can be None).
        :param expire_time: The orders expire time (optional can be None unless GTD).
        :return: The stop-limit order.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.STOP_LIMIT,
                     quantity,
                     datetime.utcnow(),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    @typechecking
    def market_if_touched(
            symbol: Symbol,
            order_id: str,
            label: str,
            order_side: OrderSide,
            quantity: int,
            price: Decimal,
            time_in_force: Optional[TimeInForce]=None,
            expire_time: Optional[datetime]=None) -> Order:
        """
        Creates and returns a new market-if-touched order with the given parameters.
        If the time in force is GTD then a valid expire time must be given.
        
        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (optional can be None).
        :param expire_time: The orders expire time (optional can be None unless GTD).
        :return: The market-if-touched order.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MIT,
                     quantity,
                     datetime.utcnow(),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    @typechecking
    def fill_or_kill(
            symbol: Symbol,
            order_id: str,
            label: str,
            order_side: OrderSide,
            quantity: int) -> Order:
        """
        Creates and returns a new fill-or-kill order with the given parameters.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The market order.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.FOC,
                     quantity,
                     datetime.utcnow(),
                     price=None,
                     time_in_force=None,
                     expire_time=None)

    @staticmethod
    @typechecking
    def immediate_or_cancel(
            symbol: Symbol,
            order_id: str,
            label: str,
            order_side: OrderSide,
            quantity: int) -> Order:
        """
        Creates and returns a new immediate-or-cancel order with the given parameters.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The market order.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.IOC,
                     quantity,
                     datetime.utcnow(),
                     price=None,
                     time_in_force=None,
                     expire_time=None)
