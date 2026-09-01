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
Test pricing behavior.
"""

import re

import pytest

from nautilus_trader.model import BlackScholesGreeksResult
from nautilus_trader.model import ForwardPrice
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OptionChainSlice
from nautilus_trader.model import OptionGreeks
from nautilus_trader.model import OptionSeriesId
from nautilus_trader.model import OptionStrikeData
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrikeRange
from nautilus_trader.model import Venue
from nautilus_trader.model import black_scholes_greeks
from nautilus_trader.model import imply_vol
from nautilus_trader.model import imply_vol_and_greeks
from nautilus_trader.model import refine_vol_and_greeks


def test_forward_price_properties() -> None:
    """
    Test forward price properties.
    """
    value = ForwardPrice(
        instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE"),
        forward_price="50123.4",
        underlying_index="BTCUSD",
        ts_event=7,
        ts_init=8,
    )

    assert value.instrument_id == InstrumentId.from_str("BTCUSDT.BINANCE")
    assert value.forward_price == "50123.4"
    assert value.underlying_index == "BTCUSD"
    assert value.ts_event == 7
    assert value.ts_init == 8


def test_option_series_id_from_expiry_and_from_str() -> None:
    """
    Test option series id from expiry and from str.
    """
    series_id = OptionSeriesId.from_expiry("DERIBIT", "BTC", "USD", "2024-03-29")
    restored = OptionSeriesId.from_str(series_id.value)

    assert series_id.venue.value == "DERIBIT"
    assert series_id.underlying == "BTC"
    assert series_id.settlement_currency == "USD"
    assert restored.value == series_id.value
    assert hash(restored) == hash(series_id)


@pytest.mark.parametrize(
    ("value", "expected_err"),
    [
        (
            "DERIBIT:BTC:USD",
            "invalid `OptionSeriesId` value 'DERIBIT:BTC:USD': "
            "expected format 'VENUE:UNDERLYING:SETTLEMENT:EXPIRY'",
        ),
        (
            ":BTC:USD:1700000000000000000",
            "invalid `OptionSeriesId` value ':BTC:USD:1700000000000000000': "
            "invalid venue: invalid string for 'value', was empty",
        ),
        (
            "DERIBIT:BTC:USD:not-a-date",
            "invalid `OptionSeriesId` value 'DERIBIT:BTC:USD:not-a-date': "
            "invalid expiration 'not-a-date': Invalid format: not-a-date",
        ),
    ],
)
def test_option_series_id_from_str_when_invalid(value: object, expected_err: object) -> None:
    """
    Test option series id from str when invalid.
    """
    with pytest.raises(ValueError, match=re.escape(expected_err)) as exc_info:
        OptionSeriesId.from_str(value)

    assert str(exc_info.value) == expected_err


@pytest.mark.parametrize(
    ("venue", "date_str", "expected_err"),
    [
        (
            "",
            "2024-03-29",
            "invalid `OptionSeriesId` value ':BTC:USD:2024-03-29': "
            "invalid venue: invalid string for 'value', was empty",
        ),
        (
            "DERIBIT",
            "not-a-date",
            "invalid `OptionSeriesId` value 'DERIBIT:BTC:USD:not-a-date': "
            "invalid expiration 'not-a-date': Invalid format: not-a-date",
        ),
    ],
)
def test_option_series_id_from_expiry_when_invalid(
    venue: Venue,
    date_str: object,
    expected_err: object,
) -> None:
    """
    Test option series id from expiry when invalid.
    """
    with pytest.raises(ValueError, match=re.escape(expected_err)) as exc_info:
        OptionSeriesId.from_expiry(venue, "BTC", "USD", date_str)

    assert str(exc_info.value) == expected_err


def test_option_greeks_and_strike_data_properties() -> None:
    """
    Test option greeks and strike data properties.
    """
    instrument_id = InstrumentId.from_str("BTC-20240329-50000-C.DERIBIT")
    quote = QuoteTick(
        instrument_id=instrument_id,
        bid_price=Price.from_str("100.0"),
        ask_price=Price.from_str("101.0"),
        bid_size=Quantity.from_str("2"),
        ask_size=Quantity.from_str("3"),
        ts_event=1,
        ts_init=2,
    )
    greeks = OptionGreeks(
        instrument_id=instrument_id,
        delta=0.5,
        gamma=0.1,
        vega=0.2,
        theta=-0.3,
        rho=0.05,
        mark_iv=0.6,
        bid_iv=0.55,
        ask_iv=0.65,
        underlying_price=50_000.0,
        open_interest=42.0,
        ts_event=3,
        ts_init=4,
    )
    strike = OptionStrikeData(quote, greeks)

    assert greeks.instrument_id == instrument_id
    assert greeks.delta == pytest.approx(0.5)
    assert greeks.gamma == pytest.approx(0.1)
    assert greeks.mark_iv == pytest.approx(0.6)
    assert greeks.underlying_price == pytest.approx(50_000.0)
    assert greeks.open_interest == pytest.approx(42.0)
    assert strike.quote == quote
    assert strike.greeks.instrument_id == instrument_id
    assert strike.greeks.mark_iv == pytest.approx(0.6)


def test_option_chain_slice_empty_state_and_lookups() -> None:
    """
    Test option chain slice empty state and lookups.
    """
    series_id = OptionSeriesId.from_expiry("DERIBIT", "BTC", "USD", "2024-03-29")
    chain = OptionChainSlice(
        series_id=series_id,
        atm_strike=Price.from_str("50000.0"),
        ts_event=5,
        ts_init=6,
    )

    assert chain.series_id == series_id
    assert chain.atm_strike == Price.from_str("50000.0")
    assert chain.ts_event == 5
    assert chain.ts_init == 6
    assert chain.is_empty()
    assert chain.call_count() == 0
    assert chain.put_count() == 0
    assert chain.strike_count() == 0
    assert chain.strikes() == []
    assert chain.get_call(Price.from_str("50000.0")) is None
    assert chain.get_put(Price.from_str("50000.0")) is None
    assert chain.get_call_quote(Price.from_str("50000.0")) is None
    assert chain.get_put_quote(Price.from_str("50000.0")) is None
    assert chain.get_call_greeks(Price.from_str("50000.0")) is None
    assert chain.get_put_greeks(Price.from_str("50000.0")) is None


@pytest.mark.parametrize(
    ("is_call", "price", "delta", "theta"),
    [
        (True, 10.4505767822, 0.6368305683, -0.0175606508),
        (False, 5.5735168457, -0.3631694317, -0.0045390302),
    ],
)
def test_black_scholes_greeks_result_properties(
    is_call: object,
    price: object,
    delta: object,
    theta: object,
) -> None:
    """
    Test black scholes greeks result properties.
    """
    result = black_scholes_greeks(100.0, 0.05, 0.05, 0.2, is_call, 100.0, 1.0)

    assert isinstance(result, BlackScholesGreeksResult)
    assert result.price == pytest.approx(price, abs=1e-5)
    assert result.vol == 0.2
    assert result.delta == pytest.approx(delta, abs=1e-5)
    assert result.gamma == pytest.approx(0.0187620167, abs=1e-5)
    assert result.vega == pytest.approx(0.3752403641, abs=1e-5)
    assert result.theta == pytest.approx(theta, abs=1e-5)


def test_black_scholes_greeks_itm_probability_uses_d2() -> None:
    """
    Test black scholes greeks itm probability uses d2.
    """
    call = black_scholes_greeks(100.0, 0.05, 0.05, 0.2, is_call=True, k=100.0, t=1.0)
    put = black_scholes_greeks(100.0, 0.05, 0.05, 0.2, is_call=False, k=100.0, t=1.0)

    assert 0.0 < call.itm_prob < call.delta
    assert 0.0 < put.itm_prob < 1.0
    assert call.itm_prob + put.itm_prob == pytest.approx(1.0)


def test_imply_vol_and_greeks_matches_input_price() -> None:
    """
    Test imply vol and greeks matches input price.
    """
    baseline = black_scholes_greeks(100.0, 0.01, 0.01, 0.2, is_call=True, k=100.0, t=0.5)
    implied = imply_vol_and_greeks(
        100.0,
        0.01,
        0.01,
        is_call=True,
        k=100.0,
        t=0.5,
        price=baseline.price,
    )

    assert implied.vol == pytest.approx(0.2, rel=1e-5)
    assert implied.delta == pytest.approx(baseline.delta)


def test_imply_vol_and_greeks_matches_put_price() -> None:
    """
    Test imply vol and greeks matches put price.
    """
    baseline = black_scholes_greeks(100.0, 0.05, 0.05, 0.25, is_call=False, k=105.0, t=0.5)
    implied = imply_vol_and_greeks(
        100.0,
        0.05,
        0.05,
        is_call=False,
        k=105.0,
        t=0.5,
        price=baseline.price,
    )

    assert implied.vol == pytest.approx(0.25, abs=1e-2)
    assert implied.price == pytest.approx(baseline.price, abs=1e-2)


def test_imply_vol_matches_baseline_vol() -> None:
    """
    Test imply vol matches baseline vol.
    """
    baseline = black_scholes_greeks(100.0, 0.01, 0.01, 0.2, is_call=True, k=100.0, t=0.5)
    implied_vol = imply_vol(100.0, 0.01, 0.01, is_call=True, k=100.0, t=0.5, price=baseline.price)

    assert implied_vol == pytest.approx(0.2, rel=1e-5)


def test_refine_vol_and_greeks_matches_input_price() -> None:
    """
    Test refine vol and greeks matches input price.
    """
    baseline = black_scholes_greeks(100.0, 0.01, 0.01, 0.2, is_call=True, k=100.0, t=0.5)
    refined = refine_vol_and_greeks(
        100.0,
        0.01,
        0.01,
        is_call=True,
        k=100.0,
        t=0.5,
        target_price=baseline.price,
        initial_vol=0.3,
    )

    assert refined.vol == pytest.approx(0.2, rel=2e-4)
    assert refined.price == pytest.approx(baseline.price, rel=2e-4)


@pytest.mark.parametrize(
    ("factory_name", "args"),
    [
        ("fixed", ([Price.from_str("50000"), Price.from_str("55000")],)),
        ("atm_relative", (2, 1)),
        ("atm_percent", (0.1,)),
        ("delta", (0.25, 0.05)),
    ],
)
def test_strike_range_factories(factory_name: object, args: tuple[object, ...]) -> None:
    """
    Test strike range factories.
    """
    factory = getattr(StrikeRange, factory_name)
    strike_range = factory(*args)

    assert isinstance(strike_range, StrikeRange)


def test_strike_range_delta_kind() -> None:
    """
    Test strike range delta kind.
    """
    strike_range = StrikeRange.delta(0.25, 0.05)

    assert strike_range.kind == "Delta"
