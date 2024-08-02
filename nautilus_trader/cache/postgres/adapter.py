# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.cache.facade import CacheDatabaseFacade
from nautilus_trader.cache.postgres.transformers import transform_account_from_pyo3
from nautilus_trader.cache.postgres.transformers import transform_account_to_pyo3
from nautilus_trader.cache.postgres.transformers import transform_bar_to_pyo3
from nautilus_trader.cache.postgres.transformers import transform_currency_from_pyo3
from nautilus_trader.cache.postgres.transformers import transform_currency_to_pyo3
from nautilus_trader.cache.postgres.transformers import transform_instrument_from_pyo3
from nautilus_trader.cache.postgres.transformers import transform_instrument_to_pyo3
from nautilus_trader.cache.postgres.transformers import transform_order_from_pyo3
from nautilus_trader.cache.postgres.transformers import transform_order_to_pyo3
from nautilus_trader.cache.postgres.transformers import transform_quote_tick_to_pyo3
from nautilus_trader.cache.postgres.transformers import transform_trade_tick_from_pyo3
from nautilus_trader.cache.postgres.transformers import transform_trade_tick_to_pyo3
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import PostgresCacheDatabase
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.orders import Order


class CachePostgresAdapter(CacheDatabaseFacade):

    def __init__(
        self,
        config: CacheConfig | None = None,
    ):
        if config:
            config = CacheConfig()
        super().__init__(config)
        self._backing: PostgresCacheDatabase = PostgresCacheDatabase.connect()

    def flush(self):
        self._backing.flush_db()

    def load(self):
        data = self._backing.load()
        return {key: bytes(value) for key, value in data.items()}

    def add(self, key: str, value: bytes):
        self._backing.add(key, value)

    def add_currency(self, currency: Currency):
        currency_pyo3 = transform_currency_to_pyo3(currency)
        self._backing.add_currency(currency_pyo3)

    def load_currencies(self) -> dict[str, Currency]:
        currencies = self._backing.load_currencies()
        return {currency.code: transform_currency_from_pyo3(currency) for currency in currencies}

    def load_currency(self, code: str) -> Currency | None:
        currency_pyo3 = self._backing.load_currency(code)
        if currency_pyo3:
            return transform_currency_from_pyo3(currency_pyo3)
        return None

    def add_instrument(self, instrument: Instrument):
        instrument_pyo3 = transform_instrument_to_pyo3(instrument)
        self._backing.add_instrument(instrument_pyo3)

    def load_instrument(self, instrument_id: InstrumentId) -> Instrument:
        instrument_id_pyo3 = nautilus_pyo3.InstrumentId.from_str(str(instrument_id))
        instrument_pyo3 = self._backing.load_instrument(instrument_id_pyo3)
        return transform_instrument_from_pyo3(instrument_pyo3)

    def add_order(self, order: Order):
        order_pyo3 = transform_order_to_pyo3(order)
        self._backing.add_order(order_pyo3)

    def update_order(self, order: Order):
        order_pyo3 = transform_order_to_pyo3(order)
        self._backing.update_order(order_pyo3)

    def load_order(self, client_order_id: ClientOrderId):
        order_id_pyo3 = nautilus_pyo3.ClientOrderId.from_str(str(client_order_id))
        order_pyo3 = self._backing.load_order(order_id_pyo3)
        if order_pyo3:
            return transform_order_from_pyo3(order_pyo3)
        return None

    def load_orders(self):
        orders = self._backing.load_orders()
        return [transform_order_from_pyo3(order) for order in orders]

    def add_account(self, account: Account):
        account_pyo3 = transform_account_to_pyo3(account)
        self._backing.add_account(account_pyo3)

    def load_account(self, account_id: AccountId):
        account_id_pyo3 = nautilus_pyo3.AccountId.from_str(str(account_id))
        account_pyo3 = self._backing.load_account(account_id_pyo3)
        if account_pyo3:
            return transform_account_from_pyo3(account_pyo3)
        return None

    def update_account(self, account: Account):
        account_pyo3 = transform_account_to_pyo3(account)
        self._backing.update_account(account_pyo3)

    def add_trade(self, trade: TradeTick):
        trade_pyo3 = transform_trade_tick_to_pyo3(trade)
        self._backing.add_trade(trade_pyo3)

    def load_trades(self, instrument_id: InstrumentId) -> list[TradeTick]:
        instrument_id_pyo3 = nautilus_pyo3.InstrumentId.from_str(str(instrument_id))
        trades = self._backing.load_trades(instrument_id_pyo3)
        return [transform_trade_tick_from_pyo3(trade) for trade in trades]

    def add_quote(self, quote: QuoteTick):
        quote_pyo3 = transform_quote_tick_to_pyo3(quote)
        self._backing.add_quote(quote_pyo3)

    def load_quotes(self, instrument_id: InstrumentId) -> list[QuoteTick]:
        instrument_id_pyo3 = nautilus_pyo3.InstrumentId.from_str(str(instrument_id))
        quotes = self._backing.load_quotes(instrument_id_pyo3)
        return [QuoteTick.from_pyo3(quote_pyo3) for quote_pyo3 in quotes]

    def add_bar(self, bar: Bar):
        bar_pyo3 = transform_bar_to_pyo3(bar)
        self._backing.add_bar(bar_pyo3)

    def load_bars(self, instrument_id: InstrumentId):
        instrument_id_pyo3 = nautilus_pyo3.InstrumentId.from_str(str(instrument_id))
        bars = self._backing.load_bars(instrument_id_pyo3)
        return [Bar.from_pyo3(bar_pyo3) for bar_pyo3 in bars]
