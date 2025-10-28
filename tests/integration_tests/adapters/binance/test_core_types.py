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
from decimal import Decimal

import pytest

from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.adapters.binance.common.types import BinanceTicker
from nautilus_trader.adapters.binance.futures.types import BinanceFuturesMarkPriceUpdate
from nautilus_trader.model.data import BarType
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.test_kit.mocks.data import setup_catalog
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def test_binance_ticker_repr():
    # Arrange
    ticker = BinanceTicker(
        instrument_id=TestIdStubs.btcusdt_binance_id(),
        price_change=Decimal("-94.99999800"),
        price_change_percent=Decimal("-95.960"),
        weighted_avg_price=Decimal("0.29628482"),
        prev_close_price=Decimal("0.10002000"),
        last_price=Decimal("4.00000200"),
        last_qty=Decimal("200.00000000"),
        bid_price=Decimal("4.00000000"),
        bid_qty=Decimal("24.00000000"),
        ask_price=Decimal("4.00000200"),
        ask_qty=Decimal("24.00000200"),
        open_price=Decimal("99.00000000"),
        high_price=Decimal("100.00000000"),
        low_price=Decimal("0.10000000"),
        volume=Decimal("8913.30000000"),
        quote_volume=Decimal("15.30000000"),
        open_time_ms=1499783499040,
        close_time_ms=1499869899040,
        first_id=28385,
        last_id=28460,
        count=76,
        ts_event=1650000000000000000,
        ts_init=1650000000000000000,
    )

    # Act, Assert
    assert (
        repr(ticker)
        == "BinanceTicker(instrument_id=BTCUSDT.BINANCE, price_change=-94.99999800, price_change_percent=-95.960, weighted_avg_price=0.29628482, prev_close_price=0.10002000, last_price=4.00000200, last_qty=200.00000000, bid_price=4.00000000, bid_qty=24.00000000, ask_price=4.00000200, ask_qty=24.00000200, open_price=99.00000000, high_price=100.00000000, low_price=0.10000000, volume=8913.30000000, quote_volume=15.30000000, open_time_ms=1499783499040, close_time_ms=1499869899040, first_id=28385, last_id=28460, count=76, ts_event=1650000000000000000, ts_init=1650000000000000000)"
    )


def test_binance_ticker_pickle():
    # Arrange
    ticker = BinanceTicker(
        instrument_id=TestIdStubs.btcusdt_binance_id(),
        price_change=Decimal("-94.99999800"),
        price_change_percent=Decimal("-95.960"),
        weighted_avg_price=Decimal("0.29628482"),
        prev_close_price=Decimal("0.10002000"),
        last_price=Decimal("4.00000200"),
        last_qty=Decimal("200.00000000"),
        bid_price=Decimal("4.00000000"),
        bid_qty=Decimal("24.00000000"),
        ask_price=Decimal("4.00000200"),
        ask_qty=Decimal("24.00000200"),
        open_price=Decimal("99.00000000"),
        high_price=Decimal("100.00000000"),
        low_price=Decimal("0.10000000"),
        volume=Decimal("8913.30000000"),
        quote_volume=Decimal("15.30000000"),
        open_time_ms=1499783499040,
        close_time_ms=1499869899040,
        first_id=28385,
        last_id=28460,
        count=76,
        ts_event=1650000000000000000,
        ts_init=1650000000000000000,
    )

    # Act
    pickled = pickle.dumps(ticker)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

    # Assert
    assert unpickled == ticker
    assert (
        repr(unpickled)
        == "BinanceTicker(instrument_id=BTCUSDT.BINANCE, price_change=-94.99999800, price_change_percent=-95.960, weighted_avg_price=0.29628482, prev_close_price=0.10002000, last_price=4.00000200, last_qty=200.00000000, bid_price=4.00000000, bid_qty=24.00000000, ask_price=4.00000200, ask_qty=24.00000200, open_price=99.00000000, high_price=100.00000000, low_price=0.10000000, volume=8913.30000000, quote_volume=15.30000000, open_time_ms=1499783499040, close_time_ms=1499869899040, first_id=28385, last_id=28460, count=76, ts_event=1650000000000000000, ts_init=1650000000000000000)"
    )


