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
Define the schemas for the GetFills endpoint.
"""

# ruff: noqa: N815

import datetime
from decimal import Decimal

import msgspec

from nautilus_trader.adapters.dydx.common.constants import DEFAULT_CURRENCY
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.adapters.dydx.common.enums import DYDXFillType
from nautilus_trader.adapters.dydx.common.enums import DYDXLiquidity
from nautilus_trader.adapters.dydx.common.enums import DYDXMarketType
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderSide
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class DYDXFillResponse(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the schema for a fill.
    """

    id: str
    side: DYDXOrderSide
    liquidity: DYDXLiquidity
    type: DYDXFillType
    market: str
    marketType: DYDXMarketType
    price: str
    size: str
    fee: str
    createdAt: datetime.datetime
    createdAtHeight: str
    subaccountNumber: int
    orderId: str | None = None
    clientMetadata: str | None = None
    affiliateRevShare: str | None = None

    def parse_to_fill_report(
        self,
        account_id: AccountId,
        client_order_id: ClientOrderId | None,
        report_id: UUID4,
        price_precision: int,
        size_precision: int,
        enum_parser: DYDXEnumParser,
        ts_init: int,
    ) -> FillReport:
        """
        Parse the fill message into a FillReport.
        """
        venue_order_id = None

        if self.orderId is not None:
            venue_order_id = VenueOrderId(self.orderId)

        return FillReport(
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            trade_id=TradeId(self.id),
            account_id=account_id,
            instrument_id=DYDXSymbol(self.market).to_instrument_id(),
            order_side=enum_parser.parse_dydx_order_side(self.side),
            last_qty=Quantity(Decimal(self.size), size_precision),
            last_px=Price(Decimal(self.price), price_precision),
            commission=Money(Decimal(self.fee), Currency.from_str(DEFAULT_CURRENCY)),
            liquidity_side=enum_parser.parse_dydx_liquidity_side(self.liquidity),
            report_id=report_id,
            ts_event=dt_to_unix_nanos(self.createdAt),
            ts_init=ts_init,
        )


class DYDXFillsResponse(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the schema for the fills response.
    """

    fills: list[DYDXFillResponse]
    pageSize: int | None = None
    totalResults: int | None = None
    offset: int | None = None
