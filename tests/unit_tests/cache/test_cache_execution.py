# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.data.providers import TestDataProvider
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.execution.algorithm import ExecAlgorithmSpecification
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
BTCUSD_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestCache:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(self.clock)

        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = Cache(
            database=None,
            logger=self.logger,
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

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    def test_cache_currencies_with_no_currencies(self):
        # Arrange, Act
        self.cache.cache_currencies()

        # Assert
        assert True  # No exception raised

    def test_cache_instruments_with_no_instruments(self):
        # Arrange, Act
        self.cache.cache_instruments()

        # Assert
        assert True  # No exception raised

    def test_cache_accounts_with_no_accounts(self):
        # Arrange, Act
        self.cache.cache_accounts()

        # Assert
        assert True  # No exception raised

    def test_cache_orders_with_no_orders(self):
        # Arrange, Act
        self.cache.cache_orders()

        # Assert
        assert True  # No exception raised

    def test_orders_for_position_when_no_position_returns_empty_list(self):
        # Arrange, Act
        result = self.cache.orders_for_position(PositionId("1"))

        # Assert
        assert result == []

    def test_cache_positions_with_no_positions(self):
        # Arrange, Act
        self.cache.cache_positions()

        # Assert
        assert True  # No exception raised

    def test_cache_commands_with_no_commands(self):
        # Arrange, Act
        self.cache.cache_commands()

        # Assert
        assert True  # No exception raised

    def test_build_index_with_no_objects(self):
        # Arrange, Act
        self.cache.build_index()

        # Assert
        assert True  # No exception raised

    def test_add_currency(self):
        # Arrange
        currency = Currency(
            code="1INCH",
            precision=8,
            iso4217=0,
            name="1inch Network",
            currency_type=CurrencyType.CRYPTO,
        )

        # Act
        self.cache.add_currency(currency)

        # Assert
        assert Currency.from_str("1INCH") == currency

    def test_add_account(self):
        # Arrange
        account = TestExecStubs.cash_account()

        # Act
        self.cache.add_account(account)

        # Assert
        assert self.cache.load_account(account.id) == account

    def test_load_instrument(self):
        # Arrange
        self.cache.add_instrument(AUDUSD_SIM)

        # Act
        result = self.cache.load_instrument(AUDUSD_SIM.id)

        # Assert
        assert result == AUDUSD_SIM

    def test_load_account(self):
        # Arrange
        account = TestExecStubs.cash_account()

        self.cache.add_account(account)

        # Act
        result = self.cache.load_account(account.id)

        # Assert
        assert result == account

    def test_account_for_venue(self):
        # Arrange, Act
        result = self.cache.account_for_venue(Venue("SIM"))

        # Assert
        assert result is None

    def test_accounts_when_no_accounts_returns_empty_list(self):
        # Arrange, Act
        result = self.cache.accounts()

        # Assert
        assert result == []

    def test_get_strategy_ids_with_no_ids_returns_empty_set(self):
        # Arrange, Act
        result = self.cache.strategy_ids()

        # Assert
        assert result == set()

    def test_get_order_ids_with_no_ids_returns_empty_set(self):
        # Arrange, Act
        result = self.cache.client_order_ids()

        # Assert
        assert result == set()

    def test_get_strategy_ids_with_id_returns_correct_set(self):
        # Arrange
        self.cache.update_strategy(self.strategy)

        # Act
        result = self.cache.strategy_ids()

        # Assert
        assert result == {self.strategy.id}

    def test_position_for_order_when_no_position_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.position_for_order(ClientOrderId("O-123456")) is None

    def test_position_exists_when_no_position_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.position_exists(PositionId("P-123456"))

    def test_order_exists_when_no_order_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.order_exists(ClientOrderId("O-123456"))

    def test_position_when_no_position_returns_none(self):
        # Arrange
        position_id = PositionId("P-123456")

        # Act
        result = self.cache.position(position_id)

        # Assert
        assert result is None

    def test_order_when_no_order_returns_none(self):
        # Arrange
        order_id = ClientOrderId("O-201908080101-000-001")

        # Act
        result = self.cache.order(order_id)

        # Assert
        assert result is None

    def test_strategy_id_for_position_when_no_strategy_registered_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.strategy_id_for_position(PositionId("P-123456")) is None

    def test_add_market_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")

        # Act
        self.cache.add_order(order, position_id)

        # Assert
        assert order.client_order_id in self.cache.client_order_ids()
        assert order.client_order_id in self.cache.client_order_ids(
            instrument_id=order.instrument_id,
        )
        assert order.client_order_id in self.cache.client_order_ids(strategy_id=self.strategy.id)
        assert order.client_order_id not in self.cache.client_order_ids(
            strategy_id=StrategyId("S-ZX1"),
        )
        assert order.client_order_id in self.cache.client_order_ids(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order in self.cache.orders()
        assert order in self.cache.orders(side=OrderSide.BUY)
        assert order not in self.cache.orders(side=OrderSide.SELL)
        assert order not in self.cache.orders_inflight()
        assert order not in self.cache.orders_emulated()
        assert not self.cache.is_order_inflight(order.client_order_id)
        assert not self.cache.is_order_emulated(order.client_order_id)
        assert self.cache.venue_order_id(order.client_order_id) is None

    def test_add_emulated_limit_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            emulation_trigger=TriggerType.BID_ASK,
        )

        position_id = PositionId("P-1")

        # Act
        self.cache.add_order(order, position_id)

        # Assert
        assert order.client_order_id in self.cache.client_order_ids_emulated()
        assert order in self.cache.orders_emulated()
        assert self.cache.is_order_emulated(order.client_order_id)
        assert self.cache.orders_emulated_count() == 1

    def test_load_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        # Act
        result = self.cache.load_order(order.client_order_id)

        # Assert
        assert result == order

    def test_add_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act
        self.cache.add_position(position, OMSType.HEDGING)

        # Assert
        assert self.cache.position_exists(position.id)
        assert position.id in self.cache.position_ids()
        assert position in self.cache.positions()
        assert position in self.cache.positions_open()
        assert position in self.cache.positions_open(instrument_id=position.instrument_id)
        assert position in self.cache.positions_open(strategy_id=self.strategy.id)
        assert position in self.cache.positions_open(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert position not in self.cache.positions_closed()
        assert position not in self.cache.positions_closed(instrument_id=position.instrument_id)
        assert position not in self.cache.positions_closed(strategy_id=self.strategy.id)
        assert position not in self.cache.positions_closed(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert self.cache.position_for_order(order.client_order_id) == position
        assert self.cache.orders_for_position(position.id) == [order]

    def test_snapshot_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act
        self.cache.snapshot_position(position)
        self.cache.snapshot_position(position)
        snapshots = self.cache.position_snapshots(position.id)

        # Assert
        assert len(snapshots) == 2
        assert snapshots[0].id.value.startswith(position.id.value)
        snapshot_dict = snapshots[0].to_dict()
        del snapshot_dict["position_id"]
        position_dict = position.to_dict()
        del position_dict["position_id"]
        assert snapshot_dict == position_dict

    def test_load_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)
        self.cache.add_position(position, OMSType.HEDGING)

        # Act
        result = self.cache.load_position(position.id)

        # Assert
        assert result == position

    def test_add_submit_order_command(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.cache.add_submit_order_command(command)

        # Assert
        assert self.cache.load_submit_order_command(order.client_order_id) is not None

    def test_load_submit_order_command(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.cache.add_submit_order_command(command)

        # Act
        result = self.cache.load_submit_order_command(order.client_order_id)

        # Assert
        assert command == result

    def test_add_and_load_submit_order_list_command(self):
        order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=self.clock,
        )

        bracket = order_factory.bracket_market_entry(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00100"),
            emulation_trigger=TriggerType.BID_ASK,
        )

        exec_algorithm_specs = [
            ExecAlgorithmSpecification(
                client_order_id=bracket.first.client_order_id,
                exec_algorithm_id=ExecAlgorithmId("VWAP"),
                params={"max_percentage": 100.0, "start": 0, "end": 1},
            ),
        ]

        command = SubmitOrderList(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            order_list=bracket,
            position_id=PositionId("P-001"),
            exec_algorithm_specs=exec_algorithm_specs,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.cache.add_submit_order_list_command(command)

        # Act
        result = self.cache.load_submit_order_list_command(bracket.id)

        # Assert
        assert command.has_emulated_order
        assert command == result

    def test_update_order_for_submitted_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        order.apply(TestEventStubs.order_submitted(order))

        # Act
        self.cache.update_order(order)

        # Assert
        assert self.cache.order_exists(order.client_order_id)
        assert order.client_order_id in self.cache.client_order_ids()
        assert order in self.cache.orders()
        assert order in self.cache.orders_inflight()
        assert order in self.cache.orders_inflight(instrument_id=order.instrument_id)
        assert order in self.cache.orders_inflight(strategy_id=self.strategy.id)
        assert order in self.cache.orders_inflight(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order not in self.cache.orders_open()
        assert order not in self.cache.orders_open(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_open(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_open(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order not in self.cache.orders_closed()
        assert order not in self.cache.orders_closed(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_closed(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_closed(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )

        assert self.cache.orders_open_count() == 0
        assert self.cache.orders_closed_count() == 0
        assert self.cache.orders_emulated_count() == 0
        assert self.cache.orders_emulated_count(side=OrderSide.BUY) == 0
        assert self.cache.orders_emulated_count(side=OrderSide.SELL) == 0
        assert self.cache.orders_inflight_count() == 1
        assert self.cache.orders_inflight_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_inflight_count(side=OrderSide.SELL) == 0
        assert self.cache.orders_total_count() == 1
        assert self.cache.orders_total_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_total_count(side=OrderSide.SELL) == 0

    def test_update_order_for_accepted_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        order.apply(TestEventStubs.order_submitted(order))
        self.cache.update_order(order)

        order.apply(TestEventStubs.order_accepted(order))

        # Act
        self.cache.update_order(order)

        # Assert
        assert self.cache.order_exists(order.client_order_id)
        assert order.client_order_id in self.cache.client_order_ids()
        assert order in self.cache.orders()
        assert order in self.cache.orders_open()
        assert order in self.cache.orders_open(instrument_id=order.instrument_id)
        assert order in self.cache.orders_open(strategy_id=self.strategy.id)
        assert order in self.cache.orders_open(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order not in self.cache.orders_closed()
        assert order not in self.cache.orders_closed(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_closed(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_closed(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order not in self.cache.orders_inflight()
        assert order not in self.cache.orders_inflight()
        assert order not in self.cache.orders_inflight(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_inflight(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_inflight(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )

        assert self.cache.orders_open_count() == 1
        assert self.cache.orders_open_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_open_count(side=OrderSide.SELL) == 0
        assert self.cache.orders_closed_count() == 0
        assert self.cache.orders_inflight_count() == 0
        assert self.cache.orders_total_count() == 1
        assert self.cache.orders_total_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_total_count(side=OrderSide.SELL) == 0

    def test_update_order_for_closed_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)
        order.apply(TestEventStubs.order_submitted(order))
        self.cache.update_order(order)

        order.apply(TestEventStubs.order_accepted(order))
        self.cache.update_order(order)

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            last_px=Price.from_str("1.00001"),
        )

        order.apply(fill)

        # Act
        self.cache.update_order(order)

        # Assert
        assert self.cache.order_exists(order.client_order_id)
        assert order.client_order_id in self.cache.client_order_ids()
        assert order in self.cache.orders()
        assert order not in self.cache.orders_open()
        assert order not in self.cache.orders_open(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_open(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_open(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order in self.cache.orders_closed()
        assert order in self.cache.orders_closed(instrument_id=order.instrument_id)
        assert order in self.cache.orders_closed(strategy_id=self.strategy.id)
        assert order in self.cache.orders_closed(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order not in self.cache.orders_inflight()
        assert order not in self.cache.orders_inflight(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_inflight(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_inflight(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )

        assert self.cache.venue_order_id(order.client_order_id) == order.venue_order_id
        assert self.cache.orders_open_count() == 0
        assert self.cache.orders_closed_count() == 1
        assert self.cache.orders_closed_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_closed_count(side=OrderSide.SELL) == 0
        assert self.cache.orders_emulated_count() == 0
        assert self.cache.orders_inflight_count() == 0
        assert self.cache.orders_total_count() == 1
        assert self.cache.orders_total_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_total_count(side=OrderSide.SELL) == 0

    def test_update_position_for_open_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)

        # Act
        self.cache.add_position(position, OMSType.HEDGING)

        # Assert
        assert self.cache.position_exists(position.id)
        assert position.id in self.cache.position_ids()
        assert position in self.cache.positions()
        assert position in self.cache.positions_open()
        assert position in self.cache.positions_open(instrument_id=position.instrument_id)
        assert position in self.cache.positions_open(strategy_id=self.strategy.id)
        assert position in self.cache.positions_open(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert position not in self.cache.positions_closed()
        assert position not in self.cache.positions_closed(instrument_id=position.instrument_id)
        assert position not in self.cache.positions_closed(strategy_id=self.strategy.id)
        assert position not in self.cache.positions_closed(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert self.cache.position(position_id) == position
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0
        assert self.cache.positions_total_count() == 1

    def test_update_position_for_closed_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position, OMSType.HEDGING)

        order2 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )
        self.cache.add_order(order2, position_id)

        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2))
        self.cache.update_order(order2)
        order2_filled = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00001"),
        )

        position.apply(order2_filled)

        # Act
        self.cache.update_position(position)

        # Assert
        assert self.cache.position_exists(position.id)
        assert position.id in self.cache.position_ids()
        assert position in self.cache.positions()
        assert position in self.cache.positions_closed()
        assert position in self.cache.positions_closed(instrument_id=position.instrument_id)
        assert position in self.cache.positions_closed(strategy_id=self.strategy.id)
        assert position in self.cache.positions_closed(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert position not in self.cache.positions_open()
        assert position not in self.cache.positions_open(instrument_id=position.instrument_id)
        assert position not in self.cache.positions_open(strategy_id=self.strategy.id)
        assert position not in self.cache.positions_open(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert self.cache.position(position_id) == position
        assert self.cache.positions_open_count() == 0
        assert self.cache.positions_closed_count() == 1
        assert self.cache.positions_total_count() == 1
        assert self.cache.position_for_order(order1.client_order_id) == position
        assert self.cache.position_for_order(order2.client_order_id) == position
        assert order1 in self.cache.orders_for_position(position.id)
        assert order2 in self.cache.orders_for_position(position.id)

    def test_positions_queries_with_multiple_open_returns_expected_positions(self):
        # Arrange
        # -- Position 1 --------------------------------------------------------
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position1, OMSType.HEDGING)

        # -- Position 2 --------------------------------------------------------

        order2 = self.strategy.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2))
        self.cache.update_order(order2)
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=GBPUSD_SIM,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("1.00001"),
        )

        position2 = Position(instrument=GBPUSD_SIM, fill=fill2)
        self.cache.add_position(position2, OMSType.HEDGING)

        # Assert
        assert position1.is_open
        assert position2.is_open
        assert position1 in self.cache.positions()
        assert position2 in self.cache.positions()
        assert self.cache.positions(
            venue=AUDUSD_SIM.venue,
            instrument_id=AUDUSD_SIM.id,
        ) == [position1]
        assert self.cache.positions(
            venue=GBPUSD_SIM.venue,
            instrument_id=GBPUSD_SIM.id,
        ) == [position2]
        assert self.cache.positions(instrument_id=GBPUSD_SIM.id, side=PositionSide.LONG) == [
            position2,
        ]
        assert self.cache.positions(instrument_id=AUDUSD_SIM.id, side=PositionSide.LONG) == [
            position1,
        ]
        assert self.cache.positions(instrument_id=GBPUSD_SIM.id, side=PositionSide.LONG) == [
            position2,
        ]
        assert self.cache.positions_open(instrument_id=AUDUSD_SIM.id, side=PositionSide.LONG) == [
            position1,
        ]
        assert self.cache.positions_open(instrument_id=GBPUSD_SIM.id, side=PositionSide.LONG) == [
            position2,
        ]
        assert position1 in self.cache.positions_open()
        assert position2 in self.cache.positions_open()
        assert position1 not in self.cache.positions_closed()
        assert position2 not in self.cache.positions_closed()

    def test_positions_queries_with_one_closed_returns_expected_positions(self):
        # Arrange
        # -- Position 1 --------------------------------------------------------
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position1, OMSType.HEDGING)

        # -- Position 2 --------------------------------------------------------

        order2 = self.strategy.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2))
        self.cache.update_order(order2)
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=GBPUSD_SIM,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("1.00001"),
        )

        position2 = Position(instrument=GBPUSD_SIM, fill=fill2)
        self.cache.add_position(position2, OMSType.HEDGING)

        order3 = self.strategy.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        order3.apply(TestEventStubs.order_submitted(order3))
        self.cache.update_order(order3)

        order3.apply(TestEventStubs.order_accepted(order3))
        self.cache.update_order(order3)
        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=GBPUSD_SIM,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("1.00001"),
        )

        position2.apply(fill3)
        self.cache.update_position(position2)

        # Assert
        assert position1.is_open
        assert position2.is_closed
        assert position1 in self.cache.positions()
        assert position1 in self.cache.positions(instrument_id=AUDUSD_SIM.id)
        assert position2 in self.cache.positions()
        assert position2 in self.cache.positions(instrument_id=GBPUSD_SIM.id)
        assert self.cache.positions_open(venue=BTCUSD_BINANCE.venue, side=PositionSide.LONG) == []
        assert self.cache.positions_open(venue=AUDUSD_SIM.venue, side=PositionSide.LONG) == [
            position1,
        ]
        assert (
            self.cache.positions_open(instrument_id=BTCUSD_BINANCE.id, side=PositionSide.LONG) == []
        )
        assert self.cache.positions_open(instrument_id=AUDUSD_SIM.id, side=PositionSide.LONG) == [
            position1,
        ]
        assert self.cache.positions_open(instrument_id=GBPUSD_SIM.id, side=PositionSide.LONG) == []
        assert self.cache.positions_closed(instrument_id=AUDUSD_SIM.id) == []
        assert self.cache.positions_closed(venue=GBPUSD_SIM.venue) == [position2]
        assert self.cache.positions_closed(instrument_id=GBPUSD_SIM.id) == [position2]

    def test_update_account(self):
        # Arrange
        account = TestExecStubs.cash_account()

        self.cache.add_account(account)

        # Act
        self.cache.update_account(account)

        # Assert
        assert True  # No exceptions raised

    def test_delete_strategy(self):
        # Arrange
        self.cache.update_strategy(self.strategy)

        # Act
        self.cache.delete_strategy(self.strategy)

        # Assert
        assert self.strategy.id not in self.cache.strategy_ids()

    def test_check_residuals(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1, OMSType.HEDGING)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)

        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2))
        self.cache.update_order(order2)

        # Act
        self.cache.check_residuals()

        # Assert
        assert True  # No exception raised

    def test_reset(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )
        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1, OMSType.HEDGING)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)

        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2))
        self.cache.update_order(order2)

        self.cache.update_order(order2)

        # Act
        self.cache.reset()

        # Assert
        assert len(self.cache.strategy_ids()) == 0
        assert self.cache.orders_total_count() == 0
        assert self.cache.positions_total_count() == 0

    def test_flush_db(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1, OMSType.HEDGING)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)
        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2))
        self.cache.update_order(order2)

        # Act
        self.cache.reset()
        self.cache.flush_db()

        # Assert
        assert True  # No exception raised


class TestExecutionCacheIntegrityCheck:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)

        # Setup venue
        self.engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=[],
        )

        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        # Setup data
        wrangler = QuoteTickDataWrangler(self.usdjpy)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv"),
            ask_data=provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv"),
        )
        self.engine.add_instrument(self.usdjpy)
        self.engine.add_data(ticks)

    def test_exec_cache_check_integrity_when_cache_cleared_fails(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=str(self.usdjpy.id),
            bar_type=str(TestDataStubs.bartype_usdjpy_1min_bid()),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Generate a lot of data
        self.engine.run()

        # Remove data
        self.engine.cache.clear_cache()

        # Act, Assert
        assert not self.engine.cache.check_integrity()

    def test_exec_cache_check_integrity_when_index_cleared_fails(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=str(self.usdjpy.id),
            bar_type="USD/JPY.SIM-15-MINUTE-BID-INTERNAL",
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Generate a lot of data
        self.engine.run()

        # Clear index
        self.engine.cache.clear_index()

        # Act, Assert
        assert not self.engine.cache.check_integrity()
