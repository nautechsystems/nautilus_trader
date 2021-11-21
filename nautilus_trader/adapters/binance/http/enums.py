# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
#
#  Heavily refactored from MIT licensed github.com/binance/binance-connector-python
#  Original author: Jeremy https://github.com/2pd
# -------------------------------------------------------------------------------------------------

from enum import Enum
from enum import auto


class NewOrderRespType(Enum):
    """
    Represents a `Binance` newOrderRespType.
    """

    ACK = "ACK"
    RESULT = "RESULT"
    FULL = "FULL"


class AutoName(Enum):
    """
    Represents a `Binance` auto name.
    """

    def _generate_next_value_(name, start, count, last_values):
        return name


class TransferType(AutoName):
    """
    Represents a `Binance` transfer type.
    """

    MAIN_C2C = auto()
    MAIN_UMFUTURE = auto()
    MAIN_CMFUTURE = auto()
    MAIN_MARGIN = auto()
    MAIN_MINING = auto()
    C2C_MAIN = auto()
    C2C_UMFUTURE = auto()
    C2C_MINING = auto()
    C2C_MARGIN = auto()
    UMFUTURE_MAIN = auto()
    UMFUTURE_C2C = auto()
    UMFUTURE_MARGIN = auto()
    CMFUTURE_MAIN = auto()
    CMFUTURE_MARGIN = auto()
    MARGIN_MAIN = auto()
    MARGIN_UMFUTURE = auto()
    MARGIN_CMFUTURE = auto()
    MARGIN_MINING = auto()
    MARGIN_C2C = auto()
    MINING_MAIN = auto()
    MINING_UMFUTURE = auto()
    MINING_C2C = auto()
    MINING_MARGIN = auto()
    MAIN_PAY = auto()
    PAY_MAIN = auto()
    ISOLATEDMARGIN_MARGIN = auto()
    MARGIN_ISOLATEDMARGIN = auto()
    ISOLATEDMARGIN_ISOLATEDMARGIN = auto()
