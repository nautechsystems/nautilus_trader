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
Test providers behavior.
"""

import io
from decimal import Decimal
from pathlib import Path

import pytest

from nautilus_trader.model import BarType
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.testkit import providers
from nautilus_trader.testkit.providers import TEST_DATA_DIR
from nautilus_trader.testkit.providers import TestDataProvider
from nautilus_trader.testkit.providers import TestInstrumentProvider


def test_xbtusd_bitmex_matches_rust_fixture() -> None:
    """
    Test xbtusd bitmex matches rust fixture.
    """
    # Mirrors the `xbtusd_bitmex` fixture in `crates/model/src/instruments/stubs.rs`
    instrument = TestInstrumentProvider.xbtusd_bitmex()

    assert instrument.id == InstrumentId.from_str("BTCUSDT.BITMEX")
    assert instrument.raw_symbol == Symbol("XBTUSD")
    assert instrument.base_currency == Currency.from_str("BTC")
    assert instrument.quote_currency == Currency.from_str("USD")
    assert instrument.settlement_currency == Currency.from_str("BTC")
    assert instrument.is_inverse
    assert instrument.price_precision == 1
    assert instrument.size_precision == 0
    assert instrument.price_increment == Price.from_str("0.5")
    assert instrument.size_increment == Quantity.from_str("1")
    assert instrument.max_price == Price.from_str("10000000")
    assert instrument.min_price == Price.from_str("0.01")
    assert instrument.margin_init == Decimal("0.01")
    assert instrument.margin_maint == Decimal("0.0035")
    assert instrument.maker_fee == Decimal("-0.00025")
    assert instrument.taker_fee == Decimal("0.00075")


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        pytest.param("2020-01-31 20:59:54.501000+00:00", 1580504394_501_000_000, id="millisecond"),
        pytest.param("2020-01-31 06:39:00.941000+00:00", 1580452740_941_000_000, id="offset"),
        pytest.param("2019-01-02T00:00:00Z", 1546387200_000_000_000, id="zulu-whole-second"),
        pytest.param("2019-01-02 00:00:00", 1546387200_000_000_000, id="naive-treated-as-utc"),
        pytest.param("2020-01-31 20:59:54.000001+00:00", 1580504394_000_001_000, id="microsecond"),
    ],
)
def test_parse_iso_to_ns_is_exact(value: str, expected: int) -> None:
    """
    Test timestamp parsing keeps nanosecond precision.
    """
    # Scaling float seconds by 1e9 loses precision, e.g. `.501` becomes `.500999936`
    assert providers._parse_iso_to_ns(value) == expected


def _load_all_csv_loaders() -> dict[str, object]:
    audusd = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    gbpusd = TestInstrumentProvider.default_fx_ccy("GBP/USD")
    ethusdt = TestInstrumentProvider.ethusdt_binance()
    btcusdt = TestInstrumentProvider.btcusdt_binance()

    return {
        "read_csv": TestDataProvider().read_csv("short-term-interest.csv").shape,
        "quotes_from_truefx_csv": TestDataProvider.quotes_from_truefx_csv(
            audusd,
            "truefx/usdjpy-ticks.csv",
            max_rows=5,
        ),
        "quotes_from_fxcm_bars": TestDataProvider.quotes_from_fxcm_bars(
            gbpusd,
            "fxcm/gbpusd-m1-bid-2012.csv",
            "fxcm/gbpusd-m1-ask-2012.csv",
            max_rows=5,
        ),
        "bars_from_fxcm_bars": TestDataProvider.bars_from_fxcm_bars(
            gbpusd,
            BarType.from_str("GBP/USD.SIM-1-MINUTE-BID-EXTERNAL"),
            "fxcm/gbpusd-m1-bid-2012.csv",
            max_rows=5,
        ),
        "trades_from_binance_csv": TestDataProvider.trades_from_binance_csv(
            ethusdt,
            "binance/ethusdt-trades.csv",
            max_rows=5,
        ),
        "bars_from_binance_csv": TestDataProvider.bars_from_binance_csv(
            btcusdt,
            BarType.from_str("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            "btc-perp-20211231-20220201_1m.csv",
            max_rows=5,
        ),
    }


def test_loaders_resolve_the_same_data_without_a_source_checkout(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test every loader falls back to the remote test data outside a source checkout.
    """
    from_checkout = _load_all_csv_loaders()

    requested: list[str] = []

    def fake_urlopen(url: str) -> io.BytesIO:
        relative_path = url.split("/test_data/", 1)[1]
        requested.append(relative_path)
        return io.BytesIO((TEST_DATA_DIR / relative_path).read_bytes())

    monkeypatch.setattr(providers, "TEST_DATA_DIR", Path("/nonexistent/test_data"))
    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)

    from_wheel = _load_all_csv_loaders()

    assert requested == [
        "short-term-interest.csv",
        "truefx/usdjpy-ticks.csv",
        "fxcm/gbpusd-m1-bid-2012.csv",
        "fxcm/gbpusd-m1-ask-2012.csv",
        "fxcm/gbpusd-m1-bid-2012.csv",
        "binance/ethusdt-trades.csv",
        "btc-perp-20211231-20220201_1m.csv",
    ]
    assert from_wheel == from_checkout
