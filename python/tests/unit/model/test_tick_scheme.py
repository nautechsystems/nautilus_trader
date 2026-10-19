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
"""
Test named tick-scheme registration, validation, and instrument rounding.
"""

from uuid import uuid4

import pytest
from tests.providers import TestInstrumentProvider

from nautilus_trader.model import FIXED_PRECISION
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import FixedTickScheme
from nautilus_trader.model import Price
from nautilus_trader.model import TieredTickScheme
from nautilus_trader.model import get_tick_scheme
from nautilus_trader.model import list_tick_schemes
from nautilus_trader.model import register_tick_scheme


def test_register_fixed_tick_scheme() -> None:
    """
    A registered fixed tick keeps its grid and output precision.
    """
    name = f"TEST_PY_FIXED_{uuid4().hex}"
    scheme = FixedTickScheme(name, price_precision=3, increment=Price.from_str("0.25"))
    register_tick_scheme(scheme)
    registered = get_tick_scheme(f" {name.lower()} ")

    assert registered.name == name.upper()
    assert registered.price_precision == 3
    assert str(registered.increment) == "0.250"
    assert str(registered.next_bid_price(1.63)) == "1.500"
    assert str(registered.next_ask_price(1.63, 1)) == "2.000"
    assert registered.next_bid_price(-1.63) == Price.from_str("-1.750")
    assert registered.next_ask_price(-1.63) == Price.from_str("-1.500")

    with pytest.raises(ValueError, match="already registered"):
        register_tick_scheme(FixedTickScheme(name.lower(), 2, Price.from_str("0.05")))

    assert str(get_tick_scheme(name).increment) == "0.250"
    names = list_tick_schemes()
    assert names == sorted(set(names))
    assert name.upper() in names


def test_register_tiered_tick_scheme() -> None:
    """
    Tier boundaries preserve exact outward rounding after registration.
    """
    name = f"TEST_PY_TIERS_{uuid4().hex}"
    scheme = TieredTickScheme(
        name,
        tiers=[(0.05, 10.00, 0.05), (10.00, float("inf"), 0.25)],
        price_precision=2,
        max_ticks_per_tier=1000,
    )
    register_tick_scheme(scheme)
    registered = get_tick_scheme(name.lower())

    assert registered.name == name.upper()
    assert registered.price_precision == 2
    assert registered.min_price == Price.from_str("0.05")
    assert registered.max_price == Price.from_str("259.75")
    assert registered.tick_count == 1199
    assert len(registered.ticks) == 1199
    assert registered.ticks[0] == Price.from_str("0.05")
    assert registered.ticks[-1] == Price.from_str("259.75")
    assert registered.next_bid_price(9.99) == Price.from_str("9.95")
    assert registered.next_ask_price(9.99) == Price.from_str("10.00")
    assert registered.next_bid_price(10.63) == Price.from_str("10.50")
    assert registered.next_ask_price(10.63) == Price.from_str("10.75")
    assert registered.next_bid_price(0.04) is None
    assert registered.next_ask_price(260.00) is None

    with pytest.raises(ValueError, match="already registered"):
        register_tick_scheme(scheme)


@pytest.mark.parametrize(
    "name",
    [
        "fixed",
        "betfair",
        "topix100",
        "crypto_0_01",
        "forex_3decimal",
        "forex_5decimal",
        "fixed_precision_01",
    ],
)
def test_builtin_tick_schemes_cannot_be_replaced(name) -> None:
    """
    Registration preserves each built-in scheme and its aliases.
    """
    with pytest.raises(ValueError, match="already registered"):
        register_tick_scheme(FixedTickScheme(name, 0))


@pytest.mark.parametrize(
    ("tick", "message"),
    [
        ("0.00", "tick must be positive"),
        ("-0.01", "tick must be positive"),
        ("0.015", "cannot be represented"),
        ("0.001", "cannot be represented"),
        ("0.050000001", "cannot be represented"),
    ],
)
def test_invalid_fixed_tick_returns_value_error(tick, message) -> None:
    """
    Invalid increments fail with the specific validation message.
    """
    with pytest.raises(ValueError, match=message):
        FixedTickScheme("INVALID_FIXED", 2, Price.from_str(tick))


@pytest.mark.parametrize("increment", [0.05, float("nan"), float("inf"), "0.05"])
def test_fixed_increment_requires_price(increment) -> None:
    """
    Fixed increments enter the scheme as exact Price values.
    """
    with pytest.raises(TypeError, match="Price"):
        FixedTickScheme("INVALID_INCREMENT_TYPE", 2, increment)


