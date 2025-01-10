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

from enum import Enum


class DatabentoSchema(Enum):
    """
    Represents a Databento schema.
    """

    MBO = "mbo"
    MBP_1 = "mbp-1"
    MBP_10 = "mbp-10"
    BBO_1S = "bbo-1s"
    BBO_1M = "bbo-1m"
    TBBO = "tbbo"
    TRADES = "trades"
    OHLCV_1S = "ohlcv-1s"
    OHLCV_1M = "ohlcv-1m"
    OHLCV_1H = "ohlcv-1h"
    OHLCV_1D = "ohlcv-1d"
    OHLCV_EOD = "ohlcv-eod"
    DEFINITION = "definition"
    IMBALANCE = "imbalance"
    STATISTICS = "statistics"
    STATUS = "status"
