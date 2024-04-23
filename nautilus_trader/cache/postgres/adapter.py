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
from nautilus_trader.core.nautilus_pyo3 import PostgresCacheDatabase
from nautilus_trader.model.objects import Currency


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
