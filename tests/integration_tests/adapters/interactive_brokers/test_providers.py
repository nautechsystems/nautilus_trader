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
import datetime
from unittest.mock import MagicMock

import pytest
from ibapi.contract import ContractDetails

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.config import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestProviderStubs


class TestIBInstrumentProvider:
    def setup(self):
        self.ib = MagicMock()
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.logger = Logger(clock=self.clock, level_stdout=LogLevel.DEBUG)
        self.provider = InteractiveBrokersInstrumentProvider(
            client=self.ib,
            logger=self.logger,
            config=InteractiveBrokersInstrumentProviderConfig(),
        )

    @staticmethod
    def async_return_value(value: object) -> asyncio.Future:
        future: asyncio.Future = asyncio.Future()
        future.set_result(value)
        return future

    def mock_ib_contract_calls(self, mocker, contract_details: ContractDetails):
        mocker.patch.object(
            self.provider._client,
            "get_contract_details",
            return_value=self.async_return_value([contract_details]),
        )

    @pytest.mark.asyncio
    async def test_load_equity_contract_instrument(self, mocker):
        # Arrange
        instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
        self.mock_ib_contract_calls(
            mocker=mocker,
            contract_details=IBTestProviderStubs.aapl_equity_contract_details(),
        )

        # Act
        await self.provider.load(IBContract(secType="STK", symbol="AAPL", exchange="NASDAQ"))
        equity = self.provider.find(instrument_id)

        # Assert
        assert InstrumentId(symbol=Symbol("AAPL"), venue=Venue("NASDAQ")) == equity.id
        assert equity.asset_class == AssetClass.EQUITY
        assert equity.asset_type == AssetType.SPOT
        assert 1 == equity.multiplier
        assert Price.from_str("0.01") == equity.price_increment
        assert 2, equity.price_precision

    @pytest.mark.asyncio
    async def test_load_futures_contract_instrument(self, mocker):
        # Arrange
        instrument_id = InstrumentId.from_str("CLZ3.NYMEX")
        self.mock_ib_contract_calls(
            mocker=mocker,
            contract_details=IBTestProviderStubs.cl_future_contract_details(),
        )

        # Act
        await self.provider.load(IBContract(secType="FUT", symbol="CLZ3", exchange="NYMEX"))
        future = self.provider.find(instrument_id)

        # Assert
        assert future.id == instrument_id
        assert future.asset_class == AssetClass.INDEX
        assert future.multiplier == 1000
        assert future.price_increment == Price.from_str("0.01")
        assert future.price_precision == 2

    @pytest.mark.asyncio
    async def test_load_options_contract_instrument(self, mocker):
        # Arrange
        instrument_id = InstrumentId.from_str("TSLA230120C00100000.MIAX")
        self.mock_ib_contract_calls(
            mocker=mocker,
            contract_details=IBTestProviderStubs.tsla_option_contract_details(),
        )

        # Act
        await self.provider.load(
            IBContract(secType="OPT", symbol="TSLA230120C00100000", exchange="MIAX"),
        )
        option = self.provider.find(instrument_id)

        # Assert
        assert option.id == instrument_id
        assert option.asset_class == AssetClass.EQUITY
        assert option.multiplier == 100
        assert option.expiry_date == datetime.date(2023, 1, 20)
        assert option.strike_price == Price.from_str("100.0")
        assert option.kind == OptionKind.CALL
        assert option.price_increment == Price.from_str("0.01")
        assert option.price_precision == 2

    @pytest.mark.asyncio
    async def test_load_forex_contract_instrument(self, mocker):
        # Arrange
        instrument_id = InstrumentId.from_str("EUR/USD.IDEALPRO")
        self.mock_ib_contract_calls(
            mocker=mocker,
            contract_details=IBTestProviderStubs.eurusd_forex_contract_details(),
        )

        # Act
        await self.provider.load(instrument_id)
        fx = self.provider.find(instrument_id)

        # Assert
        assert fx.id == instrument_id
        assert fx.asset_class == AssetClass.FX
        assert fx.multiplier == 1
        assert fx.price_increment == Price.from_str("0.00005")
        assert fx.price_precision == 5

    @pytest.mark.asyncio
    async def test_contract_id_to_instrument_id(self, mocker):
        # Arrange
        self.mock_ib_contract_calls(
            mocker=mocker,
            contract_details=IBTestProviderStubs.cl_future_contract_details(),
        )

        # Act
        await self.provider.load(IBContract(secType="FUT", symbol="CLZ3", exchange="NYMEX"))

        # Assert
        expected = {174230596: InstrumentId.from_str("CLZ3.NYMEX")}
        assert self.provider.contract_id_to_instrument_id == expected

    @pytest.mark.asyncio
    async def test_none_filters(self):
        # Act, Arrange, Assert
        self.provider.load_all(None)

    @pytest.mark.asyncio
    async def test_instrument_filter_callable_none(self, mocker):
        # Arrange
        self.mock_ib_contract_calls(
            mocker=mocker,
            contract_details=IBTestProviderStubs.aapl_equity_contract_details(),
        )

        # Act
        await self.provider.load(IBContract(secType="STK", symbol="AAPL", exchange="AMEX"))

        # Assert
        assert len(self.provider.get_all()) == 1

    @pytest.mark.skip(reason="Configs now immutable, limx0 to fix")
    @pytest.mark.asyncio
    async def test_instrument_filter_callable_option_filter(self, mocker):
        # Arrange
        self.mock_ib_contract_calls(
            mocker=mocker,
            contract_details=IBTestProviderStubs.tsla_option_contract_details(),
        )

        # Act
        self.provider.config.filter_callable = (
            "tests.integration_tests.adapters.interactive_brokers.test_kit:filter_out_options"
        )
        await self.provider.load()
        option_instruments = self.provider.get_all()

        # Assert
        assert len(option_instruments) == 0
