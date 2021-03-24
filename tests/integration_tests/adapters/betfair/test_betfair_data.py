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
import json
import os

import betfairlightweight
import pytest

from nautilus_trader.adapters.betfair.data import BetfairMarketStreamClient
from nautilus_trader.model.c_enums.orderbook_op import OrderBookOperationType
from tests import TESTS_PACKAGE_ROOT
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/betfair/responses/"


@pytest.mark.asyncio
@pytest.mark.skip  # Only runs locally, comment to run
async def test_betfair_data_client(betfair_data_client, data_engine):
    """ Local test only, ensure we can connect to betfair and receive some market data """
    betfair_client = betfairlightweight.APIClient(
        username=os.environ["BETFAIR_USERNAME"],
        password=os.environ["BETFAIR_PW"],
        app_key=os.environ["BETFAIR_APP_KEY"],
        certs=os.environ["BETFAIR_CERT_DIR"],
    )
    betfair_client.login()

    def printer(x):
        print(x)

    # TODO - mock betfairlightweight.APIClient.login won't let this pass, need to comment out to run
    socket = BetfairMarketStreamClient(client=betfair_client, message_handler=printer)
    await socket.connect()
    await socket.send_subscription_message(market_ids=["1.180634014"])
    await socket.start()


def test_individual_market_subscriptions():
    # TODO - Subscribe to a couple of markets individually
    pass


def test_market_heartbeat(betfair_data_client, data_engine):
    update = json.loads(open(TEST_PATH + "streaming_mcm_HEARTBEAT.json").read())
    betfair_data_client._on_market_update(update=update)


def test_market_sub_image_market_def(betfair_data_client, data_engine):
    update = json.loads(open(TEST_PATH + "streaming_mcm_SUB_IMAGE.json").read())
    betfair_data_client._on_market_update(update=update)
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookSnapshot"] * 7
    assert result == expected


def test_market_sub_image_no_market_def(betfair_data_client, data_engine):
    update = json.loads(
        open(TEST_PATH + "streaming_mcm_SUB_IMAGE_no_market_def.json").read()
    )
    betfair_data_client._on_market_update(update=update)
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookSnapshot"] * 270
    assert result == expected


def test_market_resub_delta(betfair_data_client, data_engine):
    update = BetfairTestStubs.streaming_mcm_RESUB_DELTA()
    betfair_data_client._on_market_update(update=update)
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookOperations"] * 284
    assert result == expected


def test_market_update(betfair_data_client, data_engine):
    update = BetfairTestStubs.streaming_mcm_UPDATE()
    betfair_data_client._on_market_update(update=update)
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookOperations"] * 1
    assert result == expected
    result = [op.op_type for op in data_engine.events[0].ops]
    expected = [OrderBookOperationType.UPDATE, OrderBookOperationType.DELETE]
    assert result == expected


@pytest.mark.skip  # TODO - waiting for market status implementation
def test_market_update_md(betfair_data_client, data_engine):
    update = BetfairTestStubs.streaming_mcm_UPDATE_md()
    betfair_data_client._on_market_update(update=update)
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookSnapshot"] * 7
    assert result == expected


@pytest.mark.skip  # We don't do anything with traded volume at this stage
def test_market_update_tv(betfair_data_client, data_engine):
    update = BetfairTestStubs.streaming_mcm_UPDATE_tv()
    betfair_data_client._on_market_update(update=update)
    result = [type(event).__name__ for event in data_engine.events]
    expected = [] * 7
    assert result == expected
