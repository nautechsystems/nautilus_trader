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

from typing import Optional

from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.cache.database import CacheDatabase
from nautilus_trader.common.logging import Logger
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.position import Position
from nautilus_trader.trading.strategy import Strategy


class MockCacheDatabase(CacheDatabase):
    """
    Provides a mock cache database for testing.

    Parameters
    ----------
    logger : Logger
        The logger for the database.
    """

    def __init__(self, logger: Logger):
        super().__init__(logger)

        self.currencies: dict[str, Currency] = {}
        self.instruments: dict[InstrumentId, Instrument] = {}
        self.accounts: dict[AccountId, Account] = {}
        self.orders: dict[ClientOrderId, Order] = {}
        self.positions: dict[PositionId, Position] = {}
        self.submit_order_commands: dict[ClientOrderId, SubmitOrder] = {}
        self.submit_order_list_commands: dict[OrderListId, SubmitOrderList] = {}

    def flush(self) -> None:
        self.accounts.clear()
        self.orders.clear()
        self.positions.clear()
        self.submit_order_commands.clear()
        self.submit_order_list_commands.clear()

    def load_currencies(self) -> dict:
        return self.currencies.copy()

    def load_instruments(self) -> dict:
        return self.instruments.copy()

    def load_accounts(self) -> dict:
        return self.accounts.copy()

    def load_orders(self) -> dict:
        return self.orders.copy()

    def load_positions(self) -> dict:
        return self.positions.copy()

    def load_submit_order_commands(self) -> dict:
        return self.submit_order_commands.copy()

    def load_submit_order_list_commands(self) -> dict:
        return self.submit_order_list_commands.copy()

    def load_currency(self, code: str) -> Currency:
        return self.currencies.get(code)

    def load_instrument(self, instrument_id: InstrumentId) -> Optional[InstrumentId]:
        return self.instruments.get(instrument_id)

    def load_account(self, account_id: AccountId) -> Optional[Account]:
        return self.accounts.get(account_id)

    def load_order(self, client_order_id: ClientOrderId) -> Optional[Order]:
        return self.orders.get(client_order_id)

    def load_position(self, position_id: PositionId) -> Optional[Position]:
        return self.positions.get(position_id)

    def load_strategy(self, strategy_id: StrategyId) -> dict:
        return {}

    def delete_strategy(self, strategy_id: StrategyId) -> None:
        pass

    def load_submit_order_command(self, client_order_id: ClientOrderId) -> Optional[SubmitOrder]:
        return self.submit_order_commands.get(client_order_id)

    def load_submit_order_list_command(
        self,
        order_list_id: OrderListId,
    ) -> Optional[SubmitOrderList]:
        return self.submit_order_commands.get(order_list_id)

    def add_currency(self, currency: Currency) -> None:
        self.currencies[currency.code] = currency

    def add_instrument(self, instrument: Instrument) -> None:
        self.instruments[instrument.id] = instrument

    def add_account(self, account: Account) -> None:
        self.accounts[account.id] = account

    def add_order(self, order: Order) -> None:
        self.orders[order.client_order_id] = order

    def add_position(self, position: Position) -> None:
        self.positions[position.id] = position

    def add_submit_order_command(self, command: SubmitOrder) -> None:
        self.submit_order_commands[command.order.client_order_id] = command

    def add_submit_order_list_command(self, command: SubmitOrderList) -> None:
        self.submit_order_list_commands[command.order_list.id] = command

    def update_account(self, event: Account) -> None:
        pass  # Would persist the event

    def update_order(self, order: Order) -> None:
        pass  # Would persist the event

    def update_position(self, position: Position) -> None:
        pass  # Would persist the event

    def update_strategy(self, strategy: Strategy) -> None:
        pass  # Would persist the user state dict
