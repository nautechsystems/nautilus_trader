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

from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.cache.facade import CacheDatabaseFacade
from nautilus_trader.cache.postgres.transformers import transform_currency_from_pyo3
from nautilus_trader.cache.postgres.transformers import transform_currency_to_pyo3
from nautilus_trader.cache.postgres.transformers import transform_instrument_from_pyo3
from nautilus_trader.cache.postgres.transformers import transform_instrument_to_pyo3
from nautilus_trader.cache.postgres.transformers import transform_order_from_pyo3
from nautilus_trader.cache.postgres.transformers import transform_order_to_pyo3
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import PostgresCacheDatabase
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
