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
"""
Define constants used in the dYdX adapter.
"""

from typing import Final

from nautilus_trader.model.identifiers import Venue


DYDX: Final[str] = "DYDX"
DYDX_VENUE: Final[Venue] = Venue(DYDX)

FEE_SCALING: Final[int] = 1_000_000
DEFAULT_CURRENCY: Final[str] = "USDC"

CURRENCY_MAP: Final[dict[str, str]] = {
    "USD": "USDC",
}

ACCOUNT_SEQUENCE_MISMATCH_ERROR_CODE = 32
DYDX_RETRY_ERRORS_GRPC: Final[list[int]] = [
    32,  # Account sequence mismatch
]
