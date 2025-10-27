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

from datetime import datetime

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateExecutionMassStatus
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestCommands:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

        self.trader_id = TestIdStubs.trader_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_submit_order_command_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        command = SubmitOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            order=order,
            position_id=PositionId("P-001"),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert SubmitOrder.from_dict(SubmitOrder.to_dict(command)) == command
        assert (
            str(command)
            == "SubmitOrder(order=LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None), position_id=P-001)"
        )
        assert (
            repr(command)
            == f"SubmitOrder(client_id=None, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-19700101-000000-000-001-1, order=LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None), position_id=P-001, command_id={uuid}, ts_init=0)"
        )

    def test_submit_order_command_with_exec_algorithm_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            exec_algorithm_params={"max_percentage": 100.0, "start": 0, "end": 1},
        )

        command = SubmitOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            order=order,
            position_id=PositionId("P-001"),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert SubmitOrder.from_dict(SubmitOrder.to_dict(command)) == command
        assert (
            str(command)
            == "SubmitOrder(order=LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, exec_algorithm_id=VWAP, exec_algorithm_params={'max_percentage': 100.0, 'start': 0, 'end': 1}, exec_spawn_id=O-19700101-000000-000-001-1, tags=None), position_id=P-001)"
        )
        assert (
            repr(command)
            == f"SubmitOrder(client_id=None, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-19700101-000000-000-001-1, order=LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, exec_algorithm_id=VWAP, exec_algorithm_params={{'max_percentage': 100.0, 'start': 0, 'end': 1}}, exec_spawn_id=O-19700101-000000-000-001-1, tags=None), position_id=P-001, command_id={uuid}, ts_init=0)"
        )

    def test_submit_bracket_order_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        bracket = self.order_factory.bracket(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00100"),
            entry_tags=["ENTRY"],
            tp_tags=["TAKE_PROFIT"],
            sl_tags=["STOP_LOSS"],
        )

        command = SubmitOrderList(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            order_list=bracket,
            position_id=PositionId("P-001"),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert not command.has_emulated_order
        assert SubmitOrderList.from_dict(SubmitOrderList.to_dict(command)) == command
        assert (
            str(command)
            == "SubmitOrderList(order_list=OrderList(id=OL-19700101-000000-000-001-1, instrument_id=AUD/USD.SIM, strategy_id=S-001, orders=[MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, contingency_type=OTO, linked_order_ids=[O-19700101-000000-000-001-2, O-19700101-000000-000-001-3], tags=['ENTRY']), StopMarketOrder(SELL 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000[DEFAULT] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-2, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-3], parent_order_id=O-19700101-000000-000-001-1, tags=['STOP_LOSS']), LimitOrder(SELL 100_000 AUD/USD.SIM LIMIT @ 1.00100 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-3, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-2], parent_order_id=O-19700101-000000-000-001-1, tags=['TAKE_PROFIT'])]), position_id=P-001)"
        )
        assert (
            repr(command)
            == f"SubmitOrderList(client_id=None, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, order_list=OrderList(id=OL-19700101-000000-000-001-1, instrument_id=AUD/USD.SIM, strategy_id=S-001, orders=[MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, contingency_type=OTO, linked_order_ids=[O-19700101-000000-000-001-2, O-19700101-000000-000-001-3], tags=['ENTRY']), StopMarketOrder(SELL 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000[DEFAULT] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-2, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-3], parent_order_id=O-19700101-000000-000-001-1, tags=['STOP_LOSS']), LimitOrder(SELL 100_000 AUD/USD.SIM LIMIT @ 1.00100 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-3, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-2], parent_order_id=O-19700101-000000-000-001-1, tags=['TAKE_PROFIT'])]), position_id=P-001, command_id={uuid}, ts_init=0)"
        )

    def test_submit_bracket_order_command_with_exec_algorithm_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        bracket = self.order_factory.bracket(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00100"),
            entry_tags=["ENTRY"],
            tp_tags=["TAKE_PROFIT"],
            sl_tags=["STOP_LOSS"],
        )

        command = SubmitOrderList(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            order_list=bracket,
            position_id=PositionId("P-001"),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert SubmitOrderList.from_dict(SubmitOrderList.to_dict(command)) == command
        assert (
            str(command)
            == "SubmitOrderList(order_list=OrderList(id=OL-19700101-000000-000-001-1, instrument_id=AUD/USD.SIM, strategy_id=S-001, orders=[MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, contingency_type=OTO, linked_order_ids=[O-19700101-000000-000-001-2, O-19700101-000000-000-001-3], tags=['ENTRY']), StopMarketOrder(SELL 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000[DEFAULT] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-2, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-3], parent_order_id=O-19700101-000000-000-001-1, tags=['STOP_LOSS']), LimitOrder(SELL 100_000 AUD/USD.SIM LIMIT @ 1.00100 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-3, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-2], parent_order_id=O-19700101-000000-000-001-1, tags=['TAKE_PROFIT'])]), position_id=P-001)"
        )
        assert (
            repr(command)
            == f"SubmitOrderList(client_id=None, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, order_list=OrderList(id=OL-19700101-000000-000-001-1, instrument_id=AUD/USD.SIM, strategy_id=S-001, orders=[MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, contingency_type=OTO, linked_order_ids=[O-19700101-000000-000-001-2, O-19700101-000000-000-001-3], tags=['ENTRY']), StopMarketOrder(SELL 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000[DEFAULT] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-2, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-3], parent_order_id=O-19700101-000000-000-001-1, tags=['STOP_LOSS']), LimitOrder(SELL 100_000 AUD/USD.SIM LIMIT @ 1.00100 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-3, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-2], parent_order_id=O-19700101-000000-000-001-1, tags=['TAKE_PROFIT'])]), position_id=P-001, command_id={uuid}, ts_init=0)"
        )

    def test_modify_order_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        command = ModifyOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            price=Price.from_str("1.00000"),
            trigger_price=Price.from_str("1.00010"),
            quantity=Quantity.from_int(100_000),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert ModifyOrder.from_dict(ModifyOrder.to_dict(command)) == command
        assert (
            str(command)
            == "ModifyOrder(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, quantity=100_000, price=1.00000, trigger_price=1.00010)"
        )
        assert (
            repr(command)
            == f"ModifyOrder(client_id=None, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, quantity=100_000, price=1.00000, trigger_price=1.00010, command_id={uuid}, ts_init=0)"
        )

    def test_modify_order_command_with_none_venue_order_id_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        command = ModifyOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=None,
            price=Price.from_str("1.00000"),
            trigger_price=Price.from_str("1.00010"),
            quantity=Quantity.from_int(100_000),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert ModifyOrder.from_dict(ModifyOrder.to_dict(command)) == command
        assert (
            str(command)
            == "ModifyOrder(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None, quantity=100_000, price=1.00000, trigger_price=1.00010)"
        )
        assert (
            repr(command)
            == f"ModifyOrder(client_id=None, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None, quantity=100_000, price=1.00000, trigger_price=1.00010, command_id={uuid}, ts_init=0)"
        )

    def test_cancel_order_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

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
            == "CancelOrder(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001)"
        )
        assert (
            repr(command)
            == f"CancelOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, command_id={uuid}, ts_init=0)"
        )

    def test_cancel_order_command_with_none_venue_order_id_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

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
            == "CancelOrder(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None)"
        )
        assert (
            repr(command)
            == f"CancelOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None, command_id={uuid}, ts_init=0)"
        )

    def test_cancel_all_orders_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        command = CancelAllOrders(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert CancelAllOrders.from_dict(CancelAllOrders.to_dict(command)) == command
        assert (
            str(command) == "CancelAllOrders(instrument_id=AUD/USD.SIM, order_side=NO_ORDER_SIDE)"
        )
        assert (
            repr(command)
            == f"CancelAllOrders(client_id=None, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, order_side=NO_ORDER_SIDE, command_id={uuid}, ts_init=0)"
        )

    def test_batch_cancel_orders_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid1 = UUID4()
        uuid2 = UUID4()
        uuid3 = UUID4()
        uuid4 = UUID4()

        cancel1 = CancelOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-1234561"),
            venue_order_id=VenueOrderId("1"),
            command_id=uuid1,
            ts_init=self.clock.timestamp_ns(),
        )
        cancel2 = CancelOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-1234562"),
            venue_order_id=VenueOrderId("2"),
            command_id=uuid2,
            ts_init=self.clock.timestamp_ns(),
        )
        cancel3 = CancelOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-1234563"),
            venue_order_id=VenueOrderId("3"),
            command_id=uuid3,
            ts_init=self.clock.timestamp_ns(),
        )

        command = BatchCancelOrders(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            cancels=[cancel1, cancel2, cancel3],
            command_id=uuid4,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert BatchCancelOrders.from_dict(BatchCancelOrders.to_dict(command)) == command
        assert (
            str(command)
            == f"BatchCancelOrders(instrument_id=AUD/USD.SIM, cancels=[CancelOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-1234561, venue_order_id=1, command_id={uuid1}, ts_init=0), CancelOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-1234562, venue_order_id=2, command_id={uuid2}, ts_init=0), CancelOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-1234563, venue_order_id=3, command_id={uuid3}, ts_init=0)])"
        )
        # TODO: TBC
        # assert (
        #     repr(command)
        #     == f"BatchCancelOrders(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None, command_id={uuid}, ts_init=0)"
        # )

    def test_query_order_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        command = QueryOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert QueryOrder.from_dict(QueryOrder.to_dict(command)) == command
        assert (
            str(command)
            == "QueryOrder(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001)"
        )
        assert (
            repr(command)
            == f"QueryOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, command_id={uuid}, ts_init=0)"
        )

    def test_query_order_command_with_none_venue_order_id_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        command = QueryOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=None,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert QueryOrder.from_dict(QueryOrder.to_dict(command)) == command
        assert (
            str(command)
            == "QueryOrder(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None)"
        )
        assert (
            repr(command)
            == f"QueryOrder(client_id=SIM, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None, command_id={uuid}, ts_init=0)"
        )

    def test_query_account_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        command = QueryAccount(
            trader_id=TraderId("TRADER-001"),
            account_id=AccountId("ACCOUNT-001"),
            command_id=uuid,
            client_id=ClientId("BROKER"),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert QueryAccount.from_dict(QueryAccount.to_dict(command)) == command
        assert (
            str(command)
            == f"QueryAccount(client_id=BROKER, trader_id=TRADER-001, account_id=ACCOUNT-001, command_id={uuid}, ts_init=0)"
        )
        assert (
            repr(command)
            == f"QueryAccount(client_id=BROKER, trader_id=TRADER-001, account_id=ACCOUNT-001, command_id={uuid}, ts_init=0)"
        )

    def test_generate_order_status_report_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        command = GenerateOrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert (
            GenerateOrderStatusReport.from_dict(GenerateOrderStatusReport.to_dict(command))
            == command
        )
        assert (
            repr(command)
            == f"GenerateOrderStatusReport(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, command_id={uuid}, ts_init=0)"
        )

    def test_generate_order_status_report_command_with_none_venue_order_id_to_from_dict_and_str_repr(
        self,
    ):
        # Arrange
        uuid = UUID4()

        command = GenerateOrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=None,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert (
            GenerateOrderStatusReport.from_dict(GenerateOrderStatusReport.to_dict(command))
            == command
        )
        assert (
            repr(command)
            == f"GenerateOrderStatusReport(instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=None, command_id={uuid}, ts_init=0)"
        )

    def test_generate_order_status_reports_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        start_time = datetime(2023, 1, 1, 12, 0, 0)
        end_time = datetime(2023, 1, 1, 18, 0, 0)

        command = GenerateOrderStatusReports(
            instrument_id=AUDUSD_SIM.id,
            start=start_time,
            end=end_time,
            open_only=True,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert (
            GenerateOrderStatusReports.from_dict(GenerateOrderStatusReports.to_dict(command))
            == command
        )
        assert (
            repr(command)
            == f"GenerateOrderStatusReports(instrument_id=AUD/USD.SIM, start={start_time}, end={end_time}, open_only=True, command_id={uuid}, ts_init=0)"
        )

    def test_generate_order_status_reports_command_with_none_instrument_id_to_from_dict_and_str_repr(
        self,
    ):
        # Arrange
        uuid = UUID4()
        start_time = datetime(2023, 1, 1, 12, 0, 0)
        end_time = datetime(2023, 1, 1, 18, 0, 0)

        command = GenerateOrderStatusReports(
            instrument_id=None,
            start=start_time,
            end=end_time,
            open_only=False,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert (
            GenerateOrderStatusReports.from_dict(GenerateOrderStatusReports.to_dict(command))
            == command
        )
        assert (
            repr(command)
            == f"GenerateOrderStatusReports(instrument_id=None, start={start_time}, end={end_time}, open_only=False, command_id={uuid}, ts_init=0)"
        )

    def test_generate_fill_reports_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        start_time = datetime(2023, 1, 1, 12, 0, 0)
        end_time = datetime(2023, 1, 1, 18, 0, 0)

        command = GenerateFillReports(
            instrument_id=AUDUSD_SIM.id,
            venue_order_id=VenueOrderId("001"),
            start=start_time,
            end=end_time,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert GenerateFillReports.from_dict(GenerateFillReports.to_dict(command)) == command
        assert (
            repr(command)
            == f"GenerateFillReports(instrument_id=AUD/USD.SIM, venue_order_id=001, start={start_time}, end={end_time}, command_id={uuid}, ts_init=0)"
        )

    def test_generate_fill_reports_command_with_none_venue_order_id_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        start_time = datetime(2023, 1, 1, 12, 0, 0)
        end_time = datetime(2023, 1, 1, 18, 0, 0)

        command = GenerateFillReports(
            instrument_id=AUDUSD_SIM.id,
            venue_order_id=None,
            start=start_time,
            end=end_time,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert GenerateFillReports.from_dict(GenerateFillReports.to_dict(command)) == command
        assert (
            repr(command)
            == f"GenerateFillReports(instrument_id=AUD/USD.SIM, venue_order_id=None, start={start_time}, end={end_time}, command_id={uuid}, ts_init=0)"
        )

    def test_generate_position_status_reports_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        start_time = datetime(2023, 1, 1, 12, 0, 0)
        end_time = datetime(2023, 1, 1, 18, 0, 0)

        command = GeneratePositionStatusReports(
            instrument_id=AUDUSD_SIM.id,
            start=start_time,
            end=end_time,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert (
            GeneratePositionStatusReports.from_dict(GeneratePositionStatusReports.to_dict(command))
            == command
        )
        assert (
            repr(command)
            == f"GeneratePositionStatusReports(instrument_id=AUD/USD.SIM, start={start_time}, end={end_time}, command_id={uuid}, ts_init=0)"
        )

    def test_generate_position_status_reports_command_with_none_instrument_id_to_from_dict_and_str_repr(
        self,
    ):
        # Arrange
        uuid = UUID4()
        start_time = datetime(2023, 1, 1, 12, 0, 0)
        end_time = datetime(2023, 1, 1, 18, 0, 0)

        command = GeneratePositionStatusReports(
            instrument_id=None,
            start=start_time,
            end=end_time,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert (
            GeneratePositionStatusReports.from_dict(GeneratePositionStatusReports.to_dict(command))
            == command
        )
        assert (
            repr(command)
            == f"GeneratePositionStatusReports(instrument_id=None, start={start_time}, end={end_time}, command_id={uuid}, ts_init=0)"
        )

    def test_generate_execution_mass_status_command_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()

        command = GenerateExecutionMassStatus(
            trader_id=TraderId("TRADER-001"),
            client_id=ClientId("BROKER-001"),
            venue=None,
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert (
            GenerateExecutionMassStatus.from_dict(GenerateExecutionMassStatus.to_dict(command))
            == command
        )
        assert (
            repr(command)
            == f"GenerateExecutionMassStatus(trader_id=TRADER-001, client_id=BROKER-001, venue=None, command_id={uuid}, ts_init=0)"
        )

    def test_generate_execution_mass_status_command_with_params_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        params = {"custom_param": "value"}

        command = GenerateExecutionMassStatus(
            trader_id=TraderId("TRADER-001"),
            client_id=ClientId("BROKER-002"),
            venue=Venue("NYMEX"),
            command_id=uuid,
            ts_init=self.clock.timestamp_ns(),
            params=params,
        )

        # Act, Assert
        assert (
            GenerateExecutionMassStatus.from_dict(GenerateExecutionMassStatus.to_dict(command))
            == command
        )
        assert (
            repr(command)
            == f"GenerateExecutionMassStatus(trader_id=TRADER-001, client_id=BROKER-002, venue=NYMEX, command_id={uuid}, ts_init=0)"
        )
        assert command.params == params
