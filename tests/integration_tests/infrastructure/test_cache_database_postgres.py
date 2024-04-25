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

import os

import pytest

from nautilus_trader.cache.postgres.adapter import CachePostgresAdapter
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.objects import Currency
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestCachePostgresAdapter:
    def setup(self):
        # set envs
        os.environ["POSTGRES_HOST"] = "localhost"
        os.environ["POSTGRES_PORT"] = "5432"
        os.environ["POSTGRES_USERNAME"] = "nautilus"
        os.environ["POSTGRES_PASSWORD"] = "pass"
        os.environ["POSTGRES_DATABASE"] = "nautilus"
        self.database: CachePostgresAdapter = CachePostgresAdapter()
        # reset database
        self.database.flush()
        self.clock = TestClock()

        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Init strategy
        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

    def teardown(self):
        self.database.flush()

    @pytest.mark.asyncio
    async def test_load_general_objects_when_nothing_in_cache_returns_empty_dict(self):
        # Arrange, Act
        result = self.database.load()

        # Assert
        assert result == {}

    @pytest.mark.asyncio
    async def test_add_general_object_adds_to_cache(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()
        key = str(bar.bar_type) + "-" + str(bar.ts_event)

        # Act
        self.database.add(key, str(bar).encode())

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load())

        # Assert
        assert self.database.load() == {key: str(bar).encode()}

    ################################################################################
    # Currency
    ################################################################################
    @pytest.mark.asyncio
    async def test_add_currency(self):
        # Arrange
        currency = Currency(
            code="BTC",
            precision=8,
            iso4217=0,
            name="BTC",
            currency_type=CurrencyType.CRYPTO,
        )

        # Act
        self.database.add_currency(currency)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_currency(currency.code))

        # Assert
        assert self.database.load_currency(currency.code) == currency

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["BTC"]
