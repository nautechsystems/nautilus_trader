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

import pytest

from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/betfair/responses/"


@pytest.mark.local
@pytest.mark.asyncio
async def test_betfair_data_client(betfair_data_client):
    """ Local test only, ensure we can connect to betfair and receive some market data """
    # TODO - implement
    assert betfair_data_client


def test_individual_market_subscriptions():
    pass


def test_market_heartbeat(betfair_data_client):
    update = json.loads(open(TEST_PATH + "streaming_mcm_HEARTBEAT.json").read())
    betfair_data_client._on_market_update(update=update)


def test_market_sub_image_market_def(betfair_data_client):
    update = json.loads(open(TEST_PATH + "streaming_mcm_SUB_IMAGE.json").read())
    betfair_data_client._on_market_update(update=update)


def test_market_sub_image_no_market_def(betfair_data_client):
    update = json.loads(
        open(TEST_PATH + "streaming_mcm_SUB_IMAGE_no_market_def.json").read()
    )
    betfair_data_client._on_market_update(update=update)


def test_market_resub_delta(betfair_data_client):
    update = json.loads(open(TEST_PATH + "streaming_mcm_RESUB_DELTA.json").read())
    betfair_data_client._on_market_update(update=update)


def test_market_update(betfair_data_client):
    update = json.loads(open(TEST_PATH + "streaming_mcm_UPDATE.json").read())
    betfair_data_client._on_market_update(update=update)


def test_market_update_md(betfair_data_client):
    update = json.loads(open(TEST_PATH + "streaming_mcm_UPDATE_md.json").read())
    betfair_data_client._on_market_update(update=update)


def test_market_update_tv(betfair_data_client):
    update = json.loads(open(TEST_PATH + "streaming_mcm_UPDATE_tv.json").read())
    betfair_data_client._on_market_update(update=update)
