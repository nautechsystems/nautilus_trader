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

import asyncio
from decimal import Decimal

import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.futures.execution import BinanceFuturesExecutionClient
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
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
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestBinanceFuturesExecutionClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
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
            account_type=BinanceAccountType.USDT_FUTURE,
        )

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

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
            trigger_price=Price.from_str("10000.00"),
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
