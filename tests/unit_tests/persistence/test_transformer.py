# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from io import BytesIO
from pathlib import Path

import pandas as pd
import pyarrow as pa
import pytest

from nautilus_trader.core.nautilus_pyo3.persistence import DataTransformer
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.persistence.wranglers_v2 import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from tests import TEST_DATA_DIR


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


def test_pyo3_quote_ticks_to_record_batch_reader() -> None:
    # Arrange
    path = Path(TEST_DATA_DIR) / "truefx-audusd-ticks.csv"
    df: pd.DataFrame = pd.read_csv(path)

    # Act
    wrangler = QuoteTickDataWrangler.from_instrument(AUDUSD_SIM)
    ticks = wrangler.from_pandas(df)

    # Act
    batches_bytes = DataTransformer.pyo3_quote_ticks_to_batches_bytes(ticks)
    batches_stream = BytesIO(batches_bytes)
    reader = pa.ipc.open_stream(batches_stream)

    # Assert
    assert len(ticks) == 100_000
    assert len(reader.read_all()) == len(ticks)
    reader.close()


def test_legacy_trade_ticks_to_record_batch_reader() -> None:
    # Arrange
    provider = TestDataProvider()
    wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BINANCE)
    ticks = wrangler.process(provider.read_csv_ticks("binance-ethusdt-trades.csv"))

    # Act
    batches_bytes = DataTransformer.pyobjects_to_batches_bytes(ticks)
    batches_stream = BytesIO(batches_bytes)
    reader = pa.ipc.open_stream(batches_stream)

    # Assert
    assert len(ticks) == 69_806
    assert len(reader.read_all()) == len(ticks)
    reader.close()


def test_legacy_deltas_to_record_batch_reader() -> None:
    # Arrange
    ticks = [
        OrderBookDelta.from_dict(
            {
                "action": "CLEAR",
                "flags": 0,
                "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                "order": {
                    "order_id": 0,
                    "price": "0",
                    "side": "NO_ORDER_SIDE",
                    "size": "0",
                },
                "sequence": 0,
                "ts_event": 1576840503572000000,
                "ts_init": 1576840503572000000,
                "type": "OrderBookDelta",
            },
        ),
    ]

    # Act
    batches_bytes = DataTransformer.pyobjects_to_batches_bytes(ticks)
    batches_stream = BytesIO(batches_bytes)
    reader = pa.ipc.open_stream(batches_stream)

    # Assert
    assert len(ticks) == 1
    assert len(reader.read_all()) == len(ticks)
    reader.close()


def test_get_schema_map_with_unsupported_type() -> None:
    # Arrange, Act, Assert
    with pytest.raises(TypeError):
        DataTransformer.get_schema_map(str)


@pytest.mark.parametrize(
    ("data_type", "expected_map"),
    [
        [
            OrderBookDelta,
            {
                "action": "UInt8",
                "flags": "UInt8",
                "order_id": "UInt64",
                "price": "Int64",
                "sequence": "UInt64",
                "side": "UInt8",
                "size": "UInt64",
                "ts_event": "UInt64",
                "ts_init": "UInt64",
            },
        ],
        [
            QuoteTick,
            {
                "bid_price": "Int64",
                "ask_price": "Int64",
                "bid_size": "UInt64",
                "ask_size": "UInt64",
                "ts_event": "UInt64",
                "ts_init": "UInt64",
            },
        ],
        [
            TradeTick,
            {
                "price": "Int64",
                "size": "UInt64",
                "aggressor_side": "UInt8",
                "trade_id": "Utf8",
                "ts_event": "UInt64",
                "ts_init": "UInt64",
            },
        ],
        [
            Bar,
            {
                "open": "Int64",
                "high": "Int64",
                "low": "Int64",
                "close": "Int64",
                "volume": "UInt64",
                "ts_event": "UInt64",
                "ts_init": "UInt64",
            },
        ],
    ],
)
def test_get_schema_map_for_all_implemented_types(
    data_type: type,
    expected_map: dict[str, str],
) -> None:
    # Arrange, Act
    schema_map = DataTransformer.get_schema_map(data_type)

    # Assert
    assert schema_map == expected_map
