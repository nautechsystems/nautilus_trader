# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.binance.common.parsing.execution import BinanceExecutionParser
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesWorkingType
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesPositionRisk
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity


class BinanceFuturesExecutionParser(BinanceExecutionParser):
    """
    Provides parsing methods for execution on the 'Binance Futures' exchange.
    """

    def __init__(self) -> None:
        super().__init__()

    def parse_binance_trigger_type(self, trigger_type: str) -> TriggerType:
        if trigger_type == BinanceFuturesWorkingType.CONTRACT_PRICE:
            return TriggerType.LAST
        elif trigger_type == BinanceFuturesWorkingType.MARK_PRICE:
            return TriggerType.MARK
        else:
            return TriggerType.NONE  # pragma: no cover (design-time error)

    def parse_futures_position_report_http(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        data: BinanceFuturesPositionRisk,
        report_id: UUID4,
        ts_init: int,
    ) -> PositionStatusReport:
        net_size = Decimal(data.positionAmt)

        if net_size > 0:
            position_side = PositionSide.LONG
        elif net_size < 0:
            position_side = PositionSide.SHORT
        else:
            position_side = PositionSide.FLAT

        return PositionStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            position_side=position_side,
            quantity=Quantity.from_str(str(abs(net_size))),
            report_id=report_id,
            ts_last=ts_init,
            ts_init=ts_init,
        )
