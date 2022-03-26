# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Optional

from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.ma_factory import MovingAverageType

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data.bar cimport Bar

from talib import abstract
from numpy import ndarray, array, nan

cdef class TaLib(Indicator):
    """
    An indicator which wraps TALIB-python.

    Parameters
    ----------
    indicator_type : str
        The string name of the indicator to wrap.
    price_types: list
        A list of prices for the indactor.
        eg: ['close', 'low', 'high']
    params : dict
        All the required indicator parameters (cannot be None).
    """

    def __init__(
        self,
        str indicator_type,
        list price_types,
        dict params,
        int lookback = 0,
    ):

        _params = [
            indicator_type,
            params
        ]
        super().__init__(params=_params)

        self.indicator_function = abstract.Function(indicator_type)
        self.indicator_params = params
        self.price_types = price_types
        self.params = params
        self.lookback = lookback

        self._high = deque(maxlen=params['timeperiod']+lookback)
        self._low = deque(maxlen=params['timeperiod']+lookback)
        self._close = deque(maxlen=params['timeperiod']+lookback)
        self._open = deque(maxlen=params['timeperiod']+lookback)
        self._volume = deque(maxlen=params['timeperiod']+lookback)

        self.value = None

    cpdef void handle_bar(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.high.as_double(), bar.low.as_double(), bar.close.as_double(),
                        bar.open.as_double(), bar.volume.as_double())

    cdef void _unpack_params(self, double high, double low, double close, double _open, double volume):
        values = self.price_types

        if 'high' in values:
            self._high.append(high)
        if 'low' in values:
            self._low.append(low)
        if 'close' in values:
            self._close.append(close)
        if 'open' in values:
            self._open.append(_open)
        if 'volume' in values:
            self.volume.append(volume)


    cpdef void update_raw(
        self,
        double high,
        double low,
        double close,
        double _open,
        double volume,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """
        # Calculate the indicator values
        self._unpack_params(high, low, close, _open, volume)
        prices = {'high': array(self._high), 'low': array(self._low), 'close': array(self._close), 'open': array(self._open), 'volume': array(self.volume)}
        self.value = self.indicator_function(prices, **self.indicator_params)

        self._check_initialized()

    cdef void _check_initialized(self) except *:
        """
        Initialization logic.
        """
        if not self.initialized:
            self._set_has_inputs(True)

            if self.value[-1] != nan:
                self._set_initialized(True)

    cpdef void _reset(self) except *:
        self.value = None
