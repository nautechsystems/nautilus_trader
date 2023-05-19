# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
import pkgutil

import aiohttp
import msgspec
import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.execution import BinanceSpotExecutionClient
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestBinanceSpotExecutionClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.logger = Logger(clock=self.clock, bypass=True)

        self.trader_id = TestIdStubs.trader_id()
        self.venue = BINANCE_VENUE
        self.account_id = AccountId(f"{self.venue.value}-001")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestComponentStubs.cache()

        self.http_client = BinanceHttpClient(
            loop=asyncio.get_event_loop(),
            clock=self.clock,
            logger=self.logger,
            key="SOME_BINANCE_API_KEY",
            secret="SOME_BINANCE_API_SECRET",
        )

        self.provider = BinanceSpotInstrumentProvider(
            client=self.http_client,
            logger=self.logger,
            clock=self.clock,
            config=InstrumentProviderConfig(load_all=True),
        )

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BinanceSpotExecutionClient(
            loop=self.loop,
            client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            instrument_provider=self.provider,
            account_type=BinanceAccountType.SPOT,
        )

        self.exec_engine.register_client(self.exec_client)

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    @pytest.mark.skip(reason="WIP")
    @pytest.mark.asyncio()
    async def test_connect(self, monkeypatch):
        # Arrange: prepare data for monkey patch
        response1 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_wallet_trading_fee.json",
        )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_market_exchange_info.json",
        )

        response3 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_wallet_account.json",
        )

        response4 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_streams_listen_key.json",
        )

        http_responses = [response4, response3, response2, response1]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # (needed for mock)
            http_method: str,  # (needed for mock)
            url_path: str,  # (needed for mock)
            payload: dict[str, str],  # (needed for mock)
        ) -> bytes:
            response = msgspec.json.decode(http_responses.pop())
            return response

        # Mock coroutine for patch
        async def mock_ws_connect(
            self,  # (needed for mock)
            url: str,  # (needed for mock)
        ) -> bytes:
            return b"connected"

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        monkeypatch.setattr(
            target=aiohttp.ClientSession,
            name="ws_connect",
            value=mock_ws_connect,
        )

        # Act
        self.exec_client.connect()
        await asyncio.sleep(1)

        # Assert
        assert self.exec_client.is_connected

    @pytest.mark.asyncio()
    async def test_submit_unsupported_order_logs_error(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.market_to_limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await asyncio.sleep(0.3)

        # Assert
        assert mock_send_request.call_args is None

    @pytest.mark.asyncio()
    async def test_submit_market_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.market(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await asyncio.sleep(0.3)

        # Assert
        request = mock_send_request.call_args[0]
        assert request[0] == "POST"
        assert request[1] == "/api/v3/order"
        assert request[2]["symbol"] == "ETHUSDT"
        assert request[2]["type"] == "MARKET"
        assert request[2]["side"] == "BUY"
        assert request[2]["quantity"] == "1"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["recvWindow"] == "5000"

    @pytest.mark.asyncio()
    async def test_submit_limit_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.80"),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await asyncio.sleep(0.3)

        # Assert
        request = mock_send_request.call_args[0]
        assert request[0] == "POST"
        assert request[1] == "/api/v3/order"
        assert request[2]["symbol"] == "ETHUSDT"
        assert request[2]["side"] == "BUY"
        assert request[2]["type"] == "LIMIT"
        assert request[2]["quantity"] == "10"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["recvWindow"] == "5000"
        assert request[2]["signature"] is not None

    @pytest.mark.asyncio()
    async def test_submit_stop_limit_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.stop_limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.80"),
            trigger_price=Price.from_str("10050.00"),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await asyncio.sleep(0.3)

        # Assert
        request = mock_send_request.call_args[0]
        assert request[0] == "POST"
        assert request[1] == "/api/v3/order"
        assert request[2]["symbol"] == "ETHUSDT"
        assert request[2]["side"] == "BUY"
        assert request[2]["type"] == "STOP_LOSS_LIMIT"
        assert request[2]["timeInForce"] == "GTC"
        assert request[2]["quantity"] == "10"
        assert request[2]["price"] == "10050.80"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["stopPrice"] == "10050.00"
        assert request[2]["recvWindow"] == "5000"
        assert request[2]["signature"] is not None

    @pytest.mark.asyncio()
    async def test_submit_limit_if_touched_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.limit_if_touched(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10100.00"),
            trigger_price=Price.from_str("10099.00"),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await asyncio.sleep(0.3)

        # Assert
        request = mock_send_request.call_args[0]
        assert request[0] == "POST"
        assert request[1] == "/api/v3/order"
        assert request[2]["symbol"] == "ETHUSDT"
        assert request[2]["side"] == "SELL"
        assert request[2]["type"] == "TAKE_PROFIT_LIMIT"
        assert request[2]["timeInForce"] == "GTC"
        assert request[2]["quantity"] == "10"
        assert request[2]["price"] == "10100.00"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["stopPrice"] == "10099.00"
        assert request[2]["recvWindow"] == "5000"
        assert request[2]["signature"] is not None

    @pytest.mark.asyncio()
    async def test_query_order(self, mocker):
        # Arrange
        mock_query_order = mocker.patch(
            target="nautilus_trader.adapters.binance.spot.execution.BinanceSpotExecutionClient.query_order",
        )

        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.80"),
        )

        # Act
        self.strategy.query_order(order)
        await asyncio.sleep(0.3)

        # Assert
        assert mock_query_order.called