def test_fixed_tick_preserves_digits_beyond_float_accuracy() -> None:
    """
    Registration and stepping preserve every digit of the supplied increment.
    """
    name = f"TEST_PY_EXACT_FIXED_{uuid4().hex}"
    tick = Price.from_str("9000000000.000000001")
    register_tick_scheme(FixedTickScheme(name, 9, tick))
    scheme = get_tick_scheme(name)

    assert str(scheme.increment) == "9000000000.000000001"
    assert scheme.price_precision == 9
    assert str(scheme.next_ask_price(0.0, 1)) == "9000000000.000000001"
    assert str(scheme.next_bid_price(0.0, 1)) == "-9000000000.000000001"


def test_fixed_tick_accepts_lower_precision_when_representable() -> None:
    """
    Trailing zeros do not prevent using an exactly representable increment.
    """
    scheme = FixedTickScheme("TRAILING_ZERO_FIXED", 2, Price.from_str("0.250"))

    assert str(scheme.increment) == "0.25"
    assert scheme.price_precision == 2
    assert scheme.next_bid_price(1.63) == Price.from_str("1.50")
    assert scheme.next_ask_price(1.63) == Price.from_str("1.75")


@pytest.mark.parametrize("scheme_type", [FixedTickScheme, TieredTickScheme])
def test_invalid_precision_returns_value_error(scheme_type) -> None:
    """
    Both scheme types reject precision above the build limit.
    """
    kwargs = {"tiers": [(1.0, 2.0, 0.1)]} if scheme_type is TieredTickScheme else {}
    with pytest.raises(ValueError, match="precision"):
        scheme_type(name="INVALID_PRECISION", price_precision=FIXED_PRECISION + 1, **kwargs)


@pytest.mark.parametrize("name", ["", "   ", "\u00e9"])
def test_invalid_registry_name_returns_value_error(name) -> None:
    """
    The registry rejects empty and non-ASCII names.
    """
    with pytest.raises(ValueError, match="name"):
        register_tick_scheme(FixedTickScheme(name, 2))


def test_unknown_scheme_and_unsupported_registration_raise() -> None:
    """
    Lookup and registration distinguish missing names from wrong input types.
    """
    with pytest.raises(ValueError, match="unknown tick scheme"):
        get_tick_scheme("UNREGISTERED_PY_SCHEME")
    with pytest.raises(TypeError, match="FixedTickScheme or TieredTickScheme"):
        register_tick_scheme(object())


@pytest.mark.parametrize("name", ["FIXED", "BETFAIR"])
def test_negative_offsets_return_value_error(name) -> None:
    """
    Fixed and tiered schemes reject negative tick offsets.
    """
    scheme = get_tick_scheme(name)
    with pytest.raises(ValueError, match="n must be >= 0"):
        scheme.next_bid_price(2.0, -1)
    with pytest.raises(ValueError, match="n must be >= 0"):
        scheme.next_ask_price(2.0, -1)


def test_fixed_precision_builtin_at_max_precision() -> None:
    """
    The finest built-in tick uses the full build precision.
    """
    scheme = get_tick_scheme(f"fixed_precision_{FIXED_PRECISION:02}")
    assert scheme.price_precision == FIXED_PRECISION
    assert str(scheme.increment) == "0." + "0" * (FIXED_PRECISION - 1) + "1"
    assert str(scheme.next_ask_price(0.0, 2)) == "0." + "0" * (FIXED_PRECISION - 1) + "2"
    assert str(scheme.next_bid_price(0.0, 2)) == "-0." + "0" * (FIXED_PRECISION - 1) + "2"


def test_registered_scheme_is_used_by_python_instrument() -> None:
    """
    Instrument construction and navigation use the registered custom grid.
    """
    name = f"TEST_PY_INSTRUMENT_{uuid4().hex}"
    register_tick_scheme(FixedTickScheme(name, 2, Price.from_str("0.05")))
    values = TestInstrumentProvider.audusd_sim().to_dict()
    values["tick_scheme"] = name.lower()
    instrument = CurrencyPair.from_dict(values)

    assert instrument.tick_scheme == name.lower()
    assert str(instrument.next_bid_price(1.13)) == "1.10000"
    assert str(instrument.next_ask_price(1.13)) == "1.15000"
    assert instrument.to_dict()["tick_scheme"] == name.lower()


def test_registered_scheme_rejects_instrument_precision_too_coarse() -> None:
    """
    Navigation rejects an instrument precision that cannot represent its tick.
    """
    name = f"TEST_PY_FINE_GRID_{uuid4().hex}"
    register_tick_scheme(FixedTickScheme(name, 6, Price.from_str("0.000001")))
    values = TestInstrumentProvider.audusd_sim().to_dict()
    values["tick_scheme"] = name
    instrument = CurrencyPair.from_dict(values)

    assert instrument.price_precision == 5
    assert instrument.tick_scheme == name
    assert instrument.next_bid_price(1.13) is None
    assert instrument.next_ask_price(1.13) is None
