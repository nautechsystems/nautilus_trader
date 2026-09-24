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
Binance custom data Parquet catalog regression tests.
"""

from decimal import Decimal

from nautilus_trader.adapters.binance import BinanceFuturesLiquidation
from nautilus_trader.adapters.binance import BinanceFuturesOpenInterest
from nautilus_trader.adapters.binance import BinanceFuturesTicker
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.persistence import ParquetDataCatalog


register_custom_data_class(BinanceFuturesTicker)
register_custom_data_class(BinanceFuturesOpenInterest)
register_custom_data_class(BinanceFuturesLiquidation)

BINANCE = Venue("BINANCE")
BTCUSDT_PERP = InstrumentId(Symbol("BTCUSDT-PERP"), BINANCE)


def _ticker(ts_event: int, ts_init: int) -> BinanceFuturesTicker:
    return BinanceFuturesTicker(
        BTCUSDT_PERP,
        Decimal("12.34"),
        Decimal("5.67"),
        Decimal("62345.123456"),
        Decimal("62350.000001"),
        Decimal("0.010000"),
        Decimal("62000.000000"),
        Decimal("63000.000000"),
        Decimal("61000.000000"),
        Decimal("1234.567890"),
        Decimal("76543210.123456"),
        10,
        11,
        100,
        200,
        300,
        ts_event,
        ts_init,
    )


def test_binance_futures_ticker_catalog_round_trip(tmp_path) -> None:
    """
    Verify binance futures ticker catalog round trip.
    """
    catalog = ParquetDataCatalog(str(tmp_path))
    data_type = DataType("BinanceFuturesTicker", None, str(BTCUSDT_PERP))
    ticker = _ticker(1_000_000_000, 1_000_000_001)

    catalog.write_custom_data([CustomData(data_type, ticker)])
    result = catalog.query_custom_data(
        "BinanceFuturesTicker",
        identifiers=[str(BTCUSDT_PERP)],
    )

    assert len(result) == 1
    assert result[0].data.last_price == Decimal("62350.000001")
    assert result[0].data.num_trades == 300
    assert result[0].data.ts_event == 1_000_000_000


def test_binance_futures_open_interest_catalog_round_trip(tmp_path) -> None:
    """
    Verify binance futures open interest catalog round trip.
    """
    catalog = ParquetDataCatalog(str(tmp_path))
    data_type = DataType("BinanceFuturesOpenInterest", None, str(BTCUSDT_PERP))
    open_interest = BinanceFuturesOpenInterest(
        BTCUSDT_PERP,
        Decimal("123456.789012345678"),
        2_000_000_000,
        2_000_000_001,
    )

    catalog.write_custom_data([CustomData(data_type, open_interest)])
    result = catalog.query_custom_data(
        "BinanceFuturesOpenInterest",
        identifiers=[str(BTCUSDT_PERP)],
    )

    assert len(result) == 1
    assert result[0].data.open_interest == Decimal("123456.789012345678")
    assert result[0].data.ts_init == 2_000_000_001


def test_binance_futures_liquidation_catalog_round_trip(tmp_path) -> None:
    """
    Verify binance futures liquidation catalog round trip.
    """
    catalog = ParquetDataCatalog(str(tmp_path))
    data_type = DataType("BinanceFuturesLiquidation", None, str(BTCUSDT_PERP))
    liquidation = BinanceFuturesLiquidation(
        BTCUSDT_PERP,
        OrderSide.SELL,
        Price.from_str("65432.10"),
        Price.from_str("65431.50"),
        Quantity.from_str("0.250"),
        Quantity.from_str("1.500"),
        3_000_000_000,
        3_000_000_001,
    )

    catalog.write_custom_data([CustomData(data_type, liquidation)])
    result = catalog.query_custom_data(
        "BinanceFuturesLiquidation",
        identifiers=[str(BTCUSDT_PERP)],
    )

    assert len(result) == 1
    assert result[0].data.side == OrderSide.SELL
    assert result[0].data.price == Price.from_str("65432.10")
    assert result[0].data.accumulated_qty == Quantity.from_str("1.500")


def test_binance_futures_ticker_list_parquet_files_discovers_legacy_layout(tmp_path) -> None:
    """
    Verify binance futures ticker file listing discovers legacy layout.
    """
    catalog = ParquetDataCatalog(str(tmp_path))
    data_type = DataType("BinanceFuturesTicker", None, str(BTCUSDT_PERP))
    ticker = _ticker(1_000_000_000, 1_000_000_001)
    catalog.write_custom_data([CustomData(data_type, ticker)])

    # Relocate the written file to the legacy Python-written layout.
    canonical = next(
        (tmp_path / "data" / "custom" / "BinanceFuturesTicker" / str(BTCUSDT_PERP)).glob(
            "*.parquet",
        ),
    )
    legacy_dir = tmp_path / "data" / "custom_binance_futures_ticker" / str(BTCUSDT_PERP)
    legacy_dir.mkdir(parents=True)
    canonical.rename(legacy_dir / canonical.name)

    files = catalog.list_parquet_files(
        NautilusDataType.Custom("BinanceFuturesTicker"),
        str(BTCUSDT_PERP),
    )

    assert len(files) == 1
    assert "custom_binance_futures_ticker" in files[0]
