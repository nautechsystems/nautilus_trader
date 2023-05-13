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

import datetime
from unittest.mock import AsyncMock

import msgspec.structs
import pytest
from ib_insync import CFD
from ib_insync import Bond
from ib_insync import ContractDetails
from ib_insync import Crypto
from ib_insync import Forex
from ib_insync import Future
from ib_insync import Option
from ib_insync import Stock

from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestProviderStubs


pytestmark = pytest.mark.skip(reason="Skip due currently flaky mocks")


def mock_ib_contract_calls(mocker, instrument_provider, contract_details: ContractDetails):
    mocker.patch.object(
        instrument_provider._client,
        "reqContractDetailsAsync",
        side_effect=AsyncMock(return_value=[contract_details]),
    )
    mocker.patch.object(
        instrument_provider._client,
        "qualifyContractsAsync",
        side_effect=AsyncMock(return_value=[contract_details]),
    )


@pytest.mark.parametrize(
    ("filters", "expected"),
    [
        (
            {"secType": "STK", "symbol": "AMD", "exchange": "SMART", "currency": "USD"},
            Stock("AMD", "SMART", "USD"),
        ),
        (
            {
                "secType": "STK",
                "symbol": "INTC",
                "exchange": "SMART",
                "primaryExchange": "NASDAQ",
                "currency": "USD",
            },
            Stock("INTC", "SMART", "USD", primaryExchange="NASDAQ"),
        ),
        (
            {"secType": "CASH", "symbol": "EUR", "currency": "USD", "exchange": "IDEALPRO"},
            Forex(symbol="EUR", currency="USD"),
        ),  # EUR/USD,
        ({"secType": "CFD", "symbol": "IBUS30"}, CFD("IBUS30")),
        (
            {
                "secType": "FUT",
                "symbol": "ES",
                "exchange": "GLOBEX",
                "lastTradeDateOrContractMonth": "20180921",
            },
            Future("ES", "20180921", "GLOBEX"),
        ),
        (
            {
                "secType": "OPT",
                "symbol": "SPY",
                "exchange": "SMART",
                "lastTradeDateOrContractMonth": "20170721",
                "strike": 240,
                "right": "C",
            },
            Option("SPY", "20170721", 240, "C", "SMART"),
        ),
        (
            {"secType": "BOND", "secIdType": "ISIN", "secId": "US03076KAA60"},
            Bond(secIdType="ISIN", secId="US03076KAA60"),
        ),
        (
            {"secType": "CRYPTO", "symbol": "BTC", "exchange": "PAXOS", "currency": "USD"},
            Crypto("BTC", "PAXOS", "USD"),
        ),
    ],
)
def test_parse_contract(filters, expected, instrument_provider):
    result = instrument_provider._parse_contract(**filters)
    fields = [f.name for f in expected.__dataclass_fields__.values() if getattr(expected, f.name)]
    for f in fields:
        assert getattr(result, f) == getattr(expected, f)


@pytest.mark.asyncio()
async def test_load_equity_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("AAPL.AMEX")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestProviderStubs.aapl_equity_contract_details(),
    )

    # Act
    await instrument_provider.load(secType="STK", symbol="AAPL", exchange="AMEX")
    equity = instrument_provider.find(instrument_id)

    # Assert
    assert InstrumentId(symbol=Symbol("AAPL"), venue=Venue("AMEX")) == equity.id
    assert equity.asset_class == AssetClass.EQUITY
    assert equity.asset_type == AssetType.SPOT
    assert equity.multiplier == 1
    assert Price.from_str("0.01") == equity.price_increment
    assert 2, equity.price_precision


@pytest.mark.asyncio()
async def test_load_futures_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("CLZ3.NYMEX")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestProviderStubs.cl_future_contract_details(),
    )

    # Act
    await instrument_provider.load(symbol="CLZ3", exchange="NYMEX")
    future = instrument_provider.find(instrument_id)

    # Assert
    assert future.id == instrument_id
    assert future.asset_class == AssetClass.INDEX
    assert future.multiplier == 1000
    assert future.price_increment == Price.from_str("0.01")
    assert future.price_precision == 2


@pytest.mark.asyncio()
async def test_load_options_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("TSLA230120C00100000.MIAX")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestProviderStubs.tsla_option_contract_details(),
    )

    # Act
    await instrument_provider.load(secType="OPT", symbol="TSLA230120C00100000", exchange="MIAX")
    option = instrument_provider.find(instrument_id)

    # Assert
    assert option.id == instrument_id
    assert option.asset_class == AssetClass.EQUITY
    assert option.multiplier == 100
    assert option.expiry_date == datetime.date(2023, 1, 20)
    assert option.strike_price == Price.from_str("100.0")
    assert option.kind == OptionKind.CALL
    assert option.price_increment == Price.from_str("0.01")
    assert option.price_precision == 2


@pytest.mark.asyncio()
async def test_load_forex_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("EUR/USD.IDEALPRO")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestProviderStubs.eurusd_forex_contract_details(),
    )

    # Act
    await instrument_provider.load(secType="CASH", symbol="EURUSD", exchange="IDEALPRO")
    fx = instrument_provider.find(instrument_id)

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
        contract_details=IBTestProviderStubs.cl_future_contract_details(),
    )

    # Act
    await instrument_provider.load(symbol="CLZ3", exchange="NYMEX")

    # Assert
    expected = {174230596: InstrumentId.from_str("CLZ3.NYMEX")}
    assert instrument_provider.contract_id_to_instrument_id == expected


@pytest.mark.asyncio()
async def test_none_filters(instrument_provider):
    # Act, Arrange, Assert
    instrument_provider.load_all(None)


@pytest.mark.asyncio()
async def test_instrument_filter_callable_none(mocker, instrument_provider):
    # Arrange
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestProviderStubs.aapl_equity_contract_details(),
    )

    # Act
    await instrument_provider.load()

    # Assert
    assert len(instrument_provider.get_all()) == 1


@pytest.mark.asyncio()
async def test_instrument_filter_callable_option_filter(mocker, instrument_provider):
    # Arrange
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestProviderStubs.tsla_option_contract_details(),
    )

    # Act
    new_cb = "tests.integration_tests.adapters.interactive_brokers.test_kit:filter_out_options"
    instrument_provider.config = msgspec.structs.replace(
        instrument_provider.config,
        filter_callable=new_cb,
    )
    await instrument_provider.load()
    option_instruments = instrument_provider.get_all()

    # Assert
    assert len(option_instruments) == 0
