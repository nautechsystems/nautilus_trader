from unittest.mock import Mock
from unittest.mock import patch

import pytest

from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestProviderStubs


@pytest.mark.asyncio
async def test_get_contract_details(ib_client):
    # Arrange
    ib_client._request_id_seq = 1
    contract = IBTestProviderStubs.aapl_equity_contract_details().contract
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
    underlying = IBTestProviderStubs.aapl_equity_contract_details().contract

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
