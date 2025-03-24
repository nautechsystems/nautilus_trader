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
Define the schemas for the GetOrders endpoint.
"""

# ruff: noqa: N815

import datetime
from decimal import Decimal

import msgspec

from nautilus_trader.adapters.dydx.common.constants import CURRENCY_MAP
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderSide
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderStatus
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderType
from nautilus_trader.adapters.dydx.common.enums import DYDXTimeInForce
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class DYDXOrderResponse(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the schema for the order response.
    """

    id: str
    subaccountId: str
    clientId: str
    clobPairId: str
    side: DYDXOrderSide
    size: str
    totalFilled: str
    price: str
    type: DYDXOrderType
    reduceOnly: bool
    orderFlags: str
    clientMetadata: str
    timeInForce: DYDXTimeInForce
    status: DYDXOrderStatus
    postOnly: bool
    ticker: str
    subaccountNumber: int
    updatedAtHeight: str | None = None
    updatedAt: datetime.datetime | None = None
    goodTilBlock: str | None = None
    goodTilBlockTime: str | None = None
    createdAtHeight: str | None = None
    triggerPrice: str | None = None

    def base_currency(self) -> str:
        """
        Return the quote currency.
        """
        currency = self.ticker.split("-")[0]
        return CURRENCY_MAP.get(currency, currency)

    def quote_currency(self) -> str:
        """
        Return the quote currency.
        """
        currency = self.ticker.split("-")[1]
        return CURRENCY_MAP.get(currency, currency)

    def parse_to_order_status_report(
        self,
        account_id: AccountId,
        client_order_id: ClientOrderId | None,
        price_precision: int,
        size_precision: int,
        report_id: UUID4,
        enum_parser: DYDXEnumParser,
        ts_init: int,
    ) -> OrderStatusReport:
        """
        Create an order status report from the order message.
        """
        filled_qty = (
            Quantity(Decimal(self.totalFilled), size_precision)
            if self.totalFilled is not None
            else Quantity(Decimal("0"), size_precision)
        )
        ts_last = dt_to_unix_nanos(self.updatedAt) if self.updatedAt is not None else ts_init
        trigger_type = (
            TriggerType.DEFAULT if self.triggerPrice is not None else TriggerType.NO_TRIGGER
        )
        trigger_price = (
            Price(Decimal(self.triggerPrice), price_precision)
            if self.triggerPrice is not None
            else None
        )

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=DYDXSymbol(self.ticker).to_instrument_id(),
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId(self.id),
            order_side=enum_parser.parse_dydx_order_side(self.side),
            order_type=enum_parser.parse_dydx_order_type(self.type),
            time_in_force=enum_parser.parse_dydx_time_in_force(self.timeInForce),
            order_status=enum_parser.parse_dydx_order_status(self.status),
            price=Price(Decimal(self.price), price_precision),
            quantity=Quantity(Decimal(self.size), size_precision),
            filled_qty=filled_qty,
            post_only=self.postOnly,
            reduce_only=self.reduceOnly,
            ts_last=ts_last,
            report_id=report_id,
            ts_accepted=0,
            ts_init=ts_init,
            trigger_type=trigger_type,
            trigger_price=trigger_price,
        )
