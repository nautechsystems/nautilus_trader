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

"""Provides the C Enums as Python Enums for external use."""

from nautilus_trader.indicators.fuzzy_enums.candle_body import CandleBodySize
from nautilus_trader.indicators.fuzzy_enums.candle_direction import CandleDirection
from nautilus_trader.indicators.fuzzy_enums.candle_size import CandleSize
from nautilus_trader.indicators.fuzzy_enums.candle_wick import CandleWickSize


__all__ = [
    "CandleBodySize",
    "CandleDirection",
    "CandleSize",
    "CandleWickSize",
]
