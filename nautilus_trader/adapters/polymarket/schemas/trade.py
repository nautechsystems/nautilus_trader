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
from typing import Any

import msgspec

from nautilus_trader.adapters.polymarket.common.enums import PolymarketLiquiditySide
from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderSide
from nautilus_trader.adapters.polymarket.common.parsing import parse_order_side
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketMakerOrder
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.stats import basis_points_as_percentage
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.model.currencies import USDC_POS
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.objects import Money


class PolymarketTradeReport(msgspec.Struct, frozen=True):
    """
    Represents a Polymarket trade report.

    References
    ----------
    https://docs.polymarket.com/#get-trades

    """

    id: str  # trade ID
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

    def to_dict(self) -> dict[str, Any]:
        return msgspec.json.decode(msgspec.json.encode(self))

    def get_maker_order(self, maker_address: str) -> PolymarketMakerOrder:
        for order in self.maker_orders:
            if order.maker_address == maker_address:
                return order

        raise ValueError("Invalid trade with no maker order owned my `maker_address`")

    def liquidity_side(self) -> LiquiditySide:
        if self.trader_side == PolymarketLiquiditySide.TAKER:
            return LiquiditySide.TAKER
        else:
            return LiquiditySide.MAKER

    def order_side(self) -> OrderSide:
        order_side = parse_order_side(self.side)
        if self.trader_side == PolymarketLiquiditySide.TAKER:
            return order_side
        else:
            return OrderSide.BUY if order_side == OrderSide.SELL else OrderSide.SELL

    def venue_order_id(self, maker_address: str) -> VenueOrderId:
        if self.trader_side == PolymarketLiquiditySide.TAKER:
            return VenueOrderId(self.taker_order_id)
        else:
            order = self.get_maker_order(maker_address)
            return VenueOrderId(order.order_id)

    def last_px(self, maker_address: str) -> Decimal:
        if self.liquidity_side() == LiquiditySide.TAKER:
            return Decimal(self.price)
        else:
            order = self.get_maker_order(maker_address)
            return Decimal(order.price)

    def last_qty(self, maker_address: str) -> Decimal:
        if self.liquidity_side() == LiquiditySide.TAKER:
            return Decimal(self.size)
        else:
            order = self.get_maker_order(maker_address)
            return Decimal(order.matched_amount)

    def get_fee_rate_bps(self, maker_address: str) -> Decimal:
        if self.liquidity_side() == LiquiditySide.TAKER:
            return Decimal(self.fee_rate_bps)
        else:
            order = self.get_maker_order(maker_address)
            return Decimal(order.fee_rate_bps)

    def parse_to_fill_report(
        self,
        account_id: AccountId,
        instrument: BinaryOption,
        client_order_id: ClientOrderId | None,
        maker_address: str,
        ts_init: int,
    ) -> FillReport:
        last_qty = instrument.make_qty(self.last_qty(maker_address))
        last_px = instrument.make_price(self.last_px(maker_address))
        fee_rate_bps = self.get_fee_rate_bps(maker_address)
        commission = float(last_qty * last_px) * basis_points_as_percentage(fee_rate_bps)

        return FillReport(
            account_id=account_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            venue_order_id=self.venue_order_id(maker_address),
            trade_id=TradeId(self.id),
            order_side=self.order_side(),
            last_qty=last_qty,
            last_px=last_px,
            commission=Money(commission, USDC_POS),
            liquidity_side=self.liquidity_side(),
            report_id=UUID4(),
            ts_event=millis_to_nanos(int(self.match_time)),
            ts_init=ts_init,
        )
