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

from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType


cdef class BarData:
    """
    Represents bar data being a bar type and bar.
    """

    def __init__(self, BarType bar_type not None, Bar bar not None):
        """
        Initialize a new instance of the `BarData` class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the data.
        bar : Bar
            The bar data.

        """
        self.bar_type = bar_type
        self.bar = bar

    def __repr__(self) -> str:
        return f"{type(self).__name__}(bar_type={self.bar_type}, bar={self.bar})"


cdef class BarDataBlock:
    """
    Represents a block of bar data being a bar type and list of bars.
    """

    def __init__(self, BarType bar_type not None, list bars not None):
        """
        Initialize a new instance of the `BarData` class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the data.
        bars : list[Bar]
            The bar data.

        """
        self.bar_type = bar_type
        self.bars = bars

    def __repr__(self) -> str:
        return f"{type(self).__name__}(bar_type={self.bar_type}, {len(self.bars)} bars)"


cdef class QuoteTickDataBlock:
    """
    Represents a block of quote tick data.
    """

    def __init__(self, list ticks not None):
        """
        Initialize a new instance of the `QuoteTickDataBlock` class.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The tick data.

        """
        self.ticks = ticks

    def __repr__(self) -> str:
        return f"{type(self).__name__}({len(self.ticks)} ticks)"


cdef class TradeTickDataBlock:
    """
    Represents a block of trade tick data.
    """

    def __init__(self, list ticks not None):
        """
        Initialize a new instance of the `TradeTickDataBlock` class.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The tick data.

        """
        self.ticks = ticks

    def __repr__(self) -> str:
        return f"{type(self).__name__}({len(self.ticks)} ticks)"


cdef class InstrumentDataBlock:
    """
    Represents a block of instrument data.
    """

    def __init__(self, list instruments not None):
        """
        Initialize a new instance of the `InstrumentDataBlock` class.

        Parameters
        ----------
        instruments : list[Instrument]
            The instrument data.

        """
        self.instruments = instruments

    def __repr__(self) -> str:
        return f"{type(self).__name__}({len(self.instruments)} instruments)"
