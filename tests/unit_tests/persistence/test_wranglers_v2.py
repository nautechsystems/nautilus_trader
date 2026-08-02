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

from decimal import Decimal

import pandas as pd
import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.objects import FIXED_PRECISION
from nautilus_trader.model.objects import FIXED_PRECISION_BYTES
from nautilus_trader.persistence.wranglers_v2 import RAW_BYTE_ORDER
from nautilus_trader.persistence.wranglers_v2 import BarDataWranglerV2
from nautilus_trader.persistence.wranglers_v2 import OrderBookDeltaDataWranglerV2
from nautilus_trader.persistence.wranglers_v2 import OrderBookDepth10DataWranglerV2
from nautilus_trader.persistence.wranglers_v2 import QuoteTickDataWranglerV2
from nautilus_trader.persistence.wranglers_v2 import TradeTickDataWranglerV2
from nautilus_trader.test_kit.providers import TestInstrumentProvider


_RAW_VALUE_BITS = FIXED_PRECISION_BYTES * 8
_FIXED_SCALE = 10**FIXED_PRECISION
_PRICE_OVERFLOW_SOURCE = (2 ** (_RAW_VALUE_BITS - 1) - 1) // _FIXED_SCALE + 1
_SIZE_OVERFLOW_SOURCE = (2**_RAW_VALUE_BITS - 1) // _FIXED_SCALE + 1
_TS_EVENT = pd.to_datetime(["2026-01-01 00:00:00"], utc=True)


def test_quote_tick_data_wrangler() -> None:
    # Arrange
    path = TEST_DATA_DIR / "truefx" / "audusd-ticks.csv"
    df = pd.read_csv(path)
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")

    # Act
    wrangler = QuoteTickDataWranglerV2.from_instrument(instrument)
    pyo3_quotes = wrangler.from_pandas(df)

    quotes = QuoteTick.from_pyo3_list(pyo3_quotes)

    # Assert
    assert len(pyo3_quotes) == 100_000
    assert len(quotes) == 100_000
    assert isinstance(quotes[0], QuoteTick)
    assert str(pyo3_quotes[0]) == "AUD/USD.SIM,0.67067,0.67070,1000000,1000000,1580398089820000000"
    assert str(pyo3_quotes[-1]) == "AUD/USD.SIM,0.66934,0.66938,1000000,1000000,1580504394501000000"


def test_trade_tick_data_wrangler() -> None:
    # Arrange
    path = TEST_DATA_DIR / "binance" / "ethusdt-trades.csv"
    df = pd.read_csv(path)
    instrument = TestInstrumentProvider.ethusdt_binance()

    # Act
    wrangler = TradeTickDataWranglerV2.from_instrument(instrument)
    pyo3_trades = wrangler.from_pandas(df)

    trades = TradeTick.from_pyo3_list(pyo3_trades)

    # Assert
    assert len(pyo3_trades) == 69806
    assert len(trades) == 69806
    assert isinstance(trades[0], TradeTick)
    assert (
        str(pyo3_trades[0]) == "ETHUSDT.BINANCE,423.76,2.67900,BUYER,148568980,1597399200223000000"
    )
    assert (
        str(pyo3_trades[-1]) == "ETHUSDT.BINANCE,426.89,0.16100,BUYER,148638715,1597417198693000000"
    )


def test_raw_byte_order_matches_rust_arrow_decoder() -> None:
    # Lock the constant: the Rust Arrow decoder reads
    # PriceRaw / QuantityRaw via from_le_bytes, so the wranglers must
    # always write little-endian. See crates/serialization/src/arrow/mod.rs.
    assert RAW_BYTE_ORDER == "little"


