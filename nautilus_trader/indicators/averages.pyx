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

from collections import deque

import numpy as np

cimport numpy as np
from libc.math cimport pow

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.stats cimport fast_mean
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.indicators.momentum cimport EfficiencyRatio
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.objects cimport Price


cdef class MovingAverage(Indicator):
    """
    The base class for all moving average type indicators.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    params : list
        The initialization parameters for the indicator.
    price_type : PriceType, optional
        The specified price type for extracting values from quotes.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        int period,
        list params not None,
        PriceType price_type,
    ):
        Condition.positive_int(period, "period")
        super().__init__(params)

        self.period = period
        self.price_type = price_type
        self.value = 0
        self.count = 0

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        raise NotImplementedError("method `update_raw` must be implemented in the subclass")  # pragma: no cover

    cpdef void _increment_count(self):
        self.count += 1

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self.count >= self.period:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._reset_ma()
        self.count = 0
        self.value = 0

    cpdef void _reset_ma(self):
        pass  # Optionally override if additional values to reset


cdef class SimpleMovingAverage(MovingAverage):
    """
    An indicator which calculates a simple moving average across a rolling window.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(self, int period, PriceType price_type=PriceType.LAST):
        Condition.positive_int(period, "period")
        super().__init__(period, params=[period], price_type=price_type)

        self._inputs = deque(maxlen=period)
        self.value = 0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self.price_type)
        self.update_raw(Price.raw_to_f64_c(price._mem.raw))

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(Price.raw_to_f64_c(tick._mem.price.raw))

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        self._inputs.append(value)

        self.value = fast_mean(np.asarray(self._inputs, dtype=np.float64))
        self._increment_count()

    cpdef void _reset_ma(self):
        self._inputs.clear()


cdef class ExponentialMovingAverage(MovingAverage):
    """
    An indicator which calculates an exponential moving average across a
    rolling window.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(self, int period, PriceType price_type=PriceType.LAST):
        Condition.positive_int(period, "period")
        super().__init__(period, params=[period], price_type=price_type)

        self.alpha = 2.0 / (period + 1.0)
        self.value = 0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self.price_type)
        self.update_raw(Price.raw_to_f64_c(price._mem.raw))

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(Price.raw_to_f64_c(tick._mem.price.raw))

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        # Check if this is the initial input
        if not self.has_inputs:
            self.value = value

        self.value = self.alpha * value + ((1.0 - self.alpha) * self.value)
        self._increment_count()


cdef class DoubleExponentialMovingAverage(MovingAverage):
    """
    The Double Exponential Moving Average attempts to a smoother average with less
    lag than the normal Exponential Moving Average (EMA).

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(self, int period, PriceType price_type=PriceType.LAST):
        Condition.positive_int(period, "period")
        super().__init__(period, params=[period], price_type=price_type)

        self._ma1 = ExponentialMovingAverage(period)
        self._ma2 = ExponentialMovingAverage(period)

        self.value = 0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self.price_type)
        self.update_raw(Price.raw_to_f64_c(price._mem.raw))

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(Price.raw_to_f64_c(tick._mem.price.raw))

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        self._ma1.update_raw(value)
        self._ma2.update_raw(self._ma1.value)

        self.value = 2.0 * self._ma1.value - self._ma2.value

        if not self.initialized:
            self._set_has_inputs(True)
            if self._ma2.initialized:
                self._set_initialized(True)

    cpdef void _reset_ma(self):
        self._ma1.reset()
        self._ma2.reset()
        self.value = 0


