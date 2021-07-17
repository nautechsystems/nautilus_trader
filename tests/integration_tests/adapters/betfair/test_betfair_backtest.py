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
import pandas as pd
import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.model.c_enums.instrument_status import InstrumentStatus
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentClosePrice
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.data import OrderBookData
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.strategies import OrderBookImbalanceStrategy


@pytest.fixture()
def betfair_client():
    return BetfairTestStubs.betfair_client()


@pytest.fixture()
def instrument_provider(betfair_client):
    return BetfairTestStubs.instrument_provider(betfair_client)


def create_engine(instruments, data):
    # Create engine
    engine = BacktestEngine(level_stdout=LogLevel.WARNING)

    # Filter and add to engine
    for instrument in instruments[:1]:
        engine.add_instrument(instrument)

        ob_data = [
            d for d in data if isinstance(d, OrderBookData) and d.instrument_id == instrument.id
        ]
        engine.add_order_book_data(ob_data)

        trade_data = [
            d for d in data if isinstance(d, TradeTick) and d.instrument_id == instrument.id
        ]
        engine.add_trade_tick_objects(instrument_id=instrument.id, data=trade_data)

        instrument_status_updates = [
            d
            for d in data
            if isinstance(d, InstrumentStatusUpdate) and d.instrument_id == instrument.id
        ]
        engine.add_data(client_id=ClientId(BETFAIR_VENUE.value), data=instrument_status_updates)

        closing_prices = [
            d
            for d in data
            if isinstance(d, InstrumentClosePrice) and d.instrument_id == instrument.id
        ]
        engine.add_data(client_id=ClientId(BETFAIR_VENUE.value), data=closing_prices)

    engine.add_venue(
        venue=BETFAIR_VENUE,
        venue_type=VenueType.EXCHANGE,
        oms_type=OMSType.NETTING,
        account_type=AccountType.CASH,
        base_currency=GBP,
        starting_balances=[Money(100_000, GBP)],
        order_book_level=BookLevel.L2,
    )
    return engine


def test_betfair_backtest(instrument_provider):
    # Load instruments
    instruments = BetfairDataProvider.raw_market_updates_instruments()
    instrument_provider.set_instruments(instruments)

    # Load market data
    all_data = BetfairDataProvider.parsed_market_updates(instrument_provider)

    # Create strategy
    strategy = OrderBookImbalanceStrategy(instrument_id=instruments[0].id, trade_size=20)

    engine = create_engine(instruments=instruments, data=all_data)
    engine.run(strategies=[strategy])

    assert strategy.instrument_status == InstrumentStatus.CLOSED
    assert strategy.close_price == 1.0

    # Check account states
    account = engine.trader.generate_account_report(BETFAIR_VENUE)
    assert account.index[0] == pd.Timestamp("2019-12-28 02:23:03.086000+0000")
    assert account.index[-1] == pd.Timestamp("2019-12-29 03:36:39.861000+00:00")
