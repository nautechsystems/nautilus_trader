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

import pandas as pd
import pytest

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.mocks.exec_clients import MockExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestOrderEmulatorWithOrderLists:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.DEBUG,
            bypass=True,
        )

        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = AccountId("BINANCE-001")

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

        update = TestEventStubs.margin_account_state(account_id=self.account_id)
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

    def test_submit_stop_order_bulk_then_emulates(self):
        # Arrange
        stop1 = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            trigger_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        stop2 = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            trigger_price=ETHUSDT_PERP_BINANCE.make_price(5010.00),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        stop3 = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            trigger_price=ETHUSDT_PERP_BINANCE.make_price(5020.00),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        order_list = OrderList(
            order_list_id=OrderListId("1"),
            orders=[stop1, stop2, stop3],
        )

        # Act
        self.strategy.submit_order_list(
            order_list=order_list,
            position_id=PositionId("P-001"),
        )

        # Assert
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert stop1 in self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert stop2 in self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert stop3 in self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()

    def test_submit_bracket_order_with_limit_entry_then_emulates_sl_tp(self):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            entry_order_type=OrderType.LIMIT,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        # Act
        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Assert
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders() == [
            bracket.first,
        ]

    def test_submit_bracket_order_with_stop_limit_entry_then_emulates_sl_tp(self):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        # Act
        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Assert
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders() == [
            bracket.first,
        ]

    def test_submit_bracket_order_with_market_entry_immediately_submits_then_emulates_sl_tp(self):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        # Act
        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Assert
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id) is None
        assert self.exec_engine.command_count == 1

    def test_submit_bracket_when_entry_filled_then_emulates_sl_and_tp(self):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.first,
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.first,
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
            ),
        )

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert bracket.orders[0] not in matching_core
        assert bracket.orders[1] in matching_core
        assert bracket.orders[2] in matching_core
        assert self.exec_engine.command_count == 1

    def test_modify_emulated_sl_quantity_also_updates_tp(self):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.first,
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.first,
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
            ),
        )

        self.strategy.modify_order(
            bracket.orders[1],
            quantity=ETHUSDT_PERP_BINANCE.make_qty(5),
        )

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert bracket.orders[0] not in matching_core
        assert bracket.orders[1] in matching_core
        assert bracket.orders[2] in matching_core
        assert self.exec_engine.command_count == 1
        assert bracket.orders[1].quantity == 5
        assert bracket.orders[2].quantity == 5

    def test_modify_emulated_tp_price(self):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.first,
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.first,
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
            ),
        )

        self.strategy.modify_order(
            bracket.orders[2],
            price=ETHUSDT_PERP_BINANCE.make_price(5050),
        )

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert bracket.orders[0] not in matching_core
        assert bracket.orders[1] in matching_core
        assert bracket.orders[2] in matching_core
        assert self.exec_engine.command_count == 1
        assert bracket.orders[2].price == 5050

    def test_submit_bracket_when_stop_limit_entry_filled_then_emulates_sl_and_tp(self):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.first,
                account_id=self.account_id,
            ),
        )

        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid=Price.from_str("4990.0"),
            ask=Price.from_str("4990.0"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert bracket.orders[0] not in matching_core
        assert bracket.orders[1] not in matching_core
        assert bracket.orders[2] not in matching_core
        assert self.exec_engine.command_count == 1

    @pytest.mark.parametrize(
        "contingency_type",
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_rejected_oto_entry_cancels_contingencies(
        self,
        contingency_type,
    ):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=contingency_type,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.first,
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_rejected(
                bracket.first,
                account_id=self.account_id,
            ),
        )

        # Assert
        assert self.exec_engine.command_count == 1
        assert bracket.orders[0].status == OrderStatus.REJECTED
        assert bracket.orders[1].status == OrderStatus.CANCELED
        assert bracket.orders[2].status == OrderStatus.CANCELED

    @pytest.mark.parametrize(
        "contingency_type",
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_cancel_oto_entry_cancels_contingencies(
        self,
        contingency_type,
    ):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            entry_order_type=OrderType.LIMIT,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=contingency_type,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.strategy.cancel_order(bracket.first)

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 0
        assert bracket.orders[0].status == OrderStatus.CANCELED
        assert bracket.orders[1].status == OrderStatus.CANCELED
        assert bracket.orders[2].status == OrderStatus.CANCELED
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert not matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    @pytest.mark.parametrize(
        "contingency_type",
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_expired_oto_entry_then_cancels_contingencies(
        self,
        contingency_type,
    ):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            time_in_force=TimeInForce.GTD,
            expire_time=pd.Timestamp("2022-02-02", tz="UTC"),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            entry_order_type=OrderType.LIMIT,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=contingency_type,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.exec_engine.process(TestEventStubs.order_expired(bracket.first))

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 0
        assert entry_order.status == OrderStatus.EXPIRED
        assert sl_order.status == OrderStatus.CANCELED
        assert tp_order.status == OrderStatus.CANCELED
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert not matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    @pytest.mark.parametrize(
        "contingency_type",
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_update_oto_entry_updates_quantity_of_contingencies(
        self,
        contingency_type,
    ):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            entry_order_type=OrderType.LIMIT,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=contingency_type,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        new_quantity = Quantity.from_int(5)

        # Act
        self.exec_engine.process(
            TestEventStubs.order_updated(
                bracket.first,
                quantity=new_quantity,
            ),
        )

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert bracket.orders[0] in matching_core
        assert self.exec_engine.command_count == 0
        assert bracket.orders[0].quantity == new_quantity
        assert bracket.orders[1].quantity == new_quantity
        assert bracket.orders[2].quantity == new_quantity

    @pytest.mark.parametrize(
        "contingency_type",
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_triggered_sl_submits_market_order(
        self,
        contingency_type,
    ):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=contingency_type,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.first,
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.first,
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
            ),
        )

        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid=Price.from_str("5100.0"),
            ask=Price.from_str("5100.0"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 2
        assert len(self.emulator.get_submit_order_commands()) == 1
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert entry_order.status == OrderStatus.FILLED
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    @pytest.mark.parametrize(
        "contingency_type",
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_triggered_stop_limit_tp_submits_limit_order(
        self,
        contingency_type,
    ):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=contingency_type,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.first,
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.first,
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
            ),
        )

        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid=Price.from_str("5100.0"),
            ask=Price.from_str("5100.0"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 1
        assert len(self.emulator.get_submit_order_commands()) == 2
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert entry_order.status == OrderStatus.FILLED
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    @pytest.mark.parametrize(
        "contingency_type",
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_triggered_then_filled_tp_cancels_sl(
        self,
        contingency_type,
    ):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=contingency_type,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.first,
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.first,
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
            ),
        )

        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid=Price.from_str("5100.0"),
            ask=Price.from_str("5100.0"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(tick)

        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.orders[2],
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.orders[2],
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
            ),
        )

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 2
        assert len(self.emulator.get_submit_order_commands()) == 0
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert self.cache.orders_open_count() == 0
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.CANCELED
        assert tp_order.status == OrderStatus.FILLED
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert not matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    def test_triggered_then_partially_filled_oco_sl_cancels_tp(self):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OCO,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.first,
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.first,
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
            ),
        )

        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid=Price.from_str("4900.0"),
            ask=Price.from_str("4900.0"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(tick)

        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.orders[1],
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.orders[1],
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
                last_qty=Quantity.from_int(5),
            ),
        )

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 2
        assert len(self.emulator.get_submit_order_commands()) == 0
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert self.cache.orders_open_count() == 1
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.PARTIALLY_FILLED
        assert tp_order.status == OrderStatus.CANCELED
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert not matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    def test_triggered_then_partially_filled_ouo_sl_updated_tp(self):
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.00),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.first,
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.first,
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
            ),
        )

        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid=Price.from_str("4900.0"),
            ask=Price.from_str("4900.0"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.data_engine.process(tick)

        self.exec_engine.process(
            TestEventStubs.order_submitted(
                bracket.orders[1],
                account_id=self.account_id,
            ),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(
                bracket.orders[1],
                instrument=ETHUSDT_PERP_BINANCE,
                account_id=self.account_id,
                last_qty=Quantity.from_int(5),
            ),
        )

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 2
        assert len(self.emulator.get_submit_order_commands()) == 1
        assert len(self.emulator.get_submit_order_list_commands()) == 1
        assert self.cache.orders_emulated_count() == 1
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.PARTIALLY_FILLED
        assert tp_order.status == OrderStatus.INITIALIZED
        assert sl_order.quantity == Quantity.from_int(10)
        assert sl_order.leaves_qty == Quantity.from_int(5)
        assert tp_order.quantity == Quantity.from_int(5)
        assert tp_order.leaves_qty == Quantity.from_int(5)
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert not matching_core.order_exists(sl_order.client_order_id)
        assert matching_core.order_exists(tp_order.client_order_id)
