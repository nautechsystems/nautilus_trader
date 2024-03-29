# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Final

from nautilus_trader.model.identifiers import Venue


BYBIT_VENUE: Final[Venue] = Venue("BYBIT")

BYBIT_MINUTE_INTERVALS: Final[tuple[int, ...]] = (1, 3, 5, 15, 30, 60, 120, 240, 360, 720)
BYBIT_HOUR_INTERVALS: Final[tuple[int, ...]] = (1, 2, 4, 6, 12)

BYBIT_SPOT_DEPTHS: Final[tuple[int, ...]] = (1, 50, 200)
BYBIT_LINEAR_DEPTHS: Final[tuple[int, ...]] = (1, 50, 200, 500)
BYBIT_OPTION_DEPTHS: Final[tuple[int, ...]] = (25, 100)
