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

import pandas as pd

from nautilus_trader.analysis.statistic import PortfolioStatistic
from nautilus_trader.analysis.statistics.loser_avg import AvgLoser
from nautilus_trader.analysis.statistics.winner_avg import AvgWinner


class Expectancy(PortfolioStatistic):
    """
    Calculates the expectancy from a realized PnLs series.
    """

    def calculate_from_realized_pnls(self, realized_pnls: pd.Series) -> Any | None:
        # Preconditions
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        # Calculate statistic
        avg_winner: float | None = AvgWinner().calculate_from_realized_pnls(realized_pnls)
        avg_loser: float | None = AvgLoser().calculate_from_realized_pnls(realized_pnls)
        if avg_winner is None or avg_loser is None:
            return 0.0

        pnls = realized_pnls.to_numpy()
        winners = pnls[pnls > 0.0]
        losers = pnls[pnls <= 0.0]
        win_rate = len(winners) / float(max(1, (len(winners) + len(losers))))
        loss_rate = 1.0 - win_rate

        return (avg_winner * win_rate) + (avg_loser * loss_rate)
