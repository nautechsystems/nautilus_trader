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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType


cdef class BarData:
    """
    Represents bar data being a bar type and bar.
    """

    def __init__(
            self,
            BarType bar_type,
            Bar bar,
    ):
        """
        Initialize a new instance of the BarData class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the data.
        bar : Bar
            The bar data.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bar, "bar")

        self.bar_type = bar_type
        self.bar = bar


cdef class BarDataBlock:
    """
    Represents a block of bar data being a bar type and list of bars.
    """

    def __init__(
            self,
            BarType bar_type,
            list bars,
    ):
        """
        Initialize a new instance of the BarData class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the data.
        bars : list[Bar]
            The bar data.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bars, "bars")

        self.bar_type = bar_type
        self.bars = bars


cdef class QuoteTickDataBlock:
    """
    Represents a block of quote tick data.
    """

    def __init__(self, list ticks):
        """
        Initialize a new instance of the QuoteTickDataBlock class.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The tick data.

        """
        Condition.not_none(ticks, "ticks")

        self.ticks = ticks


cdef class TradeTickDataBlock:
    """
    Represents a block of trade tick data.
    """

    def __init__(self, list ticks):
        """
        Initialize a new instance of the TradeTickDataBlock class.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The tick data.

        """
        Condition.not_none(ticks, "ticks")

        self.ticks = ticks


cdef class InstrumentDataBlock:
    """
    Represents a block of instrument data.
    """

    def __init__(self, list instruments):
        """
        Initialize a new instance of the InstrumentDataBlock class.

        Parameters
        ----------
        instruments : list[Instrument]
            The instrument data.

        """
        Condition.not_none(instruments, "instruments")

        self.instruments = instruments
