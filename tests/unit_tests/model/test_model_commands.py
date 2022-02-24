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

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestCommands:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()

        self.trader_id = TestStubs.trader_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_submit_order_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        command = SubmitOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            position_id=PositionId("P-001"),
            order=order,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert SubmitOrder.from_dict(SubmitOrder.to_dict(command)) == command
        assert (
            str(command)
            == "SubmitOrder(instrument_id=AUD/USD.SIM, client_order_id=O-19700101-000000-000-001-1, position_id=P-001, order=BUY 100_000 AUD/USD.SIM MARKET GTC)"  # noqa
        )
        assert (
            repr(command)
            == f"SubmitOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-19700101-000000-000-001-1, position_id=P-001, order=BUY 100_000 AUD/USD.SIM MARKET GTC, command_id={uuid}, ts_init=0)"  # noqa
        )

    def test_submit_bracket_order_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        bracket = self.order_factory.bracket_market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100000),
            stop_loss=Price.from_str("1.00000"),
            take_profit=Price.from_str("1.00100"),
        )

        command = SubmitOrderList(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            order_list=bracket,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert SubmitOrderList.from_dict(SubmitOrderList.to_dict(command)) == command
        assert (
            str(command)
            == "SubmitOrderList(instrument_id=AUD/USD.SIM, order_list=OrderList(id=1, instrument_id=AUD/USD.SIM, orders=[MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, tags=ENTRY), StopMarketOrder(SELL 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000[DEFAULT] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-2, venue_order_id=None, tags=STOP_LOSS), LimitOrder(SELL 100_000 AUD/USD.SIM LIMIT @ 1.00100 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-3, venue_order_id=None, tags=TAKE_PROFIT)]))"  # noqa
        )
        assert (
            repr(command)
            == f"SubmitOrderList(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, order_list=OrderList(id=1, instrument_id=AUD/USD.SIM, orders=[MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, tags=ENTRY), StopMarketOrder(SELL 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000[DEFAULT] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-2, venue_order_id=None, tags=STOP_LOSS), LimitOrder(SELL 100_000 AUD/USD.SIM LIMIT @ 1.00100 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-3, venue_order_id=None, tags=TAKE_PROFIT)]), command_id={uuid}, ts_init=0)"  # noqa
        )

    def test_modify_order_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        command = ModifyOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            price=Price.from_str("1.00000"),
            trigger_price=Price.from_str("1.00010"),
            quantity=Quantity.from_int(100000),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert ModifyOrder.from_dict(ModifyOrder.to_dict(command)) == command
        assert (
            str(command)
            == "ModifyOrder(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, quantity=100_000, price=1.00000, trigger_price=1.00010)"  # noqa
        )
        assert (
            repr(command)
            == f"ModifyOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, quantity=100_000, price=1.00000, trigger_price=1.00010, command_id={uuid}, ts_init=0)"  # noqa
        )

    def test_modify_order_command_with_none_venue_order_id_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        command = ModifyOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=None,
            price=Price.from_str("1.00000"),
            trigger_price=Price.from_str("1.00010"),
            quantity=Quantity.from_int(100000),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert ModifyOrder.from_dict(ModifyOrder.to_dict(command)) == command
        assert (
            str(command)
            == "ModifyOrder(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None, quantity=100_000, price=1.00000, trigger_price=1.00010)"  # noqa
        )
        assert (
            repr(command)
            == f"ModifyOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None, quantity=100_000, price=1.00000, trigger_price=1.00010, command_id={uuid}, ts_init=0)"  # noqa
        )

    def test_cancel_order_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        command = CancelOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert CancelOrder.from_dict(CancelOrder.to_dict(command)) == command
        assert (
            str(command)
            == "CancelOrder(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001)"  # noqa
        )
        assert (
            repr(command)
            == f"CancelOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, command_id={uuid}, ts_init=0)"  # noqa
        )

    def test_cancel_order_command_with_none_venue_order_id_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        command = CancelOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=None,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert CancelOrder.from_dict(CancelOrder.to_dict(command)) == command
        assert (
            str(command)
            == "CancelOrder(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None)"  # noqa
        )
        assert (
            repr(command)
            == f"CancelOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None, command_id={uuid}, ts_init=0)"  # noqa
        )

    def test_cancel_all_orders_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        command = CancelAllOrders(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert CancelAllOrders.from_dict(CancelAllOrders.to_dict(command)) == command
        assert str(command) == "CancelAllOrders(instrument_id=AUD/USD.SIM)"  # noqa
        assert (
            repr(command)
            == f"CancelAllOrders(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, command_id={uuid}, ts_init=0)"  # noqa
        )
