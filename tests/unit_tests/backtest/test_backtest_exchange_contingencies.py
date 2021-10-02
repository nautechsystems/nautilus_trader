# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from tests.test_kit.mocks import MockStrategy
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
ETHUSD_FTX = TestInstrumentProvider.ethusd_ftx()


class TestSimulatedExchangeContingencyAdvancedOrders:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(clock=self.clock)

        self.trader_id = TestStubs.trader_id()
        self.account_id = TestStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            clock=self.clock,
            cache=self.cache,
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

        self.exchange = SimulatedExchange(
            venue=SIM,
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(50),
            leverages={},
            is_frozen_account=False,
            instruments=[AUDUSD_SIM, USDJPY_SIM],
            modules=[],
            fill_model=FillModel(),
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=self.account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.cache.add_instrument(AUDUSD_SIM)
        self.cache.add_instrument(USDJPY_SIM)
        self.cache.add_instrument(XBTUSD_BITMEX)

        # Create mock strategy
        self.strategy = MockStrategy(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Start components
        self.exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    @pytest.mark.skip(reason="WIP")
    def test_submit_bracket_market_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        bracket = self.strategy.order_factory.bracket_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            stop_loss=Price.from_str("89.950"),
            take_profit=Price.from_str("90.050"),
        )

        # Act
        self.strategy.submit_order_list(bracket)

        # Assert
        # TODO: Implement
        # assert bracket.orders[1] == self.cache.order(ClientOrderId("O-19700101-000000-000-000-2"))
        # assert bracket.orders[2] == self.cache.order(ClientOrderId("O-19700101-000000-000-000-3"))
        # assert bracket.orders[0].status == OrderStatus.FILLED
        # assert bracket.orders[1].status == OrderStatus.ACCEPTED
        # assert bracket.orders[2].status == OrderStatus.ACCEPTED

    @pytest.mark.skip(reason="WIP")
    def test_submit_stop_market_order_with_bracket(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry_order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.020"),
        )

        bracket = self.strategy.order_factory.bracket_market(
            entry_order=entry_order,
            stop_loss=Price.from_str("90.000"),
            take_profit=Price.from_str("90.040"),
        )

        # Act
        self.strategy.submit_order_list(bracket)

        # Assert
        stop_loss_order = self.cache.order(ClientOrderId("O-19700101-000000-000-000-2"))
        take_profit_order = self.cache.order(ClientOrderId("O-19700101-000000-000-000-3"))

        assert entry_order.status == OrderStatus.ACCEPTED
        assert stop_loss_order.status == OrderStatus.SUBMITTED
        assert take_profit_order.status == OrderStatus.SUBMITTED
        assert len(self.exchange.get_working_orders()) == 1
        assert entry_order.client_order_id in self.exchange.get_working_orders()

    @pytest.mark.skip(reason="WIP")
    def test_update_bracket_orders_working_stop_loss(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry_order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        bracket = self.strategy.order_factory.bracket_market(
            entry_order,
            stop_loss=Price.from_str("85.000"),
            take_profit=Price.from_str("91.000"),
        )

        self.strategy.submit_order_list(bracket)

        # Act
        self.strategy.modify_order(
            bracket.orders[1],
            bracket.entry.quantity,
            Price.from_str("85.100"),
        )

        # Assert
        assert bracket.orders[1].status == OrderStatus.ACCEPTED
        assert bracket.orders[1].price == Price.from_str("85.100")

    # def test_submit_bracket_limit_entry_accepts_parent_only(self):
    #     # Arrange: Prepare market
    #     print(self.exchange.instruments)
    #     tick = QuoteTick(
    #         instrument_id=ETHUSD_FTX.id,
    #         bid=Price.from_str("3000.0"),
    #         ask=Price.from_str("3001.0"),
    #         bid_size=Quantity.from_int(100),
    #         ask_size=Quantity.from_int(100),
    #         ts_event=0,
    #         ts_init=0,
    #     )
    #     self.exchange.process_tick(tick)
    #
    #     bracket = self.strategy.order_factory.bracket_limit(
    #         instrument_id=ETHUSD_FTX.id,
    #         order_side=OrderSide.SELL,
    #         quantity=Quantity.from_int(20),
    #         entry=Price.from_str("3002.0"),
    #         stop_loss=Price.from_str("3012.0"),
    #         take_profit=Price.from_str("2990.0"),
    #     )
    #     self.strategy.submit_order_list(bracket)
    #     self.exchange.process(0)
    #
    #     # Assert
    #     assert bracket.list.orders[0] == OrderStatus.ACCEPTED
    #     assert bracket.list.orders[0] == OrderStatus.SUBMITTED
    #     assert bracket.list.orders[0] == OrderStatus.SUBMITTED

    def test_submit_bracket_market_buy_accepts_sl_and_tp(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        bracket = self.strategy.order_factory.bracket_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            stop_loss=Price.from_str("89.900"),
            take_profit=Price.from_str("91.000"),
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.ACCEPTED
        assert bracket.orders[2].status == OrderStatus.ACCEPTED

    def test_submit_bracket_market_sell_accepts_sl_and_tp(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        bracket = self.strategy.order_factory.bracket_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            stop_loss=Price.from_str("91.000"),
            take_profit=Price.from_str("89.900"),
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.ACCEPTED
        assert bracket.orders[2].status == OrderStatus.ACCEPTED

    def test_submit_bracket_limit_buy_has_sl_tp_pending(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        bracket = self.strategy.order_factory.bracket_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            entry=Price.from_str("90.000"),
            stop_loss=Price.from_str("89.900"),
            take_profit=Price.from_str("91.000"),
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)
        #
        # # Assert
        assert bracket.orders[0].status == OrderStatus.ACCEPTED
        assert bracket.orders[1].status == OrderStatus.SUBMITTED
        assert bracket.orders[2].status == OrderStatus.SUBMITTED

    def test_submit_bracket_limit_sell_has_sl_tp_pending(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        bracket = self.strategy.order_factory.bracket_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            entry=Price.from_str("90.010"),
            stop_loss=Price.from_str("91.000"),
            take_profit=Price.from_str("89.900"),
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)
        #
        # # Assert
        assert bracket.orders[0].status == OrderStatus.ACCEPTED
        assert bracket.orders[1].status == OrderStatus.SUBMITTED
        assert bracket.orders[2].status == OrderStatus.SUBMITTED

    def test_submit_bracket_limit_buy_fills_then_triggers_sl_and_tp(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        bracket = self.strategy.order_factory.bracket_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            entry=Price.from_str("90.010"),
            stop_loss=Price.from_str("89.900"),
            take_profit=Price.from_str("91.000"),
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.ACCEPTED
        assert bracket.orders[2].status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 2
        assert bracket.orders[1] in self.exchange.get_working_orders()
        assert bracket.orders[2] in self.exchange.get_working_orders()

    @pytest.mark.skip(reason="WIP")
    def test_process_quote_tick_fills_sell_limit_entry_with_bracket(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("91.100"),
        )

        bracket = self.strategy.order_factory.bracket_market(
            entry_order=entry,
            stop_loss=Price.from_str("91.200"),
            take_profit=Price.from_str("90.000"),
        )

        self.strategy.submit_order_list(bracket)

        # Act
        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("91.101"),
            ask=Price.from_str("91.102"),
            bid_size=Quantity.from_int(100000),
            ask_size=Quantity.from_int(100000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_tick(tick2)

        # Assert
        assert entry.status == OrderStatus.FILLED
        assert bracket.stop_loss.status == OrderStatus.ACCEPTED
        assert bracket.take_profit.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 2  # SL and TP
        assert bracket.stop_loss in self.exchange.get_working_orders().values()
        assert bracket.take_profit in self.exchange.get_working_orders().values()

    @pytest.mark.skip(reason="WIP")
    def test_process_trade_tick_fills_buy_limit_entry_bracket(self):
        # Arrange: Prepare market
        tick1 = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(100000),
            aggressor_side=AggressorSide.SELL,
            match_id="123456789",
            ts_event=0,
            ts_init=0,
        )

        tick2 = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("1.00001"),
            size=Quantity.from_int(100000),
            aggressor_side=AggressorSide.BUY,
            match_id="123456790",
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick1)
        self.data_engine.process(tick2)
        self.exchange.process_tick(tick1)
        self.exchange.process_tick(tick2)

        entry = self.strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("0.99900"),
        )

        bracket = self.strategy.order_factory.bracket_market(
            entry_order=entry,
            stop_loss=Price.from_str("0.99800"),
            take_profit=Price.from_str("1.100"),
        )

        self.strategy.submit_order_list(bracket)

        # Act
        tick3 = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("0.99899"),
            size=Quantity.from_int(100000),
            aggressor_side=AggressorSide.BUY,  # Lowers bid price
            match_id="123456789",
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_tick(tick3)

        # Assert
        assert entry.status == OrderStatus.FILLED
        assert bracket.stop_loss.status == OrderStatus.ACCEPTED
        assert bracket.take_profit.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 2  # SL and TP only
        assert bracket.stop_loss in self.exchange.get_working_orders().values()
        assert bracket.take_profit in self.exchange.get_working_orders().values()

    @pytest.mark.skip(reason="WIP")
    def test_filling_oco_sell_cancels_other_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("91.100"),
        )

        bracket = self.strategy.order_factory.bracket_market(
            entry_order=entry,
            stop_loss=Price.from_str("91.200"),
            take_profit=Price.from_str("90.000"),
        )

        self.strategy.submit_order_list(bracket)

        # Act
        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("91.101"),
            ask=Price.from_str("91.102"),
            bid_size=Quantity.from_int(100000),
            ask_size=Quantity.from_int(100000),
            ts_event=0,
            ts_init=0,
        )

        tick3 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("91.201"),
            ask=Price.from_str("91.203"),
            bid_size=Quantity.from_int(100000),
            ask_size=Quantity.from_int(100000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_tick(tick2)
        self.exchange.process_tick(tick3)

        # Assert
        print(self.exchange.cache.position(PositionId("2-001")))
        assert entry.status == OrderStatus.FILLED
        assert bracket.stop_loss.status == OrderStatus.FILLED
        assert bracket.take_profit.status == OrderStatus.CANCELED
        assert len(self.exchange.get_working_orders()) == 0
        # TODO: WIP - fix handling of OCO orders
        # assert len(self.exchange.cache.positions_open()) == 0
