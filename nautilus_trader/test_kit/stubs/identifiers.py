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

from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId


class TestIdStubs:
    @staticmethod
    def uuid() -> UUID4:
        return UUID4("038990c6-19d2-b5c8-37a6-fe91f9b7b9ed")

    @staticmethod
    def trader_id() -> TraderId:
        return TraderId("TESTER-000")

    @staticmethod
    def account_id() -> AccountId:
        return AccountId("SIM-000")

    @staticmethod
    def strategy_id() -> StrategyId:
        return StrategyId("S-001")

    @staticmethod
    def position_id() -> PositionId:
        return PositionId("001")

    @staticmethod
    def btcusdt_binance_id() -> InstrumentId:
        return InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE"))

    @staticmethod
    def ethusdt_binance_id() -> InstrumentId:
        return InstrumentId(Symbol("ETHUSDT"), Venue("BINANCE"))

    @staticmethod
    def adabtc_binance_id() -> InstrumentId:
        return InstrumentId(Symbol("ADABTC"), Venue("BINANCE"))

    @staticmethod
    def audusd_id() -> InstrumentId:
        return InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

    @staticmethod
    def gbpusd_id() -> InstrumentId:
        return InstrumentId(Symbol("GBP/USD"), Venue("SIM"))

    @staticmethod
    def usdjpy_id() -> InstrumentId:
        return InstrumentId(Symbol("USD/JPY"), Venue("SIM"))

    @staticmethod
    def audusd_idealpro_id() -> InstrumentId:
        return InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO"))

    @staticmethod
    def betting_instrument_id():
        from nautilus_trader.adapters.betfair.parsing.common import betfair_instrument_id

        return betfair_instrument_id(
            market_id="1.179082386",
            selection_id="50214",
            selection_handicap=None,
        )

    @staticmethod
    def synthetic_id():
        return InstrumentId(Symbol("BTC-ETH"), Venue("SYNTH"))

    @staticmethod
    def client_order_id(counter: int = 1) -> ClientOrderId:
        return ClientOrderId(f"O-20210410-022422-001-001-{counter}")

    @staticmethod
    def order_list_id(counter: int = 1) -> OrderListId:
        return OrderListId(f"OL-20210410-022422-001-001-{counter}")

    @staticmethod
    def venue_order_id() -> VenueOrderId:
        return VenueOrderId("1")

    @staticmethod
    def trade_id() -> TradeId:
        return TradeId("1")
