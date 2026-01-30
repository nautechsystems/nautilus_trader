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
import json
from decimal import Decimal
from unittest.mock import AsyncMock

import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.futures.execution import BinanceFuturesExecutionClient
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAccountInfo
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAlgoOrder
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesSymbolConfig
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId
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


ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestBinanceFuturesExecutionClient:
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

        self.provider = BinanceFuturesInstrumentProvider(
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

        self.exec_client = BinanceFuturesExecutionClient(
            loop=self.loop,
            client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.provider,
            base_url_ws="",
            config=BinanceExecClientConfig(),
            account_type=BinanceAccountType.USDT_FUTURES,
        )

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
    @pytest.mark.parametrize(
        ("_is_dual_side_position", "position_id", "expected"),
        [
            # One-way mode
            (False, None, "BOTH"),
            (False, TestIdStubs.position_id(), "BOTH"),
            (False, TestIdStubs.position_id_long(), "BOTH"),
            (False, TestIdStubs.position_id_short(), "BOTH"),
            (False, TestIdStubs.position_id_both(), "BOTH"),
            # Hedge mode
            (True, TestIdStubs.position_id_long(), "LONG"),
            (True, TestIdStubs.position_id_short(), "SHORT"),
            # (True, TestIdStubs.position_id_both(), "BOTH"),
        ],
    )
    async def test_submit_market_order(self, mocker, _is_dual_side_position, position_id, expected):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        # For one-way mode: _is_dual_side_position is False
        # For hedge mode: _is_dual_side_position is True
        mocker.patch.object(self.exec_client, "_is_dual_side_position", _is_dual_side_position)

        order = self.strategy.order_factory.market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1),
        )
        self.cache.add_order(order, None)
        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=position_id,
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
        assert request[0][1] == "/fapi/v1/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["type"] == "MARKET"
        assert request[1]["payload"]["side"] == "BUY"
        assert request[1]["payload"]["quantity"] == "1"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        ("_is_dual_side_position", "position_id", "expected"),
        [
            # One-way mode
            (False, None, "BOTH"),
            (False, TestIdStubs.position_id(), "BOTH"),
            (False, TestIdStubs.position_id_long(), "BOTH"),
            (False, TestIdStubs.position_id_short(), "BOTH"),
            (False, TestIdStubs.position_id_both(), "BOTH"),
            # Hedge mode
            (True, TestIdStubs.position_id_long(), "LONG"),
            (True, TestIdStubs.position_id_short(), "SHORT"),
            # (True, TestIdStubs.position_id_both(), "BOTH"),
        ],
    )
    async def test_submit_limit_order(self, mocker, _is_dual_side_position, position_id, expected):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mocker.patch.object(self.exec_client, "_is_dual_side_position", _is_dual_side_position)

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
            position_id=position_id,
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
        assert request[0][1] == "/fapi/v1/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "BUY"
        assert request[1]["payload"]["type"] == "LIMIT"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio
    async def test_submit_limit_order_with_price_match(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mocker.patch.object(self.exec_client, "_is_dual_side_position", True)

        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(2),
            price=Price.from_str("42000"),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=TestIdStubs.position_id_long(),
            order=order,
            command_id=UUID4(),
            ts_init=0,
            params={"price_match": "queue"},
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await eventually(lambda: mock_send_request.call_args)

        # Assert
        request = mock_send_request.call_args
        payload = request[1]["payload"]
        assert payload["priceMatch"] == "QUEUE"
        assert payload.get("price") is None
        assert payload["timeInForce"] == "GTC"

    @pytest.mark.asyncio
    async def test_submit_limit_order_with_price_match_post_only_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(1),
            price=Price.from_str("35000"),
            post_only=True,
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=TestIdStubs.position_id_short(),
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
        assert "post-only" in reason

    @pytest.mark.asyncio
    async def test_submit_limit_order_with_price_match_display_qty_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(3),
            price=Price.from_str("28000"),
            display_qty=Quantity.from_int(1),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=TestIdStubs.position_id_long(),
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
        assert "iceberg" in reason

    @pytest.mark.asyncio
    async def test_submit_market_order_with_quote_quantity_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100),
            quote_quantity=True,
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
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        denied_kwargs = mock_generate_denied.call_args.kwargs
        assert denied_kwargs["client_order_id"] == order.client_order_id
        assert denied_kwargs["reason"] == "UNSUPPORTED_QUOTE_QUANTITY"

    @pytest.mark.asyncio
    async def test_submit_market_order_with_price_match_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(1),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=TestIdStubs.position_id_short(),
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
        assert "not supported" in reason

    @pytest.mark.asyncio
    async def test_submit_limit_order_with_invalid_price_match_value_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(1),
            price=Price.from_str("33000"),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=TestIdStubs.position_id_short(),
            order=order,
            command_id=UUID4(),
            ts_init=0,
            params={"price_match": "invalid"},
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "not one of" in reason

    @pytest.mark.asyncio
    async def test_submit_limit_order_with_non_string_price_match_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(2),
            price=Price.from_str("30000"),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=TestIdStubs.position_id_long(),
            order=order,
            command_id=UUID4(),
            ts_init=0,
            params={"price_match": 123},
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "string value" in reason

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        ("_is_dual_side_position", "position_id", "expected"),
        [
            # One-way mode
            (False, None, "BOTH"),
            (False, TestIdStubs.position_id(), "BOTH"),
            (False, TestIdStubs.position_id_long(), "BOTH"),
            (False, TestIdStubs.position_id_short(), "BOTH"),
            (False, TestIdStubs.position_id_both(), "BOTH"),
            # Hedge mode
            (True, TestIdStubs.position_id_long(), "LONG"),
            (True, TestIdStubs.position_id_short(), "SHORT"),
            # (True, TestIdStubs.position_id_both(), "BOTH"),
        ],
    )
    async def test_submit_limit_post_only_order(
        self,
        mocker,
        _is_dual_side_position,
        position_id,
        expected,
    ):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mocker.patch.object(self.exec_client, "_is_dual_side_position", _is_dual_side_position)

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
            position_id=position_id,
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
        assert request[0][1] == "/fapi/v1/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "BUY"
        assert request[1]["payload"]["type"] == "LIMIT"
        assert request[1]["payload"]["timeInForce"] == "GTX"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["price"] == "10050.80"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        ("_is_dual_side_position", "position_id", "expected"),
        [
            # One-way mode
            (False, None, "BOTH"),
            (False, TestIdStubs.position_id(), "BOTH"),
            (False, TestIdStubs.position_id_long(), "BOTH"),
            (False, TestIdStubs.position_id_short(), "BOTH"),
            (False, TestIdStubs.position_id_both(), "BOTH"),
            # Hedge mode
            (True, TestIdStubs.position_id_long(), "LONG"),
            (True, TestIdStubs.position_id_short(), "SHORT"),
            # (True, TestIdStubs.position_id_both(), "BOTH"),
        ],
    )
    async def test_submit_stop_market_order(
        self,
        mocker,
        _is_dual_side_position,
        position_id,
        expected,
    ):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mocker.patch.object(self.exec_client, "_is_dual_side_position", _is_dual_side_position)
        mocker.patch.object(self.exec_client, "_use_reduce_only", not _is_dual_side_position)

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
            position_id=position_id,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await eventually(lambda: mock_send_request.call_args)

        # Assert
        # As of 2025-12-09, conditional orders use the algo order endpoint
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/algoOrder"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "SELL"
        assert request[1]["payload"]["type"] == "STOP_MARKET"
        assert request[1]["payload"]["algoType"] == "CONDITIONAL"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        if _is_dual_side_position:
            assert "reduceOnly" not in request[1]["payload"]
        else:
            assert request[1]["payload"]["reduceOnly"] == "True"
        assert request[1]["payload"]["clientAlgoId"] is not None
        assert request[1]["payload"]["triggerPrice"] == "10099.00"
        assert request[1]["payload"]["workingType"] == "CONTRACT_PRICE"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        ("_is_dual_side_position", "position_id", "expected"),
        [
            # One-way mode
            (False, None, "BOTH"),
            (False, TestIdStubs.position_id(), "BOTH"),
            (False, TestIdStubs.position_id_long(), "BOTH"),
            (False, TestIdStubs.position_id_short(), "BOTH"),
            (False, TestIdStubs.position_id_both(), "BOTH"),
            # Hedge mode
            (True, TestIdStubs.position_id_long(), "LONG"),
            (True, TestIdStubs.position_id_short(), "SHORT"),
            # (True, TestIdStubs.position_id_both(), "BOTH"),
        ],
    )
    async def test_submit_stop_limit_order(
        self,
        mocker,
        _is_dual_side_position,
        position_id,
        expected,
    ):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mocker.patch.object(self.exec_client, "_is_dual_side_position", _is_dual_side_position)

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
            position_id=position_id,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await eventually(lambda: mock_send_request.call_args)

        # Assert
        # As of 2025-12-09, conditional orders use the algo order endpoint
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/algoOrder"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "BUY"
        assert request[1]["payload"]["type"] == "STOP"
        assert request[1]["payload"]["algoType"] == "CONDITIONAL"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["price"] == "10050.80"
        assert request[1]["payload"]["clientAlgoId"] is not None
        assert request[1]["payload"]["triggerPrice"] == "10050.00"
        assert request[1]["payload"]["workingType"] == "MARK_PRICE"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        ("_is_dual_side_position", "position_id", "expected"),
        [
            # One-way mode
            (False, None, "BOTH"),
            (False, TestIdStubs.position_id(), "BOTH"),
            (False, TestIdStubs.position_id_long(), "BOTH"),
            (False, TestIdStubs.position_id_short(), "BOTH"),
            (False, TestIdStubs.position_id_both(), "BOTH"),
            # Hedge mode
            (True, TestIdStubs.position_id_long(), "LONG"),
            (True, TestIdStubs.position_id_short(), "SHORT"),
            # (True, TestIdStubs.position_id_both(), "BOTH"),
        ],
    )
    async def test_submit_market_if_touched_order(
        self,
        mocker,
        _is_dual_side_position,
        position_id,
        expected,
    ):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mocker.patch.object(self.exec_client, "_is_dual_side_position", _is_dual_side_position)

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
            position_id=position_id,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await eventually(lambda: mock_send_request.call_args)

        # Assert
        # As of 2025-12-09, conditional orders use the algo order endpoint
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/algoOrder"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "SELL"
        assert request[1]["payload"]["type"] == "TAKE_PROFIT_MARKET"
        assert request[1]["payload"]["algoType"] == "CONDITIONAL"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["clientAlgoId"] is not None
        assert request[1]["payload"]["triggerPrice"] == "10099.00"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        ("_is_dual_side_position", "position_id", "expected"),
        [
            # One-way mode
            (False, None, "BOTH"),
            (False, TestIdStubs.position_id(), "BOTH"),
            (False, TestIdStubs.position_id_long(), "BOTH"),
            (False, TestIdStubs.position_id_short(), "BOTH"),
            (False, TestIdStubs.position_id_both(), "BOTH"),
            # Hedge mode
            (True, TestIdStubs.position_id_long(), "LONG"),
            (True, TestIdStubs.position_id_short(), "SHORT"),
            # (True, TestIdStubs.position_id_both(), "BOTH"),
        ],
    )
    async def test_submit_limit_if_touched_order(
        self,
        mocker,
        _is_dual_side_position,
        position_id,
        expected,
    ):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mocker.patch.object(self.exec_client, "_is_dual_side_position", _is_dual_side_position)

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
            position_id=position_id,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await eventually(lambda: mock_send_request.call_args)

        # Assert
        # As of 2025-12-09, conditional orders use the algo order endpoint
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/algoOrder"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "SELL"
        assert request[1]["payload"]["type"] == "TAKE_PROFIT"
        assert request[1]["payload"]["algoType"] == "CONDITIONAL"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["price"] == "10050.80"
        assert request[1]["payload"]["clientAlgoId"] is not None
        assert request[1]["payload"]["triggerPrice"] == "10099.00"
        assert request[1]["payload"]["workingType"] == "CONTRACT_PRICE"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        ("_is_dual_side_position", "position_id", "expected"),
        [
            # One-way mode
            (False, None, "BOTH"),
            (False, TestIdStubs.position_id(), "BOTH"),
            (False, TestIdStubs.position_id_long(), "BOTH"),
            (False, TestIdStubs.position_id_short(), "BOTH"),
            (False, TestIdStubs.position_id_both(), "BOTH"),
            # Hedge mode
            (True, TestIdStubs.position_id_long(), "LONG"),
            (True, TestIdStubs.position_id_short(), "SHORT"),
            # (True, TestIdStubs.position_id_both(), "BOTH"),
        ],
    )
    async def test_trailing_stop_market_order(
        self,
        mocker,
        _is_dual_side_position,
        position_id,
        expected,
    ):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mocker.patch.object(self.exec_client, "_is_dual_side_position", _is_dual_side_position)
        mocker.patch.object(self.exec_client, "_use_reduce_only", not _is_dual_side_position)

        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trailing_offset=Decimal(100),
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            activation_price=Price.from_str("10000.00"),
            trigger_type=TriggerType.MARK_PRICE,
            reduce_only=True,
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=position_id,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order(submit_order)
        await eventually(lambda: mock_send_request.call_args)

        # Assert
        # As of 2025-12-09, conditional orders use the algo order endpoint
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/algoOrder"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "SELL"
        assert request[1]["payload"]["type"] == "TRAILING_STOP_MARKET"
        assert request[1]["payload"]["algoType"] == "CONDITIONAL"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        if _is_dual_side_position:
            assert "reduceOnly" not in request[1]["payload"]
        else:
            assert request[1]["payload"]["reduceOnly"] == "True"
        assert request[1]["payload"]["clientAlgoId"] is not None
        assert request[1]["payload"]["activationPrice"] == "10000.00"
        assert request[1]["payload"]["callbackRate"] == "1.0"
        assert request[1]["payload"]["workingType"] == "MARK_PRICE"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio
    async def test_submit_stop_limit_order_with_invalid_trigger_type_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.stop_limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.80"),
            trigger_price=Price.from_str("10050.00"),
            trigger_type=TriggerType.BID_ASK,
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
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "INVALID_TRIGGER_TYPE" in reason

    @pytest.mark.asyncio
    async def test_submit_stop_market_order_with_invalid_trigger_type_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(5),
            trigger_price=Price.from_str("9950.00"),
            trigger_type=TriggerType.BID_ASK,
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
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "INVALID_TRIGGER_TYPE" in reason

    @pytest.mark.asyncio
    async def test_submit_trailing_stop_with_invalid_trigger_type_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trailing_offset=Decimal(100),
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            activation_price=Price.from_str("10000.00"),
            trigger_type=TriggerType.BID_ASK,
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
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "INVALID_TRIGGER_TYPE" in reason

    @pytest.mark.asyncio
    async def test_submit_trailing_stop_with_invalid_offset_type_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trailing_offset=Decimal(50),
            trailing_offset_type=TrailingOffsetType.PRICE,
            activation_price=Price.from_str("10000.00"),
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
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "INVALID_TRAILING_OFFSET_TYPE" in reason

    @pytest.mark.asyncio
    async def test_submit_trailing_stop_with_offset_too_small_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trailing_offset=Decimal(5),  # 0.05% - too small (min is 0.1%)
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            activation_price=Price.from_str("10000.00"),
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
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "INVALID_TRAILING_OFFSET" in reason

    @pytest.mark.asyncio
    async def test_submit_trailing_stop_with_offset_too_large_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trailing_offset=Decimal(60000),  # 600% - too large (max is 5%)
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            activation_price=Price.from_str("10000.00"),
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
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "INVALID_TRAILING_OFFSET" in reason

    @pytest.mark.asyncio
    async def test_submit_trailing_stop_with_trigger_price_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trailing_offset=Decimal(100),
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            trigger_price=Price.from_str("10000.00"),  # Should use activation_price instead
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
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "INVALID_TRIGGER_PRICE" in reason

    @pytest.mark.asyncio
    async def test_submit_trailing_stop_without_activation_price_omits_param(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mocker.patch.object(self.exec_client, "_is_dual_side_position", False)

        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trailing_offset=Decimal(100),
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
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

        # Assert: Order submitted successfully with activationPrice omitted
        # As of 2025-12-09, conditional orders use the algo order endpoint
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/algoOrder"
        payload = request[1]["payload"]
        assert payload["symbol"] == "ETHUSDT"
        assert payload["type"] == "TRAILING_STOP_MARKET"
        assert payload["algoType"] == "CONDITIONAL"
        assert payload["side"] == "SELL"
        assert payload["quantity"] == "10"
        assert payload["callbackRate"] == "1.0"
        # Critical: activationPrice should NOT be in the payload when None
        # This allows Binance to use server-side current market price
        assert "activationPrice" not in payload

    @pytest.mark.asyncio
    async def test_submit_oco_order_list_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        # Create a bracket order which has linked_order_ids (simulates OCO-like structure)
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            sl_trigger_price=Price.from_str("9500.00"),
            tp_price=Price.from_str("10500.00"),
        )

        for order in bracket.orders:
            self.cache.add_order(order, None)

        submit_order_list = SubmitOrderList(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            order_list=bracket,
            position_id=None,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.submit_order_list(submit_order_list)
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        # All orders in bracket should be denied (entry + stop loss + take profit = 3 orders)
        assert mock_generate_denied.call_count == 3
        denied_client_order_ids = {
            call.kwargs["client_order_id"] for call in mock_generate_denied.call_args_list
        }
        assert denied_client_order_ids == {order.client_order_id for order in bracket.orders}
        for call in mock_generate_denied.call_args_list:
            assert "UNSUPPORTED_OCO_CONDITIONAL_ORDERS" in call.kwargs["reason"]

    @pytest.mark.asyncio
    async def test_submit_unsupported_order_type_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        # Create an order with unsupported type (MARKET_TO_LIMIT not supported on Binance)
        order = self.strategy.order_factory.market_to_limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "UNSUPPORTED_ORDER_TYPE" in reason
        assert "MARKET_TO_LIMIT" in reason

    @pytest.mark.asyncio
    async def test_submit_unsupported_time_in_force_denied(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )
        mock_generate_denied = mocker.patch.object(self.exec_client, "generate_order_denied")

        # Create an order with unsupported TIF (DAY not supported on Binance Futures)
        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.80"),
            time_in_force=TimeInForce.DAY,
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
        await eventually(lambda: mock_generate_denied.called)

        # Assert
        mock_send_request.assert_not_called()
        mock_generate_denied.assert_called_once()
        reason = mock_generate_denied.call_args.kwargs["reason"]
        assert "UNSUPPORTED_TIME_IN_FORCE" in reason
        assert "DAY" in reason

    @pytest.mark.asyncio
    async def test_leverage_initialization_from_symbol_config(self, mocker):
        """
        Test that leverage is initialized from symbolConfig endpoint.

        This ensures leverage is set for all symbols, including those without active
        positions, fixing the issue where fresh accounts would have default 1x leverage.

        """
        # Arrange
        # Mock account info query
        account_info = BinanceFuturesAccountInfo(
            feeTier=0,
            canTrade=True,
            canDeposit=True,
            canWithdraw=True,
            updateTime=1234567890000,
            assets=[],
        )
        mocker.patch.object(
            self.exec_client._futures_http_account,
            "query_futures_account_info",
            return_value=account_info,
        )

        # Mock generate_account_state
        mocker.patch.object(self.exec_client, "generate_account_state")

        # Mock account retrieval
        mock_account = mocker.Mock()
        mock_account.set_leverage = mocker.Mock()
        mocker.patch.object(self.exec_client, "get_account", return_value=mock_account)
        mocker.patch.object(self.exec_client, "_await_account_registered", return_value=None)

        # Create multiple symbol configs (including symbols without positions)
        symbol_configs = [
            BinanceFuturesSymbolConfig(
                symbol="ETHUSDT",
                marginType="CROSSED",
                isAutoAddMargin=False,
                leverage=20,
                maxNotionalValue="1000000",
            ),
            BinanceFuturesSymbolConfig(
                symbol="BTCUSDT",
                marginType="CROSSED",
                isAutoAddMargin=False,
                leverage=25,
                maxNotionalValue="2000000",
            ),
        ]
        mock_query_symbol_config = mocker.patch.object(
            self.exec_client._futures_http_account,
            "query_futures_symbol_config",
            return_value=symbol_configs,
        )

        # Mock instrument cache to return instrument IDs
        def get_instrument_id(symbol):
            if symbol == "ETHUSDT":
                return ETHUSDT_PERP_BINANCE.id
            raise KeyError(f"Symbol {symbol} not loaded")

        mocker.patch.object(
            self.exec_client,
            "_get_cached_instrument_id",
            side_effect=get_instrument_id,
        )

        # Act
        await self.exec_client._update_account_state()

        # Assert
        mock_query_symbol_config.assert_called_once()

        # Verify leverage was set for ETHUSDT (loaded instrument)
        assert mock_account.set_leverage.call_count == 1
        call_args = mock_account.set_leverage.call_args
        assert call_args[0][0] == ETHUSDT_PERP_BINANCE.id
        assert call_args[0][1] == Decimal(20)

    # -------------------------------------------------------------------------
    # Algo Order Cancellation Tests
    # -------------------------------------------------------------------------

    @pytest.mark.asyncio
    async def test_cancel_non_triggered_algo_order_uses_algo_endpoint(self, mocker):
        """
        Test that canceling an algo order that has NOT been triggered uses the algo
        cancel endpoint (/fapi/v1/algoOrder).
        """
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trigger_price=Price.from_str("10099.00"),
        )

        # Drive order through lifecycle to ACCEPTED state
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        self.cache.add_order(order, None)

        venue_order_id = VenueOrderId("1000000033225453")  # Algo ID format

        # Order is NOT in triggered set
        assert order.client_order_id not in self.exec_client._triggered_algo_order_ids

        cancel_order = CancelOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.cancel_order(cancel_order)
        await eventually(lambda: mock_send_request.call_args)

        # Assert - must use algo cancel endpoint
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.DELETE
        assert request[0][1] == "/fapi/v1/algoOrder"
        assert request[1]["payload"]["clientAlgoId"] == order.client_order_id.value

    @pytest.mark.asyncio
    async def test_cancel_triggered_algo_order_uses_regular_endpoint(self, mocker):
        """
        Test that canceling an algo order that HAS been triggered uses the regular
        cancel endpoint (/fapi/v1/order).
        """
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.stop_limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.00"),
            trigger_price=Price.from_str("10099.00"),
        )

        # Drive order through lifecycle to ACCEPTED state
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        self.cache.add_order(order, None)

        # Matching engine order ID (after trigger)
        matching_engine_order_id = VenueOrderId("88937381063")

        # Add to triggered set - this is what happens when TRIGGERED status is received
        self.exec_client._triggered_algo_order_ids.add(order.client_order_id)

        cancel_order = CancelOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            client_order_id=order.client_order_id,
            venue_order_id=matching_engine_order_id,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.cancel_order(cancel_order)
        await eventually(lambda: mock_send_request.call_args)

        # Assert - must use regular cancel endpoint with matching engine order ID
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.DELETE
        assert request[0][1] == "/fapi/v1/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"
        assert request[1]["payload"]["orderId"] == 88937381063

    @pytest.mark.asyncio
    async def test_query_open_algo_orders(self, mocker):
        """
        Test that query_open_algo_orders endpoint is called correctly.
        """
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            return_value=b"[]",  # Empty list
        )

        # Act
        result = await self.exec_client._futures_http_account.query_open_algo_orders()

        # Assert
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.GET
        assert request[0][1] == "/fapi/v1/openAlgoOrders"
        assert result == []

    @pytest.mark.asyncio
    async def test_query_open_algo_orders_with_symbol(self, mocker):
        """
        Test that query_open_algo_orders endpoint is called with symbol filter.
        """
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            return_value=b"[]",
        )

        # Act
        await self.exec_client._futures_http_account.query_open_algo_orders(symbol="ETHUSDT")

        # Assert
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.GET
        assert request[0][1] == "/fapi/v1/openAlgoOrders"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"

    @pytest.mark.asyncio
    async def test_triggered_algo_orders_skipped_but_tracked_in_triggered_set(self, mocker):
        """
        Test that triggered algo orders (with actualOrderId) are skipped during
        reconciliation but still tracked in _triggered_algo_order_ids.

        Triggered algo orders should be skipped because:
        1. The regular orders API provides accurate fill data for the triggered order
        2. Algo order reports have filled_qty=0 which causes reconciliation conflicts

        """
        # Arrange - Mock response with a triggered algo order (has actualOrderId)
        triggered_algo_order = {
            "algoId": 1000000033225453,
            "clientAlgoId": "O-20251211-053131-TEST-000-1",
            "algoType": "CONDITIONAL",
            "orderType": "STOP_MARKET",
            "symbol": "ETHUSDT",
            "side": "BUY",
            "positionSide": "BOTH",
            "quantity": "38",
            "algoStatus": "TRIGGERED",
            "triggerPrice": "0.13855",
            "workingType": "CONTRACT_PRICE",
            "actualOrderId": "88937381063",  # This indicates triggered
        }

        mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            return_value=json.dumps([triggered_algo_order]).encode(),
        )

        # Clear the triggered set
        self.exec_client._triggered_algo_order_ids.clear()

        # Act
        reports = await self.exec_client._generate_algo_order_status_reports(
            symbol=None,
            active_symbols=set(),
            open_only=True,
            start_ms=None,
            end_ms=None,
        )

        # Assert - No reports generated (triggered orders skipped)
        assert len(reports) == 0

        # Assert - ClientOrderId should still be in triggered set for cancel routing
        client_order_id = ClientOrderId("O-20251211-053131-TEST-000-1")
        assert client_order_id in self.exec_client._triggered_algo_order_ids

    @pytest.mark.asyncio
    async def test_reconciliation_does_not_populate_triggered_set_for_non_triggered_algo_orders(
        self,
        mocker,
    ):
        """
        Test that reconciliation does NOT populate _triggered_algo_order_ids for algo
        orders without actualOrderId (i.e., not yet triggered).
        """
        # Arrange - Mock response with a non-triggered algo order (no actualOrderId)
        non_triggered_algo_order = {
            "algoId": 1000000033225454,
            "clientAlgoId": "O-20251211-053131-TEST-000-2",
            "algoType": "CONDITIONAL",
            "orderType": "STOP_MARKET",
            "symbol": "ETHUSDT",
            "side": "SELL",
            "positionSide": "BOTH",
            "quantity": "10",
            "algoStatus": "NEW",
            "triggerPrice": "5000.00",
            "workingType": "CONTRACT_PRICE",
            # No actualOrderId - not triggered yet
        }

        mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            return_value=json.dumps([non_triggered_algo_order]).encode(),
        )

        # Clear the triggered set
        self.exec_client._triggered_algo_order_ids.clear()

        # Act
        reports = await self.exec_client._generate_algo_order_status_reports(
            symbol=None,
            active_symbols=set(),
            open_only=True,
            start_ms=None,
            end_ms=None,
        )

        # Assert - ClientOrderId should NOT be in triggered set
        assert len(reports) == 1
        client_order_id = ClientOrderId("O-20251211-053131-TEST-000-2")
        assert client_order_id not in self.exec_client._triggered_algo_order_ids

    @pytest.mark.asyncio
    async def test_mixed_triggered_and_non_triggered_algo_orders(self, mocker):
        """
        Test that when both triggered and non-triggered algo orders are returned, only
        non-triggered orders generate reports while triggered ones are skipped.
        """
        # Arrange - Mix of triggered and non-triggered algo orders
        algo_orders = [
            {
                "algoId": 1000000000001,
                "clientAlgoId": "O-TRIGGERED-001",
                "algoType": "CONDITIONAL",
                "orderType": "STOP_MARKET",
                "symbol": "ETHUSDT",
                "side": "BUY",
                "positionSide": "BOTH",
                "quantity": "10",
                "algoStatus": "TRIGGERED",
                "triggerPrice": "3000.00",
                "workingType": "CONTRACT_PRICE",
                "actualOrderId": "99999999001",  # Triggered
            },
            {
                "algoId": 1000000000002,
                "clientAlgoId": "O-PENDING-001",
                "algoType": "CONDITIONAL",
                "orderType": "STOP_MARKET",
                "symbol": "ETHUSDT",
                "side": "SELL",
                "positionSide": "BOTH",
                "quantity": "20",
                "algoStatus": "NEW",
                "triggerPrice": "2500.00",
                "workingType": "CONTRACT_PRICE",
                # No actualOrderId - not triggered
            },
            {
                "algoId": 1000000000003,
                "clientAlgoId": "O-FINISHED-001",
                "algoType": "CONDITIONAL",
                "orderType": "STOP_MARKET",
                "symbol": "ETHUSDT",
                "side": "BUY",
                "positionSide": "BOTH",
                "quantity": "30",
                "algoStatus": "FINISHED",
                "triggerPrice": "3500.00",
                "workingType": "CONTRACT_PRICE",
                "actualOrderId": "99999999002",  # Triggered and filled
            },
        ]

        mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            return_value=json.dumps(algo_orders).encode(),
        )

        self.exec_client._triggered_algo_order_ids.clear()

        # Act
        reports = await self.exec_client._generate_algo_order_status_reports(
            symbol=None,
            active_symbols=set(),
            open_only=True,
            start_ms=None,
            end_ms=None,
        )

        # Assert - Only non-triggered order generates a report
        assert len(reports) == 1
        assert reports[0].client_order_id == ClientOrderId("O-PENDING-001")

        # Assert - Both triggered orders are in the triggered set
        assert ClientOrderId("O-TRIGGERED-001") in self.exec_client._triggered_algo_order_ids
        assert ClientOrderId("O-FINISHED-001") in self.exec_client._triggered_algo_order_ids
        assert ClientOrderId("O-PENDING-001") not in self.exec_client._triggered_algo_order_ids

    @pytest.mark.asyncio
    async def test_algo_order_trailing_offset_converts_percent_to_basis_points(self, mocker):
        """
        Test that callbackRate from Binance (in percent) is converted to basis points.

        Binance sends 1.0 for 1%, which should become 100 basis points.

        """
        # Arrange - Mock response with trailing stop order with 1.5% callback rate
        trailing_stop_order = {
            "algoId": 1000000033225455,
            "clientAlgoId": "O-20251211-053131-TEST-000-3",
            "algoType": "CONDITIONAL",
            "orderType": "TRAILING_STOP_MARKET",
            "symbol": "ETHUSDT",
            "side": "SELL",
            "positionSide": "BOTH",
            "quantity": "10",
            "algoStatus": "NEW",
            "activatePrice": "5000.00",
            "callbackRate": "1.5",  # 1.5% from Binance
            "workingType": "CONTRACT_PRICE",
        }

        mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            return_value=json.dumps([trailing_stop_order]).encode(),
        )

        # Act
        reports = await self.exec_client._generate_algo_order_status_reports(
            symbol=None,
            active_symbols=set(),
            open_only=True,
            start_ms=None,
            end_ms=None,
        )

        # Assert - trailing_offset should be 150 basis points (1.5% * 100)
        assert len(reports) == 1
        report = reports[0]
        assert report.trailing_offset_type == TrailingOffsetType.BASIS_POINTS
        assert report.trailing_offset == 150  # 1.5% = 150 basis points

    @pytest.mark.asyncio
    async def test_algo_order_uses_activate_price_as_trigger_price_fallback(self, mocker):
        """
        Test that activatePrice is used as trigger_price when triggerPrice is absent.

        Trailing stop orders use activatePrice instead of triggerPrice.

        """
        # Arrange - Mock response with trailing stop order with activatePrice but no triggerPrice
        trailing_stop_order = {
            "algoId": 1000000033225456,
            "clientAlgoId": "O-20251211-053131-TEST-000-4",
            "algoType": "CONDITIONAL",
            "orderType": "TRAILING_STOP_MARKET",
            "symbol": "ETHUSDT",
            "side": "SELL",
            "positionSide": "BOTH",
            "quantity": "10",
            "algoStatus": "NEW",
            "activatePrice": "4500.00",  # No triggerPrice, only activatePrice
            "callbackRate": "1.0",
            "workingType": "CONTRACT_PRICE",
        }

        mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            return_value=json.dumps([trailing_stop_order]).encode(),
        )

        # Act
        reports = await self.exec_client._generate_algo_order_status_reports(
            symbol=None,
            active_symbols=set(),
            open_only=True,
            start_ms=None,
            end_ms=None,
        )

        # Assert - trigger_price should come from activatePrice
        assert len(reports) == 1
        report = reports[0]
        assert report.trigger_price == Price.from_str("4500.00")

    @pytest.mark.asyncio
    async def test_close_position_algo_order_skipped_during_reconciliation(self, mocker):
        """
        Test that close-position algo orders without quantity are skipped during
        reconciliation (logged warning) instead of crashing.
        """
        # Arrange - Mock response with close-position order (no quantity)
        close_position_order = {
            "algoId": 1000000033225457,
            "clientAlgoId": "O-20251211-053131-TEST-000-5",
            "algoType": "CONDITIONAL",
            "orderType": "STOP_MARKET",
            "symbol": "ETHUSDT",
            "side": "SELL",
            "positionSide": "BOTH",
            "algoStatus": "NEW",
            "closePosition": True,  # No quantity for close-position orders
            "triggerPrice": "4000.00",
            "workingType": "CONTRACT_PRICE",
        }

        mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            return_value=json.dumps([close_position_order]).encode(),
        )

        # Act - should not raise, just skip the order
        reports = await self.exec_client._generate_algo_order_status_reports(
            symbol=None,
            active_symbols=set(),
            open_only=True,
            start_ms=None,
            end_ms=None,
        )

        # Assert - order is skipped, no reports returned
        assert len(reports) == 0

    @pytest.mark.asyncio
    async def test_algo_order_reconciliation_deduplicates_open_and_historical(self, mocker):
        """
        Test that when open_only=False, open orders are fetched first and deduplicated
        against historical orders from allAlgoOrders endpoint.

        This ensures orders older than 7 days (beyond allAlgoOrders limit) are still
        captured via openAlgoOrders, while preventing duplicates.

        """
        # Order that appears in both endpoints (open and historical)
        shared_order = {
            "algoId": 1000000000001,
            "clientAlgoId": "O-SHARED-001",
            "algoType": "CONDITIONAL",
            "orderType": "STOP_MARKET",
            "symbol": "ETHUSDT",
            "side": "BUY",
            "positionSide": "BOTH",
            "quantity": "10",
            "algoStatus": "NEW",
            "triggerPrice": "3000.00",
            "workingType": "CONTRACT_PRICE",
            "createTime": 1733900000000,
        }

        # Order only in historical (recently closed, within 7 days)
        historical_only_order = {
            "algoId": 1000000000002,
            "clientAlgoId": "O-HISTORICAL-001",
            "algoType": "CONDITIONAL",
            "orderType": "STOP_MARKET",
            "symbol": "ETHUSDT",
            "side": "SELL",
            "positionSide": "BOTH",
            "quantity": "5",
            "algoStatus": "CANCELED",
            "triggerPrice": "2500.00",
            "workingType": "CONTRACT_PRICE",
            "createTime": 1733900000000,
        }

        # Mock the HTTP account methods directly
        mocker.patch.object(
            self.exec_client._futures_http_account,
            "query_open_algo_orders",
            new=AsyncMock(return_value=[BinanceFuturesAlgoOrder(**shared_order)]),
        )
        mocker.patch.object(
            self.exec_client._futures_http_account,
            "query_all_algo_orders",
            new=AsyncMock(
                return_value=[
                    BinanceFuturesAlgoOrder(**shared_order),  # Duplicate
                    BinanceFuturesAlgoOrder(**historical_only_order),
                ],
            ),
        )

        # Act - open_only=False with active symbols triggers historical fetch
        reports = await self.exec_client._generate_algo_order_status_reports(
            symbol=None,
            active_symbols={"ETHUSDT"},
            open_only=False,
            start_ms=None,
            end_ms=None,
        )

        # Assert - should have 2 reports, not 3 (shared order deduplicated)
        assert len(reports) == 2
        algo_ids = {r.venue_order_id.value for r in reports}
        assert "1000000000001" in algo_ids  # Shared order
        assert "1000000000002" in algo_ids  # Historical only order

    # -------------------------------------------------------------------------
    # Algo Order Modification Tests
    # -------------------------------------------------------------------------

    @pytest.mark.asyncio
    async def test_modify_non_triggered_stop_limit_order_rejected(self, mocker):
        """
        Test that modifying a non-triggered STOP_LIMIT order is rejected since Binance
        doesn't have an algo order modify endpoint.
        """
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.stop_limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.00"),
            trigger_price=Price.from_str("10099.00"),
        )

        # Drive order through lifecycle to ACCEPTED state
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        self.cache.add_order(order, None)

        # Order NOT in triggered set
        assert order.client_order_id not in self.exec_client._triggered_algo_order_ids

        modify_order = ModifyOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1000000033225453"),
            price=Price.from_str("10060.00"),
            quantity=None,
            trigger_price=None,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.modify_order(modify_order)
        await asyncio.sleep(0.1)  # Allow async processing

        # Assert - should NOT call send_request (rejected before HTTP)
        mock_send_request.assert_not_called()

    @pytest.mark.asyncio
    async def test_modify_triggered_stop_limit_order_allowed(self, mocker):
        """
        Test that modifying a TRIGGERED STOP_LIMIT order is allowed since it becomes a
        regular LIMIT order in the matching engine.
        """
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
        )

        order = self.strategy.order_factory.stop_limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.00"),
            trigger_price=Price.from_str("10099.00"),
        )

        # Drive order through lifecycle to ACCEPTED state
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        self.cache.add_order(order, None)

        # Add to triggered set - simulating that the order has been triggered
        self.exec_client._triggered_algo_order_ids.add(order.client_order_id)

        modify_order = ModifyOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("88937381063"),  # Matching engine order ID
            price=Price.from_str("10060.00"),
            quantity=None,
            trigger_price=None,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_client.modify_order(modify_order)
        await eventually(lambda: mock_send_request.call_args)

        # Assert - should call modify endpoint
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.PUT
        assert request[0][1] == "/fapi/v1/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"

    @pytest.mark.asyncio
    async def test_generate_order_status_reports_passes_correct_symbol_to_algo_orders(
        self,
        mocker,
    ):
        """
        Test that generate_order_status_reports passes the correct symbol parameter to
        _generate_algo_order_status_reports.
        """
        # Arrange
        mock_open_orders = [
            mocker.Mock(symbol="ETHUSDT"),
            mocker.Mock(symbol="BTCUSDT"),
            mocker.Mock(symbol="XRPUSDT"),
        ]
        mocker.patch.object(
            self.exec_client._http_account,
            "query_open_orders",
            return_value=mock_open_orders,
        )
        mocker.patch.object(
            self.exec_client._http_account,
            "query_all_orders",
            return_value=[],
        )
        mocker.patch.object(
            self.exec_client,
            "_get_binance_active_position_symbols",
            return_value=set(),
        )
        mocker.patch.object(
            self.exec_client,
            "_get_cache_active_symbols",
            return_value=set(),
        )
        mocker.patch.object(
            self.exec_client,
            "_parse_order_status_reports",
            return_value=[],
        )
        mock_algo_reports = mocker.patch.object(
            self.exec_client,
            "_generate_algo_order_status_reports",
            return_value=[],
        )

        # open_only=False forces iteration through active_symbols loop
        command = GenerateOrderStatusReports(
            instrument_id=None,
            start=None,
            end=None,
            open_only=False,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client.generate_order_status_reports(command)

        # Assert
        mock_algo_reports.assert_called_once()
        call_kwargs = mock_algo_reports.call_args.kwargs
        assert call_kwargs["symbol"] is None
        assert call_kwargs["open_only"] is False

    @pytest.mark.asyncio
    async def test_generate_order_status_reports_with_instrument_filter_passes_symbol(
        self,
        mocker,
    ):
        """
        Test that generate_order_status_reports passes the instrument's symbol to
        _generate_algo_order_status_reports when a specific instrument is requested.
        """
        # Arrange
        mock_open_orders = [
            mocker.Mock(symbol="ETHUSDT"),
            mocker.Mock(symbol="BTCUSDT"),
            mocker.Mock(symbol="XRPUSDT"),
        ]
        mocker.patch.object(
            self.exec_client._http_account,
            "query_open_orders",
            return_value=mock_open_orders,
        )
        mocker.patch.object(
            self.exec_client,
            "_get_binance_active_position_symbols",
            return_value=set(),
        )
        mocker.patch.object(
            self.exec_client,
            "_get_cache_active_symbols",
            return_value=set(),
        )
        mocker.patch.object(
            self.exec_client,
            "_parse_order_status_reports",
            return_value=[],
        )
        mock_algo_reports = mocker.patch.object(
            self.exec_client,
            "_generate_algo_order_status_reports",
            return_value=[],
        )
        command = GenerateOrderStatusReports(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            start=None,
            end=None,
            open_only=True,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client.generate_order_status_reports(command)

        # Assert
        mock_algo_reports.assert_called_once()
        call_kwargs = mock_algo_reports.call_args.kwargs
        assert call_kwargs["symbol"] == "ETHUSDT-PERP"
        assert call_kwargs["open_only"] is True

    @pytest.mark.asyncio
    async def test_cancel_all_open_algo_orders_endpoint(self, mocker):
        """
        Test that cancel_all_open_algo_orders endpoint is called correctly.
        """
        # Arrange
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            return_value=b'{"code": 200, "msg": "The operation of cancel all open algo orders is done."}',
        )

        # Act
        result = await self.exec_client._futures_http_account.cancel_all_open_algo_orders(
            symbol="ETHUSDT",
        )

        # Assert
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.DELETE
        assert request[0][1] == "/fapi/v1/algoOpenOrders"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"
        assert result is True

    @pytest.mark.asyncio
    async def test_cancel_algo_orders_batch_calls_http_endpoint(self, mocker):
        """
        Test that _cancel_algo_orders_batch calls the correct HTTP endpoint.
        """
        # Arrange
        mock_cancel_all_algo = mocker.patch.object(
            self.exec_client._futures_http_account,
            "cancel_all_open_algo_orders",
            new_callable=AsyncMock,
            return_value=True,
        )
        algo_order = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trigger_price=Price.from_str("10099.00"),
        )
        algo_order.apply(TestEventStubs.order_submitted(algo_order))
        algo_order.apply(TestEventStubs.order_accepted(algo_order))
        self.cache.add_order(algo_order, None)

        # Act
        await self.exec_client._cancel_algo_orders_batch(
            ETHUSDT_PERP_BINANCE.id,
            [algo_order],
        )

        # Assert
        mock_cancel_all_algo.assert_called_once_with(symbol="ETHUSDT-PERP")

    @pytest.mark.asyncio
    async def test_query_all_algo_orders(self, mocker):
        response_data = [
            {
                "algoId": 12345,
                "clientAlgoId": "test-order-id",
                "algoType": "VP",
                "orderType": "TRAILING_STOP_MARKET",
                "symbol": "ETHUSDT",
                "side": "SELL",
                "positionSide": "BOTH",
                "timeInForce": "GTC",
                "quantity": "0.1",
                "algoStatus": "CANCELLED",
                "triggerPrice": "0",
                "price": "0",
                "workingType": "CONTRACT_PRICE",
                "activatePrice": "3000.0",
                "callbackRate": "1.0",
                "reduceOnly": False,
                "closePosition": False,
                "priceProtect": False,
                "selfTradePreventionMode": "NONE",
            },
        ]
        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            return_value=json.dumps(response_data).encode(),
        )

        # Act
        result = await self.exec_client._futures_http_account.query_all_algo_orders(
            symbol="ETHUSDT",
            start_time=1700000000000,
            end_time=1700100000000,
        )

        # Assert
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.GET
        assert request[0][1] == "/fapi/v1/allAlgoOrders"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"
        assert request[1]["payload"]["startTime"] == "1700000000000"
        assert request[1]["payload"]["endTime"] == "1700100000000"
        assert len(result) == 1
        assert result[0].algoId == 12345
        assert result[0].algoStatus == "CANCELLED"

    @pytest.mark.asyncio
    async def test_generate_order_status_report_falls_back_to_algo_order(self, mocker):
        """
        Test that generate_order_status_report falls back to algo order endpoint when
        regular order query returns None.
        """
        # Arrange - mock base class returning None (order not found)
        mocker.patch.object(
            self.exec_client.__class__.__bases__[0],
            "generate_order_status_report",
            new=AsyncMock(return_value=None),
        )

        algo_order_response = BinanceFuturesAlgoOrder(
            algoId=12345,
            clientAlgoId="O-20251224-071254-eK0z-000-1",
            algoType="CONDITIONAL",
            orderType="STOP_MARKET",
            symbol="ETHUSDT",
            side="SELL",
            positionSide="BOTH",
            timeInForce="GTC",
            quantity="80.0",
            algoStatus="NEW",
            triggerPrice="0.1243",
            price="0",
            workingType="CONTRACT_PRICE",
            activatePrice=None,
            callbackRate=None,
            reduceOnly=False,
            closePosition=False,
            priceProtect=False,
            selfTradePreventionMode="NONE",
        )
        mock_query_algo = mocker.patch.object(
            self.exec_client._futures_http_account,
            "query_algo_order",
            return_value=algo_order_response,
        )

        command = GenerateOrderStatusReport(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            client_order_id=ClientOrderId("O-20251224-071254-eK0z-000-1"),
            venue_order_id=None,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        report = await self.exec_client.generate_order_status_report(command)

        # Assert
        mock_query_algo.assert_called_once_with(
            algo_id=None,
            client_algo_id="O-20251224-071254-eK0z-000-1",
        )
        assert report is not None
        assert report.client_order_id == ClientOrderId("O-20251224-071254-eK0z-000-1")
        assert report.venue_order_id == VenueOrderId("12345")

    @pytest.mark.asyncio
    async def test_generate_order_status_report_does_not_query_algo_when_regular_found(
        self,
        mocker,
    ):
        """
        Test that generate_order_status_report does not query algo order endpoint when
        regular order query succeeds.
        """
        # Arrange - mock base class returning a valid report
        mock_report = mocker.Mock()
        mocker.patch.object(
            self.exec_client.__class__.__bases__[0],
            "generate_order_status_report",
            new=AsyncMock(return_value=mock_report),
        )

        mock_query_algo = mocker.patch.object(
            self.exec_client._futures_http_account,
            "query_algo_order",
        )

        command = GenerateOrderStatusReport(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            client_order_id=ClientOrderId("O-20251224-071254-eK0z-000-1"),
            venue_order_id=None,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        report = await self.exec_client.generate_order_status_report(command)

        # Assert - algo order endpoint should NOT be called
        mock_query_algo.assert_not_called()
        assert report is mock_report

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
        mock_cancel_algo_batch = mocker.patch.object(
            self.exec_client,
            "_cancel_algo_orders_batch",
            new_callable=AsyncMock,
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._cancel_all_orders(command)

        # Assert
        mock_cancel_orders_batch.assert_called_once()
        assert limit_order in mock_cancel_orders_batch.call_args[0][1]
        mock_cancel_algo_batch.assert_not_called()

    @pytest.mark.asyncio
    async def test_cancel_all_orders_with_submitted_orders_uses_batch_cancel(self, mocker):
        """
        Test that _cancel_all_orders includes SUBMITTED (inflight) orders.
        """
        # Arrange
        mock_cancel_orders_batch = mocker.patch.object(
            self.exec_client,
            "_cancel_orders_batch",
            new_callable=AsyncMock,
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
    async def test_cancel_all_orders_does_not_double_count_pending_cancel(self, mocker):
        """
        Test that PENDING_CANCEL orders are not double-counted (they appear in both
        orders_open and orders_inflight, but we only include them once via orders_open).
        """
        # Arrange
        mock_cancel_orders_batch = mocker.patch.object(
            self.exec_client,
            "_cancel_orders_batch",
            new_callable=AsyncMock,
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("3000.00"),
        )
        self.cache.add_order(limit_order, None)
        limit_order.apply(TestEventStubs.order_submitted(limit_order))
        self.cache.update_order(limit_order)
        limit_order.apply(TestEventStubs.order_accepted(limit_order))
        self.cache.update_order(limit_order)
        limit_order.apply(TestEventStubs.order_pending_cancel(limit_order))
        self.cache.update_order(limit_order)

        command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._cancel_all_orders(command)

        # Assert - order should appear exactly once
        mock_cancel_orders_batch.assert_called_once()
        orders_passed = mock_cancel_orders_batch.call_args[0][1]
        assert orders_passed.count(limit_order) == 1

    @pytest.mark.asyncio
    async def test_cancel_all_orders_separates_algo_and_regular_orders(self, mocker):
        """
        Test that _cancel_all_orders separates algo orders (STOP_MARKET, etc.) from
        regular orders and calls the appropriate batch methods.
        """
        # Arrange
        mock_cancel_orders_batch = mocker.patch.object(
            self.exec_client,
            "_cancel_orders_batch",
            new_callable=AsyncMock,
        )
        mock_cancel_algo_batch = mocker.patch.object(
            self.exec_client,
            "_cancel_algo_orders_batch",
            new_callable=AsyncMock,
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("3000.00"),
        )
        self.cache.add_order(limit_order, None)
        limit_order.apply(TestEventStubs.order_submitted(limit_order))
        self.cache.update_order(limit_order)
        limit_order.apply(TestEventStubs.order_accepted(limit_order))
        self.cache.update_order(limit_order)

        stop_order = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trigger_price=Price.from_str("2900.00"),
        )
        self.cache.add_order(stop_order, None)
        stop_order.apply(TestEventStubs.order_submitted(stop_order))
        self.cache.update_order(stop_order)
        stop_order.apply(TestEventStubs.order_accepted(stop_order))
        self.cache.update_order(stop_order)

        command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._cancel_all_orders(command)

        # Assert
        mock_cancel_orders_batch.assert_called_once()
        assert limit_order in mock_cancel_orders_batch.call_args[0][1]
        assert stop_order not in mock_cancel_orders_batch.call_args[0][1]

        mock_cancel_algo_batch.assert_called_once()
        assert stop_order in mock_cancel_algo_batch.call_args[0][1]
        assert limit_order not in mock_cancel_algo_batch.call_args[0][1]

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
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
    async def test_cancel_all_orders_does_not_double_count_pending_update(self, mocker):
        """
        Test that PENDING_UPDATE orders are not double-counted (they appear in both
        orders_open and orders_inflight, but we only include them once via orders_open).
        """
        # Arrange
        mock_cancel_orders_batch = mocker.patch.object(
            self.exec_client,
            "_cancel_orders_batch",
            new_callable=AsyncMock,
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("3000.00"),
        )
        self.cache.add_order(limit_order, None)
        limit_order.apply(TestEventStubs.order_submitted(limit_order))
        self.cache.update_order(limit_order)
        limit_order.apply(TestEventStubs.order_accepted(limit_order))
        self.cache.update_order(limit_order)
        limit_order.apply(TestEventStubs.order_pending_update(limit_order))
        self.cache.update_order(limit_order)

        command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._cancel_all_orders(command)

        # Assert - order should appear exactly once
        mock_cancel_orders_batch.assert_called_once()
        orders_passed = mock_cancel_orders_batch.call_args[0][1]
        assert orders_passed.count(limit_order) == 1

    @pytest.mark.asyncio
    async def test_cancel_all_orders_multi_strategy_with_submitted_orders(self, mocker):
        """
        Test that multi-strategy fallback triggers when other strategy has SUBMITTED
        orders.
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
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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

        # Other strategy has a SUBMITTED order (not yet accepted)
        other_order = other_strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(5),
            price=Price.from_str("3100.00"),
        )
        self.cache.add_order(other_order, None)
        other_order.apply(TestEventStubs.order_submitted(other_order))
        self.cache.update_order(other_order)

        command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._cancel_all_orders(command)

        # Assert - should use individual cancel since other strategy has orders
        mock_cancel_orders_for_strategy.assert_called_once()
        mock_cancel_orders_batch.assert_not_called()

    @pytest.mark.asyncio
    async def test_cancel_orders_for_strategy_routes_algo_orders_individually(self, mocker):
        """
        Test that _cancel_orders_for_strategy routes algo orders through individual
        cancel (which uses the algo endpoint) rather than batch cancel.
        """
        # Arrange
        mock_cancel_orders_individual = mocker.patch.object(
            self.exec_client,
            "_cancel_orders_individual",
            new_callable=AsyncMock,
        )
        mock_process_cancel_batches = mocker.patch.object(
            self.exec_client,
            "_process_cancel_batches",
            new_callable=AsyncMock,
            return_value=([], []),
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("3000.00"),
        )
        self.cache.add_order(limit_order, None)
        limit_order.apply(TestEventStubs.order_submitted(limit_order))
        self.cache.update_order(limit_order)
        limit_order.apply(TestEventStubs.order_accepted(limit_order))
        self.cache.update_order(limit_order)

        stop_order = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trigger_price=Price.from_str("2900.00"),
        )
        self.cache.add_order(stop_order, None)
        stop_order.apply(TestEventStubs.order_submitted(stop_order))
        self.cache.update_order(stop_order)
        stop_order.apply(TestEventStubs.order_accepted(stop_order))
        self.cache.update_order(stop_order)

        command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._cancel_orders_for_strategy(
            [limit_order, stop_order],
            command,
        )

        # Assert - algo order goes through individual cancel, regular order through batch
        mock_cancel_orders_individual.assert_called_once()
        algo_orders_passed = mock_cancel_orders_individual.call_args[0][0]
        assert stop_order in algo_orders_passed
        assert limit_order not in algo_orders_passed

        mock_process_cancel_batches.assert_called_once()

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
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
            ETHUSDT_PERP_BINANCE.id,
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
    async def test_cancel_algo_orders_batch_failure_emits_cancel_rejected(self, mocker):
        """
        Test that algo batch cancel failure emits OrderCancelRejected for each order.
        """
        # Arrange
        mock_generate_cancel_rejected = mocker.patch.object(
            self.exec_client,
            "generate_order_cancel_rejected",
        )

        stop_order = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            trigger_price=Price.from_str("2900.00"),
        )
        self.cache.add_order(stop_order, None)
        stop_order.apply(TestEventStubs.order_submitted(stop_order))
        self.cache.update_order(stop_order)
        stop_order.apply(TestEventStubs.order_accepted(stop_order))
        self.cache.update_order(stop_order)

        mock_retry_manager = mocker.MagicMock()
        mock_retry_manager.result = False
        mock_retry_manager.message = "Network error"
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
        await self.exec_client._cancel_algo_orders_batch(
            ETHUSDT_PERP_BINANCE.id,
            [stop_order],
        )

        # Assert
        mock_generate_cancel_rejected.assert_called_once()
        call_args = mock_generate_cancel_rejected.call_args
        assert call_args[0][0] == stop_order.strategy_id
        assert call_args[0][1] == stop_order.instrument_id
        assert call_args[0][2] == stop_order.client_order_id
        assert call_args[0][4] == "Network error"

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
            instrument_id=ETHUSDT_PERP_BINANCE.id,
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
            ETHUSDT_PERP_BINANCE.id,
            [limit_order],
        )

        # Assert - should NOT generate cancel rejected for "Unknown order sent"
        mock_generate_cancel_rejected.assert_not_called()