def test_binance_ticker_to_from_dict():
    # Arrange
    ticker = BinanceTicker(
        instrument_id=TestIdStubs.btcusdt_binance_id(),
        price_change=Decimal("-94.99999800"),
        price_change_percent=Decimal("-95.960"),
        weighted_avg_price=Decimal("0.29628482"),
        prev_close_price=Decimal("0.10002000"),
        last_price=Decimal("4.00000200"),
        last_qty=Decimal("200.00000000"),
        bid_price=Decimal("4.00000000"),
        bid_qty=Decimal("24.00000000"),
        ask_price=Decimal("4.00000200"),
        ask_qty=Decimal("24.00000200"),
        open_price=Decimal("99.00000000"),
        high_price=Decimal("100.00000000"),
        low_price=Decimal("0.10000000"),
        volume=Decimal("8913.30000000"),
        quote_volume=Decimal("15.30000000"),
        open_time_ms=1499783499040,
        close_time_ms=1499869899040,
        first_id=28385,
        last_id=28460,
        count=76,
        ts_event=1650000000000000000,
        ts_init=1650000000000000000,
    )

    # Act
    values = ticker.to_dict(ticker)

    # Assert
    assert BinanceTicker.from_dict(values) == ticker
    assert values == {
        "type": "BinanceTicker",
        "instrument_id": "BTCUSDT.BINANCE",
        "price_change": "-94.99999800",
        "price_change_percent": "-95.960",
        "weighted_avg_price": "0.29628482",
        "prev_close_price": "0.10002000",
        "last_price": "4.00000200",
        "last_qty": "200.00000000",
        "bid_price": "4.00000000",
        "bid_qty": "24.00000000",
        "ask_price": "4.00000200",
        "ask_qty": "24.00000200",
        "open_price": "99.00000000",
        "high_price": "100.00000000",
        "low_price": "0.10000000",
        "volume": "8913.30000000",
        "quote_volume": "15.30000000",
        "open_time_ms": 1499783499040,
        "close_time_ms": 1499869899040,
        "first_id": 28385,
        "last_id": 28460,
        "count": 76,
        "ts_event": 1650000000000000000,
        "ts_init": 1650000000000000000,
    }


def test_binance_bar_repr():
    # Arrange
    bar = BinanceBar(
        bar_type=BarType(
            instrument_id=TestIdStubs.btcusdt_binance_id(),
            bar_spec=TestDataStubs.bar_spec_1min_last(),
        ),
        open=Price.from_str("0.01634790"),
        high=Price.from_str("0.01640000"),
        low=Price.from_str("0.01575800"),
        close=Price.from_str("0.01577100"),
        volume=Quantity.from_str("148976.11427815"),
        quote_volume=Decimal("2434.19055334"),
        count=100,
        taker_buy_base_volume=Decimal("1756.87402397"),
        taker_buy_quote_volume=Decimal("28.46694368"),
        ts_event=1650000000000000000,
        ts_init=1650000000000000000,
    )

    # Act, Assert
    assert (
        repr(bar)
        == "BinanceBar(bar_type=BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL, open=0.01634790, high=0.01640000, low=0.01575800, close=0.01577100, volume=148976.11427815, quote_volume=2434.19055334, count=100, taker_buy_base_volume=1756.87402397, taker_buy_quote_volume=28.46694368, taker_sell_base_volume=147219.24025418, taker_sell_quote_volume=2405.72360966, ts_event=1650000000000000000, ts_init=1650000000000000000)"
    )


