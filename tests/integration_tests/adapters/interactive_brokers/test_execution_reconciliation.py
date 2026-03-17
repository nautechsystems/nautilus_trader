from decimal import Decimal
from unittest.mock import AsyncMock

import pytest

from nautilus_trader.adapters.interactive_brokers.client.common import IBPosition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.model.enums import PositionSide
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestExecStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs


def instrument_setup(exec_client, cache, instrument=None, contract_details=None):
    instrument = instrument or IBTestContractStubs.aapl_instrument()
    contract_details = contract_details or IBTestContractStubs.aapl_equity_contract_details()
    exec_client._instrument_provider.contract_details[instrument.id] = contract_details
    exec_client._instrument_provider.contract_id_to_instrument_id[
        contract_details.contract.conId
    ] = instrument.id
    exec_client._instrument_provider.add(instrument)
    cache.add_instrument(instrument)


def msft_contract_details():
    contract_details = IBTestContractStubs.aapl_equity_contract_details()
    contract_details.contract.conId = 272093
    contract_details.contract.symbol = "MSFT"
    contract_details.contract.localSymbol = "MSFT"
    contract_details.contract.primaryExchange = "NASDAQ"
    contract_details.contract.exchange = "SMART"
    contract_details.longName = "MICROSOFT CORP"
    return contract_details


@pytest.mark.asyncio
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
        quantity=Decimal(0),
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
    assert reports[0].quantity.as_decimal() == Decimal(0)
    assert reports[0].instrument_id == instrument.id


@pytest.mark.asyncio
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
    assert reports[0].quantity.as_decimal() == Decimal(0)
    assert reports[0].instrument_id == instrument.id


@pytest.mark.asyncio
async def test_generate_position_status_reports_scopes_to_requested_instrument(exec_client, cache):
    aapl_instrument = IBTestContractStubs.aapl_instrument()
    aapl_details = IBTestContractStubs.aapl_equity_contract_details()
    instrument_setup(exec_client, cache, instrument=aapl_instrument, contract_details=aapl_details)

    msft_details = msft_contract_details()
    msft_instrument = IBTestContractStubs.create_instrument(msft_details)
    instrument_setup(exec_client, cache, instrument=msft_instrument, contract_details=msft_details)

    msft_position = IBPosition(
        account_id="DU123456",
        contract=msft_details.contract,
        quantity=Decimal(10),
        avg_cost=100.0,
    )
    exec_client._client.get_positions = AsyncMock(return_value=[msft_position])

    command = GeneratePositionStatusReports(
        instrument_id=aapl_instrument.id,
        start=None,
        end=None,
        command_id=UUID4(),
        ts_init=0,
    )

    reports = await exec_client.generate_position_status_reports(command)

    assert len(reports) == 1
    assert reports[0].instrument_id == aapl_instrument.id
    assert reports[0].position_side == PositionSide.FLAT
    assert reports[0].quantity.as_decimal() == Decimal(0)


@pytest.mark.asyncio
async def test_generate_order_status_reports_scopes_to_requested_instrument(exec_client, cache):
    aapl_instrument = IBTestContractStubs.aapl_instrument()
    aapl_details = IBTestContractStubs.aapl_equity_contract_details()
    instrument_setup(exec_client, cache, instrument=aapl_instrument, contract_details=aapl_details)

    msft_details = msft_contract_details()
    msft_instrument = IBTestContractStubs.create_instrument(msft_details)
    instrument_setup(exec_client, cache, instrument=msft_instrument, contract_details=msft_details)

    msft_order = IBTestExecStubs.aapl_buy_ib_order()
    msft_order.contract = msft_details.contract
    msft_order.order_state = IBTestExecStubs.ib_order_state()

    msft_position = IBPosition(
        account_id="DU123456",
        contract=msft_details.contract,
        quantity=Decimal(10),
        avg_cost=100.0,
    )

    exec_client._client.get_open_orders = AsyncMock(return_value=[msft_order])
    exec_client._client.get_positions = AsyncMock(return_value=[msft_position])

    command = GenerateOrderStatusReports(
        instrument_id=aapl_instrument.id,
        start=None,
        end=None,
        open_only=False,
        command_id=UUID4(),
        ts_init=0,
    )

    reports = await exec_client.generate_order_status_reports(command)

    assert reports == []
