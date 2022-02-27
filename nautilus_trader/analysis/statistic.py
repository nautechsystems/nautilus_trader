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

import re
from typing import Any, List, Optional

import pandas as pd

from nautilus_trader.model.currency import Currency
from nautilus_trader.model.position import Position


class PerformanceStatistic:
    """
    The abstract base class for all backtest performance statistics.

    """

    @classmethod
    def name(cls) -> str:
        """
        Return the name for the statistic.

        Returns
        -------
        str

        """
        klass = type(cls).__name__
        matches = re.finditer(".+?(?:(?<=[a-z])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])|$)", klass)
        return " ".join([m.group(0) for m in matches])

    @classmethod
    def fully_qualified_name(cls) -> str:
        """
        Return the fully qualified name for the statistic object.

        Returns
        -------
        str

        References
        ----------
        https://www.python.org/dev/peps/pep-3155/

        """
        return cls.__module__ + "." + cls.__qualname__

    @staticmethod
    def format_stat(stat: Any) -> str:
        """
        Return the statistic value as well formatted string for display.

        Parameters
        ----------
        stat : Any
            The statistic output to format.

        Returns
        -------
        str

        """
        # Override in implementation
        return str(stat)

    @staticmethod
    def format_stat_with_currency(stat: Any, currency: Currency) -> str:
        """
        Return the statistic value as well formatted string for display.

        Parameters
        ----------
        stat : Any
            The statistic output to format.
        currency : Currency, optional
            The currency related to the statistic.

        Returns
        -------
        str

        """
        # Override in implementation
        if currency:
            pass
        return str(stat)

    @staticmethod
    def calculate_from_positions(positions: List[Position]) -> Optional[Any]:
        """
        Add a list of positions for the calculation.

        Parameters
        ----------
        positions : List[Position]
            The positions for the calculation.

        Returns
        -------
        Any or None

        """
        pass  # Override in implementation

    @staticmethod
    def calculate_from_realized_pnls(
        currency: Currency,
        realized_pnls: pd.Series[float],
    ) -> Optional[Any]:
        """
        Calculate the statistic from the given realized PnLs.

        Parameters
        ----------
        currency : Currency
            The currency for the calculation.
        realized_pnls : pd.Series[float]
            The PnLs for the calculation.

        Returns
        -------
        Any or None

        """
        pass  # Override in implementation

    @staticmethod
    def calculate_from_returns(returns: pd.Series[float]) -> Optional[Any]:
        """
        Add a returns' series for the calculation.

        Parameters
        ----------
        returns : pd.Series[float]
            The returns for the calculation.

        Returns
        -------
        Any or None

        """
        pass  # Override in implementation
