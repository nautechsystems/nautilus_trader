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

from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookSnapshot
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuotes
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTickSizeChange
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTrade
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserOrder
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserTrade


MARKET_WS_MESSAGE: Final = list[
    list[PolymarketBookSnapshot]
    | PolymarketBookSnapshot
    | PolymarketQuotes
    | PolymarketTrade
    | PolymarketTickSizeChange
]
USER_WS_MESSAGE: Final = list[PolymarketUserOrder | PolymarketUserTrade]
