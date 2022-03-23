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

from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.ma_factory import MovingAverageType

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data.bar cimport Bar

from talib import abstract
from numpy import ndarray

cdef class ta_lib(Indicator):
    """
    An indicator which wraps TALIB.

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
        abstract not None: indicator_type
        list not None: price_types
        dict not None: params
    ):

        _params = [
            indicator_type,
            params
        ]
        super().__init__(params=_params)

        self.indicator_function = abstract.Function(indicator_type)
        self.indicator_params = params
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

        self.update_raw(bar.high.as_double(), bar.low.as_double(), bar.close.as_double())

    cdef list _unpack_params(self, double high, double low, double close):
        values = self.indicator_params.values()
        price_types = []
        if 'high' in values:
            price_types.append(high)
        if 'low' in values:
            price_types.append(low)
        if 'close' in values:
            price_types.append(close)
        return price_types

    cpdef void update_raw(
        self,
        double high,
        double low,
        double close,
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
        prices = self._unpack_params(high, low, close)
        self.value = self.indicator_function(**prices,**self.indicator_params)

        self._check_initialized()

    cdef void _check_initialized(self) except *:
        """
        Initialization logic.
        """
        if not self.initialized:
            self._set_has_inputs(True)
            if self.value:
                self._set_initialized(True)

    cpdef void _reset(self) except *:
        self.value = None
