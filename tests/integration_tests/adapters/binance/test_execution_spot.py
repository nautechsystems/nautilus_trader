import asyncio
import json
from unittest.mock import AsyncMock

import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.adapters.binance.spot.execution import BinanceSpotExecutionClient
from nautilus_trader.adapters.binance.spot.http.account import BinanceSpotAccountHttpAPI
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
from nautilus_trader.execution.messages import GenerateOrderStatusReports
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

        # Base64-encoded 32 zero bytes for Ed25519 private key (test only)
        dummy_api_secret = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

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
            environment=BinanceEnvironment.LIVE,
            api_key="SOME_BINANCE_API_KEY",
            api_secret=dummy_api_secret,
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

    def test_margin_account_http_api_uses_margin_rest_paths(self):
        # Arrange
        http_account = BinanceSpotAccountHttpAPI(
            client=self.http_client,
            clock=self.clock,
            account_type=BinanceAccountType.MARGIN,
        )

        # Assert
        assert http_account.base_endpoint == "/sapi/v1/margin/"
        assert http_account._endpoint_order.url_path == "/sapi/v1/margin/order"
        assert http_account._endpoint_open_orders.url_path == "/sapi/v1/margin/openOrders"
        assert http_account._endpoint_user_trades.url_path == "/sapi/v1/margin/myTrades"
        assert http_account._endpoint_spot_account.url_path == "/sapi/v1/margin/account"

    def test_margin_execution_client_uses_margin_listen_token_mode(self):
        # Arrange
        http_client = BinanceHttpClient(
            clock=self.clock,
            api_key="SOME_BINANCE_API_KEY",
            api_secret="SOME_BINANCE_API_SECRET",
            base_url="https://api.binance.com/",
        )
        provider = BinanceSpotInstrumentProvider(
            client=http_client,
            clock=self.clock,
            config=InstrumentProviderConfig(load_all=True),
        )

        # Base64-encoded 32 zero bytes for Ed25519 private key (test only)
        dummy_api_secret = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

        exec_client = BinanceSpotExecutionClient(
            loop=self.loop,
            client=http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            base_url_ws="",
            config=BinanceExecClientConfig(),
            account_type=BinanceAccountType.MARGIN,
            environment=BinanceEnvironment.LIVE,
            api_key="SOME_BINANCE_API_KEY",
            api_secret=dummy_api_secret,
        )

        # Assert
        assert exec_client._ws_client._use_margin_listen_token is True

    @pytest.mark.asyncio
    async def test_generate_order_status_reports_caps_spot_all_orders_limit(self, mocker):
        mock_open_orders = [
            mocker.Mock(symbol="ETHUSDT"),
            mocker.Mock(symbol="BTCUSDT"),
        ]
        mocker.patch.object(
            self.exec_client._http_account,
            "query_open_orders",
            return_value=mock_open_orders,
        )
        query_all_orders = mocker.patch.object(
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

        command = GenerateOrderStatusReports(
            instrument_id=None,
            start=None,
            end=None,
            open_only=False,
            command_id=UUID4(),
            ts_init=0,
        )

        await self.exec_client.generate_order_status_reports(command)

        assert query_all_orders.await_count == 2
        assert all(call.kwargs["limit"] == 500 for call in query_all_orders.await_args_list)

    def test_margin_outbound_account_position_refreshes_full_account_state(self, mocker):
        # Arrange
        http_client = BinanceHttpClient(
            clock=self.clock,
            api_key="SOME_BINANCE_API_KEY",
            api_secret="SOME_BINANCE_API_SECRET",
            base_url="https://api.binance.com/",
        )
        provider = BinanceSpotInstrumentProvider(
            client=http_client,
            clock=self.clock,
            config=InstrumentProviderConfig(load_all=True),
        )

        dummy_api_secret = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

        exec_client = BinanceSpotExecutionClient(
            loop=self.loop,
            client=http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            base_url_ws="",
            config=BinanceExecClientConfig(),
            account_type=BinanceAccountType.MARGIN,
            environment=BinanceEnvironment.LIVE,
            api_key="SOME_BINANCE_API_KEY",
            api_secret=dummy_api_secret,
        )
        create_task = mocker.patch.object(exec_client, "create_task")
        generate_account_state = mocker.patch.object(exec_client, "generate_account_state")
        raw = (
            b'{"e":"outboundAccountPosition","E":1564034571105,"u":1564034571073,'
            b'"B":[{"a":"PLUME","f":"0.00000000","l":"0.00000000"}]}'
        )

        # Act
        exec_client._handle_user_ws_message(raw)

        # Assert
        create_task.assert_called_once()
        scheduled_coro = create_task.call_args.args[0]
        assert scheduled_coro.cr_code.co_name == exec_client._update_account_state.__name__
        scheduled_coro.close()
        generate_account_state.assert_not_called()

    def test_portfolio_margin_spot_user_stream_ignores_futures_events(self):
        self.exec_client._handle_user_ws_message(b'{"e":"ORDER_TRADE_UPDATE"}')

    def test_portfolio_margin_spot_execution_report_decodes_without_iceberg_quantity(self):
        raw = json.dumps(
            {
                "e": "executionReport",
                "E": 1700000000000,
                "s": "ETHUSDT",
                "c": "client-1",
                "S": "BUY",
                "o": "LIMIT",
                "f": "GTC",
                "q": "1.00000000",
                "p": "2000.00000000",
                "P": "0.00000000",
                "g": -1,
                "C": "",
                "x": "NEW",
                "X": "NEW",
                "r": "NONE",
                "i": 123456,
                "l": "0.00000000",
                "z": "0.00000000",
                "L": "0.00000000",
                "T": 1700000000000,
                "t": -1,
                "I": 1,
                "w": True,
                "m": False,
                "M": False,
                "O": 1700000000000,
                "Z": "0.00000000",
                "Y": "0.00000000",
                "Q": "0.00000000",
                "W": 1700000000000,
                "V": "NONE",
            },
        ).encode()

        decoded = self.exec_client._decoder_spot_order_update.decode(raw)

        assert decoded.c == "client-1"
        assert decoded.F is None

    def test_portfolio_margin_spot_execution_report_decodes_live_pm_new_order_shape(self):
        raw = json.dumps(
            {
                "e": "executionReport",
                "E": 1774348588522,
                "s": "PLUMEUSDT",
                "c": "O-20260324-103628-SPOT-000-1",
                "S": "BUY",
                "o": "LIMIT_MAKER",
                "f": "GTC",
                "q": "1000.00000000",
                "p": "0.01054000",
                "P": "0.00000000",
                "g": -1,
                "x": "NEW",
                "X": "NEW",
                "i": 256305429,
                "l": "0.00000000",
                "z": "0.00000000",
                "L": "0.00000000",
                "n": "0",
                "T": 1774348588522,
                "t": -1,
                "w": True,
                "m": False,
                "O": 1774348588522,
                "Z": "0.00000000",
                "Y": "0.00000000",
                "j": 299587532,
                "J": 230000,
                "V": "EXPIRE_MAKER",
                "I": 2524891758153,
            },
        ).encode()

        decoded = self.exec_client._decoder_spot_order_update.decode(raw)

        assert decoded.C is None
        assert decoded.r is None
        assert decoded.M is None
        assert decoded.Q is None

    def test_isolated_margin_account_http_api_is_rejected_explicitly(self):
        with pytest.raises(RuntimeError, match="ISOLATED_MARGIN"):
            BinanceSpotAccountHttpAPI(
                client=self.http_client,
                clock=self.clock,
                account_type=BinanceAccountType.ISOLATED_MARGIN,
            )

    def test_isolated_margin_execution_client_is_rejected_explicitly(self):
        # Base64-encoded 32 zero bytes for Ed25519 private key (test only)
        dummy_api_secret = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

        with pytest.raises(ValueError, match="SPOT, cross MARGIN, or PORTFOLIO_MARGIN"):
            BinanceSpotExecutionClient(
                loop=self.loop,
                client=self.http_client,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                instrument_provider=self.provider,
                base_url_ws="",
                config=BinanceExecClientConfig(),
                account_type=BinanceAccountType.ISOLATED_MARGIN,
                environment=BinanceEnvironment.LIVE,
                api_key="SOME_BINANCE_API_KEY",
                api_secret=dummy_api_secret,
            )

    @pytest.mark.asyncio
    async def test_margin_portfolio_margin_reject_disables_further_submit_attempts(self, mocker):
        http_client = BinanceHttpClient(
            clock=self.clock,
            api_key="SOME_BINANCE_API_KEY",
            api_secret="SOME_BINANCE_API_SECRET",
            base_url="https://api.binance.com/",
        )
        provider = BinanceSpotInstrumentProvider(
            client=http_client,
            clock=self.clock,
            config=InstrumentProviderConfig(load_all=True),
        )

        dummy_api_secret = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

        exec_client = BinanceSpotExecutionClient(
            loop=self.loop,
            client=http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            base_url_ws="",
            config=BinanceExecClientConfig(),
            account_type=BinanceAccountType.MARGIN,
            environment=BinanceEnvironment.LIVE,
            api_key="SOME_BINANCE_API_KEY",
            api_secret=dummy_api_secret,
        )

        mock_send_request = mocker.patch(
            target="nautilus_trader.adapters.binance.http.client.BinanceHttpClient.send_request",
            side_effect=BinanceClientError(
                status=400,
                message={"code": -3055, "msg": "Invalid requests for Portfolio Margin user."},
                headers={},
            ),
        )

        first_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=Price.from_str("10050.80"),
        )
        self.cache.add_order(first_order, None)
        first_submit = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=first_order,
            command_id=UUID4(),
            ts_init=0,
        )

        exec_client.submit_order(first_submit)
        await eventually(lambda: mock_send_request.call_count == 1)

        second_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(11),
            price=Price.from_str("10050.90"),
        )
        self.cache.add_order(second_order, None)
        second_submit = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=second_order,
            command_id=UUID4(),
            ts_init=0,
        )

        exec_client.submit_order(second_submit)
        await asyncio.sleep(0.1)

        assert mock_send_request.call_count == 1

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
