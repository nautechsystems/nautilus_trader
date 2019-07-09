#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="algorithms.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from collections import deque

from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.objects cimport Symbol, Tick, BarType, Bar
from nautilus_trader.model.order cimport Order


cdef class TrailingStopSignal:
    """
    Represents a trailing stop signal.
    """

    def __init__(self, Price price=None):
        """
        Initializes a new instance of the TrailingStopSignal class.

        :param price: The price for the trailing stop signal.
        """
        self.price = price
        self.is_signal = True if self.price is not None else False


cdef class TrailingStopAlgorithm:
    """
    The base class for all trailing stop algorithms.
    """

    def __init__(self, Order order):
        """
        Initializes a new instance of the TickTrailingStopAlgorithm class.

        :param order: The order for the trailing stop.
        """
        self.order = order

        if order.side == OrderSide.BUY:
            self.generate = self._generate_buy
        elif order.side == OrderSide.SELL:
            self.generate = self._generate_sell
        else:
            raise ValueError(f"order side {order.side} is unrecognized")

    cdef TrailingStopSignal _generate_buy(self, Price update_price):
        if update_price < self.order.price:
            return TrailingStopSignal(update_price)
        else:
            return TrailingStopSignal()

    cdef TrailingStopSignal _generate_sell(self, Price update_price):
        if update_price > self.order.price:
            return TrailingStopSignal(update_price)
        else:
            return TrailingStopSignal()


cdef class TickTrailingStopAlgorithm(TrailingStopAlgorithm):
    """
    The base class for all trailing stop algorithms updated with ticks.
    """

    def __init__(self, Order order, Symbol symbol):
        """
        Initializes a new instance of the TickTrailingStopAlgorithm class.

        :param order: The order for the trailing stop.
        """
        super().__init__(order)

        self.symbol = symbol

        if self.order.side == OrderSide.BUY:
            self._calculate = self.calculate_buy
        elif self.order.side == OrderSide.SELL:
            self._calculate = self.calculate_sell
        else:
            raise ValueError(f"order side {order.side} is unrecognized")

    cpdef void update(self, Tick tick):
        """
        Update the algorithm with the given tick.
        
        :param tick: The tick to update with.
        """
        self._calculate(tick)

    cpdef TrailingStopSignal calculate_buy(self, Tick tick):
        """
        Run the trailing stop algorithm for buy order types.

        :param tick: The tick to run the algorithm with.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef TrailingStopSignal calculate_sell(self, Tick tick):
        """
        Run the trailing stop algorithm for sell order types.

        :param tick: The tick to run the algorithm with.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class BarTrailingStopAlgorithm(TrailingStopAlgorithm):
    """
    The base class for all trailing stop algorithms updated with bars.
    """

    def __init__(self, Order order, BarType bar_type):
        """
        Initializes a new instance of the BarTrailingStopAlgorithm class.

        :param order: The order for the trailing stop.
        """
        super().__init__(order)

        self.bar_type = bar_type

        if self.order.side == OrderSide.BUY:
            self._calculate = self.calculate_buy
        elif self.order.side == OrderSide.SELL:
            self._calculate = self.calculate_sell
        else:
            raise ValueError(f"order side {order.side} is unrecognized")

    cpdef void update(self, Bar bar):
        """
        Update the algorithm with the given tick.
        
        :param bar: The bar to update with.
        """
        self._calculate(bar)

    cpdef TrailingStopSignal calculate_buy(self, Bar bar):
        """
        Run the trailing stop algorithm for buy order types.
        
        :param bar: The bar to run the algorithm with.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef TrailingStopSignal calculate_sell(self, Bar bar):
        """
        Run the trailing stop algorithm for sell order types.

        :param bar: The bar to run the algorithm with.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class BarsBackTrail(BarTrailingStopAlgorithm):
    """
    A trailing stop algorithm based on the number of bars back.
    """

    def __init__(self,
                 list bars,
                 int bars_back,
                 float sl_atr_multiple,
                 object atr,
                 Order order,
                 BarType bar_type):
        """
        Initializes a new instance of the BarTrailingStopAlgorithm class.

        :param order: The order for the trailing stop.
        """
        super().__init__(order,
                         bar_type)

        self._bars_back = bars_back
        self._sl_atr_multiple = sl_atr_multiple
        self._atr = atr
        self._bars = deque(maxlen=bar_type)

    cpdef TrailingStopSignal calculate_buy(self, Bar bar):
        """
        Run the trailing stop algorithm for buy order types.
        
        :param bar: The bar to run the algorithm with.
        """
        self._bars.append(bar)
        return self.generate(bar[0].high + (self._atr.value * self._sl_atr_multiple))

    cpdef TrailingStopSignal calculate_sell(self, Bar bar):
        """
        Run the trailing stop algorithm for sell order types.

        :param bar: The bar to run the algorithm with.
        """
        self._bars.append(bar)
        return self.generate(bar[0].low - (self._atr.value * self._sl_atr_multiple))
