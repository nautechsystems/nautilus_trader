# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest


pytestmark = pytest.mark.skip(reason="Skip due currently flaky mocks")


def instrument_setup(data_client, instrument, contract_details):
    data_client.instrument_provider.contract_details[instrument.id.value] = contract_details
    data_client.instrument_provider.contract_id_to_instrument_id[
        contract_details.contract.conId
    ] = instrument.id
    data_client.instrument_provider.add(instrument)


@pytest.mark.asyncio()
async def test_connect(data_client):
    data_client.connect()
    await asyncio.sleep(0)
    await asyncio.sleep(0)
    assert data_client.is_connected
