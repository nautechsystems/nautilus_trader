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
Constants for the Gate.io adapter.
"""

from nautilus_trader.model.identifiers import Venue


# Venue
GATEIO_VENUE = Venue("GATEIO")

# URLs
GATEIO_HTTP_BASE_URL = "https://api.gateio.ws/api/v4"
GATEIO_WS_SPOT_URL = "wss://api.gateio.ws/ws/v4/"
GATEIO_WS_FUTURES_URL = "wss://fx-ws.gateio.ws/v4/ws/usdt"
GATEIO_WS_OPTIONS_URL = "wss://op-ws.gateio.ws/v4/ws/btc"

# Rate limits
GATEIO_RATE_LIMIT_DEFAULT = 200  # per 10 seconds
GATEIO_RATE_LIMIT_SPOT_ORDERS = 10  # per second
GATEIO_RATE_LIMIT_FUTURES_ORDERS = 100  # per second

# WebSocket
GATEIO_MAX_SUBSCRIPTIONS = 100
GATEIO_WS_PING_INTERVAL_SECS = 20
GATEIO_WS_TIMEOUT_SECS = 30
