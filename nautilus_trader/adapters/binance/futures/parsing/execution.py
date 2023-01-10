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

from nautilus_trader.adapters.binance.common.parsing.execution import BinanceExecutionParser
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesPositionSide
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesWorkingType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TriggerType


class BinanceFuturesExecutionParser(BinanceExecutionParser):
    """
    Provides enum parsing methods for execution on the 'Binance Futures' exchange.
    """

    def __init__(self) -> None:
        super().__init__()

        self.ext_position_side_to_int_position_side = {
            BinanceFuturesPositionSide.BOTH: PositionSide.FLAT,
            BinanceFuturesPositionSide.LONG: PositionSide.LONG,
            BinanceFuturesPositionSide.SHORT: PositionSide.SHORT,
        }

    def parse_binance_trigger_type(self, trigger_type: str) -> TriggerType:
        if trigger_type == BinanceFuturesWorkingType.CONTRACT_PRICE:
            return TriggerType.LAST_TRADE
        elif trigger_type == BinanceFuturesWorkingType.MARK_PRICE:
            return TriggerType.MARK_PRICE
        else:
            return TriggerType.NO_TRIGGER  # pragma: no cover (design-time error)

    def parse_futures_position_side(
        self,
        position_side: BinanceFuturesPositionSide,
    ) -> PositionSide:
        try:
            return self.ext_position_side_to_int_position_side[position_side]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized binance futures position side, was {position_side}",  # pragma: no cover
            )
