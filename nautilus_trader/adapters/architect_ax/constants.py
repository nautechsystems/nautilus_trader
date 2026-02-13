# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


AX: Final[str] = "AX"
AX_VENUE: Final[Venue] = Venue(AX)
AX_CLIENT_ID: Final[ClientId] = ClientId(AX)

AX_SUPPORTED_ORDER_TYPES = (OrderType.MARKET, OrderType.LIMIT, OrderType.STOP_LIMIT)

AX_WS_ORDERS_SANDBOX_URL = "wss://gateway.sandbox.architect.exchange/orders/ws"
AX_WS_ORDERS_PRODUCTION_URL = "wss://gateway.architect.exchange/orders/ws"