def test_binance_bar_to_from_dict():
    # Arrange
    bar = BinanceBar(
        bar_type=BarType(
            instrument_id=TestIdStubs.btcusdt_binance_id(),
            bar_spec=TestDataStubs.bar_spec_1min_last(),
        ),
        open=Price.from_str("0.01634790"),
        high=Price.from_str("0.01640000"),
        low=Price.from_str("0.01575800"),
        close=Price.from_str("0.01577100"),
        volume=Quantity.from_str("148976.11427815"),
        quote_volume=Decimal("2434.19055334"),
        count=100,
        taker_buy_base_volume=Decimal("1756.87402397"),
        taker_buy_quote_volume=Decimal("28.46694368"),
        ts_event=1650000000000000000,
        ts_init=1650000000000000000,
    )

    # Act
    values = bar.to_dict(bar)

    # Assert
    assert BinanceBar.from_dict(values) == bar
    assert values == {
        "type": "BinanceBar",
        "bar_type": "BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL",
        "open": "0.01634790",
        "high": "0.01640000",
        "low": "0.01575800",
        "close": "0.01577100",
        "volume": "148976.11427815",
        "quote_volume": "2434.19055334",
        "count": 100,
        "taker_buy_base_volume": "1756.87402397",
        "taker_buy_quote_volume": "28.46694368",
        "ts_event": 1650000000000000000,
        "ts_init": 1650000000000000000,
    }


def test_binance_bar_pickling():
    # Arrange
    bar = BinanceBar(
        bar_type=BarType(
            instrument_id=TestIdStubs.btcusdt_binance_id(),
            bar_spec=TestDataStubs.bar_spec_1min_last(),
        ),
        open=Price.from_str("0.01634790"),
        high=Price.from_str("0.01640000"),
        low=Price.from_str("0.01575800"),
        close=Price.from_str("0.01577100"),
        volume=Quantity.from_str("148976.11427815"),
        quote_volume=Decimal("2434.19055334"),
        count=100,
        taker_buy_base_volume=Decimal("1756.87402397"),
        taker_buy_quote_volume=Decimal("28.46694368"),
        ts_event=1650000000000000000,
        ts_init=1650000000000000000,
    )

    # Act
    pickled = pickle.dumps(bar)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

    # Assert
    assert unpickled == bar
    assert (
        repr(bar)
        == "BinanceBar(bar_type=BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL, open=0.01634790, high=0.01640000, low=0.01575800, close=0.01577100, volume=148976.11427815, quote_volume=2434.19055334, count=100, taker_buy_base_volume=1756.87402397, taker_buy_quote_volume=28.46694368, taker_sell_base_volume=147219.24025418, taker_sell_quote_volume=2405.72360966, ts_event=1650000000000000000, ts_init=1650000000000000000)"
    )
    assert unpickled.quote_volume == bar.quote_volume
    assert unpickled.count == bar.count
    assert unpickled.taker_buy_base_volume == bar.taker_buy_base_volume
    assert unpickled.taker_buy_quote_volume == bar.taker_buy_quote_volume


def test_binance_mark_price_to_from_dict():
    # Arrange
    update = BinanceFuturesMarkPriceUpdate(
        instrument_id=TestIdStubs.ethusdt_perp_binance_id(),
        mark=Price.from_str("1642.28584467"),
        index=Price.from_str("1642.28316456"),
        estimated_settle=Price.from_str("1639.27811452"),
        funding_rate=Decimal("0.00081453"),
        next_funding_ns=1650000000000000002,
        ts_event=1650000000000000001,
        ts_init=1650000000000000000,
    )

    # Act
    values = update.to_dict(update)

    # Assert
    BinanceFuturesMarkPriceUpdate.from_dict(values)
    assert values == {
        "type": "BinanceFuturesMarkPriceUpdate",
        "instrument_id": "ETHUSDT-PERP.BINANCE",
        "mark": "1642.28584467",
        "index": "1642.28316456",
        "estimated_settle": "1639.27811452",
        "funding_rate": "0.00081453",
        "next_funding_ns": 1650000000000000002,
        "ts_event": 1650000000000000001,
        "ts_init": 1650000000000000000,
    }


