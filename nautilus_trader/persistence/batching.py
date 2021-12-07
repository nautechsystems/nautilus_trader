# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import heapq
import sys
from collections import namedtuple
from typing import Any, Iterator, List, Set

import fsspec
import pandas as pd
import pyarrow.dataset as ds
from dask.utils import parse_bytes

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.util import clean_key


FileMeta = namedtuple("FileMeta", "filename datatype instrument_id client_id start end")


def dataset_batches(
    file_meta: FileMeta, fs: fsspec.AbstractFileSystem, n_rows: int
) -> Iterator[pd.DataFrame]:
    d: ds.Dataset = ds.dataset(file_meta.filename, filesystem=fs)
    filter_expr = (ds.field("ts_event") >= file_meta.start) & (
        ds.field("ts_event") <= file_meta.end
    )
    scanner: ds.Scanner = d.scanner(filter=filter_expr, batch_size=n_rows)
    for batch in scanner.to_batches():
        if batch.num_rows == 0:
            break
        data = batch.to_pandas()
        if file_meta.instrument_id:
            data.loc[:, "instrument_id"] = file_meta.instrument_id
        yield data


def build_filenames(catalog: DataCatalog, data_configs: List[BacktestDataConfig]) -> List[FileMeta]:
    files = []
    for config in data_configs:
        filename = catalog._make_path(cls=config.data_type)
        if config.instrument_id:
            filename += f"/instrument_id={clean_key(config.instrument_id)}"
        if not catalog.fs.exists(filename):
            continue
        files.append(
            FileMeta(
                filename=filename,
                datatype=config.data_type,
                instrument_id=config.instrument_id,
                client_id=config.client_id,
                start=config.start_time_nanos,
                end=config.end_time_nanos,
            )
        )
    return files


def frame_to_nautilus(df: pd.DataFrame, cls: type) -> List[Any]:
    return ParquetSerializer.deserialize(cls=cls, chunk=df.to_dict("records"))


def batch_files(
    catalog: DataCatalog,
    data_configs: List[BacktestDataConfig],
    read_num_rows: int = 10000,
    target_batch_size_bytes: int = parse_bytes("100mb"),  # noqa: B008
):
    files = build_filenames(catalog=catalog, data_configs=data_configs)
    buffer = {fn.filename: pd.DataFrame() for fn in files}
    datasets = {
        f.filename: dataset_batches(file_meta=f, fs=catalog.fs, n_rows=read_num_rows) for f in files
    }
    completed: Set[str] = set()
    bytes_read = 0
    values = []
    while set([f.filename for f in files]) != completed:
        # Fill buffer (if required)
        for fn in buffer:
            if len(buffer[fn]) < read_num_rows:
                next_buf = next(datasets[fn], None)
                if next_buf is None:
                    completed.add(fn)
                    continue
                buffer[fn] = buffer[fn].append(next_buf)

        # Determine minimum timestamp
        max_ts_per_frame = [df["ts_event"].max() for df in buffer.values() if not df.empty]
        if not max_ts_per_frame:
            continue
        min_ts = min(max_ts_per_frame)

        # Filter buffer dataframes based on min_timestamp
        batches = []
        for f in files:
            df = buffer[f.filename]
            if df.empty:
                continue
            ts_filter = df["ts_event"] <= min_ts
            batch = df[ts_filter]
            buffer[f.filename] = df[~ts_filter]
            # print(f"{f.filename} batch={len(batch)} buffer={len(buffer)}")
            objs = frame_to_nautilus(df=batch, cls=f.datatype)
            batches.append(objs)
            bytes_read += sum([sys.getsizeof(x) for x in objs])

        # Merge ticks
        values.extend(list(heapq.merge(*batches, key=lambda x: x.ts_event)))
        # print(f"iter complete, {bytes_read=}, flushing at target={target_batch_size_bytes}")
        if bytes_read > target_batch_size_bytes:
            yield values
            bytes_read = 0
            values = []

    if values:
        yield values
