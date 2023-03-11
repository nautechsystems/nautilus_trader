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

import msgspec.json
import pytest
from betfair_parser.spec.streaming.mcm import MarketDefinition

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import BetfairLiveExecClientFactory
from nautilus_trader.backtest.providers import TestInstrumentProvider
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.fixture()
def venue() -> Venue:
    return BETFAIR_VENUE


@pytest.fixture()
def betfair_client(event_loop, logger):
    return BetfairTestStubs.betfair_client(event_loop, logger)


@pytest.fixture()
def instrument_provider(betfair_client):
    return BetfairTestStubs.instrument_provider(betfair_client=betfair_client)


@pytest.fixture()
def instrument():
    return TestInstrumentProvider.betting_instrument()


@pytest.fixture()
def data_client(
    mocker,
    betfair_client,
    instrument_provider,
    instrument,
    venue,
    event_loop,
    msgbus,
    cache,
    clock,
    logger,
) -> BetfairDataClient:
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_client",
        return_value=betfair_client,
    )
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_instrument_provider",
        return_value=instrument_provider,
    )
    instrument_provider.add(instrument)
    return BetfairLiveDataClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=BetfairDataClientConfig(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        logger=logger,
    )


@pytest.fixture()
def exec_client(
    mocker,
    betfair_client,
    instrument_provider,
    instrument,
    venue,
    event_loop,
    msgbus,
    cache,
    clock,
    logger,
) -> BetfairExecutionClient:
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_client",
        return_value=betfair_client,
    )
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_instrument_provider",
        return_value=instrument_provider,
    )
    instrument_provider.add(instrument)
    return BetfairLiveExecClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=BetfairExecClientConfig(base_currency="GBP"),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        logger=logger,
    )


@pytest.fixture()
def account_state() -> AccountState:
    return TestEventStubs.betting_account_state(account_id=AccountId("BETFAIR-001"))


@pytest.fixture()
def market_definition_open() -> MarketDefinition:
    return msgspec.json.decode(
        r'{"bspMarket":true,"turnInPlayEnabled":false,"persistenceEnabled":false,"marketBaseRate":7.0,"eventId":"31873357","eventTypeId":"7","numberOfWinners":3,"bettingType":"ODDS","marketType":"PLACE","marketTime":"2022-11-01T01:49:00.000Z","suspendTime":"2022-11-01T01:49:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":false,"runnersVoidable":false,"numberOfActiveRunners":17,"betDelay":0,"status":"OPEN","runners":[{"adjustmentFactor":11.53,"status":"ACTIVE","sortPriority":1,"id":49808334,"name":"1. Realaide"},{"adjustmentFactor":1.25,"status":"ACTIVE","sortPriority":2,"id":45368013,"name":"2. Legend I Am"},{"adjustmentFactor":15.98,"status":"ACTIVE","sortPriority":3,"id":19143530,"name":"3. Storm Harbour"},{"adjustmentFactor":27.58,"status":"ACTIVE","sortPriority":4,"id":2329545,"name":"4. Gap Year"},{"adjustmentFactor":11.53,"status":"ACTIVE","sortPriority":5,"id":48672282,"name":"6. Unlikelyoccurrence"},{"adjustmentFactor":1.25,"status":"ACTIVE","sortPriority":6,"id":6159479,"name":"7. Winston Blue"},{"adjustmentFactor":9.84,"status":"ACTIVE","sortPriority":7,"id":10591436,"name":"8. Bonnie And Clyde"},{"adjustmentFactor":4.39,"status":"ACTIVE","sortPriority":8,"id":16206031,"name":"9. Herecum Da Drums"},{"adjustmentFactor":13.87,"status":"ACTIVE","sortPriority":9,"id":25694777,"name":"11. Rip City"},{"adjustmentFactor":2.65,"status":"ACTIVE","sortPriority":10,"id":35672106,"name":"12. Who Said So"},{"adjustmentFactor":2.93,"status":"ACTIVE","sortPriority":11,"id":49808335,"name":"13. Daulat Machtigamor"},{"adjustmentFactor":1.25,"status":"ACTIVE","sortPriority":12,"id":39000334,"name":"15. Tallahassee Lassie"},{"adjustmentFactor":9.84,"status":"ACTIVE","sortPriority":13,"id":49808338,"name":"18. Federal Agent"},{"adjustmentFactor":6.78,"status":"ACTIVE","sortPriority":14,"id":49808340,"name":"20. Frozen Prince"},{"adjustmentFactor":1.69,"status":"ACTIVE","sortPriority":15,"id":42011335,"name":"22. Claudius"},{"adjustmentFactor":11.53,"status":"ACTIVE","sortPriority":16,"id":49808342,"name":"23. Birkin Black"},{"adjustmentFactor":2.65,"status":"ACTIVE","sortPriority":17,"id":49808343,"name":"24. Dacxi Kaboom"}],"regulators":["MR_INT"],"venue":"Sunshine Coast","countryCode":"AU","discountAllowed":true,"timezone":"Australia/Queensland","openDate":"2022-11-01T01:49:00.000Z","version":4881874440,"name":"To Be Placed","eventName":"Sunshine Coast (AUS) 1st Nov"}',  # noqa
        type=MarketDefinition,
    )


