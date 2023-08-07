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

from decimal import Decimal

import pytest

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderEmulated
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderReleased
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.mocks.exec_clients import MockExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestOrderEmulatorWithSingleOrders:
    def setup(self) -> None:
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.INFO,
            bypass=True,
        )

        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache_db = MockCacheDatabase(
            logger=self.logger,
        )

        self.cache = Cache(
            database=self.cache_db,
            logger=self.logger,
        )
        self.cache.add_instrument(ETHUSDT_PERP_BINANCE)
        self.cache.add_instrument(BTCUSDT_BINANCE)
        self.cache.add_instrument(ETHUSDT_BINANCE)

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
            config=DataEngineConfig(debug=True),
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=RiskEngineConfig(debug=True),
        )

        self.emulator = OrderEmulator(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("BINANCE")
        self.exec_client = MockExecutionClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        update = TestEventStubs.margin_account_state(account_id=AccountId("BINANCE-001"))
        self.portfolio.update_account(update)
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

        self.data_engine.start()
        self.risk_engine.start()
        self.exec_engine.start()
        self.emulator.start()
        self.strategy.start()

    def test_emulator_reset(self) -> None:
        # Arrange
        self.emulator.stop()

        # Act, Assert
        self.emulator.reset()

    def test_emulator_dispose(self) -> None:
        # Arrange
        self.emulator.stop()

        # Act, Assert
        self.emulator.dispose()

    def test_create_matching_core_twice_raises_exception(self) -> None:
        # Arrange
        self.emulator.create_matching_core(
            ETHUSDT_PERP_BINANCE.id,
            ETHUSDT_PERP_BINANCE.price_increment,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            self.emulator.create_matching_core(
                ETHUSDT_PERP_BINANCE.id,
                ETHUSDT_PERP_BINANCE.price_increment,
            )

    def test_subscribed_quotes_when_nothing_subscribed_returns_empty_list(self) -> None:
        # Arrange, Act
        subscriptions = self.emulator.subscribed_quotes

        # Assert
        assert subscriptions == []

    def test_subscribed_trades_when_nothing_subscribed_returns_empty_list(self) -> None:
        # Arrange, Act
        subscriptions = self.emulator.subscribed_trades

        # Assert
        assert subscriptions == []

    def test_get_submit_order_commands_when_no_emulations_returns_empty_dict(self) -> None:
        # Arrange, Act
        commands = self.emulator.get_submit_order_commands()

        # Assert
        assert commands == {}

    def test_get_matching_core_when_no_emulations_returns_none(self) -> None:
        # Arrange, Act
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)

        # Assert
        assert matching_core is None

    def test_process_quote_tick_when_no_matching_core_setup_logs_and_does_nothing(self) -> None:
        # Arrange
        tick: QuoteTick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5060.0,
            ask=5070.0,
            bid_size=10.0,
            ask_size=10.0,
        )

        # Act
        self.emulator.on_quote_tick(tick)

        # Assert
        assert True  # No exception raised

    def test_process_trade_tick_when_no_matching_core_setup_logs_and_does_nothing(self) -> None:
        # Arrange
        tick: TradeTick = TestDataStubs.trade_tick(ETHUSDT_PERP_BINANCE)

        # Act
        self.emulator.on_trade_tick(tick)

        # Assert
        assert True  # No exception raised

    def test_execute_unrecognized_command_logs_and_continues(self) -> None:
        # Arrange
        command = QueryOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert (no exceptions raised)
        self.emulator.execute(command)

    def test_submit_limit_order_with_emulation_trigger_not_supported_then_cancels(self) -> None:
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=ETHUSDT_PERP_BINANCE.make_price(2000),
            emulation_trigger=TriggerType.INDEX_PRICE,
        )

        # Act
        self.strategy.submit_order(order)

        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)

        # Assert
        assert matching_core is None
        assert order.is_canceled
        assert not self.emulator.get_submit_order_commands()
        assert not self.emulator.subscribed_trades
        assert order not in self.cache.orders_emulated()

    def test_submit_limit_order_with_instrument_not_found_then_cancels(self) -> None:
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=AUDUSD_SIM.make_price(1.0),
            emulation_trigger=TriggerType.DEFAULT,
        )

        self.cache.add_order(order, position_id=None)

        submit = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            position_id=PositionId("P-123456"),
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.emulator.execute(submit)

        matching_core = self.emulator.get_matching_core(AUDUSD_SIM.id)

        # Assert
        assert matching_core is None
        assert order.is_canceled
        assert not self.emulator.get_submit_order_commands()
        assert not self.emulator.subscribed_trades

    @pytest.mark.parametrize(
        "emulation_trigger",
        [
            TriggerType.DEFAULT,
            TriggerType.BID_ASK,
        ],
    )
    def test_submit_limit_order_with_emulation_trigger_default_and_bid_ask_subscribes_to_data(
        self,
        emulation_trigger,
    ):
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=ETHUSDT_PERP_BINANCE.make_price(2000),
            emulation_trigger=emulation_trigger,
        )

        # Act
        self.strategy.submit_order(order)

        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)

        # Assert
        assert matching_core is not None
        assert order in matching_core.get_orders()
        assert len(self.emulator.get_submit_order_commands()) == 1
        assert self.emulator.subscribed_quotes == [InstrumentId.from_str("ETHUSDT-PERP.BINANCE")]

    def test_submit_order_with_emulation_trigger_last_subscribes_to_data(self) -> None:
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=ETHUSDT_PERP_BINANCE.make_price(2000),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        # Act
        self.strategy.submit_order(order)

        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)

        # Assert
        assert matching_core is not None
        assert order in matching_core.get_orders()
        assert len(self.emulator.get_submit_order_commands()) == 1
        assert self.emulator.subscribed_trades == [InstrumentId.from_str("ETHUSDT-PERP.BINANCE")]

    def test_cancel_all_with_emulated_order_cancels_order(self) -> None:
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=ETHUSDT_PERP_BINANCE.make_price(2000),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        self.strategy.submit_order(order)

        # Act
        self.strategy.cancel_all_orders(ETHUSDT_PERP_BINANCE.id)

        # Assert
        assert order.is_canceled

    def test_cancel_all_buy_orders_with_emulated_orders_cancels_buy_order(self) -> None:
        # Arrange
        order1 = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=ETHUSDT_PERP_BINANCE.make_price(2000),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        order2 = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            price=ETHUSDT_PERP_BINANCE.make_price(2010),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        self.strategy.submit_order(order1)
        self.strategy.submit_order(order2)

        # Act
        self.strategy.cancel_all_orders(ETHUSDT_PERP_BINANCE.id, order_side=OrderSide.BUY)

        # Assert
        assert order1.is_canceled
        assert not order2.is_canceled

    def test_cancel_all_sell_orders_with_emulated_orders_cancels_sell_order(self) -> None:
        # Arrange
        order1 = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10),
            price=ETHUSDT_PERP_BINANCE.make_price(2000),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        order2 = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(10),
            price=ETHUSDT_PERP_BINANCE.make_price(2010),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        self.strategy.submit_order(order1)
        self.strategy.submit_order(order2)

        # Act
        self.strategy.cancel_all_orders(ETHUSDT_PERP_BINANCE.id, order_side=OrderSide.SELL)

        # Assert
        assert not order1.is_canceled
        assert order2.is_canceled

    @pytest.mark.parametrize(
        ("order_side", "trigger_price"),
        [
            [OrderSide.BUY, ETHUSDT_PERP_BINANCE.make_price(5000)],
            [OrderSide.SELL, ETHUSDT_PERP_BINANCE.make_price(5000)],
        ],
    )
    def test_submit_limit_order_last_then_triggered_releases_market_order(
        self,
        order_side,
        trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            price=trigger_price,
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        self.strategy.submit_order(order)

        tick = TestDataStubs.trade_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            price=5000.0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.MARKET
        assert order.emulation_trigger == TriggerType.NO_TRIGGER
        assert len(order.events) == 4
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderInitialized)
        assert isinstance(order.events[3], OrderReleased)
        assert self.exec_client.calls == ["_start", "submit_order"]

    @pytest.mark.parametrize(
        ("order_side", "trigger_price"),
        [
            [OrderSide.BUY, ETHUSDT_PERP_BINANCE.make_price(5000)],
            [OrderSide.SELL, ETHUSDT_PERP_BINANCE.make_price(5000)],
        ],
    )
    def test_submit_limit_order_bid_ask_then_triggered_releases_market_order(
        self,
        order_side,
        trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            price=trigger_price,
            emulation_trigger=TriggerType.DEFAULT,
        )

        self.strategy.submit_order(order)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5000.0,
            ask=5000.0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.MARKET
        assert order.emulation_trigger == TriggerType.NO_TRIGGER
        assert len(order.events) == 4
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderInitialized)
        assert isinstance(order.events[3], OrderReleased)
        assert self.exec_client.calls == ["_start", "submit_order"]

    @pytest.mark.parametrize(
        ("order_side", "trigger_price"),
        [
            [OrderSide.BUY, ETHUSDT_PERP_BINANCE.make_price(5000)],
            [OrderSide.SELL, ETHUSDT_PERP_BINANCE.make_price(5000)],
        ],
    )
    def test_submit_limit_if_touched_then_triggered_releases_limit_order(
        self,
        order_side,
        trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.limit_if_touched(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            price=ETHUSDT_PERP_BINANCE.make_price(5000),
            trigger_price=trigger_price,
            emulation_trigger=TriggerType.DEFAULT,
        )

        self.strategy.submit_order(order)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5000.0,
            ask=5000.0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.LIMIT
        assert order.emulation_trigger == TriggerType.NO_TRIGGER
        assert len(order.events) == 4
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderInitialized)
        assert isinstance(order.events[3], OrderReleased)
        assert self.exec_client.calls == ["_start", "submit_order"]

    @pytest.mark.parametrize(
        ("order_side", "trigger_price"),
        [
            [OrderSide.BUY, ETHUSDT_PERP_BINANCE.make_price(5000)],
            [OrderSide.SELL, ETHUSDT_PERP_BINANCE.make_price(5000)],
        ],
    )
    def test_submit_stop_limit_order_then_triggered_releases_limit_order(
        self,
        order_side,
        trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.stop_limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            price=trigger_price,
            trigger_price=trigger_price,
            emulation_trigger=TriggerType.DEFAULT,
        )

        self.strategy.submit_order(order)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5000.0,
            ask=5000.0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.LIMIT
        assert order.emulation_trigger == TriggerType.NO_TRIGGER
        assert len(order.events) == 4
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderInitialized)
        assert isinstance(order.events[3], OrderReleased)
        assert self.exec_client.calls == ["_start", "submit_order"]

    @pytest.mark.parametrize(
        ("order_side", "trigger_price"),
        [
            [OrderSide.BUY, ETHUSDT_PERP_BINANCE.make_price(5070)],
            [OrderSide.SELL, ETHUSDT_PERP_BINANCE.make_price(5060)],
        ],
    )
    def test_submit_market_if_touched_order_then_triggered_releases_market_order(
        self,
        order_side,
        trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.market_if_touched(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            trigger_price=trigger_price,
            trigger_type=TriggerType.BID_ASK,
            emulation_trigger=TriggerType.BID_ASK,
        )

        self.strategy.submit_order(order)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5060.0,
            ask=5070.0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.MARKET
        assert order.emulation_trigger == TriggerType.NO_TRIGGER
        assert len(order.events) == 4
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderInitialized)
        assert isinstance(order.events[3], OrderReleased)
        assert self.exec_client.calls == ["_start", "submit_order"]

    @pytest.mark.parametrize(
        ("order_side", "trigger_price"),
        [
            [OrderSide.BUY, ETHUSDT_PERP_BINANCE.make_price(5060)],
            [OrderSide.SELL, ETHUSDT_PERP_BINANCE.make_price(5070)],
        ],
    )
    def test_submit_stop_market_order_then_triggered_releases_market_order(
        self,
        order_side,
        trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            trigger_price=trigger_price,
            trigger_type=TriggerType.BID_ASK,
            emulation_trigger=TriggerType.BID_ASK,
        )

        self.strategy.submit_order(order)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5060.0,
            ask=5070.0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.MARKET
        assert order.emulation_trigger == TriggerType.NO_TRIGGER
        assert len(order.events) == 4
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderInitialized)
        assert isinstance(order.events[3], OrderReleased)
        assert self.exec_client.calls == ["_start", "submit_order"]
        assert order not in self.cache.orders_emulated()

    @pytest.mark.parametrize(
        ("order_side", "expected_trigger_price"),
        [
            [OrderSide.BUY, ETHUSDT_PERP_BINANCE.make_price(5075)],
            [OrderSide.SELL, ETHUSDT_PERP_BINANCE.make_price(5055)],
        ],
    )
    def test_submit_trailing_stop_market_order_with_no_trigger_price_then_updates(
        self,
        order_side,
        expected_trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            trigger_type=TriggerType.BID_ASK,
            trailing_offset=Decimal(5),
            trailing_offset_type=TrailingOffsetType.PRICE,
            emulation_trigger=TriggerType.BID_ASK,
        )

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5060.0,
            ask=5070.0,
        )

        self.data_engine.process(tick)

        # Act
        self.strategy.submit_order(order)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.TRAILING_STOP_MARKET
        assert order.emulation_trigger == TriggerType.BID_ASK
        assert len(order.events) == 3
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderUpdated)
        assert isinstance(order.events[2], OrderEmulated)
        assert order.trigger_price == expected_trigger_price

    @pytest.mark.parametrize(
        ("order_side", "trigger_price", "expected_trigger_price"),
        [
            [
                OrderSide.BUY,
                ETHUSDT_PERP_BINANCE.make_price(5075.0),
                ETHUSDT_PERP_BINANCE.make_price(5070.0),
            ],
            [
                OrderSide.SELL,
                ETHUSDT_PERP_BINANCE.make_price(5055.0),
                ETHUSDT_PERP_BINANCE.make_price(5060.0),
            ],
        ],
    )
    def test_submit_trailing_stop_market_order_with_trigger_price_then_updates(
        self,
        order_side,
        trigger_price,
        expected_trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            trigger_type=TriggerType.BID_ASK,
            trigger_price=trigger_price,
            trailing_offset=Decimal(5),
            trailing_offset_type=TrailingOffsetType.PRICE,
            emulation_trigger=TriggerType.BID_ASK,
        )

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5060.0,
            ask=5070.0,
        )

        self.data_engine.process(tick)

        tick = TestDataStubs.trade_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            price=5010.0,
        )

        self.data_engine.process(tick)

        # Act
        self.strategy.submit_order(order)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5065.0,
            ask=5065.0,
        )
        self.data_engine.process(tick)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.TRAILING_STOP_MARKET
        assert order.emulation_trigger == TriggerType.BID_ASK
        assert len(order.events) == 3
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderUpdated)
        assert order.trigger_price == expected_trigger_price

    @pytest.mark.parametrize(
        ("order_side", "trigger_price"),
        [
            [OrderSide.BUY, ETHUSDT_PERP_BINANCE.make_price(5075)],
            [OrderSide.SELL, ETHUSDT_PERP_BINANCE.make_price(5055)],
        ],
    )
    def test_submit_trailing_stop_market_order_with_trigger_price_then_triggers(
        self,
        order_side,
        trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.trailing_stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            trigger_type=TriggerType.BID_ASK,
            trigger_price=trigger_price,
            trailing_offset=Decimal(5),
            trailing_offset_type=TrailingOffsetType.PRICE,
            emulation_trigger=TriggerType.BID_ASK,
        )

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5060.0,
            ask=5070.0,
        )
        self.data_engine.process(tick)

        # Act
        self.strategy.submit_order(order)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5055.0,
            ask=5075.0,
        )
        self.data_engine.process(tick)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.MARKET
        assert order.emulation_trigger == TriggerType.NO_TRIGGER
        assert len(order.events) == 4
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderInitialized)
        assert isinstance(order.events[3], OrderReleased)
        assert order not in self.cache.orders_emulated()

    @pytest.mark.parametrize(
        ("order_side", "price", "expected_trigger_price"),
        [
            [
                OrderSide.BUY,
                ETHUSDT_PERP_BINANCE.make_price(5070),
                ETHUSDT_PERP_BINANCE.make_price(5075),
            ],
            [
                OrderSide.SELL,
                ETHUSDT_PERP_BINANCE.make_price(5060),
                ETHUSDT_PERP_BINANCE.make_price(5055),
            ],
        ],
    )
    def test_submit_trailing_stop_limit_order_with_no_trigger_price_then_updates(
        self,
        order_side,
        price,
        expected_trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            limit_offset=Decimal(5),
            trailing_offset=Decimal(5),
            price=price,
            trigger_type=TriggerType.BID_ASK,
            trailing_offset_type=TrailingOffsetType.PRICE,
            emulation_trigger=TriggerType.BID_ASK,
        )

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5060.0,
            ask=5070.0,
        )
        self.data_engine.process(tick)

        # Act
        self.strategy.submit_order(order)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.TRAILING_STOP_LIMIT
        assert order.emulation_trigger == TriggerType.BID_ASK
        assert len(order.events) == 3
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderUpdated)
        assert isinstance(order.events[2], OrderEmulated)
        assert order.trigger_price == expected_trigger_price

    @pytest.mark.parametrize(
        ("order_side", "price", "trigger_price", "expected_trigger_price"),
        [
            [
                OrderSide.BUY,
                ETHUSDT_PERP_BINANCE.make_price(5075.0),
                ETHUSDT_PERP_BINANCE.make_price(5070.0),
                ETHUSDT_PERP_BINANCE.make_price(5070.0),
            ],
            [
                OrderSide.SELL,
                ETHUSDT_PERP_BINANCE.make_price(5055.0),
                ETHUSDT_PERP_BINANCE.make_price(5060.0),
                ETHUSDT_PERP_BINANCE.make_price(5060.0),
            ],
        ],
    )
    def test_submit_trailing_stop_limit_order_with_trigger_price_then_updates(
        self,
        order_side,
        price,
        trigger_price,
        expected_trigger_price,
    ):
        # Arrange
        order = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            limit_offset=Decimal(5),
            trailing_offset=Decimal(5),
            price=price,
            trigger_type=TriggerType.BID_ASK,
            trigger_price=trigger_price,
            trailing_offset_type=TrailingOffsetType.PRICE,
            emulation_trigger=TriggerType.BID_ASK,
        )

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5060.0,
            ask=5070.0,
        )

        self.data_engine.process(tick)

        # Act
        self.strategy.submit_order(order)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5065.0,
            ask=5065.0,
        )
        self.data_engine.process(tick)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.TRAILING_STOP_LIMIT
        assert order.emulation_trigger == TriggerType.BID_ASK
        assert len(order.events) == 3
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderUpdated)
        assert order.trigger_price == expected_trigger_price

    @pytest.mark.parametrize(
        ("order_side", "price"),
        [
            [OrderSide.BUY, ETHUSDT_PERP_BINANCE.make_price(5070)],
            [OrderSide.SELL, ETHUSDT_PERP_BINANCE.make_price(5060)],
        ],
    )
    def test_submit_trailing_stop_limit_order_with_trigger_price_then_triggers(
        self,
        order_side,
        price,
    ):
        # Arrange
        order = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            limit_offset=Decimal(5),
            trailing_offset=Decimal(5),
            price=price,
            trigger_price=price,
            trigger_type=TriggerType.BID_ASK,
            trailing_offset_type=TrailingOffsetType.PRICE,
            emulation_trigger=TriggerType.BID_ASK,
        )

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5060.0,
            ask=5070.0,
        )

        self.data_engine.process(tick)

        # Act
        self.strategy.submit_order(order)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5055.0,
            ask=5075.0,
        )

        self.data_engine.process(tick)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.LIMIT
        assert order.emulation_trigger == TriggerType.NO_TRIGGER
        assert len(order.events) == 4
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderInitialized)
        assert isinstance(order.events[3], OrderReleased)
        assert order not in self.cache.orders_emulated()

    @pytest.mark.parametrize(
        ("order_side", "trigger_price", "price"),
        [
            [
                OrderSide.BUY,
                ETHUSDT_PERP_BINANCE.make_price(5070.0),
                ETHUSDT_PERP_BINANCE.make_price(5070.0),
            ],
            [
                OrderSide.SELL,
                ETHUSDT_PERP_BINANCE.make_price(5060.0),
                ETHUSDT_PERP_BINANCE.make_price(5060.0),
            ],
        ],
    )
    def test_submit_limit_if_touched_immediately_triggered_releases_limit_order(
        self,
        order_side,
        trigger_price,
        price,
    ):
        # Arrange
        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid=5060.0,
            ask=5070.0,
        )

        self.emulator.create_matching_core(
            ETHUSDT_PERP_BINANCE.id,
            ETHUSDT_PERP_BINANCE.price_increment,
        )
        self.emulator.on_quote_tick(tick)

        # Act
        order = self.strategy.order_factory.limit_if_touched(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            price=price,
            trigger_price=trigger_price,
            trigger_type=TriggerType.BID_ASK,
            emulation_trigger=TriggerType.BID_ASK,
        )

        self.strategy.submit_order(order)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.LIMIT
        assert order.emulation_trigger == TriggerType.NO_TRIGGER
        assert len(order.events) == 3
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderInitialized)
        assert isinstance(order.events[2], OrderReleased)
        assert self.exec_client.calls == ["_start", "submit_order"]
        assert order not in self.cache.orders_emulated()

    @pytest.mark.parametrize(
        ("order_side"),
        [
            OrderSide.BUY,
            OrderSide.SELL,
        ],
    )
    def test_submit_limit_order_bid_ask_with_synthetic_instrument_trigger(
        self,
        order_side,
    ):
        # Arrange
        synthetic = TestInstrumentProvider.synthetic_instrument()
        self.cache.add_synthetic(synthetic)

        order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_BINANCE.id,
            order_side=order_side,
            quantity=Quantity.from_int(10),
            price=Price.from_str("30000.00"),  # <-- Synthetic price
            emulation_trigger=TriggerType.DEFAULT,
            trigger_instrument_id=synthetic.id,
        )

        self.strategy.submit_order(order)

        tick1 = TestDataStubs.quote_tick(
            instrument=ETHUSDT_BINANCE,
            bid=10_000.0,
            ask=10_000.0,
        )
        tick2 = TestDataStubs.quote_tick(
            instrument=BTCUSDT_BINANCE,
            bid=50_000.0,
            ask=50_000.0,
        )

        # Act
        self.data_engine.process(tick1)  # <-- No synthetic tick emitted yet
        self.data_engine.process(tick2)

        # Assert
        order = self.cache.order(order.client_order_id)  # Recover transformed order from cache
        assert order.order_type == OrderType.MARKET
        assert order.emulation_trigger == TriggerType.NO_TRIGGER
        assert len(order.events) == 4
        assert isinstance(order.events[0], OrderInitialized)
        assert isinstance(order.events[1], OrderEmulated)
        assert isinstance(order.events[2], OrderInitialized)
        assert isinstance(order.events[3], OrderReleased)
        assert self.exec_client.calls == ["_start", "submit_order"]
