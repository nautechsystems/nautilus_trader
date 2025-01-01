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

import re
from typing import Any

import pandas as pd

from nautilus_trader.model.orders import Order
from nautilus_trader.model.position import Position


class PortfolioStatistic:
    """
    The base class for all portfolio performance statistics.

    Notes
    -----
    The return value should be a JSON serializable primitive.

    """

    @classmethod
    def fully_qualified_name(cls) -> str:
        """
        Return the fully qualified name for the `PortfolioStatistic` class.

        Returns
        -------
        str

        References
        ----------
        https://www.python.org/dev/peps/pep-3155/

        """
        return cls.__module__ + ":" + cls.__qualname__

    @property
    def name(self) -> str:
        """
        Return the name for the statistic.

        Returns
        -------
        str

        """
        klass = type(self).__name__
        matches = re.finditer(".+?(?:(?<=[a-z])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])|$)", klass)
        return " ".join([m.group(0) for m in matches])

    def calculate_from_returns(self, returns: pd.Series) -> Any | None:
        """
        Calculate the statistic value from the given raw returns.

        Parameters
        ----------
        returns : pd.Series
            The returns to use for the calculation.

        Returns
        -------
        Any or ``None``
            A JSON serializable primitive.

        """
        # Override in implementation

    def calculate_from_realized_pnls(self, realized_pnls: pd.Series) -> Any | None:
        """
        Calculate the statistic value from the given raw realized PnLs.

        Parameters
        ----------
        realized_pnls : pd.Series
            The raw PnLs for the calculation.

        Returns
        -------
        Any or ``None``
            A JSON serializable primitive.

        """
        # Override in implementation

    def calculate_from_orders(self, orders: list[Order]) -> Any | None:
        """
        Calculate the statistic value from the given orders.

        Parameters
        ----------
        orders : list[Order]
            The positions to use for the calculation.

        Returns
        -------
        Any or ``None``
            A JSON serializable primitive.

        """
        # Override in implementation

    def calculate_from_positions(self, positions: list[Position]) -> Any | None:
        """
        Calculate the statistic value from the given positions.

        Parameters
        ----------
        positions : list[Position]
            The positions to use for the calculation.

        Returns
        -------
        Any or ``None``
            A JSON serializable primitive.

        """
        # Override in implementation

    def _check_valid_returns(self, returns: pd.Series) -> bool:
        if returns is None or returns.empty or returns.isna().all():
            return False
        else:
            return True

    def _downsample_to_daily_bins(self, returns: pd.Series) -> pd.Series:
        return returns.dropna().resample("1D").sum()
