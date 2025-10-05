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

import pickle

import pytest

from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import CurrencyType
from nautilus_trader.model.currencies import register_currency
from nautilus_trader.model.objects import FIXED_PRECISION
from nautilus_trader.model.objects import Currency as CurrencyCython


AUD = Currency.from_str("AUD")
BTC = Currency.from_str("BTC")
ETH = Currency.from_str("ETH")
GBP = Currency.from_str("GBP")


class TestCurrency:
    def test_currency_with_negative_precision_raises_overflow_error(self):
        # Arrange, Act, Assert
        with pytest.raises(OverflowError):
            Currency(
                code="AUD",
                precision=-1,
                iso4217=36,
                name="Australian dollar",
                currency_type=CurrencyType.FIAT,
            )

    def test_currency_with_precision_over_maximum_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Currency(
                code="AUD",
                precision=FIXED_PRECISION + 20,
                iso4217=36,
                name="Australian dollar",
                currency_type=CurrencyType.FIAT,
            )

    def test_currency_properties(self):
        # Testing this as `code` and `precision` are being returned from Rust
        # Arrange
        currency = Currency(
            code="AUD",
            precision=2,
            iso4217=36,
            name="Australian dollar",
            currency_type=CurrencyType.FIAT,
        )

        # Act, Assert
        assert currency.code == "AUD"
        assert currency.precision == 2
        assert currency.iso4217 == 36
        assert currency.name == "Australian dollar"
        assert currency.currency_type == CurrencyType.FIAT

    def test_currency_equality(self):
        # Arrange
        currency1 = Currency(
            code="AUD",
            precision=2,
            iso4217=36,
            name="Australian dollar",
            currency_type=CurrencyType.FIAT,
        )

        currency2 = Currency(
            code="AUD",
            precision=2,
            iso4217=36,
            name="Australian dollar",
            currency_type=CurrencyType.FIAT,
        )

        currency3 = Currency(
            code="GBP",
            precision=2,
            iso4217=826,
            name="British pound",
            currency_type=CurrencyType.FIAT,
        )

        # Act, Assert
        assert currency1 == currency1
        assert currency1 == currency2
        assert currency1 != currency3

    def test_currency_hash(self):
        # Arrange
        currency = Currency(
            code="AUD",
            precision=2,
            iso4217=36,
            name="Australian dollar",
            currency_type=CurrencyType.FIAT,
        )

        # Act, Assert
        assert isinstance(hash(currency), int)
        assert hash(currency) == hash(currency)

    def test_str_repr(self):
        # Arrange
        currency = Currency(
            code="AUD",
            precision=2,
            iso4217=36,
            name="Australian dollar",
            currency_type=CurrencyType.FIAT,
        )

        # Act, Assert
        assert str(currency) == "AUD"
        assert currency.code == "AUD"
        assert currency.name == "Australian dollar"
        assert (
            repr(currency)
            == "Currency(code='AUD', precision=2, iso4217=36, name='Australian dollar', currency_type=FIAT)"
        )

    def test_currency_pickle(self):
        # Arrange
        currency = Currency(
            code="AUD",
            precision=2,
            iso4217=36,
            name="Australian dollar",
            currency_type=CurrencyType.FIAT,
        )

        # Act
        pickled = pickle.dumps(currency)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert unpickled == currency
        assert (
            repr(unpickled)
            == "Currency(code='AUD', precision=2, iso4217=36, name='Australian dollar', currency_type=FIAT)"
        )

    def test_register_adds_currency_to_internal_currency_map(self):
        # Arrange, Act
        ape_coin = Currency(
            code="APE",
            precision=8,
            iso4217=0,
            name="ApeCoin",
            currency_type=CurrencyType.CRYPTO,
        )

        Currency.register(ape_coin)
        result = Currency.from_str("APE")

        assert result == ape_coin

    def test_register_when_overwrite_false_does_not_overwrite_internal_currency_map(self):
        # Arrange, Act
        another_aud = Currency(
            code="AUD",
            precision=8,  # <-- Different precision
            iso4217=0,
            name="AUD",
            currency_type=CurrencyType.CRYPTO,
        )
        Currency.register(another_aud, overwrite=False)

        result = Currency.from_str("AUD")

        assert result.precision == 2  # Correct precision from built-in currency
        assert result.currency_type == CurrencyType.FIAT

    def test_from_internal_map_when_unknown(self):
        # Arrange, Act, Assert
        result = Currency.from_str("SOME_CURRENCY")

        # Assert
        assert result.code == "SOME_CURRENCY"
        assert result.precision == 8
        assert result.currency_type == CurrencyType.CRYPTO

    def test_from_internal_map_when_exists(self):
        # Arrange, Act
        result = Currency.from_str("AUD")

        # Assert
        assert result.code == "AUD"
        assert result.precision == 2
        assert result.iso4217 == 36
        assert result.name == "Australian dollar"
        assert result.currency_type == CurrencyType.FIAT

    def test_from_str_in_strict_mode_given_unknown_code_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Currency.from_str("SOME_CURRENCY", strict=True)

    def test_from_str_not_in_strict_mode_returns_crypto(self):
        # Arrange, Act
        result = Currency.from_str("ZXX_EXOTIC", strict=False)

        # Assert
        assert result.code == "ZXX_EXOTIC"
        assert result.precision == 8
        assert result.iso4217 == 0
        assert result.name == "ZXX_EXOTIC"
        assert result.currency_type == CurrencyType.CRYPTO

    @pytest.mark.parametrize(
        ("string", "expected"),
        [["AUD", AUD], ["GBP", GBP], ["BTC", BTC], ["ETH", ETH]],
    )
    def test_from_str(self, string, expected):
        # Arrange, Act
        result = Currency.from_str(string)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [["AUD", True], ["BTC", False], ["XAG", False]],
    )
    def test_is_fiat(self, string, expected):
        # Arrange, Act
        result = Currency.is_fiat(string)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [["BTC", True], ["AUD", False], ["XAG", False]],
    )
    def test_is_crypto(self, string, expected):
        # Arrange, Act
        result = Currency.is_crypto(string)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [["BTC", False], ["AUD", False], ["XAG", True]],
    )
    def test_is_commodity_backed(self, string, expected):
        # Arrange, Act
        result = Currency.is_commodity_backed(string)

        # Assert
        assert result == expected


