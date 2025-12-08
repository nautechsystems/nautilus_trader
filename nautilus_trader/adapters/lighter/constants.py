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

from __future__ import annotations

from typing import Final

from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


LIGHTER: Final[str] = "LIGHTER"
LIGHTER_VENUE: Final[Venue] = Venue(LIGHTER)
LIGHTER_CLIENT_ID: Final[ClientId] = ClientId(LIGHTER)

LIGHTER_MAINNET_HTTP_BASE: Final[str] = "https://mainnet.zklighter.elliot.ai"
LIGHTER_TESTNET_HTTP_BASE: Final[str] = "https://testnet.zklighter.elliot.ai"
LIGHTER_MAINNET_WS_BASE: Final[str] = "wss://mainnet.zklighter.elliot.ai/stream"
LIGHTER_TESTNET_WS_BASE: Final[str] = "wss://testnet.zklighter.elliot.ai/stream"

ENV_API_KEY_PRIVATE_KEY = "LIGHTER_API_KEY_PRIVATE_KEY"
ENV_API_KEY_PRIVATE_KEY_TESTNET = "LIGHTER_TESTNET_API_KEY_PRIVATE_KEY"
ENV_ACCOUNT_INDEX = "LIGHTER_ACCOUNT_INDEX"
ENV_ACCOUNT_INDEX_TESTNET = "LIGHTER_TESTNET_ACCOUNT_INDEX"
