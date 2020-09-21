# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from collections import deque

from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.tick cimport Tick


cdef class TrailingStopSignal:
    """
    Represents a trailing stop signal.
    """

    def __init__(self, Price price=None):
        """
        Initialize a new instance of the TrailingStopSignal class.

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
        Initialize a new instance of the TrailingStopAlgorithm class.

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
        Initialize a new instance of the TickTrailingStopAlgorithm class.

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

    cpdef void update(self, Tick tick) except *:
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
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef TrailingStopSignal calculate_sell(self, Tick tick):
        """
        Run the trailing stop algorithm for sell order types.

        :param tick: The tick to run the algorithm with.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class BarTrailingStopAlgorithm(TrailingStopAlgorithm):
    """
    The base class for all trailing stop algorithms updated with bars.
    """

    def __init__(self, Order order, BarType bar_type):
        """
        Initialize a new instance of the BarTrailingStopAlgorithm class.

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

    cpdef void update(self, Bar bar) except *:
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
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef TrailingStopSignal calculate_sell(self, Bar bar):
        """
        Run the trailing stop algorithm for sell order types.

        :param bar: The bar to run the algorithm with.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class BarsBackTrail(BarTrailingStopAlgorithm):
    """
    A trailing stop algorithm based on the number of bars back.
    """

    def __init__(self,
                 list bars,
                 int bars_back,
                 double sl_atr_multiple,
                 object atr,
                 Order order,
                 BarType bar_type):
        """
        Initialize a new instance of the BarsBackTrail class.

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