cdef class WeightedMovingAverage(MovingAverage):
    """
    An indicator which calculates a weighted moving average across a rolling window.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    weights : iterable
        The weights for the moving average calculation (if not ``None`` then = period).
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(
        self,
        int period,
        weights = None,
        PriceType price_type = PriceType.LAST,
    ):
        Condition.positive_int(period, "period")
        if weights is not None:
            if not isinstance(weights, np.ndarray):
                # convert weights to the np.ndarray if it's possible
                weights = np.asarray(weights, dtype=np.float64)
                # to avoid the case when weights = [[1.0, 2.0, 3,0], [1.0, 2.0, 3,0]] ...
            if weights.ndim != 1:
                raise ValueError("weights must be iterable with ndim == 1.")
            else:
                Condition.is_true(weights.dtype == np.float64, "weights ndarray.dtype must be 'float64'")
                Condition.is_true(weights.ndim == 1, "weights ndarray.ndim must be 1")
            Condition.equal(len(weights), period, "len(weights)", "period")
            eps = np.finfo(np.float64).eps
            Condition.is_true(eps < weights.sum(), f"sum of weights must be positive > {eps}")
        super().__init__(period, params=[period, weights], price_type=price_type)

        self._inputs = deque(maxlen=period)
        self.weights = weights
        self.value = 0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self.price_type)
        self.update_raw(Price.raw_to_f64_c(price._mem.raw))

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(Price.raw_to_f64_c(tick._mem.price.raw))

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        self._inputs.append(value)

        if self.initialized or self.weights is None:
            self.value = np.average(self._inputs, weights=self.weights, axis=0)
        else:
            self.value = np.average(self._inputs, weights=self.weights[-len(self._inputs):], axis=0)

        self._increment_count()

    cpdef void _reset_ma(self):
        self._inputs.clear()


cdef class HullMovingAverage(MovingAverage):
    """
    An indicator which calculates a Hull Moving Average (HMA) across a rolling
    window. The HMA, developed by Alan Hull, is an extremely fast and smooth
    moving average.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(self, int period, PriceType price_type=PriceType.LAST):
        Condition.positive_int(period, "period")
        super().__init__(period, params=[period], price_type=price_type)

        cdef int period_halved = int(self.period / 2)
        cdef int period_sqrt = int(np.sqrt(self.period))

        self._w1 = self._get_weights(period_halved)
        self._w2 = self._get_weights(period)
        self._w3 = self._get_weights(period_sqrt)

        self._ma1 = WeightedMovingAverage(period_halved, weights=self._w1)
        self._ma2 = WeightedMovingAverage(period, weights=self._w2)
        self._ma3 = WeightedMovingAverage(period_sqrt, weights=self._w3)

        self.value = 0

    cdef np.ndarray _get_weights(self, int size):
        cdef np.ndarray w = np.arange(1, size + 1, dtype=np.float64)
        w /= w.sum()
        return w

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self.price_type)
        self.update_raw(Price.raw_to_f64_c(price._mem.raw))

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(Price.raw_to_f64_c(tick._mem.price.raw))

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        self._ma1.update_raw(value)
        self._ma2.update_raw(value)
        self._ma3.update_raw(self._ma1.value * 2.0 - self._ma2.value)

        self.value = self._ma3.value
        self._increment_count()

    cpdef void _reset_ma(self):
        self._ma1.reset()
        self._ma2.reset()
        self._ma3.reset()


cdef class AdaptiveMovingAverage(MovingAverage):
    """
    An indicator which calculates an adaptive moving average (AMA) across a
    rolling window. Developed by Perry Kaufman, the AMA is a moving average
    designed to account for market noise and volatility. The AMA will closely
    follow prices when the price swings are relatively small and the noise is
    low. The AMA will increase lag when the price swings increase.

    Parameters
    ----------
    period_er : int
        The period for the internal `EfficiencyRatio` indicator (> 0).
    period_alpha_fast : int
        The period for the fast smoothing constant (> 0).
    period_alpha_slow : int
        The period for the slow smoothing constant (> 0 < alpha_fast).
    price_type : PriceType
        The specified price type for extracting values from quotes.
    """

    def __init__(
        self,
        int period_er,
        int period_alpha_fast,
        int period_alpha_slow,
        PriceType price_type=PriceType.LAST,
    ):
        Condition.positive_int(period_er, "period_er")
        Condition.positive_int(period_alpha_fast, "period_alpha_fast")
        Condition.positive_int(period_alpha_slow, "period_alpha_slow")
        Condition.is_true(period_alpha_slow > period_alpha_fast, "period_alpha_slow was <= period_alpha_fast")

        params = [
            period_er,
            period_alpha_fast,
            period_alpha_slow
        ]
        super().__init__(period_er, params=params, price_type=price_type)

        self.period_er = period_er
        self.period_alpha_fast = period_alpha_fast
        self.period_alpha_slow = period_alpha_slow
        self.alpha_fast = 2.0 / (float(period_alpha_fast) + 1.0)
        self.alpha_slow = 2.0 / (float(period_alpha_slow) + 1.0)
        self.alpha_diff = self.alpha_fast - self.alpha_slow
        self._efficiency_ratio = EfficiencyRatio(self.period_er)
        self._prior_value = 0
        self.value = 0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self.price_type)
        self.update_raw(Price.raw_to_f64_c(price._mem.raw))

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(Price.raw_to_f64_c(tick._mem.price.raw))

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        # Check if this is the initial input (then initialize variables)
        if not self.has_inputs:
            self.value = value

        self._efficiency_ratio.update_raw(value)
        self._prior_value = self.value

        # Calculate smoothing constant (sc)
        cdef double sc = pow(self._efficiency_ratio.value * self.alpha_diff + self.alpha_slow, 2)

        # Calculate AMA
        self.value = self._prior_value + sc * (value - self._prior_value)

        self._increment_count()

    cpdef void _reset_ma(self):
        self._efficiency_ratio.reset()
        self._prior_value = 0


