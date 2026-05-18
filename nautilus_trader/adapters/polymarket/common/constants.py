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

from decimal import Decimal
from typing import Final

from nautilus_trader.adapters.polymarket.common.enums import PolymarketTradeStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


POLYMARKET: Final[str] = "POLYMARKET"
POLYMARKET_VENUE: Final[Venue] = Venue(POLYMARKET)
POLYMARKET_CLIENT_ID: Final[ClientId] = ClientId(POLYMARKET)

POLYMARKET_MAX_PRICE: Final[float] = 0.999
POLYMARKET_MIN_PRICE: Final[float] = 0.001
POLYMARKET_MAX_PRECISION_TAKER: Final[int] = 2
POLYMARKET_MAX_PRECISION_MAKER: Final[int] = 5

VALID_POLYMARKET_TIME_IN_FORCE: Final[set[TimeInForce]] = {
    TimeInForce.GTC,
    TimeInForce.GTD,
    TimeInForce.FOK,
    TimeInForce.IOC,
}

VALID_POLYMARKET_MARKET_TIME_IN_FORCE: Final[set[TimeInForce]] = {
    TimeInForce.FOK,
    TimeInForce.IOC,
}

POLYMARKET_INVALID_API_KEY: Final[str] = "Unauthorized/Invalid api key"
POLYMARKET_CANCEL_ALREADY_DONE: Final[str] = "already canceled or matched"
POLYMARKET_NAUTILUS_BUILDER_CODE: Final[str] = (
    "0x4f2c0bba608033563f74b82300e2ed59f54f8d0de08281031f03fb2c62819e63"
)

POLYMARKET_FINALIZED_TRADE_STATUSES: Final[tuple[PolymarketTradeStatus, ...]] = (
    PolymarketTradeStatus.MINED,
    PolymarketTradeStatus.CONFIRMED,
)

POLYMARKET_HTTP_RATE_LIMIT: Final[int] = 100  # requests per minute

# Minimum position size (in shares) reported in position status reports.
# Smaller positions are filtered as dust during reconciliation.
DUST_POSITION_THRESHOLD: Final[float] = 0.01

# Dust band (in shares) for fill quantity normalization. Set to one
# cent-share, matching Polymarket's CLOB tick quantization.
#
# Live-fill snapping is overfill-only: when the venue fill exceeds
# ``submitted_qty`` by less than ``DUST_SNAP_THRESHOLD``, the fill is snapped
# DOWN to ``submitted_qty``. Underfill is preserved on the per-fill path and
# resolved at terminal ``MATCHED`` status by the synthetic dust fill mechanism.
# ``OrderStatusReport.filled_qty`` snapping at terminal ``Filled`` status uses
# this same threshold in both directions.
#
# Two observed drift sources sit within this band:
#
# - CLOB cent-tick truncation (underfill, up to 0.01 shares).
# - V2 market-BUY USDC-scale truncation in ``adjust_market_buy_amount``
#   (overfill, microshares; largest reproduced production overage is 0.000066 shares).
#
# A diff at or above this threshold is left unsnapped and surfaces to the
# engine. See ``docs/integrations/polymarket.md`` (Fill quantity normalization).
DUST_SNAP_THRESHOLD: Final[float] = 0.01

# Decimal form of ``DUST_SNAP_THRESHOLD`` for Decimal arithmetic paths.
DUST_SNAP_THRESHOLD_DEC: Final[Decimal] = Decimal("0.01")
