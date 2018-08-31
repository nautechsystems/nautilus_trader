#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="factories.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import pytz

from decimal import Decimal
from datetime import datetime
from typing import List, Dict, Optional

from inv_trader.core.typing import typechecking
from inv_trader.core.preconditions import Precondition
from inv_trader.model.enums import OrderSide, OrderType, TimeInForce
from inv_trader.model.objects import Symbol
from inv_trader.model.order import Order

# Constants
# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, pytz.UTC)
SEPARATOR = '-'
MILLISECONDS_PER_SECOND = 1000
OrderId = str


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
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     datetime.now(tz=pytz.UTC),
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
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.LIMIT,
                     quantity,
                     datetime.now(tz=pytz.UTC),
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
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.STOP_MARKET,
                     quantity,
                     datetime.now(tz=pytz.UTC),
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
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.STOP_LIMIT,
                     quantity,
                     datetime.now(tz=pytz.UTC),
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
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MIT,
                     quantity,
                     datetime.now(tz=pytz.UTC),
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
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     datetime.now(tz=pytz.UTC),
                     price=None,
                     time_in_force=TimeInForce.FOC,
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
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     datetime.now(tz=pytz.UTC),
                     price=None,
                     time_in_force=TimeInForce.IOC,
                     expire_time=None)


class OrderIdGenerator:
    """
    Provides a generator for unique order identifiers.
    """

    @typechecking
    def __init__(self, order_id_tag: str):
        """
        Initializes a new instance of the OrderIdentifierFactory class.

        :param order_id_tag: The generators unique order identifier tag.
        """
        Precondition.valid_string(order_id_tag, 'order_id_tag')

        self._order_id_tag = order_id_tag
        self._order_symbol_counts = {}  # type: Dict[Symbol, int]
        self._order_ids = []            # type: List[OrderId]

    @typechecking
    def generate(self, order_symbol: Symbol) -> OrderId:
        """
        Create a unique order identifier for the strategy using the given symbol.

        :param order_symbol: The order symbol for the unique identifier.
        :return: The unique order identifier.
        """
        if order_symbol not in self._order_symbol_counts:
            self._order_symbol_counts[order_symbol] = 0

        self._order_symbol_counts[order_symbol] += 1
        milliseconds = str(self._milliseconds_since_unix_epoch())
        order_count = str(self._order_symbol_counts[order_symbol])
        order_id = (str(order_symbol.code)
                    + SEPARATOR + str(order_symbol.venue.name)
                    + SEPARATOR + order_count
                    + SEPARATOR + self._order_id_tag
                    + SEPARATOR + milliseconds)

        if order_id in self._order_ids:
            return self.generate(order_symbol)
        self._order_ids.append(order_id)
        return order_id

    @typechecking
    def _milliseconds_since_unix_epoch(self) -> int:
        """
        Returns the number of ticks of the given time now since the Unix Epoch.

        :return: The milliseconds since the Unix Epoch.
        """
        return int((datetime.now(tz=pytz.UTC) - UNIX_EPOCH).total_seconds() * MILLISECONDS_PER_SECOND)