def test_order_book_depth10_data_wrangler_round_trip() -> None:
    # Regression for issue #4111: wrangler wrote raw fixed-point values as
    # big-endian, but the Rust Arrow decoder reads little-endian, so
    # from_pandas raised ValueError with an inflated raw value.
    # Arrange
    instrument_id = "BTC-USDT.BINANCE"
    price_precision = 2
    size_precision = 3
    expected_bid_prices = [Decimal("100.00") - i for i in range(10)]
    expected_ask_prices = [Decimal("101.00") + i for i in range(10)]
    expected_bid_sizes = [Decimal("1.000") + Decimal("0.100") * i for i in range(10)]
    expected_ask_sizes = [Decimal("2.000") + Decimal("0.100") * i for i in range(10)]

    row = {"ts_event": pd.to_datetime(["2026-01-01 00:00:00"], utc=True)}
    for i in range(10):
        row[f"bid_price_{i}"] = [float(expected_bid_prices[i])]
        row[f"ask_price_{i}"] = [float(expected_ask_prices[i])]
        row[f"bid_size_{i}"] = [float(expected_bid_sizes[i])]
        row[f"ask_size_{i}"] = [float(expected_ask_sizes[i])]

    wrangler = OrderBookDepth10DataWranglerV2(
        instrument_id=instrument_id,
        price_precision=price_precision,
        size_precision=size_precision,
    )

    # Act
    depths = wrangler.from_pandas(pd.DataFrame(row))

    # Assert
    assert len(depths) == 1
    depth = depths[0]
    assert str(depth.instrument_id) == instrument_id
    assert len(depth.bids) == 10
    assert len(depth.asks) == 10
    for i in range(10):
        assert depth.bids[i].price.as_decimal() == expected_bid_prices[i]
        assert depth.asks[i].price.as_decimal() == expected_ask_prices[i]
        assert depth.bids[i].size.as_decimal() == expected_bid_sizes[i]
        assert depth.asks[i].size.as_decimal() == expected_ask_sizes[i]


def test_bar_wrangler_normal_values_still_work() -> None:
    # Arrange
    wrangler = BarDataWranglerV2(
        bar_type="AAPL.XNAS-1-MINUTE-LAST-EXTERNAL",
        price_precision=2,
        size_precision=0,
    )
    df = pd.DataFrame(
        {
            "open": [3812.25],
            "high": [3813.0],
            "low": [3811.0],
            "close": [3811.0],
            "volume": [100.0],
            "ts_event": _TS_EVENT,
        },
    )

    # Act
    bars = wrangler.from_pandas(df)

    # Assert
    assert len(bars) == 1
    bar = bars[0]
    assert bar.open.as_decimal() == Decimal("3812.25")
    assert bar.high.as_decimal() == Decimal("3813.00")
    assert bar.low.as_decimal() == Decimal("3811.00")
    assert bar.close.as_decimal() == Decimal("3811.00")
    assert bar.volume.as_decimal() == Decimal(100)


def test_bar_wrangler_price_raw_overflow_identifies_column() -> None:
    # Arrange
    wrangler = BarDataWranglerV2(
        bar_type="AAPL.XNAS-1-MINUTE-LAST-EXTERNAL",
        price_precision=0,
        size_precision=0,
    )
    df = pd.DataFrame(
        {
            "open": [_PRICE_OVERFLOW_SOURCE],
            "high": [1],
            "low": [1],
            "close": [1],
            "volume": [1],
            "ts_event": _TS_EVENT,
        },
    )

    # Act
    with pytest.raises(ValueError) as excinfo:
        wrangler.from_pandas(df)

    # Assert
    message = str(excinfo.value)
    assert "Wrangler overflow" in message
    assert "price column 'open'" in message
    assert "price_precision=0" in message


def test_bar_wrangler_size_raw_overflow_identifies_column() -> None:
    # Arrange
    wrangler = BarDataWranglerV2(
        bar_type="AAPL.XNAS-1-MINUTE-LAST-EXTERNAL",
        price_precision=0,
        size_precision=0,
    )
    df = pd.DataFrame(
        {
            "open": [1],
            "high": [1],
            "low": [1],
            "close": [1],
            "volume": [_SIZE_OVERFLOW_SOURCE],
            "ts_event": _TS_EVENT,
        },
    )

    # Act
    with pytest.raises(ValueError) as excinfo:
        wrangler.from_pandas(df)

    # Assert
    message = str(excinfo.value)
    assert "Wrangler overflow" in message
    assert "size column 'volume'" in message
    assert "size_precision=0" in message


def test_bar_wrangler_negative_size_raw_overflow_identifies_column() -> None:
    # Arrange
    wrangler = BarDataWranglerV2(
        bar_type="AAPL.XNAS-1-MINUTE-LAST-EXTERNAL",
        price_precision=0,
        size_precision=0,
    )
    df = pd.DataFrame(
        {
            "open": [1],
            "high": [1],
            "low": [1],
            "close": [1],
            "volume": [-1],
            "ts_event": _TS_EVENT,
        },
    )

    # Act
    with pytest.raises(ValueError) as excinfo:
        wrangler.from_pandas(df)

    # Assert
    message = str(excinfo.value)
    assert "Wrangler overflow" in message
    assert "size column 'volume'" in message
    assert "unsigned" in message
    assert "size_precision=0" in message


