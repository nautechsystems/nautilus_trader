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
import asyncio
import os

import betfairlightweight
import pytest

from adapters.betfair.data import InstrumentSearch
from nautilus_trader.adapters.betfair.data import BetfairMarketStreamClient
from nautilus_trader.model.c_enums.orderbook_op import OrderBookOperationType
from nautilus_trader.model.data import DataType
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


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
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_HEARTBEAT())


def test_market_sub_image_market_def(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_SUB_IMAGE())
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookSnapshot"] * 7
    assert result == expected
    # Check prices are probabilities
    result = [
        float(order[0])
        for ob_snap in data_engine.events
        for order in ob_snap.bids + ob_snap.asks
    ]
    expected = [
        0.02174,
        0.39370,
        0.36765,
        0.21739,
        0.00102,
        0.17241,
        0.00102,
        0.55556,
        0.45872,
        0.21739,
        0.00769,
        0.02381,
    ]
    assert result == expected


def test_market_sub_image_no_market_def(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(
        BetfairTestStubs.streaming_mcm_SUB_IMAGE_no_market_def()
    )
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookSnapshot"] * 270
    assert result == expected


def test_market_resub_delta(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_RESUB_DELTA())
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookOperations"] * 284
    assert result == expected


def test_market_update(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_UPDATE())
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookOperations"] * 1
    assert result == expected
    result = [op.op_type for op in data_engine.events[0].ops]
    expected = [OrderBookOperationType.UPDATE, OrderBookOperationType.DELETE]
    assert result == expected
    # Ensure order prices are coming through as probability
    update_op = data_engine.events[0].ops[0]
    assert update_op.order.price == 0.21277


@pytest.mark.skip  # TODO - waiting for market status implementation
def test_market_update_md(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_UPDATE_md())
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookSnapshot"] * 7
    assert result == expected


@pytest.mark.skip  # We don't do anything with traded volume at this stage
def test_market_update_tv(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_UPDATE_tv())
    result = [type(event).__name__ for event in data_engine.events]
    expected = [] * 7
    assert result == expected


def test_market_update_live(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_live_IMAGE())
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookSnapshot"] * 2
    assert result == expected


@pytest.mark.asyncio
async def test_request_search_instruments(betfair_data_client, data_engine, uuid):
    req = DataType(
        data_type=InstrumentSearch,
        metadata={"event_type_id": "6"},
    )
    betfair_data_client.request(req, uuid)
    await asyncio.sleep(0)
    resp = data_engine.responses[0]
    assert len(resp.data.data.instruments) == 495
