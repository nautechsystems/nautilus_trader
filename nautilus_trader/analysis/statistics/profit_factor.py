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

from typing import Any

import numpy as np
import pandas as pd

from nautilus_trader.analysis.statistic import PortfolioStatistic


class ProfitFactor(PortfolioStatistic):
    """
    Calculates the annualized profit factor or ratio (wins/loss).
    """

    def calculate_from_returns(self, returns: pd.Series) -> Any | None:
        # Preconditions
        if not self._check_valid_returns(returns):
            return np.nan

        positive_returns_sum = returns[returns >= 0].sum()
        negative_returns_sum = returns[returns < 0].sum()

        if negative_returns_sum == 0:
            return np.nan
        else:
            return abs(positive_returns_sum / negative_returns_sum)
