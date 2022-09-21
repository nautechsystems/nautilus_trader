# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import itertools
import os

from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.rust.reader import ParquetFileReader
from nautilus_trader.persistence.catalog.rust.writer import ParquetWriter


def test_parquet_writer_round_trip_quote():
    n = 100
    ticks = [
        QuoteTick(
            InstrumentId.from_str("EUR/USD.SIM"),
            Price(1.234, 4),
            Price(1.234, 4),
            Quantity(5, 0),
            Quantity(5, 0),
            0,
            0,
        )
    ] * n
    file_path = os.path.join(os.getcwd(), "quote_test.parquet")
    if os.path.exists(file_path):
        os.remove(file_path)
    metadata = {"instrument_id": "EUR/USD.SIM", "price_precision": "5", "size_precision": "0"}
    writer = ParquetWriter(QuoteTick, metadata)
    writer.write(ticks)

    data = writer.flush()
    with open(file_path, "wb") as f:
        f.write(data)

    assert os.path.exists(file_path)
    reader = ParquetFileReader(QuoteTick, file_path)
    ticks = list(itertools.chain(*list(reader)))
    print(ticks)


def test_parquet_writer_round_trip_trade():
    n = 100
    ticks = [
        TradeTick(
            InstrumentId.from_str("EUR/USD.SIM"),
            Price(1.234, 4),
            Quantity(5, 4),
            AggressorSide.BUY,
            TradeId("123456"),
            0,
            0,
        )
    ] * n
    file_path = os.path.join(os.getcwd(), "trade_test.parquet")
    if os.path.exists(file_path):
        os.remove(file_path)
    metadata = {"instrument_id": "EUR/USD.SIM", "price_precision": "4", "size_precision": "4"}
    writer = ParquetWriter(TradeTick, metadata)
    writer.write(ticks)

    data = writer.flush()
    with open(file_path, "wb") as f:
        f.write(data)

    assert os.path.exists(file_path)
    reader = ParquetFileReader(TradeTick, file_path)
    ticks = list(itertools.chain(*list(reader)))
    print(ticks)


if __name__ == "__main__":
    test_parquet_writer_round_trip_quote()
    test_parquet_writer_round_trip_trade()
