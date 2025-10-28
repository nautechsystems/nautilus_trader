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

from decimal import Decimal
from unittest.mock import AsyncMock

import pytest

from nautilus_trader.adapters.interactive_brokers.client.common import IBPosition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.model.enums import PositionSide
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs


def instrument_setup(exec_client, cache, instrument=None, contract_details=None):
    instrument = instrument or IBTestContractStubs.aapl_instrument()
    contract_details = contract_details or IBTestContractStubs.aapl_equity_contract_details()
    exec_client._instrument_provider.contract_details[instrument.id.value] = contract_details
    exec_client._instrument_provider.contract_id_to_instrument_id[
        contract_details.contract.conId
    ] = instrument.id
    exec_client._instrument_provider.add(instrument)
    cache.add_instrument(instrument)


@pytest.mark.asyncio()
async def test_generate_position_status_reports_with_zero_quantity(exec_client, cache):
    """
    Test that zero-quantity positions generate FLAT PositionStatusReport.

    Verifies fix for issue #3023 where IB adapter should emit FLAT reports when
    positions are closed externally.

    """
    # Arrange
    instrument = IBTestContractStubs.aapl_instrument()
    instrument_setup(exec_client, cache, instrument=instrument)

    zero_position = IBPosition(
        account_id="DU123456",
        contract=IBTestContractStubs.aapl_equity_ib_contract(),
        quantity=Decimal("0"),
        avg_cost=100.0,
    )

    exec_client._client.get_positions = AsyncMock(return_value=[zero_position])

    command = GeneratePositionStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        command_id=UUID4(),
        ts_init=0,
    )

    # Act
    reports = await exec_client.generate_position_status_reports(command)

    # Assert
    assert len(reports) == 1
    assert reports[0].position_side == PositionSide.FLAT
    assert reports[0].quantity.as_decimal() == Decimal("0")
    assert reports[0].instrument_id == instrument.id


@pytest.mark.asyncio()
async def test_generate_position_status_reports_flat_when_no_positions(exec_client, cache):
    """
    Test that FLAT report is generated when specific instrument has no positions.

    Verifies fix for issue #3023 where reconciliation requests position for specific
    instrument but IB returns no positions.

    """
    # Arrange
    instrument = IBTestContractStubs.aapl_instrument()
    instrument_setup(exec_client, cache, instrument=instrument)

    exec_client._client.get_positions = AsyncMock(return_value=None)

    command = GeneratePositionStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        command_id=UUID4(),
        ts_init=0,
    )

    # Act
    reports = await exec_client.generate_position_status_reports(command)

    # Assert
    assert len(reports) == 1
    assert reports[0].position_side == PositionSide.FLAT
    assert reports[0].quantity.as_decimal() == Decimal("0")
    assert reports[0].instrument_id == instrument.id
