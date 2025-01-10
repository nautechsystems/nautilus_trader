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

import pandas as pd
import pytest

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import OrderEmulatorConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestOrderEmulatorWithOrderLists:
    def setup(self) -> None:
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = AccountId("BINANCE-001")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache_db = MockCacheDatabase()

        self.cache = Cache(
            database=self.cache_db,
        )
        self.cache.add_instrument(ETHUSDT_PERP_BINANCE)

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=DataEngineConfig(debug=True),
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=RiskEngineConfig(debug=True),
        )

        self.emulator = OrderEmulator(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=OrderEmulatorConfig(debug=True),
        )

        self.venue = Venue("BINANCE")
        self.exchange = SimulatedExchange(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-asset wallet
            starting_balances=[Money(200, ETH), Money(1_000_000, USDT)],
            default_leverage=Decimal(10),
            leverages={},
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            support_contingent_orders=False,
        )
        self.exchange.add_instrument(ETHUSDT_PERP_BINANCE)

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        update = TestEventStubs.margin_account_state(account_id=self.account_id)
        self.portfolio.update_account(update)

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

    def test_submit_stop_order_bulk_then_emulates(self) -> None:
        # Arrange
        stop1 = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            trigger_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            emulation_trigger=TriggerType.LAST_PRICE,
        )

        stop2 = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            trigger_price=ETHUSDT_PERP_BINANCE.make_price(5010.0),
            emulation_trigger=TriggerType.LAST_PRICE,
        )

        stop3 = self.strategy.order_factory.stop_market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            trigger_price=ETHUSDT_PERP_BINANCE.make_price(5020.0),
            emulation_trigger=TriggerType.LAST_PRICE,
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
        assert len(self.emulator.get_submit_order_commands()) == 3
        assert stop1 in self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert stop2 in self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert stop3 in self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()

    def test_submit_bracket_order_with_limit_entry_then_emulates_sl_tp(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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
        assert len(self.emulator.get_submit_order_commands()) == 1
        assert self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders() == [
            bracket.first,
        ]

    def test_submit_bracket_order_with_stop_limit_entry_then_emulates_sl_tp(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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
        assert len(self.emulator.get_submit_order_commands()) == 1
        assert self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders() == [
            bracket.first,
        ]

    def test_submit_bracket_order_with_market_entry_immediately_submits_then_emulates_sl_tp(
        self,
    ) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        # Act
        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Assert
        assert self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id) is None
        assert self.exec_engine.command_count == 1

    def test_submit_bracket_when_entry_filled_then_emulates_sl_and_tp(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        position_id = PositionId("P-001")
        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=position_id,
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
        assert len(self.emulator.get_submit_order_commands()) == 3
        assert bracket.orders[0] not in matching_core
        assert bracket.orders[1] in matching_core
        assert bracket.orders[2] in matching_core
        assert bracket.orders[0].position_id == position_id
        assert bracket.orders[1].position_id == position_id
        assert bracket.orders[2].position_id == position_id
        assert self.exec_engine.command_count == 1

    def test_modify_emulated_sl_quantity_also_updates_tp(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        position_id = PositionId("P-001")
        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=position_id,
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
        assert len(self.emulator.get_submit_order_commands()) == 3
        assert bracket.orders[0] not in matching_core
        assert bracket.orders[1] in matching_core
        assert bracket.orders[2] in matching_core
        assert self.exec_engine.command_count == 1
        assert bracket.orders[1].quantity == 5
        assert bracket.orders[2].quantity == 5
        assert bracket.orders[0].position_id == position_id
        assert bracket.orders[1].position_id == position_id
        assert bracket.orders[2].position_id == position_id

    def test_modify_emulated_tp_price(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        position_id = PositionId("P-001")
        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=position_id,
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
            price=ETHUSDT_PERP_BINANCE.make_price(5050.0),
        )

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert len(self.emulator.get_submit_order_commands()) == 3
        assert bracket.orders[0] not in matching_core
        assert bracket.orders[1] in matching_core
        assert bracket.orders[2] in matching_core
        assert self.exec_engine.command_count == 1
        assert bracket.orders[2].price == 5050
        assert bracket.orders[0].position_id == position_id
        assert bracket.orders[1].position_id == position_id
        assert bracket.orders[2].position_id == position_id

    def test_submit_bracket_when_stop_limit_entry_filled_then_emulates_sl_and_tp(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=4990.0,
            ask_price=4990.0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders()
        assert len(self.emulator.get_submit_order_commands()) == 0
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
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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
    def test_cancel_bracket(
        self,
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            entry_order_type=OrderType.LIMIT,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=contingency_type,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        # Act
        self.strategy.cancel_order(bracket.orders[1])

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 0
        assert bracket.orders[0].status == OrderStatus.EMULATED
        assert bracket.orders[1].status == OrderStatus.CANCELED
        assert bracket.orders[2].status == OrderStatus.CANCELED
        assert matching_core.order_exists(entry_order.client_order_id)
        assert not matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    @pytest.mark.parametrize(
        "contingency_type",
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_cancel_oto_entry_cancels_contingencies(
        self,
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            time_in_force=TimeInForce.GTD,
            expire_time=pd.Timestamp("2022-02-02", tz="UTC"),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5100.0,
            ask_price=5100.0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 2
        assert len(self.emulator.get_submit_order_commands()) == 2
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
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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
            TestEventStubs.order_released(
                bracket.first,
            ),
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

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5100.0,
            ask_price=5100.0,
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
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5100.0,
            ask_price=5100.0,
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
                venue_order_id=VenueOrderId("2"),
            ),
        )

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 2
        assert len(self.emulator.get_submit_order_commands()) == 1
        assert self.cache.orders_open_count() == 0
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.CANCELED
        assert tp_order.status == OrderStatus.FILLED
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert not matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    def test_triggered_then_partially_filled_oco_sl_cancels_tp(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=4900.0,
            ask_price=4900.0,
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
                venue_order_id=VenueOrderId("2"),
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
        assert self.cache.orders_open_count() == 1
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.PARTIALLY_FILLED
        assert tp_order.status == OrderStatus.CANCELED
        assert not entry_order.is_active_local
        assert not sl_order.is_active_local
        assert not tp_order.is_active_local
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert not matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    def test_triggered_then_partially_filled_ouo_sl_updated_tp(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
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

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=4900.0,
            ask_price=4900.0,
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
                venue_order_id=VenueOrderId("2"),
                last_qty=Quantity.from_int(5),
            ),
        )

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 2
        assert len(self.emulator.get_submit_order_commands()) == 2
        assert self.cache.orders_emulated_count() == 1
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.PARTIALLY_FILLED
        assert tp_order.status == OrderStatus.EMULATED
        assert sl_order.quantity == Quantity.from_int(10)
        assert sl_order.leaves_qty == Quantity.from_int(5)
        assert tp_order.quantity == Quantity.from_int(5)
        assert tp_order.leaves_qty == Quantity.from_int(5)
        assert not entry_order.is_active_local
        assert not sl_order.is_active_local
        assert tp_order.is_active_local
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert not matching_core.order_exists(sl_order.client_order_id)
        assert matching_core.order_exists(tp_order.client_order_id)

    def test_released_order_with_quote_quantity_sets_contingent_orders_to_base_quantity(
        self,
    ) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5001.0),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            quote_quantity=True,
            emulation_trigger=TriggerType.BID_ASK,
        )

        self.strategy.submit_order_list(order_list=bracket)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5000.0,
            ask_price=5000.0,
        )

        # Act
        self.data_engine.process(tick)

        # Assert
        self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 1
        assert len(self.emulator.get_submit_order_commands()) == 0
        assert self.cache.orders_emulated_count() == 2
        assert not entry_order.is_quote_quantity
        assert not sl_order.is_quote_quantity
        assert not tp_order.is_quote_quantity
        assert not entry_order.is_active_local
        assert sl_order.is_active_local
        assert tp_order.is_active_local
        assert entry_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(0.002)
        assert sl_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(0.002)
        assert tp_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(0.002)
        assert entry_order.leaves_qty == ETHUSDT_PERP_BINANCE.make_qty(0.002)
        assert sl_order.leaves_qty == ETHUSDT_PERP_BINANCE.make_qty(0.002)
        assert tp_order.leaves_qty == ETHUSDT_PERP_BINANCE.make_qty(0.002)

    def test_restart_emulator_with_emulated_parent(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            entry_order_type=OrderType.LIMIT,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        self.emulator.stop()
        self.emulator.reset()

        # Act
        self.emulator.start()

        # Assert
        assert len(self.emulator.get_submit_order_commands()) == 1
        assert self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id).get_orders() == [
            bracket.first,
        ]
        assert bracket.orders[0].status == OrderStatus.EMULATED
        assert bracket.orders[1].status == OrderStatus.INITIALIZED
        assert bracket.orders[2].status == OrderStatus.INITIALIZED

    def test_restart_emulator_with_partially_filled_parent(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            entry_order_type=OrderType.LIMIT,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5000.0,
            ask_price=5000.0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)
        self.exchange.process(0)

        self.emulator.stop()
        self.emulator.reset()

        # Act
        self.emulator.start()

        # Assert
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        assert entry_order.status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.EMULATED
        assert bracket.orders[2].status == OrderStatus.EMULATED

    def test_restart_emulator_then_cancel_bracket(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            entry_order_type=OrderType.LIMIT,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )

        self.emulator.stop()
        self.emulator.reset()
        self.emulator.start()

        # Act
        self.strategy.cancel_order(bracket.orders[1])

        # Assert
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        assert entry_order.status == OrderStatus.EMULATED
        assert bracket.orders[1].status == OrderStatus.CANCELED
        assert bracket.orders[2].status == OrderStatus.CANCELED

    def test_restart_emulator_with_closed_parent_position(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            entry_order_type=OrderType.LIMIT,
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        position_id = PositionId("P-001")
        self.strategy.submit_order_list(
            order_list=bracket,
            position_id=position_id,
        )

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5000.0,
            ask_price=5000.0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)
        self.exchange.process(0)

        self.emulator.stop()
        self.emulator.reset()

        closing_order = self.strategy.order_factory.market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
        )

        self.strategy.submit_order(closing_order, position_id=position_id)
        self.exchange.process(0)

        # Act
        self.emulator.start()

        # Assert
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        assert entry_order.status == OrderStatus.FILLED
        assert closing_order.status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.CANCELED
        assert bracket.orders[2].status == OrderStatus.CANCELED

    def test_managed_contingent_orders_with_canceled_bracket(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_order_type=OrderType.LIMIT_IF_TOUCHED,
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5150.0),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        config = StrategyConfig(
            manage_contingent_orders=True,
            manage_gtd_expiry=True,
        )
        strategy = Strategy(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        strategy.start()

        # Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5000.0,
            ask_price=5000.0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)
        self.exchange.process(0)

        # Submit order
        strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )
        self.exchange.process(0)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5100.0,
            ask_price=5100.0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)
        self.exchange.process(0)

        # Act
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        strategy.cancel_order(tp_order)
        self.exchange.process(0)

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 3
        assert len(self.emulator.get_submit_order_commands()) == 1
        assert self.cache.orders_emulated_count() == 0
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.CANCELED
        assert tp_order.status == OrderStatus.CANCELED
        assert sl_order.quantity == Quantity.from_int(10)
        assert tp_order.quantity == Quantity.from_int(10)
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert not matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    def test_managed_contingent_orders_with_modified_open_order(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_order_type=OrderType.LIMIT_IF_TOUCHED,
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5150.0),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        config = StrategyConfig(
            manage_contingent_orders=True,
            manage_gtd_expiry=True,
        )
        strategy = Strategy(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        strategy.start()

        # Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5000.0,
            ask_price=5000.0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)
        self.exchange.process(0)

        # Submit order
        strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )
        self.exchange.process(0)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5100.0,
            ask_price=5100.0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)
        self.exchange.process(0)

        # Act
        new_quantity = Quantity.from_int(5)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        strategy.modify_order(tp_order, new_quantity)
        self.exchange.process(0)

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 3
        assert len(self.emulator.get_submit_order_commands()) == 2
        assert self.cache.orders_emulated_count() == 1
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.EMULATED
        assert tp_order.status == OrderStatus.ACCEPTED
        assert sl_order.quantity == new_quantity
        assert tp_order.quantity == new_quantity
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)

    def test_managed_contingent_orders_with_modified_emulated_order(self) -> None:
        # Arrange
        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4900.0),
            tp_order_type=OrderType.LIMIT_IF_TOUCHED,
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5150.0),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5100.0),
            emulation_trigger=TriggerType.BID_ASK,
            contingency_type=ContingencyType.OUO,
        )

        config = StrategyConfig(
            manage_contingent_orders=True,
            manage_gtd_expiry=True,
        )
        strategy = Strategy(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        strategy.start()

        # Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5000.0,
            ask_price=5000.0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)
        self.exchange.process(0)

        # Submit order
        strategy.submit_order_list(
            order_list=bracket,
            position_id=PositionId("P-001"),
        )
        self.exchange.process(0)

        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5100.0,
            ask_price=5100.0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)
        self.exchange.process(0)

        # Act
        new_quantity = Quantity.from_int(5)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        strategy.modify_order(sl_order, new_quantity)
        self.exchange.process(0)

        # Assert
        matching_core = self.emulator.get_matching_core(ETHUSDT_PERP_BINANCE.id)
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert self.exec_engine.command_count == 3
        assert len(self.emulator.get_submit_order_commands()) == 2
        assert self.cache.orders_emulated_count() == 1
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.EMULATED
        assert tp_order.status == OrderStatus.ACCEPTED
        assert sl_order.quantity == new_quantity
        assert tp_order.quantity == new_quantity
        assert not matching_core.order_exists(entry_order.client_order_id)
        assert matching_core.order_exists(sl_order.client_order_id)
        assert not matching_core.order_exists(tp_order.client_order_id)
