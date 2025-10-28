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
from unittest.mock import AsyncMock
from unittest.mock import Mock
from unittest.mock import patch

import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.mocks.exec_clients import MockLiveExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestLiveExecutionEngine:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.trader_id = TestIdStubs.trader_id()
        self._engines_to_cleanup = []

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
        )

        self.emulator = OrderEmulator(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.instrument_provider = InstrumentProvider()
        self.instrument_provider.add(AUDUSD_SIM)
        self.instrument_provider.add(GBPUSD_SIM)
        self.cache.add_instrument(AUDUSD_SIM)
        self.cache.add_instrument(GBPUSD_SIM)

        self.client = MockLiveExecutionClient(
            loop=self.loop,
            client_id=ClientId(SIM.value),
            venue=SIM,
            account_type=AccountType.CASH,
            base_currency=USD,
            instrument_provider=self.instrument_provider,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.portfolio.update_account(TestEventStubs.cash_account_state())
        self.exec_engine.register_client(self.client)

        self.cache.add_instrument(AUDUSD_SIM)

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine.start()
        self.risk_engine.start()
        self.exec_engine.start()
        self.emulator.start()
        self.strategy.start()

        yield

        # Teardown - stop all engines and clean up any tasks
        self.emulator.stop()
        self.exec_engine.stop()
        self.risk_engine.stop()
        self.data_engine.stop()

        # Clean up any additional engines created during tests
        for engine in self._engines_to_cleanup:
            if hasattr(engine, "stop") and not engine.is_stopped:
                engine.stop()

    @pytest.mark.asyncio()
    async def test_start_when_loop_not_running_logs(self):
        # Arrange, Act
        self.exec_engine.start()

        # Assert
        assert True  # No exceptions raised
        self.exec_engine.stop()

    @pytest.mark.asyncio()
    async def test_message_qsize_at_max_blocks_on_put_command(self):
        # Arrange
        # Deregister test fixture ExecutionEngine from msgbus)
        self.msgbus.deregister(
            endpoint="ExecEngine.execute",
            handler=self.exec_engine.execute,
        )
        self.msgbus.deregister(
            endpoint="ExecEngine.process",
            handler=self.exec_engine.process,
        )
        self.msgbus.deregister(
            endpoint="ExecEngine.reconcile_execution_report",
            handler=self.exec_engine.reconcile_execution_report,
        )
        self.msgbus.deregister(
            endpoint="ExecEngine.reconcile_execution_mass_status",
            handler=self.exec_engine.reconcile_execution_mass_status,
        )

        new_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveExecEngineConfig(
                debug=True,
                inflight_check_threshold_ms=0,
            ),
        )
        self._engines_to_cleanup.append(new_engine)
        self.exec_engine = new_engine

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
        self.exec_engine.execute(submit_order)
        self.exec_engine.execute(submit_order)

        # Assert
        await eventually(lambda: self.exec_engine.cmd_qsize() == 2)
        assert self.exec_engine.command_count == 0

    @pytest.mark.asyncio()
    async def test_message_qsize_at_max_blocks_on_put_event(self):
        # Arrange
        # Deregister test fixture ExecutionEngine from msgbus)
        self.msgbus.deregister(
            endpoint="ExecEngine.execute",
            handler=self.exec_engine.execute,
        )
        self.msgbus.deregister(
            endpoint="ExecEngine.process",
            handler=self.exec_engine.process,
        )
        self.msgbus.deregister(
            endpoint="ExecEngine.reconcile_execution_report",
            handler=self.exec_engine.reconcile_execution_report,
        )
        self.msgbus.deregister(
            endpoint="ExecEngine.reconcile_execution_mass_status",
            handler=self.exec_engine.reconcile_execution_mass_status,
        )

        new_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveExecEngineConfig(
                debug=True,
                inflight_check_threshold_ms=0,
            ),
        )
        self._engines_to_cleanup.append(new_engine)
        self.exec_engine = new_engine

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
        self.exec_engine.execute(submit_order)
        self.exec_engine.process(event)  # Add over max size

        # Assert
        await eventually(lambda: self.exec_engine.cmd_qsize() == 1)
        assert self.exec_engine.command_count == 0

    @pytest.mark.asyncio()
    async def test_start(self):
        # Arrange, Act
        self.exec_engine.start()

        # Assert
        await eventually(lambda: self.exec_engine.is_running)

        # Tear Down
        self.exec_engine.stop()

    @pytest.mark.asyncio()
    async def test_kill_when_running_and_no_messages_on_queues(self):
        # Arrange, Act
        self.exec_engine.kill()

        # Assert
        assert self.exec_engine.is_stopped

    @pytest.mark.asyncio()
    async def test_kill_when_not_running_with_messages_on_queue(self):
        # Arrange, Act
        self.exec_engine.stop()
        await eventually(lambda: self.exec_engine.is_stopped)
        self.exec_engine.kill()

        # Assert
        assert self.exec_engine.is_stopped

    @pytest.mark.asyncio()
    async def test_execute_command_places_command_on_queue(self):
        # Arrange
        self.exec_engine.start()

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
        self.exec_engine.execute(submit_order)

        # Assert
        await eventually(lambda: self.exec_engine.evt_qsize() == 0)
        await eventually(lambda: self.exec_engine.command_count == 1)

        # Tear Down
        self.exec_engine.stop()

    @pytest.mark.asyncio
    async def test_handle_order_status_report(self):
        # Arrange
        order_report = OrderStatusReport(
            account_id=AccountId("SIM-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            order_list_id=OrderListId("1"),
            venue_order_id=VenueOrderId("2"),
            order_side=OrderSide.SELL,
            order_type=OrderType.STOP_LIMIT,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            time_in_force=TimeInForce.DAY,
            expire_time=None,
            order_status=OrderStatus.REJECTED,
            price=Price.from_str("0.90090"),
            trigger_price=Price.from_str("0.90100"),
            trigger_type=TriggerType.DEFAULT,
            limit_offset=None,
            trailing_offset=Decimal("0.00010"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            quantity=Quantity.from_int(1_000_000),
            filled_qty=Quantity.from_int(0),
            display_qty=None,
            avg_px=None,
            post_only=True,
            reduce_only=False,
            cancel_reason="SOME_REASON",
            report_id=UUID4(),
            ts_accepted=1_000_000,
            ts_triggered=1_500_000,
            ts_last=2_000_000,
            ts_init=3_000_000,
        )

        # Act
        self.exec_engine.reconcile_execution_report(order_report)

        # Assert
        assert self.exec_engine.report_count == 1

    @pytest.mark.asyncio
    async def test_handle_fill_report(self):
        # Arrange
        fill_report = FillReport(
            account_id=AccountId("SIM-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456789"),
            venue_order_id=VenueOrderId("1"),
            venue_position_id=PositionId("2"),
            trade_id=TradeId("3"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("100.50"),
            commission=Money("4.50", USD),
            liquidity_side=LiquiditySide.TAKER,
            report_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.exec_engine.reconcile_execution_report(fill_report)

        # Assert
        assert self.exec_engine.report_count == 1

    @pytest.mark.asyncio
    async def test_handle_position_status_report(self):
        # Arrange
        position_report = PositionStatusReport(
            account_id=AccountId("SIM-001"),
            instrument_id=AUDUSD_SIM.id,
            venue_position_id=PositionId("1"),
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(1_000_000),
            report_id=UUID4(),
            ts_last=0,
            ts_init=0,
        )

        # Act
        self.exec_engine.reconcile_execution_report(position_report)

        # Assert
        assert self.exec_engine.report_count == 1

    @pytest.mark.asyncio
    async def test_execution_mass_status(self):
        # Arrange
        mass_status = ExecutionMassStatus(
            client_id=ClientId("SIM"),
            account_id=TestIdStubs.account_id(),
            venue=Venue("SIM"),
            report_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exec_engine.reconcile_execution_mass_status(mass_status)

        # Assert
        assert self.exec_engine.report_count == 1

    @pytest.mark.asyncio
    async def test_check_inflight_order_status(self):
        # Arrange
        # Deregister test fixture ExecutionEngine from msgbus
        order = self.strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=AUDUSD_SIM.make_price(0.70000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))

        # Assert
        await eventually(lambda: self.exec_engine.command_count >= 1, timeout=3.0)

    @pytest.mark.asyncio
    async def test_resolve_inflight_order_when_submitted(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=AUDUSD_SIM.make_price(0.70000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))

        await eventually(lambda: order.status == OrderStatus.SUBMITTED)

        self.exec_engine._resolve_inflight_order(order)

        # Assert
        assert order.status == OrderStatus.REJECTED

    @pytest.mark.asyncio
    async def test_resolve_inflight_order_when_pending_update(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=AUDUSD_SIM.make_price(0.70000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_pending_update(order))

        await eventually(lambda: order.status == OrderStatus.PENDING_UPDATE)

        self.exec_engine._resolve_inflight_order(order)

        # Assert
        assert order.status == OrderStatus.CANCELED

    @pytest.mark.asyncio
    async def test_resolve_inflight_order_when_pending_cancel(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=AUDUSD_SIM.make_price(0.70000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_pending_cancel(order))

        await eventually(lambda: order.status == OrderStatus.PENDING_CANCEL)

        self.exec_engine._resolve_inflight_order(order)

        # Assert
        assert order.status == OrderStatus.CANCELED

    @pytest.mark.asyncio
    async def test_graceful_shutdown_cmd_queue_exception_enabled_calls_shutdown_system(self):
        """
        Test that when graceful_shutdown_on_exception=True, shutdown_system is called on
        command queue exception.
        """
        # Arrange
        # Create fresh msgbus to avoid endpoint conflicts
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveExecEngineConfig(graceful_shutdown_on_exception=True)
        engine = LiveExecutionEngine(
            loop=self.loop,
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
            assert engine._is_shutting_down is True

            engine.stop()
            await eventually(lambda: engine.cmd_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_cmd_queue_exception_disabled_calls_os_exit(self):
        """
        Test that when graceful_shutdown_on_exception=False, os._exit is called on
        command queue exception.
        """
        # Arrange
        # Create fresh msgbus to avoid endpoint conflicts
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveExecEngineConfig(graceful_shutdown_on_exception=False)
        engine = LiveExecutionEngine(
            loop=self.loop,
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
        # Create fresh msgbus to avoid endpoint conflicts
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveExecEngineConfig(graceful_shutdown_on_exception=True)
        engine = LiveExecutionEngine(
            loop=self.loop,
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
            assert engine._is_shutting_down is True

            engine.stop()
            await eventually(lambda: engine.evt_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_evt_queue_exception_disabled_calls_os_exit(self):
        """
        Test that when graceful_shutdown_on_exception=False, os._exit is called on event
        queue exception.
        """
        # Arrange
        # Create fresh msgbus to avoid endpoint conflicts
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveExecEngineConfig(graceful_shutdown_on_exception=False)
        engine = LiveExecutionEngine(
            loop=self.loop,
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

    @pytest.mark.asyncio()
    async def test_reconciliation_with_none_mass_status_returns_false(self):
        """
        Test that reconciliation returns False when mass_status is None.
        """

        # Arrange
        async def mock_generate_mass_status(lookback_mins):
            return None

        self.client.generate_mass_status = mock_generate_mass_status
        self.exec_engine.start()

        # Act
        result = await self.exec_engine.reconcile_execution_state()

        # Assert - should return False because mass_status is None
        assert result is False

        # Cleanup
        self.exec_engine.stop()

    @pytest.mark.asyncio()
    async def test_filled_qty_mismatch_with_zero_report(self):
        """
        Test that filled_qty mismatch is detected when report.filled_qty is less than
        cached.
        """
        # Arrange
        order = self.order_factory.market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=AUDUSD_SIM.make_qty(100),
        )

        self.cache.add_order(order)
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        filled_event = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            last_qty=AUDUSD_SIM.make_qty(100),
        )
        order.apply(filled_event)
        self.cache.update_order(order)

        report = OrderStatusReport(
            account_id=AccountId("MOCK-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("V-123"),
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.FILLED,
            quantity=AUDUSD_SIM.make_qty(100),
            filled_qty=AUDUSD_SIM.make_qty(0),  # Zero filled (error: less than cached)
            report_id=UUID4(),
            ts_accepted=self.clock.timestamp_ns(),
            ts_last=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        result = self.exec_engine.reconcile_execution_report(report)

        # Assert - should correctly detect and fail on backwards fill quantity
        assert result is False

    @pytest.mark.asyncio()
    async def test_inflight_timeout_resolves_order(self):
        """
        Test that inflight orders are resolved after exceeding max retries.
        """
        # Arrange
        order = self.order_factory.market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=AUDUSD_SIM.make_qty(100),
        )

        self.cache.add_order(order)
        # Create an old submitted event so the order appears delayed
        old_ts = (
            self.clock.timestamp_ns()
            - self.exec_engine._inflight_check_threshold_ns
            - 1_000_000_000
        )
        order.apply(TestEventStubs.order_submitted(order, ts_event=old_ts))
        self.cache.update_order(order)

        # Set retry count to max so next check will resolve
        self.exec_engine._recon_check_retries[order.client_order_id] = (
            self.exec_engine.inflight_check_max_retries
        )

        # Verify preconditions
        assert order.is_inflight
        assert self.cache.orders_inflight() == [order]

        # Act - trigger the inflight check which should resolve the order
        await self.exec_engine._check_inflight_orders()

        # Assert - order should be resolved as REJECTED
        assert order.status == OrderStatus.REJECTED
        assert not order.is_inflight
        assert order.client_order_id not in self.exec_engine._recon_check_retries

        # Cleanup
        self.exec_engine.stop()

    @pytest.mark.asyncio()
    async def test_shutdown_flag_suppresses_reconciliation(self):
        """
        Test that _is_shutting_down flag prevents reconciliation from issuing HTTP
        calls.
        """
        # Arrange - mock the client's generate methods to track if they're called
        mock_generate_order_status = AsyncMock()
        self.client.generate_order_status_reports = mock_generate_order_status

        # Act - set shutdown flag and manually trigger reconciliation checks
        self.exec_engine._is_shutting_down = True

        await self.exec_engine._check_inflight_orders()
        await self.exec_engine._check_orders_consistency()

        # Assert - client methods should NOT have been called due to early exit
        mock_generate_order_status.assert_not_called()
