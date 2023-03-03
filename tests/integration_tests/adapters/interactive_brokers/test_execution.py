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
import asyncio
import datetime
from unittest.mock import patch

import pytest
from ib_insync import CommissionReport
from ib_insync import Contract
from ib_insync import Fill
from ib_insync import LimitOrder
from ib_insync import Trade

from nautilus_trader.adapters.interactive_brokers.execution import InteractiveBrokersExecutionClient
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.base.base_execution import TestBaseExecClient
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestExecStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestProviderStubs


class TestInteractiveBrokersExecution(TestBaseExecClient):
    @pytest.fixture(autouse=True, scope="function")
    def ib_init(self, mocker, exec_client, cache):
        self.contract_details = IBTestProviderStubs.aapl_equity_contract_details()
        self.contract = self.contract_details.contract

    def instrument_setup(self, instrument=None, contract_details=None):
        instrument = instrument or self.instrument
        contract_details = contract_details or self.contract_details
        self.exec_client._instrument_provider.contract_details[
            instrument.id.value
        ] = contract_details
        self.exec_client._instrument_provider.contract_id_to_instrument_id[
            contract_details.contract.conId
        ] = instrument.id
        self.exec_client._instrument_provider.add(instrument)
        self.cache.add_instrument(instrument)

    def order_setup(self, status: OrderStatus = OrderStatus.SUBMITTED):
        order = TestExecStubs.limit_order(
            instrument_id=self.instrument.id,
            client_order_id=self.client_order_id,
        )
        if status == OrderStatus.SUBMITTED:
            order = TestExecStubs.make_submitted_order(order)
        elif status == OrderStatus.ACCEPTED:
            order = TestExecStubs.make_accepted_order(order, venue_order_id=self.venue_order_id)
        else:
            raise ValueError(status)
        self.exec_client._cache.add_order(order, PositionId("1"))
        return order

    @pytest.mark.asyncio
    async def test_connect(self, mocker):
        # Arrange
        mocker.patch.object(
            self.exec_client._client,
            "accountValues",
            return_value=IBTestDataStubs.account_values(),
        )

        # Act
        self.exec_client.connect()
        await asyncio.sleep(0)
        await asyncio.sleep(0)

        # Assert
        assert self.exec_client.is_connected

    @pytest.mark.asyncio
    async def test_disconnect(self, mocker):
        # Arrange
        mocker.patch.object(
            self.exec_client._client,
            "accountValues",
            return_value=IBTestDataStubs.account_values(),
        )
        self.exec_client.connect()
        await asyncio.sleep(0)
        await asyncio.sleep(0)

        # Act
        self.exec_client.disconnect()
        await asyncio.sleep(0)
        await asyncio.sleep(0)

        # Assert
        assert not self.exec_client.is_connected

    @pytest.mark.asyncio
    async def test_factory(self, event_loop):
        # Act
        exec_client = self.exec_client

        # Assert
        assert exec_client is not None

    def test_submit_order(self, mocker):
        # Arrange
        self.instrument_setup(instrument=self.instrument, contract_details=self.contract_details)
        trade = IBTestExecStubs.trade_submitted(client_order_id=self.client_order_id)
        mock_place_order = mocker.patch.object(
            self.exec_client._client,
            "placeOrder",
            return_value=trade,
        )

        # Act
        order = TestExecStubs.limit_order(
            instrument_id=self.instrument.id,
            client_order_id=self.client_order_id,
        )
        command = TestCommandStubs.submit_order_command(order=order)
        self.exec_client.submit_order(command=command)

        # Assert
        expected = {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
            "order": LimitOrder(action="BUY", totalQuantity=100.0, lmtPrice=55.0),
        }

        # Assert
        kwargs = mock_place_order.call_args.kwargs
        # Can't directly compare kwargs for some reason?
        assert kwargs["contract"] == expected["contract"]
        assert kwargs["order"].action == expected["order"].action
        assert kwargs["order"].totalQuantity == expected["order"].totalQuantity
        assert kwargs["order"].lmtPrice == expected["order"].lmtPrice

    def test_submit_bracket_order(self):
        # TODO - not implemented
        pass

    def test_modify_order(self, mocker):
        # Arrange
        instrument = IBTestProviderStubs.aapl_instrument()
        contract_details = IBTestProviderStubs.aapl_equity_contract_details()
        contract = contract_details.contract
        order = IBTestExecStubs.create_order(quantity=50)
        self.instrument_setup(instrument=instrument, contract_details=contract_details)
        self.exec_client._ib_insync_orders[self.client_order_id] = Trade(
            contract=contract,
            order=order,
        )
        mock_place_order = mocker.patch.object(self.exec_client._client, "placeOrder")

        # Act
        command = TestCommandStubs.modify_order_command(
            instrument_id=instrument.id,
            client_order_id=self.client_order_id,
            price=Price.from_int(10),
            quantity=Quantity.from_str("100"),
        )
        self.exec_client.modify_order(command=command)

        # Assert
        expected = {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
            "order": LimitOrder(
                orderId=1,
                clientId=1,
                action="BUY",
                totalQuantity=100.0,
                lmtPrice=10.0,
                orderRef="C-1",
            ),
        }

        # Assert
        kwargs = mock_place_order.call_args.kwargs
        # Can't directly compare kwargs for some reason?
        assert kwargs["contract"] == expected["contract"]
        assert kwargs["order"].action == expected["order"].action
        assert kwargs["order"].totalQuantity == expected["order"].totalQuantity
        assert kwargs["order"].lmtPrice == expected["order"].lmtPrice

    def test_cancel_order(self, mocker):
        # Arrange
        instrument = IBTestProviderStubs.aapl_instrument()
        contract_details = IBTestProviderStubs.aapl_equity_contract_details()
        contract = contract_details.contract
        order = IBTestExecStubs.create_order()
        self.instrument_setup(instrument=instrument, contract_details=contract_details)
        self.exec_client._ib_insync_orders[TestIdStubs.client_order_id()] = Trade(
            contract=contract,
            order=order,
        )
        mock_place_order = mocker.patch.object(self.exec_client._client, "cancelOrder")

        # Act
        command = TestCommandStubs.cancel_order_command(instrument_id=instrument.id)
        self.exec_client.cancel_order(command=command)

        # Assert
        expected = {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
            "order": LimitOrder(action="BUY", totalQuantity=100_000, lmtPrice=105.0),
        }

        # Assert
        kwargs = mock_place_order.call_args.kwargs
        # Can't directly compare kwargs for some reason?
        assert kwargs["order"].action == expected["order"].action
        assert kwargs["order"].totalQuantity == expected["order"].totalQuantity
        assert kwargs["order"].lmtPrice == expected["order"].lmtPrice

    @pytest.mark.asyncio
    @patch.object(InteractiveBrokersExecutionClient, "generate_order_accepted")
    async def test_on_submitted_event(self, mock_generate_order_accepted):
        # Arrange
        self.instrument_setup()
        self.order_setup()
        trade = IBTestExecStubs.trade_pre_submit(client_order_id=self.client_order_id)

        # Act
        self.exec_client._on_order_update_event(trade)

        # Assert
        kwargs = mock_generate_order_accepted.call_args.kwargs
        expected = {
            "client_order_id": self.client_order_id,
            "instrument_id": InstrumentId.from_str("AAPL.AMEX"),
            "strategy_id": StrategyId("S-001"),
            "ts_event": 1646449586871811000,
            "venue_order_id": VenueOrderId("0"),
        }
        assert kwargs == expected

    @pytest.mark.asyncio
    @patch.object(InteractiveBrokersExecutionClient, "generate_order_filled")
    async def test_on_exec_details(self, mock_generate_order_filled):
        # Arrange
        self.instrument_setup()
        self.order_setup()
        contract = IBTestProviderStubs.aapl_equity_contract_details().contract

        # Act
        execution = IBTestExecStubs.execution()
        fill = Fill(
            contract=contract,
            execution=execution,
            commissionReport=CommissionReport(
                execId="1",
                commission=1.0,
                currency="USD",
            ),
            time=datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc),
        )
        trade = IBTestExecStubs.trade_submitted(client_order_id=self.client_order_id)
        self.exec_client._on_execution_detail(trade, fill)

        # Assert
        kwargs = mock_generate_order_filled.call_args.kwargs

        expected = {
            "client_order_id": self.client_order_id,
            "commission": Money("1.00", USD),
            "instrument_id": InstrumentId.from_str("AAPL.AMEX"),
            "last_px": Price.from_str("50.00"),
            "last_qty": Quantity.from_str("100"),
            "liquidity_side": LiquiditySide.NO_LIQUIDITY_SIDE,
            "order_side": 1,
            "order_type": OrderType.LIMIT,
            "quote_currency": USD,
            "strategy_id": StrategyId("S-001"),
            "trade_id": TradeId("1"),
            "ts_event": 0,
            "venue_order_id": VenueOrderId("0"),
            "venue_position_id": None,
        }
        assert kwargs == expected

    @pytest.mark.asyncio
    @patch.object(InteractiveBrokersExecutionClient, "generate_order_updated")
    async def test_on_order_modify(self, mock_generate_order_updated):
        # Arrange
        self.instrument_setup()
        self.order_setup(status=OrderStatus.ACCEPTED)
        order = IBTestExecStubs.create_order(permId=1, client_order_id=self.client_order_id)
        trade = IBTestExecStubs.trade_submitted(order=order)

        # Act
        self.exec_client._on_order_modify(trade)

        # Assert
        kwargs = mock_generate_order_updated.call_args.kwargs
        expected = {
            "client_order_id": self.client_order_id,
            "instrument_id": self.instrument.id,
            "price": Price.from_str("105.00"),
            "quantity": Quantity.from_str("100000"),
            "strategy_id": TestIdStubs.strategy_id(),
            "trigger_price": None,
            "ts_event": 1646449588378175000,
            "venue_order_id": VenueOrderId("1"),
            "venue_order_id_modified": False,
        }
        assert kwargs == expected

    @pytest.mark.asyncio
    async def test_on_order_cancel_pending(self):
        # Arrange
        self.instrument_setup()
        nautilus_order = self.order_setup()
        order = IBTestExecStubs.create_order(permId=1, client_order_id=self.client_order_id)
        trade = IBTestExecStubs.trade_pre_cancel(order=order)

        # Act
        pending_cancel = OrderPendingCancel(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=self.instrument.id,
            client_order_id=self.client_order_id,
            venue_order_id=self.venue_order_id,
            account_id=self.account_id,
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        nautilus_order.apply(pending_cancel)
        self.exec_client._on_order_pending_cancel(trade)

        # Assert
        order = self.cache.order(self.client_order_id)
        assert order.status == OrderStatus.PENDING_CANCEL

    @pytest.mark.asyncio
    @patch.object(InteractiveBrokersExecutionClient, "generate_order_canceled")
    async def test_on_order_cancel_cancelled(self, mock_generate_order_canceled):
        # Arrange
        self.instrument_setup()
        self.order_setup(status=OrderStatus.ACCEPTED)
        order = IBTestExecStubs.create_order(permId=1, client_order_id=self.client_order_id)
        trade = IBTestExecStubs.trade_canceled(order=order)

        # Act
        self.exec_client._on_order_cancelled(trade)

        # Assert
        kwargs = mock_generate_order_canceled.call_args.kwargs
        expected = {
            "client_order_id": self.client_order_id,
            "instrument_id": InstrumentId.from_str("AAPL.AMEX"),
            "strategy_id": StrategyId("S-001"),
            "ts_event": 1646533382000847000,
            "venue_order_id": VenueOrderId("1"),
        }
        assert kwargs == expected

    @pytest.mark.asyncio
    @patch.object(InteractiveBrokersExecutionClient, "generate_account_state")
    async def test_on_account_update(self, mock_generate_account_state):
        # Arrange
        account_values = IBTestDataStubs.account_values()

        # Act
        self.exec_client.on_account_update(account_values)

        # Assert
        kwargs = mock_generate_account_state.call_args.kwargs
        expected = {
            "balances": [
                AccountBalance.from_dict(
                    {
                        "free": "900000.08",
                        "locked": "100000.16",
                        "total": "1000000.24",
                        "currency": "AUD",
                    },
                ),
            ],
            "margins": [
                MarginBalance.from_dict(
                    {
                        "currency": "AUD",
                        "initial": "200000.97",
                        "instrument_id": None,
                        "maintenance": "200000.36",
                        "type": "MarginBalance",
                    },
                ),
            ],
            "reported": True,
            "ts_event": kwargs["ts_event"],
        }
        assert expected["balances"][0].to_dict() == kwargs["balances"][0].to_dict()
        assert expected["margins"][0].to_dict() == kwargs["margins"][0].to_dict()
        assert all([kwargs[k] == expected[k] for k in kwargs if k not in ("balances", "margins")])

    def test_generate_order_status_report(self):
        pass
