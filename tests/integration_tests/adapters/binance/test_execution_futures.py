# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.futures.execution import BinanceFuturesExecutionClient
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAccountInfo
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesSymbolConfig
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
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

        yield

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "SELL"
        assert request[1]["payload"]["type"] == "STOP_MARKET"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        if _is_dual_side_position:
            assert "reduceOnly" not in request[1]["payload"]
        else:
            assert request[1]["payload"]["reduceOnly"] == "True"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["stopPrice"] == "10099.00"
        assert request[1]["payload"]["workingType"] == "CONTRACT_PRICE"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio()
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
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "BUY"
        assert request[1]["payload"]["type"] == "STOP"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["price"] == "10050.80"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["stopPrice"] == "10050.00"
        assert request[1]["payload"]["workingType"] == "MARK_PRICE"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio()
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
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "SELL"
        assert request[1]["payload"]["type"] == "TAKE_PROFIT_MARKET"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["stopPrice"] == "10099.00"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio()
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
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "SELL"
        assert request[1]["payload"]["type"] == "TAKE_PROFIT"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        assert request[1]["payload"]["price"] == "10050.80"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["stopPrice"] == "10099.00"
        assert request[1]["payload"]["workingType"] == "CONTRACT_PRICE"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio()
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
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/order"
        assert request[1]["payload"]["symbol"] == "ETHUSDT"  # -PERP was stripped
        assert request[1]["payload"]["side"] == "SELL"
        assert request[1]["payload"]["type"] == "TRAILING_STOP_MARKET"
        assert request[1]["payload"]["timeInForce"] == "GTC"
        assert request[1]["payload"]["quantity"] == "10"
        if _is_dual_side_position:
            assert "reduceOnly" not in request[1]["payload"]
        else:
            assert request[1]["payload"]["reduceOnly"] == "True"
        assert request[1]["payload"]["newClientOrderId"] is not None
        assert request[1]["payload"]["activationPrice"] == "10000.00"
        assert request[1]["payload"]["callbackRate"] == "1.0"
        assert request[1]["payload"]["workingType"] == "MARK_PRICE"
        assert request[1]["payload"]["recvWindow"] == "5000"
        assert request[1]["payload"]["signature"] is not None
        assert request[1]["payload"]["positionSide"] == expected

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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
        request = mock_send_request.call_args
        assert request[0][0] == HttpMethod.POST
        assert request[0][1] == "/fapi/v1/order"
        payload = request[1]["payload"]
        assert payload["symbol"] == "ETHUSDT"
        assert payload["type"] == "TRAILING_STOP_MARKET"
        assert payload["side"] == "SELL"
        assert payload["quantity"] == "10"
        assert payload["callbackRate"] == "1.0"
        # Critical: activationPrice should NOT be in the payload when None
        # This allows Binance to use server-side current market price
        assert "activationPrice" not in payload

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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

    @pytest.mark.asyncio()
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
        assert call_args[0][1] == Decimal("20")
