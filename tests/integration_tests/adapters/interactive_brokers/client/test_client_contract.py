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

from unittest.mock import Mock
from unittest.mock import patch

import pytest

from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs


@pytest.mark.asyncio
async def test_get_contract_details(ib_client):
    # Arrange
    ib_client._request_id_seq = 1
    contract = IBTestContractStubs.aapl_equity_contract()
    ib_client._eclient.reqContractDetails = Mock()

    # Act
    with patch("asyncio.wait_for"):
        await ib_client.get_contract_details(contract)

    # Assert
    ib_client._eclient.reqContractDetails.assert_called_once_with(
        reqId=1,
        contract=contract,
    )


@pytest.mark.asyncio
async def test_get_option_chains(ib_client):
    # Arrange
    ib_client._request_id_seq = 1
    underlying = IBTestContractStubs.aapl_equity_contract()

    ib_client._eclient.reqSecDefOptParams = Mock()

    # Act
    with patch("asyncio.wait_for"):
        await ib_client.get_option_chains(underlying)

    # Assert
    ib_client._eclient.reqSecDefOptParams.assert_called_once_with(
        reqId=1,
        underlyingSymbol=underlying.symbol,
        futFopExchange="",
        underlyingSecType=underlying.secType,
        underlyingConId=underlying.conId,
    )
