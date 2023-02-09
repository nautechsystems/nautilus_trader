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

import itertools
import sys
from collections.abc import Generator
from pathlib import Path
from typing import Optional, Union

import fsspec
import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq

from nautilus_trader.core.data import Data
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetReader
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetReaderType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.external.util import py_type_to_parquet_type
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer


def _generate_batches_within_time_range(
    batches: Generator[list[Data], None, None],
    start_nanos: int = None,
    end_nanos: int = None,
) -> Generator[list[Data], None, None]:
    if start_nanos is None and end_nanos is None:
        yield from batches
        return

    if start_nanos is None:
        start_nanos = 0

    if end_nanos is None:
        end_nanos = sys.maxsize

    start = start_nanos
    end = end_nanos
    started = False
    for batch in batches:
        min = batch[0].ts_init
        max = batch[-1].ts_init
        if min < start and max < start:
            batch = []  # not started yet

        if max >= start and not started:
            timestamps = np.array([x.ts_init for x in batch])
            mask = timestamps >= start
            masked = list(itertools.compress(batch, mask))
            batch = masked
            started = True

        if max > end:
            timestamps = np.array([x.ts_init for x in batch])
            mask = timestamps <= end
            masked = list(itertools.compress(batch, mask))
            batch = masked
            if batch:
                yield batch
            return  # stop iterating

        yield batch


def _generate_batches_rust(
    files: list[str],
    cls: type,
    batch_size: int = 10_000,
) -> Generator[list[Union[QuoteTick, TradeTick]], None, None]:
    assert cls in (QuoteTick, TradeTick)

    files = sorted(files, key=lambda x: Path(x).stem)
    for file in files:
        reader = ParquetReader(
            file,
            batch_size,
            py_type_to_parquet_type(cls),
            ParquetReaderType.File,
        )
        for capsule in reader:
            # PyCapsule > List
            if cls == QuoteTick:
                objs = QuoteTick.list_from_capsule(capsule)
            elif cls == TradeTick:
                objs = TradeTick.list_from_capsule(capsule)

            yield objs


def generate_batches_rust(
    files: list[str],
    cls: type,
    batch_size: int = 10_000,
    start_nanos: int = None,
    end_nanos: int = None,
) -> Generator[list[Data], None, None]:
    batches = _generate_batches_rust(files=files, cls=cls, batch_size=batch_size)
    yield from _generate_batches_within_time_range(batches, start_nanos, end_nanos)


def _generate_batches(
    files: list[str],
    cls: type,
    fs: fsspec.AbstractFileSystem,
    instrument_id: Optional[InstrumentId] = None,  # should be stored in metadata of parquet file?
    batch_size: int = 10_000,
) -> Generator[list[Data], None, None]:
    files = sorted(files, key=lambda x: Path(x).stem)
    for file in files:
        for batch in pq.ParquetFile(fs.open(file)).iter_batches(batch_size=batch_size):
            if batch.num_rows == 0:
                break

            table = pa.Table.from_batches([batch])

            if instrument_id is not None and "instrument_id" not in batch.schema.names:
                table = table.append_column(
                    "instrument_id",
                    pa.array([str(instrument_id)] * len(table), pa.string()),
                )
            objs = ParquetSerializer.deserialize(cls=cls, chunk=table.to_pylist())
            yield objs


def generate_batches(
    files: list[str],
    cls: type,
    fs: fsspec.AbstractFileSystem,
    instrument_id: Optional[InstrumentId] = None,
    batch_size: int = 10_000,
    start_nanos: int = None,
    end_nanos: int = None,
) -> Generator[list[Data], None, None]:
    batches = _generate_batches(
        files=files,
        cls=cls,
        instrument_id=instrument_id,
        fs=fs,
        batch_size=batch_size,
    )
    yield from _generate_batches_within_time_range(batches, start_nanos, end_nanos)
