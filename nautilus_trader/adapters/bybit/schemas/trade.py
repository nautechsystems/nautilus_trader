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

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitExecType
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitStopOrderType
from nautilus_trader.adapters.bybit.schemas.common import BybitListResultWithCursor
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BybitExecution(msgspec.Struct, omit_defaults=True, kw_only=True):
    symbol: str
    orderId: str
    orderLinkId: str
    side: BybitOrderSide
    orderPrice: str
    orderQty: str
    leavesQty: str
    createType: str | None = None
    orderType: BybitOrderType
    stopOrderType: BybitStopOrderType | None = None
    execFee: str
    execId: str
    execPrice: str
    execQty: str
    execType: BybitExecType
    execValue: str
    execTime: str
    feeCurrency: str
    isMaker: bool
    feeRate: str
    tradeIv: str
    markIv: str
    markPrice: str
    indexPrice: str
    underlyingPrice: str
    blockTradeId: str
    closedSize: str
    seq: int

    def parse_to_fill_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        enum_parser: BybitEnumParser,
        ts_init: int,
    ) -> OrderStatusReport:
        client_order_id = ClientOrderId(self.orderLinkId) if self.orderLinkId else None
        return FillReport(
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId(str(self.orderId)),
            trade_id=TradeId(self.execId),
            account_id=account_id,
            instrument_id=instrument_id,
            order_side=enum_parser.parse_bybit_order_side(self.side),
            last_qty=Quantity.from_str(self.execQty),
            last_px=Price.from_str(self.execPrice),
            commission=Money(
                Decimal(self.execFee or 0),
                Currency.from_str(self.feeCurrency or "USDT"),
            ),
            liquidity_side=LiquiditySide.MAKER if self.isMaker else LiquiditySide.TAKER,
            report_id=report_id,
            ts_event=millis_to_nanos(Decimal(self.execTime)),
            ts_init=ts_init,
        )


class BybitTradeHistoryResponseStruct(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResultWithCursor[BybitExecution]
    time: int