def test_binance_mark_price_pickling():
    # Arrange
    update = BinanceFuturesMarkPriceUpdate(
        instrument_id=TestIdStubs.ethusdt_perp_binance_id(),
        mark=Price.from_str("1642.28584467"),
        index=Price.from_str("1642.28316456"),
        estimated_settle=Price.from_str("1639.27811452"),
        funding_rate=Decimal("0.00081453"),
        next_funding_ns=1650000000000000002,
        ts_event=1650000000000000001,
        ts_init=1650000000000000000,
    )

    # Act
    pickled = pickle.dumps(update)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

    # Assert
    assert unpickled.to_dict(unpickled) == {
        "type": "BinanceFuturesMarkPriceUpdate",
        "instrument_id": "ETHUSDT-PERP.BINANCE",
        "mark": "1642.28584467",
        "index": "1642.28316456",
        "estimated_settle": "1639.27811452",
        "funding_rate": "0.00081453",
        "next_funding_ns": 1650000000000000002,
        "ts_event": 1650000000000000001,
        "ts_init": 1650000000000000000,
    }


@pytest.fixture
def catalog(tmp_path) -> ParquetDataCatalog:
    return setup_catalog(protocol="memory", path=tmp_path / "catalog")


def test_binance_bar_data_catalog_serialization(catalog: ParquetDataCatalog):
    """
    Test that BinanceBar can be serialized to and deserialized from a data catalog.

    Regression test for the BinanceBar serialization issue where Arrow registration was
    incomplete (missing encoder/decoder).

    """
    # Arrange
    bar = BinanceBar(
        bar_type=BarType(
            instrument_id=TestIdStubs.btcusdt_binance_id(),
            bar_spec=TestDataStubs.bar_spec_1min_last(),
        ),
        open=Price.from_str("0.01634790"),
        high=Price.from_str("0.01640000"),
        low=Price.from_str("0.01575800"),
        close=Price.from_str("0.01577100"),
        volume=Quantity.from_str("148976.11427815"),
        quote_volume=Decimal("2434.19055334"),
        count=100,
        taker_buy_base_volume=Decimal("1756.87402397"),
        taker_buy_quote_volume=Decimal("28.46694368"),
        ts_event=1650000000000000000,
        ts_init=1650000000000000000,
    )

    # Act
    catalog.write_data([bar])
    binance_bars = catalog.custom_data(cls=BinanceBar)

    assert len(binance_bars) == 1
    retrieved_bar = binance_bars[0]

    # Verify all standard bar fields
    assert retrieved_bar.bar_type == bar.bar_type
    assert retrieved_bar.open == bar.open
    assert retrieved_bar.high == bar.high
    assert retrieved_bar.low == bar.low
    assert retrieved_bar.close == bar.close
    assert retrieved_bar.volume == bar.volume
    assert retrieved_bar.ts_event == bar.ts_event
    assert retrieved_bar.ts_init == bar.ts_init

    # Verify Binance-specific fields are preserved
    assert hasattr(retrieved_bar, "quote_volume")
    assert hasattr(retrieved_bar, "count")
    assert hasattr(retrieved_bar, "taker_buy_base_volume")
    assert hasattr(retrieved_bar, "taker_buy_quote_volume")
    assert hasattr(retrieved_bar, "taker_sell_base_volume")
    assert hasattr(retrieved_bar, "taker_sell_quote_volume")

    assert retrieved_bar.quote_volume == bar.quote_volume
    assert retrieved_bar.count == bar.count
    assert retrieved_bar.taker_buy_base_volume == bar.taker_buy_base_volume
    assert retrieved_bar.taker_buy_quote_volume == bar.taker_buy_quote_volume
    assert retrieved_bar.taker_sell_base_volume == bar.taker_sell_base_volume
    assert retrieved_bar.taker_sell_quote_volume == bar.taker_sell_quote_volume

    # Verify object equality
    assert retrieved_bar == bar
