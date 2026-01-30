# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from unittest.mock import AsyncMock

import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.execution import BinanceSpotExecutionClient
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestBinanceSpotExecutionClient:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()

        self.trader_id = TestIdStubs.trader_id()
        self.venue = BINANCE_VENUE
        self.account_id = AccountId(f"{self.venue.value}-001")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.http_client = BinanceHttpClient(
            clock=self.clock,
            api_key="SOME_BINANCE_API_KEY",
            api_secret="SOME_BINANCE_API_SECRET",
            base_url="https://api.binance.com/",  # Spot/Margin
        )

        self.provider = BinanceSpotInstrumentProvider(
            client=self.http_client,
            clock=self.clock,
            config=InstrumentProviderConfig(load_all=True),
        )

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_client = BinanceSpotExecutionClient(
            loop=self.loop,
            client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.provider,
            base_url_ws="",  # Not required for testing
            config=BinanceExecClientConfig(),
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
        )

        return

    @pytest.mark.asyncio
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

    @pytest.mark.asyncio
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
        await eventually(lambda: mock_send_request.call_args)

        # Assert
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/api/v3/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"
        assert request[1]["payload"]["type"] == "MARKET"
        assert request[1]["payload"]["side"] == "BUY"
        assert request[1]["payload"]["quantity"] == "1"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["recvWindow"] == "5000"

    @pytest.mark.asyncio
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
        await eventually(lambda: mock_send_request.call_args)

        # Assert
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/api/v3/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"
        assert request[1]["payload"]["side"] == "BUY"
        assert request[1]["payload"]["type"] == "LIMIT"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None

    @pytest.mark.asyncio
    async def test_submit_limit_order_with_price_match_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

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
            params={"price_match": "QUEUE"},
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "only supported for Binance futures" in reason

    @pytest.mark.asyncio
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
        await eventually(lambda: mock_send_request.call_args)

        # Assert
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/api/v3/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"
        assert request[1]["payload"]["side"] == "BUY"
        assert request[1]["payload"]["type"] == "STOP_LOSS_LIMIT"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["price"] == "10050.80"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["stopPrice"] == "10050.00"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None

    @pytest.mark.asyncio
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
        await eventually(lambda: mock_send_request.call_args)

        # Assert
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/api/v3/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"
        assert request[1]["payload"]["side"] == "SELL"
        assert request[1]["payload"]["type"] == "TAKE_PROFIT_LIMIT"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["price"] == "10100.00"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["stopPrice"] == "10099.00"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None

    @pytest.mark.asyncio
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

        # Assert
        await eventually(lambda: mock_query_order.called)

    @pytest.mark.asyncio
    async def test_cancel_all_orders_with_open_orders_uses_batch_cancel(self, mocker):
        """
        Test that _cancel_all_orders uses batch cancel when strategy owns all orders.
        """
        # Arrange
        mock_cancel_orders_batch = mocker.patch.object(
            self.exec_client,
            "_cancel_orders_batch",
            new_callable=AsyncMock,
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("3000.00"),
        )
        self.cache.add_order(limit_order, None)
        limit_order.apply(TestEventStubs.order_submitted(limit_order))
        self.cache.update_order(limit_order)
        limit_order.apply(TestEventStubs.order_accepted(limit_order))
        self.cache.update_order(limit_order)

        command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._cancel_all_orders(command)

        # Assert
        mock_cancel_orders_batch.assert_called_once()
        assert limit_order in mock_cancel_orders_batch.call_args[0][1]

    @pytest.mark.asyncio
    async def test_cancel_all_orders_with_submitted_orders_uses_batch_cancel(self, mocker):
        """
        Test that _cancel_all_orders includes SUBMITTED (inflight) orders for spot.
        """
        # Arrange
        mock_cancel_orders_batch = mocker.patch.object(
            self.exec_client,
            "_cancel_orders_batch",
            new_callable=AsyncMock,
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("3000.00"),
        )
        self.cache.add_order(limit_order, None)
        limit_order.apply(TestEventStubs.order_submitted(limit_order))
        self.cache.update_order(limit_order)

        command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._cancel_all_orders(command)

        # Assert
        mock_cancel_orders_batch.assert_called_once()
        assert limit_order in mock_cancel_orders_batch.call_args[0][1]

    @pytest.mark.asyncio
    async def test_cancel_all_orders_multi_strategy_uses_individual_cancel(self, mocker):
        """
        Test that _cancel_all_orders falls back to individual cancels when multiple
        strategies have orders for the same instrument.
        """
        # Arrange
        mock_cancel_orders_for_strategy = mocker.patch.object(
            self.exec_client,
            "_cancel_orders_for_strategy",
            new_callable=AsyncMock,
        )
        mock_cancel_orders_batch = mocker.patch.object(
            self.exec_client,
            "_cancel_orders_batch",
            new_callable=AsyncMock,
        )

        strategy_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("3000.00"),
        )
        self.cache.add_order(strategy_order, None)
        strategy_order.apply(TestEventStubs.order_submitted(strategy_order))
        self.cache.update_order(strategy_order)
        strategy_order.apply(TestEventStubs.order_accepted(strategy_order))
        self.cache.update_order(strategy_order)

        other_strategy = Strategy(config=StrategyConfig(strategy_id="other"))
        other_strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        other_order = other_strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(5),
            price=Price.from_str("3100.00"),
        )
        self.cache.add_order(other_order, None)
        other_order.apply(TestEventStubs.order_submitted(other_order))
        self.cache.update_order(other_order)
        other_order.apply(TestEventStubs.order_accepted(other_order))
        self.cache.update_order(other_order)

        command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._cancel_all_orders(command)

        # Assert - should use individual cancel, not batch
        mock_cancel_orders_for_strategy.assert_called_once()
        mock_cancel_orders_batch.assert_not_called()

    @pytest.mark.asyncio
    async def test_cancel_orders_batch_failure_emits_cancel_rejected(self, mocker):
        """
        Test that batch cancel failure emits OrderCancelRejected for each order.
        """
        # Arrange
        mock_generate_cancel_rejected = mocker.patch.object(
            self.exec_client,
            "generate_order_cancel_rejected",
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("3000.00"),
        )
        self.cache.add_order(limit_order, None)
        limit_order.apply(TestEventStubs.order_submitted(limit_order))
        self.cache.update_order(limit_order)
        limit_order.apply(TestEventStubs.order_accepted(limit_order))
        self.cache.update_order(limit_order)

        mock_retry_manager = mocker.MagicMock()
        mock_retry_manager.result = False
        mock_retry_manager.message = "Rate limit exceeded"
        mock_retry_manager.run = AsyncMock()

        mocker.patch.object(
            self.exec_client._retry_manager_pool,
            "acquire",
            new_callable=AsyncMock,
            return_value=mock_retry_manager,
        )
        mocker.patch.object(
            self.exec_client._retry_manager_pool,
            "release",
            new_callable=AsyncMock,
        )

        # Act
        await self.exec_client._cancel_orders_batch(
            ETHUSDT_BINANCE.id,
            [limit_order],
        )

        # Assert
        mock_generate_cancel_rejected.assert_called_once()
        call_args = mock_generate_cancel_rejected.call_args
        assert call_args[0][0] == limit_order.strategy_id
        assert call_args[0][1] == limit_order.instrument_id
        assert call_args[0][2] == limit_order.client_order_id
        assert call_args[0][4] == "Rate limit exceeded"

    @pytest.mark.asyncio
    async def test_cancel_orders_batch_unknown_order_does_not_emit_rejected(self, mocker):
        """
        Test that 'Unknown order sent' error does not emit OrderCancelRejected.
        """
        # Arrange
        mock_generate_cancel_rejected = mocker.patch.object(
            self.exec_client,
            "generate_order_cancel_rejected",
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("3000.00"),
        )
        self.cache.add_order(limit_order, None)
        limit_order.apply(TestEventStubs.order_submitted(limit_order))
        self.cache.update_order(limit_order)
        limit_order.apply(TestEventStubs.order_accepted(limit_order))
        self.cache.update_order(limit_order)

        mock_retry_manager = mocker.MagicMock()
        mock_retry_manager.result = False
        mock_retry_manager.message = "Unknown order sent"
        mock_retry_manager.run = AsyncMock()

        mocker.patch.object(
            self.exec_client._retry_manager_pool,
            "acquire",
            new_callable=AsyncMock,
            return_value=mock_retry_manager,
        )
        mocker.patch.object(
            self.exec_client._retry_manager_pool,
            "release",
            new_callable=AsyncMock,
        )

        # Act
        await self.exec_client._cancel_orders_batch(
            ETHUSDT_BINANCE.id,
            [limit_order],
        )

        # Assert
        mock_generate_cancel_rejected.assert_not_called()
