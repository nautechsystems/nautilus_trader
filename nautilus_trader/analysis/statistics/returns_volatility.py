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
import quantstats

class ReturnsVolatility(PortfolioStatistic):
    """
    Calculates the annualized volatility of returns.

    Parameters
    ----------
    period : int, default 252
        The trading period in days.
    """

    def __init__(self, period: int = 365):
        self.period = period

    @property
    def name(self) -> str:
        return f"Returns Volatility ({self.period} days)"

    def calculate_from_returns(self, returns: pd.Series) -> Optional[Any]:
        return returns.std() * np.sqrt(self.period)
