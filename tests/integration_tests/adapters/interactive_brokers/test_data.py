import asyncio

import pytest


pytestmark = pytest.mark.skip(reason="Skip due currently flaky mocks")


def instrument_setup(data_client, instrument, contract_details):
    data_client.instrument_provider.contract_details[instrument.id] = contract_details
    data_client.instrument_provider.contract_id_to_instrument_id[
        contract_details.contract.conId
    ] = instrument.id
    data_client.instrument_provider.add(instrument)


@pytest.mark.asyncio
async def test_connect(data_client):
    data_client.connect()
    await asyncio.sleep(0)
    await asyncio.sleep(0)
    assert data_client.is_connected
