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

from typing import Optional

from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.cache.database import CacheDatabase
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import SyntheticInstrument
from nautilus_trader.model.orders import Order
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

        self.general: dict[str, bytes] = {}
        self.currencies: dict[str, Currency] = {}
        self.instruments: dict[InstrumentId, Instrument] = {}
        self.synthetics: dict[InstrumentId, SyntheticInstrument] = {}
        self.accounts: dict[AccountId, Account] = {}
        self.orders: dict[ClientOrderId, Order] = {}
        self.positions: dict[PositionId, Position] = {}
        self._index_order_position: dict[ClientOrderId, PositionId] = {}
        self._index_order_client: dict[ClientOrderId, ClientId] = {}

    def flush(self) -> None:
        self.general.clear()
        self.currencies.clear()
        self.instruments.clear()
        self.synthetics.clear()
        self.accounts.clear()
        self.orders.clear()
        self.positions.clear()
        self._index_order_position.clear()
        self._index_order_client.clear()

    def load(self) -> dict:
        return self.general.copy()

    def load_currencies(self) -> dict:
        return self.currencies.copy()

    def load_instruments(self) -> dict:
        return self.instruments.copy()

    def load_synthetics(self) -> dict:
        return self.synthetics.copy()

    def load_accounts(self) -> dict:
        return self.accounts.copy()

    def load_orders(self) -> dict:
        return self.orders.copy()

    def load_positions(self) -> dict:
        return self.positions.copy()

    def load_currency(self, code: str) -> Currency:
        return self.currencies.get(code)

    def load_instrument(self, instrument_id: InstrumentId) -> Optional[Instrument]:
        return self.instruments.get(instrument_id)

    def load_synthetic(self, instrument_id: InstrumentId) -> Optional[SyntheticInstrument]:
        return self.synthetics.get(instrument_id)

    def load_account(self, account_id: AccountId) -> Optional[Account]:
        return self.accounts.get(account_id)

    def load_order(self, client_order_id: ClientOrderId) -> Optional[Order]:
        return self.orders.get(client_order_id)

    def load_index_order_position(self) -> dict[ClientOrderId, PositionId]:
        return self._index_order_position

    def load_index_order_client(self) -> dict[ClientOrderId, ClientId]:
        return self._index_order_client

    def load_position(self, position_id: PositionId) -> Optional[Position]:
        return self.positions.get(position_id)

    def load_strategy(self, strategy_id: StrategyId) -> dict:
        return {}

    def delete_strategy(self, strategy_id: StrategyId) -> None:
        pass

    def add_currency(self, currency: Currency) -> None:
        self.currencies[currency.code] = currency

    def add_instrument(self, instrument: Instrument) -> None:
        self.instruments[instrument.id] = instrument

    def add_synthetic(self, synthetic: SyntheticInstrument) -> None:
        self.synthetics[synthetic.id] = synthetic

    def add_account(self, account: Account) -> None:
        self.accounts[account.id] = account

    def add_order(
        self,
        order: Order,
        position_id: Optional[PositionId] = None,
        client_id: Optional[ClientId] = None,
    ) -> None:
        self.orders[order.client_order_id] = order
        self._index_order_position[order.client_order_id] = position_id
        self._index_order_client[order.client_order_id] = client_id

    def add_position(self, position: Position) -> None:
        self.positions[position.id] = position

    def index_order_position(self, client_order_id: ClientOrderId, position_id: PositionId) -> None:
        self._index_order_position[client_order_id] = position_id

    def update_account(self, event: Account) -> None:
        pass  # Would persist the event

    def update_order(self, order: Order) -> None:
        pass  # Would persist the event

    def update_position(self, position: Position) -> None:
        pass  # Would persist the event

    def update_strategy(self, strategy: Strategy) -> None:
        pass  # Would persist the user state dict
