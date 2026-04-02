# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model import FIXED_PRECISION
from nautilus_trader.model import Currency
from nautilus_trader.model import CurrencyType


def test_negative_precision_raises():
    with pytest.raises(OverflowError):
        Currency(
            code="AUD",
            precision=-1,
            iso4217=36,
            name="Australian dollar",
            currency_type=CurrencyType.FIAT,
        )


def test_precision_over_maximum_raises():
    with pytest.raises(ValueError, match="precision"):
        Currency(
            code="AUD",
            precision=FIXED_PRECISION + 20,
            iso4217=36,
            name="Australian dollar",
            currency_type=CurrencyType.FIAT,
        )


def test_properties():
    currency = Currency(
        code="AUD",
        precision=2,
        iso4217=36,
        name="Australian dollar",
        currency_type=CurrencyType.FIAT,
    )

    assert currency.code == "AUD"
    assert currency.precision == 2
    assert currency.iso4217 == 36
    assert currency.name == "Australian dollar"
    assert currency.currency_type == CurrencyType.FIAT


def test_equality():
    c1 = Currency(
        code="AUD",
        precision=2,
        iso4217=36,
        name="Australian dollar",
        currency_type=CurrencyType.FIAT,
    )
    c2 = Currency(
        code="AUD",
        precision=2,
        iso4217=36,
        name="Australian dollar",
        currency_type=CurrencyType.FIAT,
    )
    c3 = Currency(
        code="GBP",
        precision=2,
        iso4217=826,
        name="British pound",
        currency_type=CurrencyType.FIAT,
    )

    assert c1 == c2
    assert c1 != c3


def test_hash():
    c1 = Currency(
        code="AUD",
        precision=2,
        iso4217=36,
        name="Australian dollar",
        currency_type=CurrencyType.FIAT,
    )
    c2 = Currency(
        code="AUD",
        precision=2,
        iso4217=36,
        name="Australian dollar",
        currency_type=CurrencyType.FIAT,
    )
    assert isinstance(hash(c1), int)
    assert hash(c1) == hash(c2)


def test_str_and_repr():
    currency = Currency(
        code="AUD",
        precision=2,
        iso4217=36,
        name="Australian dollar",
        currency_type=CurrencyType.FIAT,
    )

    assert str(currency) == "AUD"
    assert repr(currency) == (
        "Currency(code='AUD', precision=2, iso4217=36, "
        "name='Australian dollar', currency_type=FIAT)"
    )


def test_pickle_round_trip():
    currency = Currency(
        code="AUD",
        precision=2,
        iso4217=36,
        name="Australian dollar",
        currency_type=CurrencyType.FIAT,
    )
    pickled = pickle.dumps(currency)
    unpickled = pickle.loads(pickled)  # noqa: S301

    assert unpickled == currency
    assert repr(unpickled) == repr(currency)


def test_register_and_lookup():
    ape = Currency(
        code="APE",
        precision=8,
        iso4217=0,
        name="ApeCoin",
        currency_type=CurrencyType.CRYPTO,
    )
    Currency.register(ape)
    result = Currency.from_str("APE")

    assert result == ape


def test_register_overwrite_false_preserves_existing():
    another_aud = Currency(
        code="AUD",
        precision=8,
        iso4217=0,
        name="AUD",
        currency_type=CurrencyType.CRYPTO,
    )
    Currency.register(another_aud, overwrite=False)
    result = Currency.from_str("AUD")

    assert result.precision == 2
    assert result.currency_type == CurrencyType.FIAT


def test_from_str_known_currency():
    result = Currency.from_str("AUD")

    assert result.code == "AUD"
    assert result.precision == 2
    assert result.iso4217 == 36
    assert result.name == "Australian dollar"
    assert result.currency_type == CurrencyType.FIAT


def test_from_str_unknown_defaults_to_crypto():
    result = Currency.from_str("SOME_CURRENCY")

    assert result.code == "SOME_CURRENCY"
    assert result.precision == 8
    assert result.currency_type == CurrencyType.CRYPTO


def test_from_str_strict_unknown_raises():
    with pytest.raises(ValueError, match="Unknown currency"):
        Currency.from_str("SOME_CURRENCY", strict=True)


def test_from_str_not_strict_returns_crypto():
    result = Currency.from_str("ZXX_EXOTIC", strict=False)

    assert result.code == "ZXX_EXOTIC"
    assert result.precision == 8
    assert result.iso4217 == 0
    assert result.name == "ZXX_EXOTIC"
    assert result.currency_type == CurrencyType.CRYPTO


@pytest.mark.parametrize(
    ("code", "expected"),
    [
        ("AUD", Currency.from_str("AUD")),
        ("GBP", Currency.from_str("GBP")),
        ("BTC", Currency.from_str("BTC")),
        ("ETH", Currency.from_str("ETH")),
    ],
)
def test_from_str_builtins(code, expected):
    assert Currency.from_str(code) == expected


@pytest.mark.parametrize(
    ("code", "expected"),
    [("AUD", True), ("BTC", False), ("XAG", False)],
)
def test_is_fiat(code, expected):
    assert Currency.is_fiat(code) == expected


@pytest.mark.parametrize(
    ("code", "expected"),
    [("BTC", True), ("AUD", False), ("XAG", False)],
)
def test_is_crypto(code, expected):
    assert Currency.is_crypto(code) == expected


@pytest.mark.parametrize(
    ("code", "expected"),
    [("BTC", False), ("AUD", False), ("XAG", True)],
)
def test_is_commodity_backed(code, expected):
    assert Currency.is_commodity_backed(code) == expected


def test_equality_with_none():
    assert Currency.from_str("AUD") != None  # noqa: E711


def test_register_overwrite_true_replaces_existing():
    custom = Currency(
        code="AUD",
        precision=4,
        iso4217=0,
        name="Custom AUD",
        currency_type=CurrencyType.CRYPTO,
    )
    Currency.register(custom, overwrite=True)
    result = Currency.from_str("AUD")

    assert result.precision == 4
    assert result.currency_type == CurrencyType.CRYPTO

    original = Currency(
        code="AUD",
        precision=2,
        iso4217=36,
        name="Australian dollar",
        currency_type=CurrencyType.FIAT,
    )
    Currency.register(original, overwrite=True)
