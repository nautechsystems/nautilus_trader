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

from unittest.mock import AsyncMock

import msgspec
import pytest
from ibapi.contract import ContractDetails

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import InstrumentClass
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs


def mock_ib_contract_calls(mocker, instrument_provider, contract_details: ContractDetails):
    mocker.patch.object(
        instrument_provider._client,
        "get_contract_details",
        side_effect=AsyncMock(return_value=[contract_details]),
    )


@pytest.mark.asyncio()
async def test_load_equity_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.aapl_equity_contract_details(),
    )

    # Act
    await instrument_provider.load_async(
        IBContract(secType="STK", symbol="AAPL", exchange="NASDAQ"),
    )
    equity = instrument_provider.find(instrument_id)
    instrument_provider._client.stop()

    # Assert
    assert InstrumentId(symbol=Symbol("AAPL"), venue=Venue("NASDAQ")) == equity.id
    assert equity.asset_class == AssetClass.EQUITY
    assert equity.instrument_class == InstrumentClass.SPOT
    assert equity.multiplier == 1
    assert Price.from_str("0.01") == equity.price_increment
    assert 2, equity.price_precision


@pytest.mark.asyncio()
async def test_load_futures_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("CLZ23.NYMEX")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.cl_future_contract_details(),
    )

    # Act
    await instrument_provider.load_async(IBContract(secType="FUT", symbol="CLZ3", exchange="NYMEX"))
    future = instrument_provider.find(instrument_id)
    instrument_provider._client.stop()

    # Assert
    assert future.id == instrument_id
    assert future.asset_class == AssetClass.INDEX
    assert future.multiplier == 1000
    assert future.price_increment == Price.from_str("0.01")
    assert future.price_precision == 2


@pytest.mark.asyncio()
async def test_load_option_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("TSLA230120C00100000.MIAX")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.tsla_option_contract_details(),
    )

    # Act
    await instrument_provider.load_async(
        IBContract(secType="OPT", symbol="TSLA230120C00100000", exchange="MIAX"),
    )
    option = instrument_provider.find(instrument_id)
    instrument_provider._client.stop()

    # Assert
    assert option.id == instrument_id
    assert option.asset_class == AssetClass.EQUITY
    assert option.multiplier == 100
    assert option.expiration_ns == 1674172800000000000
    assert option.strike_price == Price.from_str("100.0")
    assert option.option_kind == OptionKind.CALL
    assert option.price_increment == Price.from_str("0.01")
    assert option.price_precision == 2


@pytest.mark.asyncio()
async def test_load_forex_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("EUR/USD.IDEALPRO")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.eurusd_forex_contract_details(),
    )

    # Act
    await instrument_provider.load_async(instrument_id)
    fx = instrument_provider.find(instrument_id)
    instrument_provider._client.stop()

    # Assert
    assert fx.id == instrument_id
    assert fx.asset_class == AssetClass.FX
    assert fx.multiplier == 1
    assert fx.price_increment == Price.from_str("0.00005")
    assert fx.price_precision == 5


@pytest.mark.asyncio()
async def test_contract_id_to_instrument_id(mocker, instrument_provider):
    # Arrange
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.cl_future_contract_details(),
    )

    # Act
    await instrument_provider.load_async(IBContract(secType="FUT", symbol="CLZ3", exchange="NYMEX"))
    instrument_provider._client.stop()

    # Assert
    expected = {174230596: InstrumentId.from_str("CLZ23.NYMEX")}
    assert instrument_provider.contract_id_to_instrument_id == expected


@pytest.mark.asyncio()
async def test_load_instrument_using_contract_id(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("EUR/USD.IDEALPRO")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.eurusd_forex_contract_details(),
    )

    # Act
    fx = await instrument_provider.find_with_contract_id(12087792)
    instrument_provider._client.stop()

    # Assert
    assert fx.id == instrument_id
    assert fx.asset_class == AssetClass.FX
    assert fx.multiplier == 1
    assert fx.price_increment == Price.from_str("0.00005")
    assert fx.price_precision == 5


@pytest.mark.skip(reason="Scope of test not clear!")
@pytest.mark.asyncio()
async def test_none_filters(instrument_provider):
    # Act, Arrange, Assert
    instrument_provider.load_all(None)


@pytest.mark.skip(reason="Scope of test not clear!")
@pytest.mark.asyncio()
async def test_instrument_filter_callable_none(mocker, instrument_provider):
    # Arrange
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.aapl_equity_contract_details(),
    )

    # Act
    await instrument_provider.load_async(
        IBContract(secType="STK", symbol="AAPL", exchange="NASDAQ"),
    )

    # Assert
    assert len(instrument_provider.get_all()) == 1


@pytest.mark.skip(reason="Scope of test not clear!")
@pytest.mark.asyncio()
async def test_instrument_filter_callable_option_filter(mocker, instrument_provider):
    # Arrange
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.tsla_option_contract_details(),
    )

    # Act
    new_cb = "tests.integration_tests.adapters.interactive_brokers.test_kit:filter_out_options"
    instrument_provider.config = msgspec.structs.replace(
        instrument_provider.config,
        filter_callable=new_cb,
    )
    await instrument_provider.load_async(instrument_id=None)
    option_instruments = instrument_provider.get_all()

    # Assert
    assert len(option_instruments) == 0
