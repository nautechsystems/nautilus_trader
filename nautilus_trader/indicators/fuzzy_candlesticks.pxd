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

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.indicators.fuzzy_enums.candle_body cimport CandleBodySize
from nautilus_trader.indicators.fuzzy_enums.candle_direction cimport CandleDirection
from nautilus_trader.indicators.fuzzy_enums.candle_size cimport CandleSize
from nautilus_trader.indicators.fuzzy_enums.candle_wick cimport CandleWickSize
from nautilus_trader.model.data cimport Bar


cdef class FuzzyCandle:
    cdef readonly CandleDirection direction
    """The candles close direction.\n\n:returns: `CandleDirection`"""
    cdef readonly CandleSize size
    """The candles fuzzy overall size.\n\n:returns: `CandleSize`"""
    cdef readonly CandleBodySize body_size
    """The candles fuzzy body size.\n\n:returns: `CandleBodySize`"""
    cdef readonly CandleWickSize upper_wick_size
    """The candles fuzzy upper wick size.\n\n:returns: `CandleWickSize`"""
    cdef readonly CandleWickSize lower_wick_size
    """The candles fuzzy lower wick size.\n\n:returns: `CandleWickSize`"""


cdef class FuzzyCandlesticks(Indicator):
    cdef double _threshold1
    cdef double _threshold2
    cdef double _threshold3
    cdef double _threshold4
    cdef object _lengths
    cdef object _body_percents
    cdef object _upper_wick_percents
    cdef object _lower_wick_percents
    cdef double _last_open
    cdef double _last_high
    cdef double _last_low
    cdef double _last_close

    cdef readonly int period
    """The window period.\n\n:returns: `int`"""
    cdef readonly list vector
    """The fuzzy candle represented as a vector of ints.\n\n:returns: `list[int]`"""
    cdef readonly FuzzyCandle value
    """The last fuzzy candle.\n\n:returns: `FuzzyCandle`"""

    cpdef void handle_bar(self, Bar bar)
    cpdef void update_raw(
        self,
        double open,
        double high,
        double low,
        double close,
    )

    cdef CandleDirection _fuzzify_direction(self, double open, double close)
    cdef CandleSize _fuzzify_size(self, double length, double mean_length, double sd_lengths)
    cdef CandleBodySize _fuzzify_body_size(self, double body_percent, double mean_body_percent, double sd_body_percents)
    cdef CandleWickSize _fuzzify_wick_size(self, double wick_percent, double mean_wick_percent, double sd_wick_percents)
