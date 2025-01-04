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

from decimal import Decimal
from typing import Final

from nautilus_trader.adapters.binance.common.enums import BinanceErrorCode
from nautilus_trader.model.identifiers import Venue


BINANCE: Final[str] = "BINANCE"
BINANCE_VENUE: Final[Venue] = Venue(BINANCE)

BINANCE_MIN_CALLBACK_RATE: Final[Decimal] = Decimal("0.1")
BINANCE_MAX_CALLBACK_RATE: Final[Decimal] = Decimal("10.0")

# Set of Binance error codes for which Nautilus will attempt retries,
# potentially temporary conditions where a retry might make sense.
BINANCE_RETRY_ERRORS: set[BinanceErrorCode] = {
    BinanceErrorCode.DISCONNECTED,
    BinanceErrorCode.TOO_MANY_REQUESTS,  # Short retry delays may result in bans
    BinanceErrorCode.TIMEOUT,
    BinanceErrorCode.SERVER_BUSY,
    BinanceErrorCode.INVALID_TIMESTAMP,
    BinanceErrorCode.CANCEL_REJECTED,
    BinanceErrorCode.ME_RECVWINDOW_REJECT,
}