def test_register_currency_helper_makes_available_to_both():
    test_currency = CurrencyCython(
        code="TESTCROSS1",
        precision=8,
        iso4217=0,
        name="Test Cross Currency 1",
        currency_type=CurrencyType.CRYPTO,
    )

    register_currency(test_currency, overwrite=True)

    cython_result = CurrencyCython.from_str("TESTCROSS1")
    assert cython_result.code == "TESTCROSS1"
    assert cython_result.precision == 8
    assert cython_result.name == "Test Cross Currency 1"

    pyo3_result = Currency.from_str("TESTCROSS1", strict=True)
    assert pyo3_result.code == "TESTCROSS1"
    assert pyo3_result.precision == 8
    assert pyo3_result.name == "Test Cross Currency 1"


def test_cython_from_str_only_registers_in_cython_map():
    cython_currency = CurrencyCython.from_str("TESTCROSS2")
    assert cython_currency.code == "TESTCROSS2"
    assert cython_currency.precision == 8

    with pytest.raises(ValueError, match="Unknown currency"):
        Currency.from_str("TESTCROSS2", strict=True)


def test_pyo3_from_str_only_registers_in_pyo3_map():
    pyo3_currency = Currency.from_str("TESTCROSS3")
    assert pyo3_currency.code == "TESTCROSS3"
    assert pyo3_currency.precision == 8

    cython_result = CurrencyCython.from_str("TESTCROSS3", strict=True)
    assert cython_result is None


def test_pyo3_register_makes_available_to_pyo3_only():
    test_currency = Currency(
        code="TESTPYO3ONLY",
        precision=6,
        iso4217=0,
        name="PyO3 Only Currency",
        currency_type=CurrencyType.CRYPTO,
    )

    Currency.register(test_currency, overwrite=True)

    pyo3_result = Currency.from_str("TESTPYO3ONLY", strict=True)
    assert pyo3_result.code == "TESTPYO3ONLY"

    cython_result = CurrencyCython.from_str("TESTPYO3ONLY")
    assert cython_result.code == "TESTPYO3ONLY"
    assert cython_result.precision == 8


def test_cython_register_makes_available_to_cython_only():
    test_currency = CurrencyCython(
        code="TESTCYTHONONLY",
        precision=6,
        iso4217=0,
        name="Cython Only Currency",
        currency_type=CurrencyType.CRYPTO,
    )

    CurrencyCython.register(test_currency, overwrite=True)

    cython_result = CurrencyCython.from_str("TESTCYTHONONLY")
    assert cython_result.code == "TESTCYTHONONLY"
    assert cython_result.precision == 6

    pyo3_result = Currency.from_str("TESTCYTHONONLY")
    assert pyo3_result.code == "TESTCYTHONONLY"
    assert pyo3_result.precision == 8
