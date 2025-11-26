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
from unittest.mock import Mock
from unittest.mock import patch

import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.mocks.exec_clients import MockExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestLiveRiskEngine:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=self.clock,
        )

        self.random_order_factory = OrderFactory(
            trader_id=TraderId("RANDOM-042"),
            strategy_id=StrategyId("S-042"),
            clock=self.clock,
        )

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveExecEngineConfig(debug=True),
        )

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveRiskEngineConfig(debug=True),
        )

        self.exec_client = MockExecutionClient(
            client_id=ClientId("SIM"),
            venue=SIM,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)

        yield

        # Teardown
        if self.risk_engine.is_running:
            self.risk_engine.stop()
        self.risk_engine.dispose()

    @pytest.mark.asyncio
    async def test_start_when_loop_not_running_logs(self):
        # Arrange, Act
        self.risk_engine.start()

        # Assert
        assert True  # No exceptions raised
        self.risk_engine.stop()

    @pytest.mark.asyncio
    async def test_message_qsize_at_max_blocks_on_put_command(self):
        # Arrange
        self.msgbus.deregister("RiskEngine.execute", self.risk_engine.execute)
        self.msgbus.deregister("RiskEngine.process", self.risk_engine.process)

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveRiskEngineConfig(qsize=1),
        )

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        self.risk_engine.execute(submit_order)

        # Assert
        await eventually(lambda: self.risk_engine.cmd_qsize() == 1)
        assert self.risk_engine.command_count == 0

    @pytest.mark.asyncio
    async def test_message_qsize_at_max_blocks_on_put_event(self):
        # Arrange
        self.msgbus.deregister("RiskEngine.execute", self.risk_engine.execute)
        self.msgbus.deregister("RiskEngine.process", self.risk_engine.process)

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveRiskEngineConfig(qsize=1),
        )

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        event = TestEventStubs.order_submitted(order)

        # Act
        self.risk_engine.execute(submit_order)
        self.risk_engine.process(event)  # Add over max size

        # Assert
        await eventually(lambda: self.risk_engine.cmd_qsize() == 1)
        assert self.risk_engine.event_count == 0

    @pytest.mark.asyncio
    async def test_start(self):
        # Arrange, Act
        self.risk_engine.start()

        # Assert
        await eventually(lambda: self.risk_engine.is_running)

    @pytest.mark.asyncio
    async def test_kill_when_running_and_no_messages_on_queues(self):
        # Arrange, Act
        self.risk_engine.start()
        await asyncio.sleep(0)
        self.risk_engine.kill()

        # Assert
        assert self.risk_engine.is_stopped

    @pytest.mark.asyncio
    async def test_kill_when_not_running_with_messages_on_queue(self):
        # Arrange, Act
        self.risk_engine.kill()

        # Assert
        assert self.risk_engine.cmd_qsize() == 0
        assert self.risk_engine.evt_qsize() == 0

    @pytest.mark.asyncio
    async def test_execute_command_places_command_on_queue(self):
        # Arrange
        self.risk_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        await eventually(lambda: self.risk_engine.cmd_qsize() == 0)
        await eventually(lambda: self.risk_engine.command_count == 1)

    @pytest.mark.asyncio
    async def test_handle_position_opening_with_position_id_none(self):
        # Arrange
        self.risk_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        event = TestEventStubs.order_submitted(order)

        # Act
        self.risk_engine.process(event)

        # Assert
        await eventually(lambda: self.risk_engine.cmd_qsize() == 0)
        await eventually(lambda: self.risk_engine.event_count == 1)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_cmd_queue_exception_enabled_calls_shutdown_system(self):
        """
        Test that when graceful_shutdown_on_exception=True, shutdown_system is called on
        command queue exception.
        """
        # Arrange
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        test_portfolio = Portfolio(
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        config = LiveRiskEngineConfig(graceful_shutdown_on_exception=True)
        engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=test_portfolio,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock shutdown_system to track calls
        shutdown_mock = Mock()
        engine.shutdown_system = shutdown_mock

        # Mock _execute_command to raise an exception
        def mock_execute_command(command):
            raise ValueError("Test exception for graceful shutdown in cmd queue")

        with patch.object(engine, "_execute_command", side_effect=mock_execute_command):
            engine.start()

            # Act - Send command that will trigger the exception
            order = self.order_factory.market(
                instrument_id=AUDUSD_SIM.id,
                order_side=OrderSide.BUY,
                quantity=AUDUSD_SIM.make_qty(100),
            )
            submit_order = SubmitOrder(
                trader_id=self.trader_id,
                strategy_id=StrategyId("S-001"),
                order=order,
                command_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
            )
            engine.execute(submit_order)

            # Wait for processing and shutdown call
            await eventually(lambda: shutdown_mock.called)

            # Assert
            shutdown_mock.assert_called_once()
            args = shutdown_mock.call_args[0]
            assert "Test exception for graceful shutdown in cmd queue" in args[0]
            assert engine._shutdown_initiated is True

            engine.stop()
            await eventually(lambda: engine.cmd_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_cmd_queue_exception_disabled_calls_os_exit(self):
        """
        Test that when graceful_shutdown_on_exception=False, os._exit is called on
        command queue exception.
        """
        # Arrange
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        test_portfolio = Portfolio(
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        config = LiveRiskEngineConfig(graceful_shutdown_on_exception=False)
        engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=test_portfolio,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock os._exit to track calls instead of actually exiting
        with patch("os._exit") as exit_mock:
            # Mock _execute_command to raise an exception
            def mock_execute_command(command):
                raise ValueError("Test exception for immediate crash in cmd queue")

            with patch.object(engine, "_execute_command", side_effect=mock_execute_command):
                engine.start()

                # Act - Send command that will trigger the exception
                order = self.order_factory.market(
                    instrument_id=AUDUSD_SIM.id,
                    order_side=OrderSide.BUY,
                    quantity=AUDUSD_SIM.make_qty(100),
                )
                submit_order = SubmitOrder(
                    trader_id=self.trader_id,
                    strategy_id=StrategyId("S-001"),
                    order=order,
                    command_id=UUID4(),
                    ts_init=self.clock.timestamp_ns(),
                )
                engine.execute(submit_order)

                # Wait for processing and os._exit call
                await eventually(lambda: exit_mock.called)

                # Assert
                exit_mock.assert_called_once_with(1)

            engine.stop()
            await eventually(lambda: engine.cmd_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_evt_queue_exception_enabled_calls_shutdown_system(self):
        """
        Test that when graceful_shutdown_on_exception=True, shutdown_system is called on
        event queue exception.
        """
        # Arrange
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        test_portfolio = Portfolio(
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        config = LiveRiskEngineConfig(graceful_shutdown_on_exception=True)
        engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=test_portfolio,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock shutdown_system to track calls
        shutdown_mock = Mock()
        engine.shutdown_system = shutdown_mock

        # Mock _handle_event to raise an exception
        def mock_handle_event(event):
            raise ValueError("Test exception for graceful shutdown in evt queue")

        with patch.object(engine, "_handle_event", side_effect=mock_handle_event):
            engine.start()

            # Act - Send event that will trigger the exception
            order = self.order_factory.market(
                instrument_id=AUDUSD_SIM.id,
                order_side=OrderSide.BUY,
                quantity=AUDUSD_SIM.make_qty(100),
            )
            event = TestEventStubs.order_submitted(order)
            engine.process(event)

            # Wait for processing and shutdown call
            await eventually(lambda: shutdown_mock.called)

            # Assert
            shutdown_mock.assert_called_once()
            args = shutdown_mock.call_args[0]
            assert "Test exception for graceful shutdown in evt queue" in args[0]
            assert engine._shutdown_initiated is True

            engine.stop()
            await eventually(lambda: engine.evt_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_evt_queue_exception_disabled_calls_os_exit(self):
        """
        Test that when graceful_shutdown_on_exception=False, os._exit is called on event
        queue exception.
        """
        # Arrange
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        test_portfolio = Portfolio(
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        config = LiveRiskEngineConfig(graceful_shutdown_on_exception=False)
        engine = LiveRiskEngine(
            loop=self.loop,
            portfolio=test_portfolio,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock os._exit to track calls instead of actually exiting
        with patch("os._exit") as exit_mock:
            # Mock _handle_event to raise an exception
            def mock_handle_event(event):
                raise ValueError("Test exception for immediate crash in evt queue")

            with patch.object(engine, "_handle_event", side_effect=mock_handle_event):
                engine.start()

                # Act - Send event that will trigger the exception
                order = self.order_factory.market(
                    instrument_id=AUDUSD_SIM.id,
                    order_side=OrderSide.BUY,
                    quantity=AUDUSD_SIM.make_qty(100),
                )
                event = TestEventStubs.order_submitted(order)
                engine.process(event)

                # Wait for processing and os._exit call
                await eventually(lambda: exit_mock.called)

                # Assert
                exit_mock.assert_called_once_with(1)

            engine.stop()
            await eventually(lambda: engine.evt_qsize() == 0)

    @pytest.mark.asyncio
    async def test_trailing_stop_market_order_uses_quotes_when_no_trade_data(self):
        # Arrange
        self.cache.add_instrument(AUDUSD_SIM)

        # Add only quote data (no trade data) to test LAST_OR_BID_ASK fallback
        quote = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("0.99900"),
            ask_price=Price.from_str("1.00100"),
            bid_size=Quantity.from_int(100000),
            ask_size=Quantity.from_int(100000),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_quote_tick(quote)

        order = self.order_factory.trailing_stop_market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            trailing_offset=Decimal("0.00010"),
            trailing_offset_type=TrailingOffsetType.PRICE,
        )

        assert order.trigger_price is None

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.order_factory.strategy_id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.start()

        # Act
        self.risk_engine.execute(submit_order)

        await eventually(lambda: self.risk_engine.command_count > 0)

        # Assert - order should pass risk check using quote fallback
        assert self.risk_engine.command_count == 1

        self.risk_engine.stop()

    @pytest.mark.asyncio
    async def test_trailing_stop_market_order_risk_check_without_trigger_price(self):
        # Arrange
        self.cache.add_instrument(AUDUSD_SIM)

        trade = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("1"),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_trade_tick(trade)

        order = self.order_factory.trailing_stop_market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            trailing_offset=Decimal("0.00010"),
            trailing_offset_type=TrailingOffsetType.PRICE,
        )

        assert order.trigger_price is None

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.order_factory.strategy_id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.start()

        # Act
        self.risk_engine.execute(submit_order)

        await eventually(lambda: self.risk_engine.command_count > 0)

        # Assert
        assert self.risk_engine.command_count == 1

        self.risk_engine.stop()

    @pytest.mark.asyncio
    async def test_trailing_stop_market_order_denies_unsupported_offset_type(self):
        # Arrange
        self.cache.add_instrument(AUDUSD_SIM)

        trade = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("1"),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_trade_tick(trade)

        order = self.order_factory.trailing_stop_market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            trailing_offset=Decimal("0.00010"),
            trailing_offset_type=TrailingOffsetType.PRICE_TIER,
        )

        assert order.trigger_price is None

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.order_factory.strategy_id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.exec_engine.start()
        self.risk_engine.start()

        # Act
        self.risk_engine.execute(submit_order)

        await eventually(lambda: self.risk_engine.cmd_qsize() == 0)

        # Assert
        assert len(self.exec_client.commands) == 0

        self.risk_engine.stop()
        self.exec_engine.stop()

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        "offset_type",
        [
            TrailingOffsetType.PRICE,
            TrailingOffsetType.BASIS_POINTS,
            TrailingOffsetType.TICKS,
        ],
    )
    async def test_trailing_stop_market_order_accepts_supported_offset_types(
        self,
        offset_type,
    ):
        # Arrange
        self.cache.add_instrument(AUDUSD_SIM)

        trade = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("1"),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_trade_tick(trade)

        offset = Decimal("0.00010") if offset_type == TrailingOffsetType.PRICE else Decimal(10)
        order = self.order_factory.trailing_stop_market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            trailing_offset=offset,
            trailing_offset_type=offset_type,
        )

        assert order.trigger_price is None

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.order_factory.strategy_id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.start()

        # Act
        self.risk_engine.execute(submit_order)

        await eventually(lambda: self.risk_engine.command_count > 0)

        # Assert
        assert self.risk_engine.command_count == 1
        assert order.status != OrderStatus.DENIED

        self.risk_engine.stop()
