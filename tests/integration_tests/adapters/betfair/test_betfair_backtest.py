# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevel
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.book import OrderBookData
from nautilus_trader.model.tick import TradeTick
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.fixture()
def betfair_client():
    return BetfairTestStubs.betfair_client()


@pytest.fixture()
def instrument_provider(betfair_client):
    return BetfairTestStubs.instrument_provider(betfair_client)


def test_betfair_backtest(instrument_provider):
    # Load instruments
    instruments = BetfairTestStubs.raw_market_updates_instruments()
    instrument_provider.set_instruments(instruments)

    # Load market data
    all_data = BetfairTestStubs.parsed_market_updates(instrument_provider)

    # Create engine
    engine = BacktestEngine()

    # Filter and add to engine
    for instrument in instruments:
        engine.add_instrument(instrument)

        ob_data = [
            d
            for d in all_data
            if isinstance(d, OrderBookData) and d.instrument_id == instrument.id
        ]
        engine.add_order_book_data(ob_data)

        trade_data = [
            d
            for d in all_data
            if isinstance(d, TradeTick) and d.instrument_id == instrument.id
        ]
        engine.add_trade_tick_objects(instrument_id=instrument.id, data=trade_data)

    engine.add_exchange(
        venue=BETFAIR_VENUE,
        oms_type=OMSType.NETTING,
        starting_balances=[Money(10_000, GBP)],
        order_book_level=OrderBookLevel.L2,
    )

    engine.run(strategies=[])
