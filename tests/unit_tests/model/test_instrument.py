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

from decimal import Decimal

import pytest

from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import option_kind_from_str
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


provider = TestDataProvider()

AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
BTCUSDT_220325 = TestInstrumentProvider.btcusdt_future_binance()
ETHUSD_BITMEX = TestInstrumentProvider.ethusd_bitmex()
AAPL_EQUITY = TestInstrumentProvider.equity(symbol="AAPL", venue="XNAS")
ES_FUTURE = TestInstrumentProvider.es_future(expiry_year=2023, expiry_month=12)
AAPL_OPTION = TestInstrumentProvider.aapl_option()
BETTING_INSTRUMENT = TestInstrumentProvider.betting_instrument()


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
        expected = provider.read("binance/btcusdt-instrument-repr.txt").decode()
        assert str(BTCUSDT_BINANCE) + "\n" == expected
        assert repr(BTCUSDT_BINANCE) + "\n" == expected

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
            "raw_symbol": "BTCUSDT",
            "asset_class": "CRYPTOCURRENCY",
            "instrument_class": "SPOT",
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
            "tick_scheme_name": None,
            "info": None,
        }

    def test_base_from_dict_returns_expected_instrument(self):
        # Arrange
        values = {
            "type": "Instrument",
            "id": "BTCUSDT.BINANCE",
            "raw_symbol": "BTCUSDT",
            "asset_class": "CRYPTOCURRENCY",
            "instrument_class": "SPOT",
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
            "tick_scheme_name": None,
            "info": None,
        }

        # Act
        result = Instrument.base_from_dict(values)

        # Assert
        assert result == BTCUSDT_BINANCE

    def test_betting_instrument_to_dict(self):
        # Arrange, Act
        result = BettingInstrument.to_dict(BETTING_INSTRUMENT)

        # Assert
        assert BettingInstrument.from_dict(result) == BETTING_INSTRUMENT
        assert result == {
            "type": "BettingInstrument",
            "id": "1-123456789-50214-None.BETFAIR",
            "raw_symbol": "1-123456789-50214-None",
            "venue_name": "BETFAIR",
            "event_type_id": 6423,
            "event_type_name": "American Football",
            "competition_id": 12282733,
            "competition_name": "NFL",
            "event_id": 29678534,
            "event_name": "NFL",
            "event_country_code": "GB",
            "event_open_date": 1644276600000000000,
            "betting_type": "ODDS",
            "market_id": "1-123456789",
            "market_name": "AFC Conference Winner",
            "market_type": "SPECIAL",
            "market_start_time": 1644276600000000000,
            "selection_id": 50214,
            "selection_name": "Kansas City Chiefs",
            "selection_handicap": -9999999.0,
            "price_precision": 2,
            "size_precision": 2,
            "price_increment": "0.01",
            "size_increment": "0.01",
            "currency": "GBP",
            "maker_fee": "0",
            "taker_fee": "0",
            "margin_init": "1",
            "margin_maint": "1",
            "max_quantity": None,
            "min_quantity": None,
            "max_notional": None,
            "min_notional": "1.00 GBP",
            "max_price": None,
            "min_price": None,
            "ts_event": 0,
            "ts_init": 0,
            "info": {},
            "tick_scheme_name": None,
        }

    def test_crypto_perpetual_instrument_to_dict(self):
        # Arrange, Act
        result = CryptoPerpetual.to_dict(XBTUSD_BITMEX)

        # Assert
        assert CryptoPerpetual.from_dict(result) == XBTUSD_BITMEX
        assert result == {
            "type": "CryptoPerpetual",
            "id": "BTC/USD.BITMEX",
            "raw_symbol": "XBTUSD",
            "base_currency": "BTC",
            "quote_currency": "USD",
            "settlement_currency": "BTC",
            "is_inverse": True,
            "price_precision": 1,
            "price_increment": "0.5",
            "size_precision": 0,
            "size_increment": "1",
            "multiplier": "1",
            "lot_size": "1",
            "max_quantity": None,
            "min_quantity": None,
            "max_notional": "10000000.00 USD",
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
            "tick_scheme_name": None,
        }

    def test_crypto_future_instrument_to_dict(self):
        # Arrange, Act
        result = CryptoFuture.to_dict(BTCUSDT_220325)

        # Assert
        assert CryptoFuture.from_dict(result) == BTCUSDT_220325
        assert result == {
            "type": "CryptoFuture",
            "id": "BTCUSDT_220325.BINANCE",
            "raw_symbol": "BTCUSDT",
            "underlying": "BTC",
            "quote_currency": "USDT",
            "settlement_currency": "USDT",
            "is_inverse": False,
            "activation_ns": 1640390400000000000,
            "expiration_ns": 1648166400000000000,
            "price_precision": 2,
            "price_increment": "0.01",
            "size_precision": 6,
            "size_increment": "0.000001",
            "multiplier": "1",
            "lot_size": "1",
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
            "tick_scheme_name": None,
        }

    def test_equity_instrument_to_dict(self):
        # Arrange, Act
        result = Equity.to_dict(AAPL_EQUITY)

        # Assert
        assert Equity.from_dict(result) == AAPL_EQUITY
        assert result == {
            "type": "Equity",
            "id": "AAPL.XNAS",
            "raw_symbol": "AAPL",
            "currency": "USD",
            "price_precision": 2,
            "price_increment": "0.01",
            "lot_size": "100",
            "max_price": None,
            "max_quantity": None,
            "min_price": None,
            "min_quantity": None,
            "isin": "US0378331005",
            "margin_init": "0",
            "margin_maint": "0",
            "maker_fee": "0",
            "taker_fee": "0",
            "ts_event": 0,
            "ts_init": 0,
            "info": None,
            "tick_scheme_name": None,
        }

    def test_future_instrument_to_dict(self):
        # Arrange, Act
        result = FuturesContract.to_dict(ES_FUTURE)

        # Assert
        assert FuturesContract.from_dict(result) == ES_FUTURE
        assert result == {
            "type": "FuturesContract",
            "id": "ESZ3.GLBX",
            "raw_symbol": "ESZ3",
            "asset_class": "INDEX",
            "exchange": "XCME",
            "underlying": "ES",
            "currency": "USD",
            "activation_ns": 1622842200000000000,
            "expiration_ns": 1702650600000000000,
            "max_price": None,
            "max_quantity": None,
            "min_price": None,
            "min_quantity": "1",
            "lot_size": "1",
            "margin_init": "0",
            "margin_maint": "0",
            "multiplier": "1",
            "price_increment": "0.25",
            "price_precision": 2,
            "size_increment": "1",
            "size_precision": 0,
            "maker_fee": "0",
            "taker_fee": "0",
            "ts_event": 1622842200000000000,
            "ts_init": 1622842200000000000,
            "info": None,
            "tick_scheme_name": None,
        }

    def test_option_instrument_to_dict(self):
        # Arrange, Act
        result = OptionContract.to_dict(AAPL_OPTION)

        # Assert
        assert OptionContract.from_dict(result) == AAPL_OPTION
        assert result == {
            "type": "OptionContract",
            "id": "AAPL211217C00150000.OPRA",
            "raw_symbol": "AAPL211217C00150000",
            "asset_class": "EQUITY",
            "exchange": "GMNI",
            "underlying": "AAPL",
            "currency": "USD",
            "activation_ns": 1631836800000000000,
            "expiration_ns": 1639699200000000000,
            "option_kind": "CALL",
            "lot_size": "1",
            "max_price": None,
            "max_quantity": None,
            "min_price": None,
            "min_quantity": "1",
            "margin_init": "0",
            "margin_maint": "0",
            "multiplier": "100",
            "price_increment": "0.01",
            "price_precision": 2,
            "size_increment": "1",
            "size_precision": 0,
            "strike_price": "149.00",
            "maker_fee": "0",
            "taker_fee": "0",
            "ts_event": 0,
            "ts_init": 0,
            "info": None,
            "tick_scheme_name": None,
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

    def test_make_qty_when_non_zero_value_rounded_to_zero_raises_exception(self):
        # Arrange
        onethousandrats = TestInstrumentProvider.onethousandrats_perp_binance()

        # Act, Assert
        with pytest.raises(ValueError):
            onethousandrats.make_qty(0.004)

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
        ("value", "round_down", "expected_str"),
        [
            # Test cases with round_down=True
            [1.2345678, True, "1.234567"],  # Rounds down to precision 6
            [1.5000009, True, "1.500000"],  # Rounds down to precision 6
            [2.5000009, True, "2.500000"],  # Rounds down to precision 6
            [2.9999999, True, "2.999999"],  # Rounds down to precision 6
            # Comparison cases with default rounding (round_down=False)
            [1.2345678, False, "1.234568"],  # Standard rounding behavior
            [1.5000005, False, "1.500001"],  # Standard rounding behavior
            [2.5000005, False, "2.500001"],  # Standard rounding behavior
            [2.9999995, False, "2.999999"],  # Corrected expected value
            # Edge cases - using values that won't trigger the zero validation error
            [0.000001, True, "0.000001"],  # Smallest representable value
            [0.000002, True, "0.000002"],  # Small value that won't round to zero
            [0.0000015, False, "0.000002"],  # Small value with standard rounding
        ],
    )
    def test_make_qty_with_round_down_parameter(
        self,
        value,
        round_down,
        expected_str,
    ):
        # Arrange
        instrument = TestInstrumentProvider.btcusdt_binance()

        # Act
        qty = instrument.make_qty(value, round_down=round_down)

        # Assert
        assert str(qty) == expected_str

        @pytest.mark.parametrize(
            ("value", "expected"),
            [
                [0.0000501, Decimal("0.0000500")],
                [0.0000540, Decimal("0.0000500")],
                [0.0000550, Decimal("0.0000600")],
            ],
        )
        def test_make_price_with_lower_precision_minimum_increment(
            self,
            value: float,
            expected: Decimal,
        ) -> None:
            # Arrange
            onethousandrats = TestInstrumentProvider.onethousandrats_perp_binance()

            # Act
            price = onethousandrats.make_price(value)

            # Assert
            assert price == expected

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
            [ETHUSD_BITMEX, BTC],  # Quanto
        ],
    )
    def test_settlement_currency_for_various_instruments(self, instrument, expected):
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
        ("use_quote_for_inverse", "expected"),
        [
            [False, Money(8.70049419, BTC)],
            [True, Money(100000.00, USD)],
        ],
    )
    def test_calculate_notional_value_for_inverse(self, use_quote_for_inverse, expected):
        # Arrange
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = instrument.notional_value(
            Quantity.from_int(100_000),
            Price.from_str("11493.60"),
            use_quote_for_inverse=use_quote_for_inverse,
        )

        # Assert
        assert result == expected

    def test_calculate_base_quantity_audusd(self):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")

        # Act
        result = instrument.calculate_base_quantity(
            quantity=Quantity.from_str("1000"),
            last_px=Price.from_str("0.80"),
        )

        # Assert
        assert result == Quantity.from_str("1250")

    def test_next_bid_price_when_no_tick_scheme(self):
        # Arrange, Act
        instrument = TestInstrumentProvider.btcusdt_binance()

        with pytest.raises(ValueError) as exc_info:
            instrument.next_bid_price(100_000.0)

        # Assert
        assert (
            str(exc_info.value)
            == "No tick scheme for instrument BTCUSDT.BINANCE. You can specify a tick scheme by passing a `tick_scheme_name` at initialization."
        )

    def test_next_ask_price_when_no_tick_scheme(self):
        # Arrange, Act
        instrument = TestInstrumentProvider.btcusdt_binance()

        with pytest.raises(ValueError) as exc_info:
            instrument.next_ask_price(100_000.0)

        # Assert
        assert (
            str(exc_info.value)
            == "No tick scheme for instrument BTCUSDT.BINANCE. You can specify a tick scheme by passing a `tick_scheme_name` at initialization."
        )

    def test_set_tick_scheme_name_enables_tick_scheme(self):
        # Arrange
        instrument = TestInstrumentProvider.btcusdt_binance()

        with pytest.raises(ValueError):
            instrument.next_bid_price(100.123456)

        tick_scheme_name = "FOREX_5DECIMAL"

        # Act
        instrument.set_tick_scheme(tick_scheme_name)
        result = instrument.next_bid_price(100.123456)

        # Assert
        assert instrument.tick_scheme_name == tick_scheme_name
        assert result == Price.from_str("100.12345")

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
        assert AAPL_OPTION.option_kind == option_kind_from_str("CALL")

    def test_next_bid_prices_when_no_tick_scheme(self):
        # Arrange, Act
        instrument = TestInstrumentProvider.btcusdt_binance()

        with pytest.raises(ValueError) as exc_info:
            instrument.next_bid_prices(100_000.0)

        # Assert
        assert (
            str(exc_info.value)
            == "No tick scheme for instrument BTCUSDT.BINANCE. You can specify a tick scheme by passing a `tick_scheme_name` at initialization."
        )

    def test_next_ask_prices_when_no_tick_scheme(self):
        # Arrange, Act
        instrument = TestInstrumentProvider.btcusdt_binance()

        with pytest.raises(ValueError) as exc_info:
            instrument.next_ask_prices(100_000.0)

        # Assert
        assert (
            str(exc_info.value)
            == "No tick scheme for instrument BTCUSDT.BINANCE. You can specify a tick scheme by passing a `tick_scheme_name` at initialization."
        )

    @pytest.mark.parametrize(
        ("instrument", "value", "num_ticks", "expected_len"),
        [
            (AUDUSD_SIM, 0.72001, 5, 5),
            (AUDUSD_SIM, 0.90001, 3, 3),
            (AUDUSD_SIM, 0.72001, 0, 0),
        ],
    )
    def test_next_ask_prices_length(self, instrument, value, num_ticks, expected_len):
        # Arrange, Act
        result = instrument.next_ask_prices(value, num_ticks=num_ticks)

        # Assert
        assert len(result) == expected_len
        if expected_len > 0:
            assert isinstance(result[0], Decimal)

    @pytest.mark.parametrize(
        ("instrument", "value", "num_ticks", "expected_len"),
        [
            (AUDUSD_SIM, 0.72000, 5, 5),
            (AUDUSD_SIM, 0.90000, 3, 3),
            (AUDUSD_SIM, 0.72000, 0, 0),
        ],
    )
    def test_next_bid_prices_length(self, instrument, value, num_ticks, expected_len):
        # Arrange, Act
        result = instrument.next_bid_prices(value, num_ticks=num_ticks)

        # Assert
        assert len(result) == expected_len
        if expected_len > 0:
            assert isinstance(result[0], Decimal)

    @pytest.mark.parametrize(
        ("instrument", "value", "num_ticks", "expected_first", "expected_last"),
        [
            (AUDUSD_SIM, 0.72000, 10, "0.72", "0.72009"),
            (AUDUSD_SIM, 0.72001, 5, "0.72001", "0.72005"),  # Price-inclusive: start at boundary
            (AUDUSD_SIM, 0.90001, 3, "0.90001", "0.90003"),
        ],
    )
    def test_next_ask_prices_values(
        self,
        instrument,
        value,
        num_ticks,
        expected_first,
        expected_last,
    ):
        # Arrange, Act
        result = instrument.next_ask_prices(value, num_ticks=num_ticks)

        # Assert
        assert len(result) == num_ticks
        assert str(result[0]) == expected_first
        assert str(result[-1]) == expected_last
        # Check that prices are increasing
        for i in range(1, len(result)):
            assert result[i] > result[i - 1]

    @pytest.mark.parametrize(
        ("instrument", "value", "num_ticks", "expected_first", "expected_last"),
        [
            (AUDUSD_SIM, 0.72000, 10, "0.72", "0.71991"),  # Price-inclusive: start at boundary
            (AUDUSD_SIM, 0.72000, 5, "0.72", "0.71996"),  # Price-inclusive: start at boundary
            (AUDUSD_SIM, 0.90000, 3, "0.9", "0.89998"),
        ],
    )
    def test_next_bid_prices_values(
        self,
        instrument,
        value,
        num_ticks,
        expected_first,
        expected_last,
    ):
        # Arrange, Act
        result = instrument.next_bid_prices(value, num_ticks=num_ticks)

        # Assert
        assert len(result) == num_ticks
        assert str(result[0]) == expected_first
        assert str(result[-1]) == expected_last
        # Check that prices are decreasing
        for i in range(1, len(result)):
            assert result[i] < result[i - 1]


