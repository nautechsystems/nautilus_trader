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
Nested depth display schema and output conversion regressions.
"""

from pathlib import Path

import pyarrow as pa
import pytest

from nautilus_trader.model import BookOrder
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OrderBookDepth
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence.catalog_to_df import CatalogOutput
from nautilus_trader.persistence.catalog_to_df import query_catalog


@pytest.mark.parametrize(
    ("output", "timestamp_resolution_ns"),
    [(output, 1_000 if output == CatalogOutput.DUCKDB else 1) for output in CatalogOutput],
)
def test_depth_display_keeps_all_nested_fields(
    tmp_path: Path,
    output: CatalogOutput,
    timestamp_resolution_ns: int,
) -> None:
    """
    Verify every level and field survives each supported DataFrame output.
    """
    if output == CatalogOutput.POLARS:
        pytest.importorskip("polars")
    if output == CatalogOutput.DUCKDB:
        pytest.importorskip("duckdb")
    catalog = ParquetDataCatalog(str(tmp_path))
    instrument = InstrumentId.from_str("AAPL.XNAS")
    expected = []
    snapshots = []

    for row, (bid_length, ask_length) in enumerate([(0, 0), (5, 3), (25, 27)]):
        sides = []
        orders = []

        for side, length, offset in [
            (OrderSide.BUY, bid_length, 0),
            (OrderSide.SELL, ask_length, 100),
        ]:
            levels = [
                {
                    "price": 100.25 + offset + i,
                    "size": 200.5 + offset + i,
                    "count": 300 + offset + i,
                    "order_id": 2**64 - 1 - offset - i,
                }
                for i in range(length)
            ]
            sides.append(levels)
            orders.append(
                [
                    BookOrder(
                        side,
                        Price.from_str(str(level["price"])),
                        Quantity.from_str(str(level["size"])),
                        level["order_id"],
                    )
                    for level in levels
                ],
            )
        snapshots.append(
            OrderBookDepth(
                instrument_id=instrument,
                bids=orders[0],
                asks=orders[1],
                bid_counts=[level["count"] for level in sides[0]],
                ask_counts=[level["count"] for level in sides[1]],
                flags=row + 1,
                sequence=row + 30,
                ts_event=row + 40,
                ts_init=row + 50,
            ),
        )
        expected.append(
            {
                "instrument_id": str(instrument),
                "bids": sides[0],
                "asks": sides[1],
                "flags": row + 1,
                "sequence": row + 30,
                "ts_event": (row + 40) // timestamp_resolution_ns * timestamp_resolution_ns,
                "ts_init": (row + 50) // timestamp_resolution_ns * timestamp_resolution_ns,
                "identifier": str(instrument),
            },
        )
    catalog.write_order_book_depths(snapshots)

    result = query_catalog(catalog, NautilusDataType.OrderBookDepth, output=output)
    if output == CatalogOutput.PANDAS:
        actual = result.to_dict(orient="records")
        for row in actual:
            row["bids"] = list(row["bids"])
            row["asks"] = list(row["asks"])
            row["ts_event"] = row["ts_event"].value
            row["ts_init"] = row["ts_init"].value
        assert actual == expected
        return
    if output == CatalogOutput.POLARS:
        table = result.to_arrow()
    elif output == CatalogOutput.DUCKDB:
        table = result.to_arrow_table()
    else:
        table = result
    for name in ("ts_event", "ts_init"):
        index = table.schema.get_field_index(name)
        table = table.set_column(
            index,
            name,
            table[name].cast(pa.timestamp("ns", tz="UTC")).cast(pa.int64()),
        )
    actual = table.to_pylist()

    assert actual == expected


def test_depth_display_empty_query_has_nested_schema(tmp_path: Path) -> None:
    """
    Verify empty catalog queries retain the full nested schema.
    """
    catalog = ParquetDataCatalog(str(tmp_path))
    table = query_catalog(catalog, NautilusDataType.OrderBookDepth, output=CatalogOutput.ARROW)
    level = pa.struct(
        [
            pa.field("price", pa.float64(), nullable=True),
            pa.field("size", pa.float64(), nullable=True),
            pa.field("count", pa.uint32(), nullable=False),
            pa.field("order_id", pa.uint64(), nullable=False),
        ],
    )
    side = pa.list_(pa.field("item", level, nullable=False))

    assert table.num_rows == 0
    assert table.schema.field("bids") == pa.field("bids", side, nullable=False)
    assert table.schema.field("asks") == pa.field("asks", side, nullable=False)
