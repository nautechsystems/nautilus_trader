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

import asyncio
import os
import sys

import pytest

from nautilus_trader.cache.adapter import CachePostgresAdapter
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.signal import generate_signal_class
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import AggressorSide
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarAggregation
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.filters import NewsImpact
from nautilus_trader.trading.strategy import Strategy


_TEST_TIMEOUT = 5.0
_AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")

# Requirements:
# - A Postgres service listening on the default port 5432

pytestmark = pytest.mark.skipif(
    sys.platform != "linux",
    reason="databases only supported on Linux",
)


@pytest.mark.xdist_group(name="postgres_integration")
class TestCachePostgresAdapter:
    def setup(self) -> None:
        # set envs
        os.environ["POSTGRES_HOST"] = "localhost"
        os.environ["POSTGRES_PORT"] = "5432"
        os.environ["POSTGRES_USERNAME"] = "nautilus"
        os.environ["POSTGRES_PASSWORD"] = "pass"
        os.environ["POSTGRES_DATABASE"] = "nautilus"
        try:
            self.database: CachePostgresAdapter = CachePostgresAdapter()
            # reset database
            self.database.flush()
        except BaseException as e:
            message = str(e)
            if (
                "error communicating with database" in message
                or "Operation not permitted" in message
            ):
                pytest.skip(
                    "Postgres service not available; skipping Postgres adapter integration tests.",
                )
                return
            raise
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
        database = getattr(self, "database", None)
        if database is not None:
            database.flush()
            database.dispose()

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
        await eventually(lambda: self.database.load(), timeout=_TEST_TIMEOUT)

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
    # Instrument - Betting
    ################################################################################
    @pytest.mark.skip(reason="from_pyo3 must be implemented")
    @pytest.mark.asyncio
    async def test_add_instrument_betting(self):
        betting = TestInstrumentProvider.betting_instrument()
        self.database.add_currency(betting.quote_currency)

        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: self.database.load_currencies(), timeout=_TEST_TIMEOUT)

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["GBP"]

        # add instrument
        self.database.add_instrument(betting)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(betting.id), timeout=_TEST_TIMEOUT)

        # Assert
        assert betting == self.database.load_instrument(betting.id)

    ################################################################################
    # Instrument - Binary Option
    ################################################################################
    @pytest.mark.skip(reason="from_pyo3 must be implemented")
    @pytest.mark.asyncio
    async def test_add_instrument_binary_option(self):
        binary_option = TestInstrumentProvider.binary_option()
        self.database.add_currency(binary_option.quote_currency)

        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: self.database.load_currencies(), timeout=_TEST_TIMEOUT)

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["USDC"]

        # add instrument
        self.database.add_instrument(binary_option)

        # Allow MPSC thread to insert
        await eventually(
            lambda: self.database.load_instrument(binary_option.id),
            timeout=_TEST_TIMEOUT,
        )

        # Assert
        assert binary_option == self.database.load_instrument(binary_option.id)

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

        # Check that we have added target currencies, because of foreign key constraints
        await eventually(
            lambda: len(self.database.load_currencies().keys()) >= 2,
            timeout=_TEST_TIMEOUT,
        )

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["BTC", "USDT"]

        # add instrument
        self.database.add_instrument(btc_usdt_crypto_future)

        # Allow MPSC thread to insert
        await eventually(
            lambda: self.database.load_instrument(btc_usdt_crypto_future.id),
            timeout=_TEST_TIMEOUT,
        )

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

        # Check that we have added target currencies, because of foreign key constraints
        await eventually(
            lambda: len(self.database.load_currencies().keys()) >= 2,
            timeout=_TEST_TIMEOUT,
        )

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["ETH", "USDT"]

        # add instrument
        self.database.add_instrument(eth_usdt_crypto_perpetual)

        # Allow MPSC thread to insert
        await eventually(
            lambda: self.database.load_instrument(eth_usdt_crypto_perpetual.id),
            timeout=_TEST_TIMEOUT,
        )

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

        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: len(self.database.load_currencies()) >= 2, timeout=_TEST_TIMEOUT)

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["AUD", "USD"]

        self.database.add_instrument(_AUDUSD_SIM)

        # Allow MPSC thread to insert
        await eventually(
            lambda: self.database.load_instrument(_AUDUSD_SIM.id),
            timeout=_TEST_TIMEOUT,
        )

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
        await eventually(
            lambda: self.database.load_instrument(_AUDUSD_SIM.id).min_price
            == Price.from_str("111"),
            timeout=_TEST_TIMEOUT,
        )

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

        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: len(self.database.load_currencies()) >= 1, timeout=_TEST_TIMEOUT)

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["USD"]

        # add instrument
        self.database.add_instrument(appl_equity)

        # Allow MPSC thread to insert
        await eventually(
            lambda: self.database.load_instrument(appl_equity.id),
            timeout=_TEST_TIMEOUT,
        )

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
        await eventually(lambda: len(self.database.load_currencies()) >= 1, timeout=_TEST_TIMEOUT)

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["USD"]

        # add instrument
        self.database.add_instrument(es_futures)

        # Allow MPSC thread to insert
        await eventually(
            lambda: self.database.load_instrument(es_futures.id),
            timeout=_TEST_TIMEOUT,
        )

        # Assert
        assert es_futures == self.database.load_instrument(es_futures.id)

    ################################################################################
    # Instrument - Option Contract
    ################################################################################
    @pytest.mark.asyncio
    async def test_add_instrument_option_contract(self):
        aapl_option = TestInstrumentProvider.aapl_option()
        self.database.add_currency(aapl_option.quote_currency)

        # Check that we have added target currencies, because of foreign key constraints
        await eventually(lambda: self.database.load_currencies(), timeout=_TEST_TIMEOUT)

        currencies = self.database.load_currencies()
        assert list(currencies.keys()) == ["USD"]

        # add instrument
        self.database.add_instrument(aapl_option)

        # Allow MPSC thread to insert
        await eventually(
            lambda: self.database.load_instrument(aapl_option.id),
            timeout=_TEST_TIMEOUT,
        )

        # Assert
        assert aapl_option == self.database.load_instrument(aapl_option.id)

    ################################################################################
    # Orders
    ################################################################################
    @pytest.mark.asyncio
    async def test_add_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            tags=["A", "B", "C"],
        )
        # Add foreign key dependencies: instrument and currencies
        self.database.add_currency(_AUDUSD_SIM.base_currency)
        self.database.add_currency(_AUDUSD_SIM.quote_currency)
        self.database.add_instrument(_AUDUSD_SIM)

        # Act
        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(
            lambda: self.database.load_order(order.client_order_id),
            timeout=_TEST_TIMEOUT,
        )

        # Assert
        result = self.database.load_order(order.client_order_id)
        assert result == order
        # assert order.to_dict() == result.to_dict()  # TODO: Fix tags

    @pytest.mark.asyncio
    async def test_update_order_for_closed_order(self):
        self.database.add_currency(_AUDUSD_SIM.quote_currency)
        # Arrange
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )
        # Add foreign key dependencies: instrument and currencies
        self.database.add_currency(_AUDUSD_SIM.base_currency)
        self.database.add_currency(_AUDUSD_SIM.quote_currency)
        self.database.add_instrument(_AUDUSD_SIM)

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(
            lambda: self.database.load_order(order.client_order_id),
            timeout=_TEST_TIMEOUT,
        )

        order.apply(TestEventStubs.order_submitted(order))
        self.database.update_order(order)

        order.apply(TestEventStubs.order_accepted(order))
        self.database.update_order(order)

        fill = TestEventStubs.order_filled(
            order,
            instrument=_AUDUSD_SIM,
            last_px=Price.from_str("1.00001"),
        )

        order.apply(fill)
        self.database.update_order(order)

        await eventually(
            lambda: len(self.database.load_order(order.client_order_id).events) >= 4,
            timeout=_TEST_TIMEOUT,
        )

        result = self.database.load_order(order.client_order_id)
        assert result == order
        assert order.to_dict() == result.to_dict()

    @pytest.mark.asyncio
    async def test_update_order_for_open_order(self):
        self.database.add_currency(_AUDUSD_SIM.quote_currency)
        order = self.strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        # Add foreign key dependencies: instrument and currencies
        self.database.add_currency(_AUDUSD_SIM.base_currency)
        self.database.add_currency(_AUDUSD_SIM.quote_currency)
        self.database.add_instrument(_AUDUSD_SIM)

        self.database.add_order(order)
        # Allow MPSC thread to insert
        await eventually(
            lambda: self.database.load_order(order.client_order_id),
            timeout=_TEST_TIMEOUT,
        )

        order.apply(TestEventStubs.order_submitted(order))
        self.database.update_order(order)

        order.apply(TestEventStubs.order_accepted(order))

        # Act
        self.database.update_order(order)

        await eventually(
            lambda: len(self.database.load_order(order.client_order_id).events) >= 3,
            timeout=_TEST_TIMEOUT,
        )

        result = self.database.load_order(order.client_order_id)
        assert result == order
        assert order.to_dict() == result.to_dict()

    @pytest.mark.asyncio
    async def test_add_order_snapshot(self):
        self.database.add_currency(_AUDUSD_SIM.quote_currency)
        order1 = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )
        order2 = self.strategy.order_factory.limit(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )
        order3 = self.strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        # Add foreign key dependencies: instrument and currencies
        self.database.add_currency(_AUDUSD_SIM.base_currency)
        self.database.add_currency(_AUDUSD_SIM.quote_currency)
        self.database.add_instrument(_AUDUSD_SIM)

        self.database.add_order_snapshot(order1)
        self.database.add_order_snapshot(order2)
        self.database.add_order_snapshot(order3)

        await eventually(
            lambda: self.database.load_order_snapshot(order1.client_order_id),
            timeout=_TEST_TIMEOUT,
        )
        await eventually(
            lambda: self.database.load_order_snapshot(order2.client_order_id),
            timeout=_TEST_TIMEOUT,
        )
        await eventually(
            lambda: self.database.load_order_snapshot(order3.client_order_id),
            timeout=_TEST_TIMEOUT,
        )
        snapshot1 = self.database.load_order_snapshot(order1.client_order_id)
        snapshot2 = self.database.load_order_snapshot(order2.client_order_id)
        snapshot3 = self.database.load_order_snapshot(order3.client_order_id)

        assert isinstance(snapshot1, nautilus_pyo3.OrderSnapshot)
        assert isinstance(snapshot2, nautilus_pyo3.OrderSnapshot)
        assert isinstance(snapshot3, nautilus_pyo3.OrderSnapshot)

    @pytest.mark.asyncio
    async def test_add_position_snapshot(self):
        self.database.add_currency(_AUDUSD_SIM.quote_currency)
        order = self.strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )
        # Add foreign key dependencies: instrument and currencies
        self.database.add_currency(_AUDUSD_SIM.base_currency)
        self.database.add_currency(_AUDUSD_SIM.quote_currency)
        self.database.add_instrument(_AUDUSD_SIM)

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=_AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=_AUDUSD_SIM, fill=fill)

        self.database.add_position_snapshot(position, Money.from_str("2.00 USD"))

        await eventually(
            lambda: self.database.load_position_snapshot(position.id),
            timeout=_TEST_TIMEOUT,
        )
        snapshot = self.database.load_position_snapshot(position.id)

        assert isinstance(snapshot, nautilus_pyo3.PositionSnapshot)

    ################################################################################
    # Accounts
    ################################################################################
    @pytest.mark.asyncio
    async def test_add_and_update_account(self):
        account = TestExecStubs.cash_account()

        self.database.add_currency(account.base_currency)
        self.database.add_account(account)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_account(account.id), timeout=_TEST_TIMEOUT)

        assert self.database.load_account(account.id) == account
        # apply modified account event
        account_event = AccountState(
            account_id=account.id,
            account_type=account.type,
            base_currency=account.base_currency,
            reported=True,  # reported
            balances=[
                AccountBalance(
                    Money(1_000_000, account.base_currency),
                    Money(100_000, account.base_currency),
                    Money(900_000, account.base_currency),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )
        account.apply(account_event)

        self.database.update_account(account)

        await eventually(
            lambda: len(self.database.load_account(account.id).events) >= 2,
            timeout=_TEST_TIMEOUT,
        )

        result = self.database.load_account(account.id)
        assert result == account

    @pytest.mark.asyncio
    async def test_update_account(self):
        # Arrange
        account = TestExecStubs.cash_account()

        self.database.add_currency(account.base_currency)
        self.database.add_account(account)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_account(account.id), timeout=_TEST_TIMEOUT)

        # Act
        self.database.update_account(account)
        await asyncio.sleep(0.5)

        # Assert
        assert self.database.load_account(account.id) == account

    ################################################################################
    # Market data
    ################################################################################
    @pytest.mark.asyncio
    async def test_add_and_load_trades(self):
        # add target instruments and currencies
        instrument = TestInstrumentProvider.ethusdt_perp_binance()
        self.database.add_currency(instrument.base_currency)
        self.database.add_currency(instrument.quote_currency)
        self.database.add_instrument(instrument)

        trade = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("1500.00"),
            size=Quantity.from_int(10),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456789"),
            ts_event=1,
            ts_init=2,
        )
        self.database.add_trade(trade)

        await eventually(
            lambda: len(self.database.load_trades(instrument.id)) > 0,
            timeout=_TEST_TIMEOUT,
        )

        trades = self.database.load_trades(instrument.id)
        assert len(trades) == 1
        target_trade = trades[0]
        assert target_trade.instrument_id == trade.instrument_id
        assert target_trade.price == trade.price
        assert target_trade.size == trade.size
        assert target_trade.aggressor_side == trade.aggressor_side
        assert target_trade.trade_id == trade.trade_id
        assert target_trade.ts_event == trade.ts_event
        assert target_trade.ts_init == trade.ts_init

    @pytest.mark.asyncio
    async def test_add_and_load_quotes(self):
        # add target instruments and currencies
        instrument = TestInstrumentProvider.ethusdt_perp_binance()
        self.database.add_currency(instrument.base_currency)
        self.database.add_currency(instrument.quote_currency)
        self.database.add_instrument(instrument)

        quote = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("1400.00"),
            ask_price=Price.from_str("1400.50"),
            bid_size=Quantity.from_int(4),
            ask_size=Quantity.from_int(5),
            ts_event=1,
            ts_init=1,
        )
        self.database.add_quote(quote)

        await eventually(
            lambda: len(self.database.load_quotes(instrument.id)) > 0,
            timeout=_TEST_TIMEOUT,
        )

        quotes = self.database.load_quotes(instrument.id)
        assert len(quotes) == 1
        target_quote = quotes[0]
        assert target_quote.instrument_id == quote.instrument_id
        assert target_quote.bid_price == quote.bid_price
        assert target_quote.bid_size == quote.bid_size
        assert target_quote.ask_price == quote.ask_price
        assert target_quote.ask_size == quote.ask_size
        assert target_quote.ts_event == quote.ts_event
        assert target_quote.ts_init == quote.ts_init

    @pytest.mark.asyncio
    async def test_add_and_load_bars(self):
        instrument = TestInstrumentProvider.ethusdt_perp_binance()
        self.database.add_currency(instrument.base_currency)
        self.database.add_currency(instrument.quote_currency)
        self.database.add_instrument(instrument)

        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
        bar_type = BarType(instrument.id, bar_spec)
        bar = Bar(
            bar_type=bar_type,
            open=Price.from_str("1500.00"),
            high=Price.from_str("1505.00"),
            low=Price.from_str("1490.00"),
            close=Price.from_str("1502.00"),
            volume=Quantity.from_int(2_000),
            ts_event=1,
            ts_init=2,
        )
        self.database.add_bar(bar)

        await eventually(
            lambda: len(self.database.load_bars(instrument.id)) > 0,
            timeout=_TEST_TIMEOUT,
        )

        bars = self.database.load_bars(instrument.id)
        assert len(bars) == 1
        target_bar = bars[0]
        assert target_bar.bar_type == bar.bar_type
        assert target_bar.open == bar.open
        assert target_bar.close == bar.close
        assert target_bar.low == bar.low
        assert target_bar.high == bar.high
        assert target_bar.volume == bar.volume
        assert target_bar.ts_init == bar.ts_init
        assert target_bar.ts_event == bar.ts_event

    @pytest.mark.asyncio
    async def test_add_and_load_signals(self):
        signal_cls = generate_signal_class("example", value_type=float)
        signal = signal_cls(value=1.0, ts_event=1, ts_init=2)
        signal_name = signal.__class__.__name__
        assert signal_name == "SignalExample"

        self.database.add_signal(signal)

        await eventually(
            lambda: len(self.database.load_signals(signal_cls, signal_name)) > 0,
            timeout=_TEST_TIMEOUT,
        )

        signals = self.database.load_signals(signal_cls, signal_name)
        assert len(signals) == 1

    @pytest.mark.skip(reason="WIP")
    @pytest.mark.asyncio
    async def test_add_and_load_custom_data(self):
        metadata = {"a": "1", "b": "2"}
        data_type = DataType(NewsEvent, metadata)
        event = NewsEvent(
            impact=NewsImpact.LOW,
            name="something-happened",
            currency="USD",
            ts_event=1,
            ts_init=2,
        )
        data = CustomData(data_type, event)

        self.database.add_custom_data(data)

        # TODO: WIP - loading needs more work
        # await eventually(lambda: len(self.database.load_custom_data(data_type)) > 0, timeout=_TEST_TIMEOUT)
        #
        # signals = self.database.load_custom_data(data_type)
        # assert len(signals) == 1