def test_pyo3_equity_to_legacy_equity() -> None:
    # Arrange
    pyo3_instrument = TestInstrumentProviderPyo3.aapl_equity()

    # Act
    instrument = Equity.from_dict(pyo3_instrument.to_dict())

    # Assert
    assert isinstance(instrument, Equity)
    assert instrument.id == InstrumentId.from_str("AAPL.XNAS")


def test_pyo3_future_to_legacy_future() -> None:
    # Arrange
    pyo3_instrument = TestInstrumentProviderPyo3.futures_contract_es()

    # Act
    instrument = FuturesContract.from_dict(pyo3_instrument.to_dict())

    # Assert
    assert isinstance(instrument, FuturesContract)
    assert instrument.id == InstrumentId.from_str("ESZ21.GLBX")


def test_pyo3_option_to_legacy_option() -> None:
    # Arrange
    pyo3_instrument = TestInstrumentProviderPyo3.aapl_option()

    # Act
    instrument = OptionContract.from_dict(pyo3_instrument.to_dict())

    # Assert
    assert isinstance(instrument, OptionContract)
    assert instrument.id == InstrumentId.from_str("AAPL211217C00150000.OPRA")


def test_binary_option_dict_round_trip() -> None:
    # Arrange
    instrument = TestInstrumentProvider.binary_option()

    # Act
    from_dict = BinaryOption.from_dict(BinaryOption.to_dict(instrument))

    # Assert
    assert instrument == from_dict


def test_betting_instrument_dict_round_trip() -> None:
    # Arrange
    instrument = TestInstrumentProvider.betting_instrument()

    # Act
    from_dict = BettingInstrument.from_dict(BettingInstrument.to_dict(instrument))

    # Assert
    assert instrument == from_dict
