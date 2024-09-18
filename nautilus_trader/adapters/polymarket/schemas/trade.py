# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec

from nautilus_trader.adapters.polymarket.common.enums import PolymarketLiquiditySide
from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderSide
from nautilus_trader.adapters.polymarket.common.parsing import parse_order_side
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketMakerOrder
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import BinaryOption


class PolymarketTradeReport(msgspec.Struct, frozen=True):
    """
    Represents a Polymarket trade report.

    References
    ----------
    https://docs.polymarket.com/#get-trades

    """

    id: str  # Trade ID
    taker_order_id: str
    market: str
    asset_id: str
    side: PolymarketOrderSide
    size: str
    fee_rate_bps: str
    price: str
    status: str
    match_time: str
    last_update: str
    outcome: str
    bucket_index: int
    owner: str
    maker_address: str
    transaction_hash: str
    maker_orders: list[PolymarketMakerOrder]
    trader_side: PolymarketLiquiditySide

    def liqudity_side(self) -> LiquiditySide:
        if self.trader_side == PolymarketLiquiditySide.MAKER:
            return LiquiditySide.MAKER
        else:
            return LiquiditySide.TAKER

    def venue_order_id(self) -> VenueOrderId:
        if self.trader_side == PolymarketLiquiditySide.MAKER:
            return VenueOrderId(self.maker_orders[-1].order_id)
        else:
            return VenueOrderId(self.taker_order_id)

    def parse_to_fill_report(
        self,
        account_id: AccountId,
        instrument: BinaryOption,
        client_order_id: ClientOrderId | None,
        ts_init: int,
    ) -> FillReport:
        return FillReport(
            account_id=account_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            venue_order_id=self.venue_order_id(),
            trade_id=TradeId(self.id),
            order_side=parse_order_side(self.side),
            last_qty=instrument.make_qty(float(self.size)),
            last_px=instrument.make_price(float(self.price)),
            liquidity_side=self.liqudity_side(),
            report_id=UUID4(),
            ts_event=millis_to_nanos(int(self.match_time)),
            ts_init=ts_init,
        )
