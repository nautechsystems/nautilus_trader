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

from typing import Final

from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


OKX: Final[str] = "OKX"
OKX_VENUE: Final[Venue] = Venue(OKX)
OKX_CLIENT_ID: Final[ClientId] = ClientId(OKX)

# OKX error codes that should trigger retries
# Based on OKX API documentation: https://www.okx.com/docs-v5/en/#error-codes
# Only retry on temporary network/system issues
OKX_RETRY_ERROR_CODES: Final[set[str]] = {
    # Temporary system errors
    "50001",  # Service temporarily unavailable
    "50004",  # API endpoint request timeout (does not mean that the request was successful or failed, please check the request result)
    "50005",  # API is offline or unavailable
    "50013",  # System busy, please try again later
    "50026",  # System error, please try again later
    # Rate limit errors (temporary)
    "50011",  # Request too frequent
    "50113",  # API requests exceed the limit
    # WebSocket connection issues (temporary)
    "60001",  # OK not received in time
    "60005",  # Connection closed as there was no data transmission in the last 30 seconds
}
