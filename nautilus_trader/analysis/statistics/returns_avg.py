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


class ReturnsAverage(PortfolioStatistic):
    """
    Calculates the average return.
    """

    @property
    def name(self) -> str:
        return "Average (Return)"

    def calculate_from_returns(self, returns: pd.Series) -> Any | None:
        # Preconditions
        if not self._check_valid_returns(returns):
            return np.nan

        return returns[returns != 0].dropna().mean()
