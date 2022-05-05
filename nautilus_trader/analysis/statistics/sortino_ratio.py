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

from typing import Any, Optional

import numpy as np
import pandas as pd

from nautilus_trader.analysis.statistic import PortfolioStatistic


class SortinoRatio(PortfolioStatistic):
    """
    Calculates the annualized Sortino Ratio from returns.

    The returns will be downsampled into daily bins.

    Parameters
    ----------
    period : int, default 252
        The trading period in days.
    """

    def __init__(self, period: int = 252):
        self.period = period

    @property
    def name(self) -> str:
        return f"Sortino Ratio ({self.period} days)"

    def calculate_from_returns(self, returns: pd.Series) -> Optional[Any]:
        # Preconditions
        if not self._check_valid_returns(returns):
            return np.nan

        returns = self._downsample_to_daily_bins(returns)

        downside = np.sqrt((returns[returns < 0] ** 2).sum() / len(returns))
        if downside == 0:
            return np.nan

        res = returns.mean() / downside

        return res * np.sqrt(self.period)
