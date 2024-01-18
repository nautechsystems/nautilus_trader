# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from decimal import Decimal

import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.futures.execution import BinanceFuturesExecutionClient
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


@pytest.mark.skip(reason="WIP")
class TestBinanceFuturesExecutionClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.logger = Logger(bypass=True)

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
            clock=self.clock,
            logger=self.logger,
            key="SOME_BINANCE_API_KEY",
            secret="SOME_BINANCE_API_SECRET",
            base_url="https://api.binance.com/",  # Spot/Margin
        )

        self.provider = BinanceFuturesInstrumentProvider(
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

        self.exec_client = BinanceFuturesExecutionClient(
            loop=self.loop,
            client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            instrument_provider=self.provider,
            account_type=BinanceAccountType.USDT_FUTURE,
        )

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    @pytest.mark.asyncio()
    async def test_submit_market_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
        assert request[1] == "/fapi/v1/order"
        assert request[2]["symbol"] == "ETHUSDT"  # -PERP was stripped
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
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
        assert request[1] == "/fapi/v1/order"
        assert request[2]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[2]["side"] == "BUY"
        assert request[2]["type"] == "LIMIT"
        assert request[2]["quantity"] == "10"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["recvWindow"] == "5000"
        assert request[2]["signature"] is not None

    @pytest.mark.asyncio()
    async def test_submit_limit_post_only_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.80"),
            post_only=True,
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
        assert request[1] == "/fapi/v1/order"
        assert request[2]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[2]["side"] == "BUY"
        assert request[2]["type"] == "LIMIT"
        assert request[2]["timeInForce"] == "GTX"
        assert request[2]["quantity"] == "10"
        assert request[2]["price"] == "10050.80"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["recvWindow"] == "5000"
        assert request[2]["signature"] is not None

    @pytest.mark.asyncio()
    async def test_submit_stop_market_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trigger_price=Price.from_str("10099.00"),
            reduce_only=True,
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
        assert request[1] == "/fapi/v1/order"
        assert request[2]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[2]["side"] == "SELL"
        assert request[2]["type"] == "STOP_MARKET"
        assert request[2]["timeInForce"] == "GTC"
        assert request[2]["quantity"] == "10"
        assert request[2]["reduceOnly"] == "True"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["stopPrice"] == "10099.00"
        assert request[2]["workingType"] == "CONTRACT_PRICE"
        assert request[2]["recvWindow"] == "5000"
        assert request[2]["signature"] is not None

    @pytest.mark.asyncio()
    async def test_submit_stop_limit_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.stop_limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.80"),
            trigger_price=Price.from_str("10050.00"),
            trigger_type=TriggerType.MARK_PRICE,
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
        assert request[1] == "/fapi/v1/order"
        assert request[2]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[2]["side"] == "BUY"
        assert request[2]["type"] == "STOP"
        assert request[2]["timeInForce"] == "GTC"
        assert request[2]["quantity"] == "10"
        assert request[2]["price"] == "10050.80"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["stopPrice"] == "10050.00"
        assert request[2]["workingType"] == "MARK_PRICE"
        assert request[2]["recvWindow"] == "5000"
        assert request[2]["signature"] is not None

    @pytest.mark.asyncio()
    async def test_submit_market_if_touched_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.market_if_touched(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
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
        assert request[1] == "/fapi/v1/order"
        assert request[2]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[2]["side"] == "SELL"
        assert request[2]["type"] == "TAKE_PROFIT_MARKET"
        assert request[2]["timeInForce"] == "GTC"
        assert request[2]["quantity"] == "10"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["stopPrice"] == "10099.00"
        assert request[2]["recvWindow"] == "5000"
        assert request[2]["signature"] is not None

    @pytest.mark.asyncio()
    async def test_submit_limit_if_touched_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.limit_if_touched(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.80"),
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
        assert request[1] == "/fapi/v1/order"
        assert request[2]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[2]["side"] == "SELL"
        assert request[2]["type"] == "TAKE_PROFIT"
        assert request[2]["timeInForce"] == "GTC"
        assert request[2]["quantity"] == "10"
        assert request[2]["price"] == "10050.80"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["stopPrice"] == "10099.00"
        assert request[2]["workingType"] == "CONTRACT_PRICE"
        assert request[2]["recvWindow"] == "5000"
        assert request[2]["signature"] is not None

    @pytest.mark.asyncio()
    async def test_trailing_stop_market_order(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trailing_offset=Decimal(100),
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            trigger_price=Price.from_str("10000.00"),
            trigger_type=TriggerType.MARK_PRICE,
            reduce_only=True,
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
        assert request[1] == "/fapi/v1/order"
        assert request[2]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[2]["side"] == "SELL"
        assert request[2]["type"] == "TRAILING_STOP_MARKET"
        assert request[2]["timeInForce"] == "GTC"
        assert request[2]["quantity"] == "10"
        assert request[2]["reduceOnly"] == "True"
        assert request[2]["newClientOrderId"] is not None
        assert request[2]["activationPrice"] == "10000.00"
        assert request[2]["callbackRate"] == "1"
        assert request[2]["workingType"] == "MARK_PRICE"
        assert request[2]["recvWindow"] == "5000"
        assert request[2]["signature"] is not None
