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

from decimal import Decimal

import pytest

from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import option_kind_from_str
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import OptionsContract
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


provider = TestDataProvider()

AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
BTCUSDT_220325 = TestInstrumentProvider.btcusdt_future_binance()
ETHUSD_BITMEX = TestInstrumentProvider.ethusd_bitmex()
AAPL_EQUITY = TestInstrumentProvider.aapl_equity()
ES_FUTURE = TestInstrumentProvider.es_future()
AAPL_OPTION = TestInstrumentProvider.aapl_option()
NFL_INSTRUMENT = TestInstrumentProvider.betting_instrument()


class TestInstrument:
    @pytest.mark.parametrize(
        ("instrument1", "instrument2", "expected1", "expected2"),
        [
            [AUDUSD_SIM, AUDUSD_SIM, True, False],
            [AUDUSD_SIM, USDJPY_SIM, False, True],
        ],
    )
    def test_equality(self, instrument1, instrument2, expected1, expected2):
        # Arrange, Act
        result1 = instrument1 == instrument2
        result2 = instrument1 != instrument2

        # Assert
        assert result1 == expected1
        assert result2 == expected2

    def test_str_repr_returns_expected(self):
        # Arrange, Act, Assert
        expected = provider.read("binance-btcusdt-instrument-repr.txt").decode()
        assert str(BTCUSDT_BINANCE) == expected
        assert repr(BTCUSDT_BINANCE) == expected

    def test_hash(self):
        # Arrange, Act, Assert
        assert isinstance(hash(BTCUSDT_BINANCE), int)
        assert hash(BTCUSDT_BINANCE), hash(BTCUSDT_BINANCE)

    def test_symbol_returns_expected_symbol(self):
        # Arrange, Act, Assert
        assert BTCUSDT_BINANCE.symbol == BTCUSDT_BINANCE.id.symbol

    def test_base_to_dict_returns_expected_dict(self):
        # Arrange, Act
        result = Instrument.base_to_dict(BTCUSDT_BINANCE)

        # Assert
        assert result == {
            "type": "Instrument",
            "id": "BTCUSDT.BINANCE",
            "native_symbol": "BTCUSDT",
            "asset_class": "CRYPTOCURRENCY",
            "asset_type": "SPOT",
            "quote_currency": "USDT",
            "is_inverse": False,
            "price_precision": 2,
            "price_increment": "0.01",
            "size_precision": 6,
            "size_increment": "0.000001",
            "multiplier": "1",
            "lot_size": None,
            "max_quantity": "9000.000000",
            "min_quantity": "0.000001",
            "max_notional": None,
            "min_notional": "10.00000000 USDT",
            "max_price": "1000000.00",
            "min_price": "0.01",
            "margin_init": "0",
            "margin_maint": "0",
            "maker_fee": "0.001",
            "taker_fee": "0.001",
            "ts_event": 0,
            "ts_init": 0,
            "info": None,
        }

    def test_base_from_dict_returns_expected_instrument(self):
        # Arrange
        values = {
            "type": "Instrument",
            "id": "BTCUSDT.BINANCE",
            "native_symbol": "BTCUSDT",
            "asset_class": "CRYPTOCURRENCY",
            "asset_type": "SPOT",
            "quote_currency": "USDT",
            "is_inverse": False,
            "price_precision": 2,
            "price_increment": "0.01",
            "size_precision": 6,
            "size_increment": "0.000001",
            "multiplier": "1",
            "lot_size": None,
            "max_quantity": "9000.000000",
            "min_quantity": "0.000001",
            "max_notional": None,
            "min_notional": "10.00000000 USDT",
            "max_price": "1000000.00",
            "min_price": "0.01",
            "margin_init": "0",
            "margin_maint": "0",
            "maker_fee": "0.001",
            "taker_fee": "0.001",
            "ts_event": 0,
            "ts_init": 0,
            "info": None,
        }

        # Act
        result = Instrument.base_from_dict(values)

        # Assert
        assert result == BTCUSDT_BINANCE

    def test_crypto_perpetual_instrument_to_dict(self):
        # Arrange, Act
        result = CryptoPerpetual.to_dict(XBTUSD_BITMEX)

        # Assert
        assert CryptoPerpetual.from_dict(result) == XBTUSD_BITMEX
        assert result == {
            "type": "CryptoPerpetual",
            "id": "BTC/USD.BITMEX",
            "native_symbol": "XBTUSD",
            "base_currency": "BTC",
            "quote_currency": "USD",
            "settlement_currency": "BTC",
            "is_inverse": True,
            "price_precision": 1,
            "price_increment": "0.5",
            "size_precision": 0,
            "size_increment": "1",
            "max_quantity": None,
            "min_quantity": None,
            "max_notional": "10_000_000.00 USD",
            "min_notional": "1.00 USD",
            "max_price": "1000000.0",
            "min_price": "0.5",
            "margin_init": "0.01",
            "margin_maint": "0.0035",
            "maker_fee": "-0.00025",
            "taker_fee": "0.00075",
            "ts_event": 0,
            "ts_init": 0,
            "info": None,
        }

    def test_crypto_future_instrument_to_dict(self):
        # Arrange, Act
        result = CryptoFuture.to_dict(BTCUSDT_220325)

        # Assert
        assert CryptoFuture.from_dict(result) == BTCUSDT_220325
        assert result == {
            "type": "CryptoFuture",
            "id": "BTCUSDT_220325.BINANCE",
            "native_symbol": "BTCUSDT",
            "underlying": "BTC",
            "quote_currency": "USDT",
            "settlement_currency": "USDT",
            "expiry_date": "2022-03-25",
            "price_precision": 2,
            "price_increment": "0.01",
            "size_precision": 6,
            "size_increment": "0.000001",
            "max_quantity": "9000.000000",
            "min_quantity": "0.000001",
            "max_notional": None,
            "min_notional": "10.00000000 USDT",
            "max_price": "1000000.00",
            "min_price": "0.01",
            "margin_init": "0",
            "margin_maint": "0",
            "maker_fee": "0.001",
            "taker_fee": "0.001",
            "ts_event": 0,
            "ts_init": 0,
            "info": None,
        }

    def test_equity_instrument_to_dict(self):
        # Arrange, Act
        result = Equity.to_dict(AAPL_EQUITY)

        # Assert
        assert Equity.from_dict(result) == AAPL_EQUITY
        assert result == {
            "type": "Equity",
            "id": "AAPL.NASDAQ",
            "native_symbol": "AAPL",
            "currency": "USD",
            "price_precision": 2,
            "price_increment": "0.01",
            "size_precision": 0,
            "size_increment": "1",
            "multiplier": "1",
            "lot_size": "1",
            "isin": "US0378331005",
            "margin_init": "0",
            "margin_maint": "0",
            "maker_fee": "0",
            "taker_fee": "0",
            "ts_event": 0,
            "ts_init": 0,
        }

    def test_future_instrument_to_dict(self):
        # Arrange, Act
        result = FuturesContract.to_dict(ES_FUTURE)

        # Assert
        assert FuturesContract.from_dict(result) == ES_FUTURE
        assert result == {
            "asset_class": "INDEX",
            "currency": "USD",
            "expiry_date": "2021-12-17",
            "id": "ESZ21.CME",
            "lot_size": "1",
            "margin_init": "0",
            "margin_maint": "0",
            "multiplier": "1",
            "native_symbol": "ESZ21",
            "price_increment": "0.01",
            "price_precision": 2,
            "size_increment": "1",
            "size_precision": 0,
            "ts_event": 0,
            "ts_init": 0,
            "type": "FuturesContract",
            "underlying": "ES",
        }

    def test_option_instrument_to_dict(self):
        # Arrange, Act
        result = OptionsContract.to_dict(AAPL_OPTION)

        # Assert
        assert OptionsContract.from_dict(result) == AAPL_OPTION
        assert result == {
            "asset_class": "EQUITY",
            "currency": "USD",
            "expiry_date": "2021-12-17",
            "id": "AAPL211217C00150000.OPRA",
            "kind": "CALL",
            "lot_size": "1",
            "margin_init": "0",
            "margin_maint": "0",
            "multiplier": "100",
            "native_symbol": "AAPL211217C00150000",
            "price_increment": "0.01",
            "price_precision": 2,
            "size_increment": "1",
            "size_precision": 0,
            "strike_price": "149.00",
            "ts_event": 0,
            "ts_init": 0,
            "type": "OptionsContract",
            "underlying": "AAPL",
        }

    @pytest.mark.parametrize(
        ("value", "expected_str"),
        [
            [0, "0.00000"],
            [1, "1.00000"],
            [1.23456, "1.23456"],
            [1.234567, "1.23457"],  # <-- rounds to precision
            ["0", "0.00000"],
            ["1.00", "1.00000"],
            [Decimal(0), "0.00000"],
            [Decimal(1), "1.00000"],
            [Decimal("0.85"), "0.85000"],
        ],
    )
    def test_make_price_with_various_values_returns_expected(
        self,
        value,
        expected_str,
    ):
        # Arrange, Act
        price = AUDUSD_SIM.make_price(value)

        # Assert
        assert str(price) == expected_str

    @pytest.mark.parametrize(
        ("value", "expected_str"),
        [
            [0, "0.000000"],
            [1, "1.000000"],
            [1.23456, "1.234560"],
            [1.2345678, "1.234568"],  # <-- rounds to precision
            ["0", "0.000000"],
            ["1.00", "1.000000"],
            [Decimal(0), "0.000000"],
            [Decimal(1), "1.000000"],
            [Decimal("0.85"), "0.850000"],
        ],
    )
    def test_make_qty_with_various_values_returns_expected(
        self,
        value,
        expected_str,
    ):
        # Arrange, Act
        qty = BTCUSDT_BINANCE.make_qty(value)

        # Assert
        assert str(qty) == expected_str

    @pytest.mark.parametrize(
        ("instrument", "expected"),
        [
            [AUDUSD_SIM, AUD],
            [BTCUSDT_BINANCE, BTC],
            [XBTUSD_BITMEX, BTC],
            [ETHUSD_BITMEX, ETH],
        ],
    )
    def test_base_currency_for_various_instruments(self, instrument, expected):
        # Arrange, Act, Asset
        assert instrument.get_base_currency() == expected

    @pytest.mark.parametrize(
        ("instrument", "expected"),
        [
            [AUDUSD_SIM, USD],
            [BTCUSDT_BINANCE, USDT],
            [XBTUSD_BITMEX, BTC],
            [ETHUSD_BITMEX, ETH],
        ],
    )
    def test_cost_currency_for_various_instruments(self, instrument, expected):
        # Arrange, Act, Asset
        assert instrument.get_settlement_currency() == expected

    def test_calculate_notional_value(self):
        # Arrange
        instrument = TestInstrumentProvider.btcusdt_binance()

        # Act
        result = instrument.notional_value(
            Quantity.from_int(10),
            Price.from_str("11493.60"),
        )

        # Assert
        assert result == Money(114936.00000000, USDT)

    @pytest.mark.parametrize(
        ("inverse_as_quote", "expected"),
        [
            [False, Money(8.70049419, BTC)],
            [True, Money(100000.00, USD)],
        ],
    )
    def test_calculate_notional_value_for_inverse(self, inverse_as_quote, expected):
        # Arrange
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = instrument.notional_value(
            Quantity.from_int(100_000),
            Price.from_str("11493.60"),
            inverse_as_quote=inverse_as_quote,
        )

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("instrument", "value", "n", "expected"),
        [
            (AUDUSD_SIM, 0.720006, 0, "0.72001"),
            (AUDUSD_SIM, 0.900001, 0, "0.90001"),
        ],
    )
    def test_next_ask_price(self, instrument, value, n, expected):
        result = instrument.next_ask_price(value, num_ticks=n)
        expected = Price.from_str(expected)
        assert result == expected

    @pytest.mark.parametrize(
        ("instrument", "value", "n", "expected"),
        [
            (AUDUSD_SIM, 0.7200006, 0, "0.72000"),
            (AUDUSD_SIM, 0.9000001, 0, "0.90000"),
        ],
    )
    def test_next_bid_price(self, instrument, value, n, expected):
        result = instrument.next_bid_price(value, num_ticks=n)
        expected = Price.from_str(expected)
        assert result == expected

    def test_option_attributes(self):
        assert AAPL_OPTION.underlying == "AAPL"
        assert AAPL_OPTION.kind == option_kind_from_str("CALL")


class TestBettingInstrument:
    def setup(self):
        self.instrument = TestInstrumentProvider.betting_instrument()

    def test_notional_value(self):
        notional = self.instrument.notional_value(
            quantity=Quantity.from_int(100),
            price=Price.from_str("0.5"),
            inverse_as_quote=False,
        ).as_decimal()
        # We are long 100 at 0.5 probability, aka 2.0 in odds terms
        assert notional == Decimal("200.0")

    @pytest.mark.parametrize(
        ("value", "n", "expected"),
        [
            (101, 0, "110"),
        ],
    )
    def test_next_ask_price(self, value, n, expected):
        result = self.instrument.next_ask_price(value, num_ticks=n)
        expected = Price.from_str(expected)
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "n", "expected"),
        [
            (1.999, 0, "1.99"),
        ],
    )
    def test_next_bid_price(self, value, n, expected):
        result = self.instrument.next_bid_price(value, num_ticks=n)
        expected = Price.from_str(expected)
        assert result == expected

    def test_min_max_price(self):
        assert self.instrument.min_price == Price.from_str("1.01")
        assert self.instrument.max_price == Price.from_str("1000")
