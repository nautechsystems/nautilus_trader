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
from nautilus_trader.model import black_scholes_greeks
from nautilus_trader.model import imply_vol
from nautilus_trader.model import imply_vol_and_greeks
from nautilus_trader.model import refine_vol_and_greeks


def test_forward_price_properties():
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


def test_option_series_id_from_expiry_and_from_str():
    series_id = OptionSeriesId.from_expiry("DERIBIT", "BTC", "USD", "2024-03-29")
    restored = OptionSeriesId.from_str(series_id.value)

    assert series_id.venue.value == "DERIBIT"
    assert series_id.underlying == "BTC"
    assert series_id.settlement_currency == "USD"
    assert restored.value == series_id.value
    assert hash(restored) == hash(series_id)


def test_option_greeks_and_strike_data_properties():
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


def test_option_chain_slice_empty_state_and_lookups():
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


def test_black_scholes_greeks_result_properties():
    result = black_scholes_greeks(100.0, 0.01, 0.01, 0.2, True, 100.0, 0.5)

    assert isinstance(result, BlackScholesGreeksResult)
    assert result.price > 0.0
    assert result.vol == pytest.approx(0.2)
    assert 0.0 < result.delta < 1.0
    assert result.gamma > 0.0
    assert result.vega > 0.0
    assert 0.0 <= result.itm_prob <= 1.0


def test_imply_vol_and_greeks_matches_input_price():
    baseline = black_scholes_greeks(100.0, 0.01, 0.01, 0.2, True, 100.0, 0.5)
    implied = imply_vol_and_greeks(100.0, 0.01, 0.01, True, 100.0, 0.5, baseline.price)

    assert implied.vol == pytest.approx(0.2, rel=1e-5)
    assert implied.delta == pytest.approx(baseline.delta)


def test_imply_vol_matches_baseline_vol():
    baseline = black_scholes_greeks(100.0, 0.01, 0.01, 0.2, True, 100.0, 0.5)
    implied_vol = imply_vol(100.0, 0.01, 0.01, True, 100.0, 0.5, baseline.price)

    assert implied_vol == pytest.approx(0.2, rel=1e-5)


def test_refine_vol_and_greeks_matches_input_price():
    baseline = black_scholes_greeks(100.0, 0.01, 0.01, 0.2, True, 100.0, 0.5)
    refined = refine_vol_and_greeks(100.0, 0.01, 0.01, True, 100.0, 0.5, baseline.price, 0.3)

    assert refined.vol == pytest.approx(0.2, rel=2e-4)
    assert refined.price == pytest.approx(baseline.price, rel=2e-4)


@pytest.mark.parametrize(
    ("factory_name", "args"),
    [
        ("fixed", ([Price.from_str("50000"), Price.from_str("55000")],)),
        ("atm_relative", (2, 1)),
        ("atm_percent", (0.1,)),
    ],
)
def test_strike_range_factories(factory_name, args):
    factory = getattr(StrikeRange, factory_name)
    strike_range = factory(*args)

    assert isinstance(strike_range, StrikeRange)
