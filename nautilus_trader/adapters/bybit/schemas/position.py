# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.bybit.common.enums import BybitPositionSide
from nautilus_trader.adapters.bybit.schemas.common import BybitListResult
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity


class BybitPositionStruct(msgspec.Struct):
    positionIdx: int
    riskId: int
    riskLimitValue: str
    symbol: str
    side: BybitPositionSide
    size: str
    avgPrice: str
    positionValue: str
    tradeMode: int
    positionStatus: str
    autoAddMargin: int
    adlRankIndicator: int
    leverage: str
    positionBalance: str
    markPrice: str
    liqPrice: str
    bustPrice: str
    positionMM: str
    positionIM: str
    tpslMode: str
    takeProfit: str
    stopLoss: str
    trailingStop: str
    unrealisedPnl: str
    cumRealisedPnl: str
    createdTime: str
    updatedTime: str

    def parse_to_position_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        ts_init: int,
    ) -> PositionStatusReport:
        position_side = self.side.parse_to_position_side()
        size = Quantity.from_str(self.positionValue)
        return PositionStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            position_side=position_side,
            quantity=size,
            report_id=report_id,
            ts_init=ts_init,
            ts_last=ts_init,
        )


class BybitPositionResponseStruct(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitPositionStruct]
    time: int