@pytest.fixture()
def market_definition_close() -> MarketDefinition:
    return msgspec.json.decode(
        r'{"bspMarket":true,"turnInPlayEnabled":false,"persistenceEnabled":false,"marketBaseRate":7.0,"eventId":"31873357","eventTypeId":"7","numberOfWinners":3,"bettingType":"ODDS","marketType":"PLACE","marketTime":"2022-11-01T01:49:00.000Z","suspendTime":"2022-11-01T01:49:00.000Z","bspReconciled":true,"complete":true,"inPlay":false,"crossMatching":false,"runnersVoidable":false,"numberOfActiveRunners":0,"betDelay":0,"status":"CLOSED","settledTime":"2022-11-01T02:02:23.000Z","runners":[{"adjustmentFactor":27.58,"status":"REMOVED","sortPriority":1,"removalDate":"2022-10-31T07:39:36.000Z","id":2329545,"name":"4. Gap Year"},{"adjustmentFactor":8.37,"status":"REMOVED","sortPriority":2,"removalDate":"2022-10-31T21:18:59.000Z","id":49808340,"name":"20. Frozen Prince"},{"adjustmentFactor":2.41,"status":"REMOVED","sortPriority":3,"removalDate":"2022-10-31T22:52:32.000Z","id":42011335,"name":"22. Claudius"},{"adjustmentFactor":11.78,"status":"WINNER","sortPriority":4,"bsp":4.2,"id":49808334,"name":"1. Realaide"},{"adjustmentFactor":1.58,"status":"LOSER","sortPriority":5,"bsp":16.17,"id":45368013,"name":"2. Legend I Am"},{"adjustmentFactor":14.46,"status":"LOSER","sortPriority":6,"bsp":5.27,"id":19143530,"name":"3. Storm Harbour"},{"adjustmentFactor":12.65,"status":"LOSER","sortPriority":7,"bsp":3.45,"id":48672282,"name":"6. Unlikelyoccurrence"},{"adjustmentFactor":1.27,"status":"LOSER","sortPriority":8,"bsp":16.0,"id":6159479,"name":"7. Winston Blue"},{"adjustmentFactor":11.64,"status":"LOSER","sortPriority":9,"bsp":4.5,"id":10591436,"name":"8. Bonnie And Clyde"},{"adjustmentFactor":6.06,"status":"LOSER","sortPriority":10,"bsp":10.1,"id":16206031,"name":"9. Herecum Da Drums"},{"adjustmentFactor":12.24,"status":"LOSER","sortPriority":11,"bsp":3.98,"id":25694777,"name":"11. Rip City"},{"adjustmentFactor":3.88,"status":"LOSER","sortPriority":12,"bsp":14.37,"id":35672106,"name":"12. Who Said So"},{"adjustmentFactor":12.65,"status":"WINNER","sortPriority":13,"bsp":3.58,"id":49808335,"name":"13. Daulat Machtigamor"},{"adjustmentFactor":3.61,"status":"LOSER","sortPriority":14,"bsp":9.13,"id":39000334,"name":"15. Tallahassee Lassie"},{"adjustmentFactor":19.61,"status":"LOSER","sortPriority":15,"bsp":1.76,"id":49808338,"name":"18. Federal Agent"},{"adjustmentFactor":21.79,"status":"WINNER","sortPriority":16,"bsp":1.86,"id":49808342,"name":"23. Birkin Black"},{"adjustmentFactor":2.87,"status":"LOSER","sortPriority":17,"bsp":15.04,"id":49808343,"name":"24. Dacxi Kaboom"}],"regulators":["MR_INT"],"venue":"Sunshine Coast","countryCode":"AU","discountAllowed":true,"timezone":"Australia/Queensland","openDate":"2022-11-01T01:49:00.000Z","version":4883105963,"name":"To Be Placed","eventName":"Sunshine Coast (AUS) 1st Nov"}',  # noqa
        type=MarketDefinition,
    )