def test_quote_wrangler_price_raw_overflow_identifies_column() -> None:
    # Arrange
    wrangler = QuoteTickDataWranglerV2(
        instrument_id="AAPL.XNAS",
        price_precision=0,
        size_precision=0,
    )
    df = pd.DataFrame(
        {
            "bid_price": [_PRICE_OVERFLOW_SOURCE],
            "ask_price": [1],
            "bid_size": [1],
            "ask_size": [1],
            "ts_event": _TS_EVENT,
        },
    )

    # Act
    with pytest.raises(ValueError) as excinfo:
        wrangler.from_pandas(df)

    # Assert
    message = str(excinfo.value)
    assert "Wrangler overflow" in message
    assert "price column 'bid_price'" in message
    assert "price_precision=0" in message


def test_trade_wrangler_price_raw_overflow_identifies_column() -> None:
    # Arrange
    wrangler = TradeTickDataWranglerV2(
        instrument_id="AAPL.XNAS",
        price_precision=0,
        size_precision=0,
    )
    df = pd.DataFrame(
        {
            "price": [_PRICE_OVERFLOW_SOURCE],
            "size": [1],
            "aggressor_side": [True],
            "trade_id": ["1"],
            "ts_event": _TS_EVENT,
        },
    )

    # Act
    with pytest.raises(ValueError) as excinfo:
        wrangler.from_pandas(df)

    # Assert
    message = str(excinfo.value)
    assert "Wrangler overflow" in message
    assert "price column 'price'" in message
    assert "price_precision=0" in message


def test_order_book_depth10_wrangler_price_raw_overflow_identifies_column() -> None:
    # Arrange
    wrangler = OrderBookDepth10DataWranglerV2(
        instrument_id="AAPL.XNAS",
        price_precision=0,
        size_precision=0,
    )
    df = pd.DataFrame(
        {
            "bid_price_0": [_PRICE_OVERFLOW_SOURCE],
            "ask_price_0": [1],
            "bid_size_0": [1],
            "ask_size_0": [1],
            "ts_event": _TS_EVENT,
        },
    )

    # Act
    with pytest.raises(ValueError) as excinfo:
        wrangler.from_pandas(df)

    # Assert
    message = str(excinfo.value)
    assert "Wrangler overflow" in message
    assert "price column 'bid_price_0'" in message
    assert "price_precision=0" in message


def test_order_book_delta_wrangler_fractional_values_still_work() -> None:
    # Arrange
    wrangler = OrderBookDeltaDataWranglerV2(
        instrument_id="AAPL.XNAS",
        price_precision=2,
        size_precision=1,
    )
    df = pd.DataFrame(
        {
            "price": [100.25],
            "size": [1.5],
            "order_id": [1],
            "action": [1],
            "aggressor_side": [1],
            "ts_event": _TS_EVENT,
        },
    )

    # Act
    deltas = wrangler.from_pandas(df)

    # Assert
    assert len(deltas) == 1
    delta = deltas[0]
    assert delta.order.price.as_decimal() == Decimal("100.25")
    assert delta.order.size.as_decimal() == Decimal("1.5")


def test_order_book_delta_wrangler_price_raw_overflow_identifies_column() -> None:
    # Arrange
    wrangler = OrderBookDeltaDataWranglerV2(
        instrument_id="AAPL.XNAS",
        price_precision=0,
        size_precision=0,
    )
    df = pd.DataFrame(
        {
            "price": [_PRICE_OVERFLOW_SOURCE],
            "size": [1],
            "order_id": [1],
            "aggressor_side": [1],
            "ts_event": _TS_EVENT,
        },
    )

    # Act
    with pytest.raises(ValueError) as excinfo:
        wrangler.from_pandas(df)

    # Assert
    message = str(excinfo.value)
    assert "Wrangler overflow" in message
    assert "price column 'price'" in message
    assert "price_precision=0" in message


def test_order_book_delta_wrangler_size_raw_overflow_identifies_column() -> None:
    # Arrange
    wrangler = OrderBookDeltaDataWranglerV2(
        instrument_id="AAPL.XNAS",
        price_precision=0,
        size_precision=0,
    )
    df = pd.DataFrame(
        {
            "price": [1],
            "size": [_SIZE_OVERFLOW_SOURCE],
            "order_id": [1],
            "aggressor_side": [1],
            "ts_event": _TS_EVENT,
        },
    )

    # Act
    with pytest.raises(ValueError) as excinfo:
        wrangler.from_pandas(df)

    # Assert
    message = str(excinfo.value)
    assert "Wrangler overflow" in message
    assert "size column 'size'" in message
    assert "size_precision=0" in message
