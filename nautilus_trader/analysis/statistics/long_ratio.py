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

from typing import Any, List, Optional

from nautilus_trader.analysis.statistic import PortfolioStatistic
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.position import Position


class LongRatio(PortfolioStatistic):
    """
    Calculates the ratio of long (to short) positions.

    Parameters
    ----------
    precision : int, default 2
        The decimal precision for the output.
    """

    def __init__(self, precision: int = 2):
        self.precision = precision

    def calculate_from_positions(self, positions: List[Position]) -> Optional[Any]:
        # Preconditions
        if not positions:
            return None

        # Calculate statistic
        longs = [p for p in positions if p.entry == OrderSide.BUY]
        value = len(longs) / len(positions)

        return f"{value:.{self.precision}f}"