cdef class WilderMovingAverage(MovingAverage):
    """
    The Wilder's Moving Average is simply an Exponential Moving Average (EMA) with
    a modified alpha = 1 / period.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(self, int period, PriceType price_type=PriceType.LAST):
        Condition.positive_int(period, "period")
        super().__init__(period, params=[period], price_type=price_type)

        self.alpha = 1.0 / period
        self.value = 0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self.price_type)
        self.update_raw(Price.raw_to_f64_c(price._mem.raw))

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(Price.raw_to_f64_c(tick._mem.price.raw))

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        # Check if this is the initial input
        if not self.has_inputs:
            self.value = value

        self.value = self.alpha * value + ((1.0 - self.alpha) * self.value)
        self._increment_count()


cdef class VariableIndexDynamicAverage(MovingAverage):
    """
    Variable Index Dynamic Average (VIDYA) was developed by Tushar Chande. It is
    similar to an Exponential Moving Average, but it has a dynamically adjusted
    lookback period dependent on relative price volatility as measured by Chande
    Momentum Oscillator (CMO). When volatility is high, VIDYA reacts faster to
    price changes. It is often used as moving average or trend identifier.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    price_type : PriceType
        The specified price type for extracting values from quotes.
    cmo_ma_type : int
        The moving average type for CMO indicator.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
        If `cmo_ma_type` is ``VARIABLE_INDEX_DYNAMIC``.
    """

    def __init__(
        self,
        int period,
        PriceType price_type=PriceType.LAST,
        MovingAverageType cmo_ma_type=MovingAverageType.SIMPLE,
    ):
        Condition.positive_int(period, "period")
        Condition.is_true(cmo_ma_type != MovingAverageType.VARIABLE_INDEX_DYNAMIC, "cmo_ma_type was invalid (VARIABLE_INDEX_DYNAMIC)")
        super().__init__(period, params=[period], price_type=price_type)

        from nautilus_trader.indicators.momentum import ChandeMomentumOscillator
        self.cmo = ChandeMomentumOscillator(period, cmo_ma_type)
        self.cmo_pct = 0
        self.alpha = 2.0 / (period + 1.0)
        self.value = 0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self.price_type)
        self.update_raw(Price.raw_to_f64_c(price._mem.raw))

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(Price.raw_to_f64_c(tick._mem.price.raw))

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        # Check if this is the initial input
        self.cmo.update_raw(value)
        self.cmo_pct = abs(self.cmo.value / 100)

        if self.initialized:
            self.value = self.alpha * self.cmo_pct * value + (1.0 - self.alpha *  self.cmo_pct) * self.value

        # Initialization logic
        if not self.initialized:
            if self.cmo.initialized:
                self._set_initialized(True)

        self._increment_count()


cdef class MovingAverageFactory:
    """
    Provides a factory to construct different moving average indicators.
    """

    @staticmethod
    def create(
        int period,
        ma_type: MovingAverageType,
        **kwargs,
    ) -> MovingAverage:
        """
        Create a moving average indicator corresponding to the given ma_type.

        Parameters
        ----------
        period : int
            The period of the moving average (> 0).
        ma_type : MovingAverageType
            The moving average type.

        Returns
        -------
        MovingAverage

        Raises
        ------
        ValueError
            If `period` is not positive (> 0).

        """
        Condition.positive_int(period, "period")

        if ma_type == MovingAverageType.SIMPLE:
            return SimpleMovingAverage(period)

        elif ma_type == MovingAverageType.EXPONENTIAL:
            return ExponentialMovingAverage(period)

        elif ma_type == MovingAverageType.WEIGHTED:
            return WeightedMovingAverage(period, **kwargs)

        elif ma_type == MovingAverageType.HULL:
            return HullMovingAverage(period)

        elif ma_type == MovingAverageType.WILDER:
            return WilderMovingAverage(period)

        elif ma_type == MovingAverageType.DOUBLE_EXPONENTIAL:
            return DoubleExponentialMovingAverage(period)

        elif ma_type == MovingAverageType.VARIABLE_INDEX_DYNAMIC:
            return VariableIndexDynamicAverage(period, **kwargs)
