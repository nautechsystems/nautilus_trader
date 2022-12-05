# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from unittest.mock import patch

import pytest
from ib_insync import Contract
from ib_insync import Ticker

from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.data import InteractiveBrokersDataClient
from nautilus_trader.adapters.interactive_brokers.factories import (
    InteractiveBrokersLiveDataClientFactory,
)
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.enums import BookType
from tests.integration_tests.adapters.interactive_brokers.base import InteractiveBrokersTestBase
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs


@pytest.mark.skip
class TestInteractiveBrokersData(InteractiveBrokersTestBase):
    def setup(self):
        super().setup()
        self.instrument = TestInstrumentProvider.aapl_equity()
        with patch("nautilus_trader.adapters.interactive_brokers.factories.get_cached_ib_client"):
            self.data_client: InteractiveBrokersDataClient = (
                InteractiveBrokersLiveDataClientFactory.create(
                    loop=self.loop,
                    name="IB",
                    config=InteractiveBrokersDataClientConfig(  # noqa: S106
                        username="test",
                        password="test",
                    ),
                    msgbus=self.msgbus,
                    cache=self.cache,
                    clock=self.clock,
                    logger=self.logger,
                )
            )
            assert isinstance(self.data_client, InteractiveBrokersDataClient)

    def instrument_setup(self, instrument, contract_details):
        self.data_client.instrument_provider.contract_details[
            instrument.id.value
        ] = contract_details
        self.data_client.instrument_provider.contract_id_to_instrument_id[
            contract_details.contract.conId
        ] = instrument.id
        self.data_client.instrument_provider.add(instrument)

    @pytest.mark.asyncio
    async def test_factory(self, event_loop):
        # Arrange
        # Act
        data_client = self.data_client

        # Assert
        assert data_client is not None

    @pytest.mark.asyncio
    async def test_subscribe_trade_ticks(self, event_loop):
        # Arrange
        instrument_aapl = IBTestDataStubs.instrument(symbol="AAPL")
        self.data_client.instrument_provider.contract_details[
            instrument_aapl.id.value
        ] = IBTestDataStubs.contract_details("AAPL")

        # Act
        with patch.object(self.data_client._client, "reqMktData") as mock:
            self.data_client.subscribe_trade_ticks(instrument_id=instrument_aapl.id)

        # Assert
        kwargs = mock.call_args.kwargs
        expected = {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
        }
        assert kwargs == expected

    @pytest.mark.asyncio
    async def test_subscribe_order_book_deltas(self, event_loop):
        # Arrange
        instrument = IBTestDataStubs.instrument(symbol="AAPL")
        self.instrument_setup(instrument, IBTestDataStubs.contract_details("AAPL"))

        # Act
        with patch.object(self.data_client._client, "reqMktDepth") as mock:
            self.data_client.subscribe_order_book_snapshots(
                instrument_id=instrument.id,
                book_type=BookType.L2_MBP,
            )

        # Assert
        kwargs = mock.call_args.kwargs
        expected = {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
            "numRows": 5,
        }
        assert kwargs == expected

    @pytest.mark.asyncio
    async def test_on_book_update(self, event_loop):
        # Arrange
        self.instrument_setup(
            IBTestDataStubs.instrument(symbol="EURUSD"),
            IBTestDataStubs.contract_details("EURUSD"),
        )

        # Act
        for ticker in IBTestDataStubs.market_depth(name="eurusd"):
            self.data_client._on_order_book_snapshot(ticker=ticker, book_type=BookType.L2_MBP)

    @pytest.mark.asyncio
    async def test_on_ticker_update(self, event_loop):
        # Arrange
        self.instrument_setup(
            IBTestDataStubs.instrument(symbol="EURUSD"),
            IBTestDataStubs.contract_details("EURUSD"),
        )

        # Act
        for ticker in IBTestDataStubs.tickers("eurusd"):
            self.data_client._on_trade_ticker_update(ticker=ticker)

    @pytest.mark.asyncio
    async def test_on_quote_tick_update(self, event_loop):
        # Arrange
        self.instrument_setup(
            IBTestDataStubs.instrument(symbol="EURUSD"),
            IBTestDataStubs.contract_details("EURUSD"),
        )
        contract = IBTestDataStubs.contract_details("EURUSD").contract
        ticker = Ticker(
            time=datetime.datetime(2022, 3, 4, 6, 8, 36, 992576, tzinfo=datetime.timezone.utc),
            bid=99.45,
            ask=99.5,
            bidSize=44600.0,
            askSize=29500.0,
        )

        # Act
        self.data_client._on_quote_tick_update(tick=ticker, contract=contract)

    @pytest.mark.asyncio
    async def test_on_quote_tick_update_nans(self, event_loop):
        # Arrange
        self.instrument_setup(self.instrument, IBTestDataStubs.contract_details("AAPL"))
        contract = IBTestDataStubs.contract_details("AAPL").contract
        ticker = Ticker(
            time=datetime.datetime(2022, 3, 4, 6, 8, 36, 992576, tzinfo=datetime.timezone.utc),
            bidSize=44600.0,
            askSize=29500.0,
        )
        data = []
        self.data_client._handle_data = data.append

        # Act
        self.data_client._on_quote_tick_update(tick=ticker, contract=contract)
        update = data[0]
        await asyncio.sleep(0)

        # Assert
        expected = QuoteTick.from_dict(
            {
                "type": "QuoteTick",
                "instrument_id": "AAPL.NASDAQ",
                "bid": "0.00",
                "ask": "0.00",
                "bid_size": "44600",
                "ask_size": "29500",
                "ts_event": 1646374116992576000,
                "ts_init": 1658919315437688375,
            },
        )
        assert update == expected
