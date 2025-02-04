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

import random

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport NANOSECONDS_IN_MILLISECOND
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.model.functions cimport liquidity_side_to_str
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class FillModel:
    """
    Provides probabilistic modeling for order fill dynamics including probability
    of fills and slippage by order type.

    Parameters
    ----------
    prob_fill_on_limit : double
        The probability of limit order filling if the market rests on its price.
    prob_fill_on_stop : double
        The probability of stop orders filling if the market rests on its price.
    prob_slippage : double
        The probability of order fill prices slipping by one tick.
    random_seed : int, optional
        The random seed (if None then no random seed).

    Raises
    ------
    ValueError
        If any probability argument is not within range [0, 1].
    TypeError
        If `random_seed` is not None and not of type `int`.
    """

    def __init__(
        self,
        double prob_fill_on_limit = 1.0,
        double prob_fill_on_stop = 1.0,
        double prob_slippage = 0.0,
        random_seed: int | None = None,
    ):
        Condition.in_range(prob_fill_on_limit, 0.0, 1.0, "prob_fill_on_limit")
        Condition.in_range(prob_fill_on_stop, 0.0, 1.0, "prob_fill_on_stop")
        Condition.in_range(prob_slippage, 0.0, 1.0, "prob_slippage")
        if random_seed is not None:
            Condition.type(random_seed, int, "random_seed")
            random.seed(random_seed)
        else:
            random.seed()

        self.prob_fill_on_limit = prob_fill_on_limit
        self.prob_fill_on_stop = prob_fill_on_stop
        self.prob_slippage = prob_slippage

    cpdef bint is_limit_filled(self):
        """
        Return a value indicating whether a ``LIMIT`` order filled.

        Returns
        -------
        bool

        """
        return self._event_success(self.prob_fill_on_limit)

    cpdef bint is_stop_filled(self):
        """
        Return a value indicating whether a ``STOP-MARKET`` order filled.

        Returns
        -------
        bool

        """
        return self._event_success(self.prob_fill_on_stop)

    cpdef bint is_slipped(self):
        """
        Return a value indicating whether an order fill slipped.

        Returns
        -------
        bool

        """
        return self._event_success(self.prob_slippage)

    cdef bint _event_success(self, double probability):
        # Return a result indicating whether an event occurred based on the
        # given probability of the event occurring [0, 1].
        if probability == 0:
            return False
        elif probability == 1:
            return True
        else:
            return probability >= random.random()


cdef class LatencyModel:
    """
    Provides a latency model for simulated exchange message I/O.

    Parameters
    ----------
    base_latency_nanos : int, default 1_000_000_000
        The base latency (nanoseconds) for the model.
    insert_latency_nanos : int, default 0
        The order insert latency (nanoseconds) for the model.
    update_latency_nanos : int, default 0
        The order update latency (nanoseconds) for the model.
    cancel_latency_nanos : int, default 0
        The order cancel latency (nanoseconds) for the model.

    Raises
    ------
    ValueError
        If `base_latency_nanos` is negative (< 0).
    ValueError
        If `insert_latency_nanos` is negative (< 0).
    ValueError
        If `update_latency_nanos` is negative (< 0).
    ValueError
        If `cancel_latency_nanos` is negative (< 0).
    """

    def __init__(
        self,
        uint64_t base_latency_nanos = NANOSECONDS_IN_MILLISECOND,
        uint64_t insert_latency_nanos = 0,
        uint64_t update_latency_nanos = 0,
        uint64_t cancel_latency_nanos = 0,
    ):
        Condition.not_negative_int(base_latency_nanos, "base_latency_nanos")
        Condition.not_negative_int(insert_latency_nanos, "insert_latency_nanos")
        Condition.not_negative_int(update_latency_nanos, "update_latency_nanos")
        Condition.not_negative_int(cancel_latency_nanos, "cancel_latency_nanos")

        self.base_latency_nanos = base_latency_nanos
        self.insert_latency_nanos = base_latency_nanos + insert_latency_nanos
        self.update_latency_nanos = base_latency_nanos + update_latency_nanos
        self.cancel_latency_nanos = base_latency_nanos + cancel_latency_nanos


cdef class FeeModel:
    """
    Provides an abstract fee model for trades.
    """

    cpdef Money get_commission(
        self,
        Order order,
        Quantity fill_qty,
        Price fill_px,
        Instrument instrument,
    ):
        """
        Return the commission for a trade.

        Parameters
        ----------
        order : Order
            The order to calculate the commission for.
        fill_qty : Quantity
            The fill quantity of the order.
        fill_px : Price
            The fill price of the order.
        instrument : Instrument
            The instrument for the order.

        Returns
        -------
        Money

        """
        raise NotImplementedError("Method 'get_commission' must be implemented in a subclass.")


cdef class MakerTakerFeeModel(FeeModel):
    """
    Provide a fee model for trades based on a maker/taker fee schedule
    and notional value of the trade.

    """

    cpdef Money get_commission(
        self,
        Order order,
        Quantity fill_qty,
        Price fill_px,
        Instrument instrument,
    ):
        cdef double notional = instrument.notional_value(
            quantity=fill_qty,
            price=fill_px,
            use_quote_for_inverse=False,
        ).as_f64_c()

        cdef double commission_f64
        if order.liquidity_side == LiquiditySide.MAKER:
            commission_f64 = notional * float(instrument.maker_fee)
        elif order.liquidity_side == LiquiditySide.TAKER:
            commission_f64 = notional * float(instrument.taker_fee)
        else:
            raise ValueError(
                f"invalid `LiquiditySide`, was {liquidity_side_to_str(order.liquidity_side)}"
            )

        cdef Money commission
        if instrument.is_inverse:  # Not using quote for inverse (see above):
            commission = Money(commission_f64, instrument.base_currency)
        else:
            commission = Money(commission_f64, instrument.quote_currency)

        return commission


cdef class FixedFeeModel(FeeModel):
    """
    Provides a fixed fee model for trades.

    Parameters
    ----------
    commission : Money
        The fixed commission amount for trades.
    charge_commission_once : bool, default True
        Whether to charge the commission once per order or per fill.

    Raises
    ------
    ValueError
        If `commission` is not a positive amount.

    """

    def __init__(
        self,
        Money commission not None,
        bint charge_commission_once: bool = True,
    ):
        Condition.positive(commission, "commission")

        self._commission = commission
        self._zero_commission = Money(0, commission.currency)
        self._charge_commission_once = charge_commission_once

    cpdef Money get_commission(
        self,
        Order order,
        Quantity fill_qty,
        Price fill_px,
        Instrument instrument,
    ):
        if not self._charge_commission_once or order.filled_qty == 0:
            return self._commission
        else:
            return self._zero_commission


cdef class PerContractFeeModel(FeeModel):
    """
    Provides a fee model which charges a commission per contract traded.

    Parameters
    ----------
    commission : Money
        The commission amount per contract.

    Raises
    ------
    ValueError
        If `commission` is negative (< 0).

    """

    def __init__(self, Money commission not None):
        Condition.not_negative(commission, "commission")

        self._commission = commission

    cpdef Money get_commission(
        self,
        Order order,
        Quantity fill_qty,
        Price fill_px,
        Instrument instrument,
    ):
        return Money(self._commission * fill_qty, self._commission.currency)
