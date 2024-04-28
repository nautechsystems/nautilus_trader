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

import asyncio
import os
import sys

import pytest

from nautilus_trader.cache.postgres.adapter import CachePostgresAdapter
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


_AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")

# Requirements:
# - A Postgres service listening on the default port 5432

pytestmark = pytest.mark.skipif(
    sys.platform != "linux",
    reason="databases only supported on Linux",
)


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

    ################################################################################
    # General
    ################################################################################

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

    ################################################################################
    # Instrument - Crypto Future
    ################################################################################
    @pytest.mark.asyncio
    async def test_add_instrument_crypto_future(self):
        # Arrange, Act
        btc_usdt_crypto_future = TestInstrumentProvider.btcusdt_future_binance()
        self.database.add_currency(btc_usdt_crypto_future.underlying)
        self.database.add_currency(btc_usdt_crypto_future.quote_currency)
        self.database.add_currency(btc_usdt_crypto_future.settlement_currency)

        await asyncio.sleep(0.5)
        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: self.database.load_currencies())

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["BTC", "USDT"]

        # add instrument
        self.database.add_instrument(btc_usdt_crypto_future)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(btc_usdt_crypto_future.id))

        # Assert
        result = self.database.load_instrument(btc_usdt_crypto_future.id)
        assert result == btc_usdt_crypto_future

    ################################################################################
    # Instrument - Crypto Perpetual
    ################################################################################
    @pytest.mark.asyncio
    async def test_add_instrument_crypto_perpetual(self):
        eth_usdt_crypto_perpetual = TestInstrumentProvider.ethusdt_perp_binance()
        self.database.add_currency(eth_usdt_crypto_perpetual.base_currency)
        self.database.add_currency(eth_usdt_crypto_perpetual.quote_currency)
        self.database.add_currency(eth_usdt_crypto_perpetual.settlement_currency)

        await asyncio.sleep(0.5)
        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: self.database.load_currencies())

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["ETH", "USDT"]

        # add instrument
        self.database.add_instrument(eth_usdt_crypto_perpetual)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(eth_usdt_crypto_perpetual.id))

        # Assert
        result = self.database.load_instrument(eth_usdt_crypto_perpetual.id)
        assert result == eth_usdt_crypto_perpetual

    ################################################################################
    # Instrument - Currency Pair
    ################################################################################

    @pytest.mark.asyncio
    async def test_add_instrument_currency_pair(self):
        self.database.add_currency(_AUDUSD_SIM.base_currency)
        self.database.add_currency(_AUDUSD_SIM.quote_currency)
        await asyncio.sleep(0.6)

        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: self.database.load_currencies())
        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["AUD", "USD"]

        self.database.add_instrument(_AUDUSD_SIM)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(_AUDUSD_SIM.id))

        # Assert
        assert _AUDUSD_SIM == self.database.load_instrument(_AUDUSD_SIM.id)

        # Update some fields, to check that add_instrument is idempotent
        aud_usd_currency_pair_updated = CurrencyPair(
            instrument_id=_AUDUSD_SIM.id,
            raw_symbol=_AUDUSD_SIM.raw_symbol,
            base_currency=_AUDUSD_SIM.base_currency,
            quote_currency=_AUDUSD_SIM.quote_currency,
            price_precision=_AUDUSD_SIM.price_precision,
            size_precision=_AUDUSD_SIM.size_precision,
            price_increment=_AUDUSD_SIM.price_increment,
            size_increment=_AUDUSD_SIM.size_increment,
            lot_size=_AUDUSD_SIM.lot_size,
            max_quantity=_AUDUSD_SIM.max_quantity,
            min_quantity=_AUDUSD_SIM.min_quantity,
            max_price=_AUDUSD_SIM.max_price,
            min_price=Price.from_str("111"),  # <-- changed this
            max_notional=_AUDUSD_SIM.max_notional,
            min_notional=_AUDUSD_SIM.min_notional,
            margin_init=_AUDUSD_SIM.margin_init,
            margin_maint=_AUDUSD_SIM.margin_maint,
            maker_fee=_AUDUSD_SIM.maker_fee,
            taker_fee=_AUDUSD_SIM.taker_fee,
            tick_scheme_name=_AUDUSD_SIM.tick_scheme_name,
            ts_event=123,  # <-- changed this
            ts_init=456,  # <-- changed this
        )

        self.database.add_instrument(aud_usd_currency_pair_updated)

        # We have to manually sleep and not use eventually
        await asyncio.sleep(0.5)

        # Assert
        result = self.database.load_instrument(_AUDUSD_SIM.id)
        assert result.id == _AUDUSD_SIM.id
        assert result.ts_event == 123
        assert result.ts_init == 456
        assert result.min_price == Price.from_str("111")

    ################################################################################
    # Instrument - Equity
    ################################################################################

    @pytest.mark.asyncio
    async def test_add_instrument_equity(self):
        appl_equity = TestInstrumentProvider.equity()
        self.database.add_currency(appl_equity.quote_currency)

        await asyncio.sleep(0.1)
        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: self.database.load_currencies())

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["USD"]

        # add instrument
        self.database.add_instrument(appl_equity)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(appl_equity.id))

        # Assert
        assert appl_equity == self.database.load_instrument(appl_equity.id)

    ################################################################################
    # Instrument - Futures Contract
    ################################################################################
    @pytest.mark.asyncio
    async def test_add_instrument_futures_contract(self):
        es_futures = TestInstrumentProvider.es_future(expiry_year=2023, expiry_month=12)
        self.database.add_currency(es_futures.quote_currency)

        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: self.database.load_currencies())

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["USD"]

        # add instrument
        self.database.add_instrument(es_futures)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(es_futures.id))

        # Assert
        assert es_futures == self.database.load_instrument(es_futures.id)

    ################################################################################
    # Instrument - Options Contract
    ################################################################################
    @pytest.mark.asyncio
    async def test_add_instrument_options_contract(self):
        aapl_option = TestInstrumentProvider.aapl_option()
        self.database.add_currency(aapl_option.quote_currency)

        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: self.database.load_currencies())

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["USD"]

        # add instrument
        self.database.add_instrument(aapl_option)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(aapl_option.id))

        # Assert
        assert aapl_option == self.database.load_instrument(aapl_option.id)
