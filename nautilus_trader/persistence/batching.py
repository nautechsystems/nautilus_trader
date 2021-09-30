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
from collections import namedtuple
from typing import Any, List, Set

import fsspec
import pandas as pd
import pyarrow.dataset as ds

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.serialization.arrow.util import class_to_filename


FileMeta = namedtuple("FileMeta", "filename datatype instrument_id")


def dataset_batches(
    file_meta: FileMeta, fs: fsspec.AbstractFileSystem, batch_size: int, filter_expr=None
):
    d = ds.dataset(file_meta.filename, filesystem=fs)  # type: ds.Dataset
    scanner = d.scanner(filter=filter_expr, batch_size=batch_size)  # type: ds.Scanner
    for batch in scanner.to_batches():
        if batch.num_rows == 0:
            break
        data = batch.to_pandas()
        data.loc[:, "instrument_id"] = file_meta.instrument_id
        yield batch.nbytes, data


def build_filenames(catalog: DataCatalog, data_configs: List[BacktestDataConfig]) -> List[FileMeta]:
    files = []
    for config in data_configs:
        filename = f"{catalog.path}/data/{class_to_filename(config.data_type)}.parquet/instrument_id={config.instrument_id}"
        if not catalog.fs.exists(filename):
            continue
        files.append(
            FileMeta(
                filename=filename, datatype=config.data_type, instrument_id=config.instrument_id
            )
        )
    return files


def frame_to_nautilus(df: pd.DataFrame, cls: type) -> List[Any]:
    return [cls.from_dict(d) for d in df.to_dict("records")]  # type: ignore


def batch_files(
    catalog: DataCatalog,
    data_configs: List[BacktestDataConfig],
    start_time: int,
    end_time: int,
    batch_size: int = 10000,
):
    filter_expr = (ds.field("ts_init") > start_time) & (ds.field("ts_init") < end_time)
    files = build_filenames(catalog=catalog, data_configs=data_configs)
    buffer = {fn.filename: pd.DataFrame() for fn in files}
    datasets = {
        f.filename: dataset_batches(
            file_meta=f, fs=catalog.fs, batch_size=batch_size, filter_expr=filter_expr
        )
        for f in files
    }
    completed: Set[str] = set()

    while set([f.filename for f in files]) != completed:
        # Fill buffer (if required)
        for fn in buffer:
            if len(buffer[fn]) < batch_size:
                next_buf = next(datasets[fn], None)
                if next_buf is None:
                    completed.add(fn)
                    continue
                buffer[fn] = buffer[fn].append(next_buf)

        # Determine minimum timestamp
        max_ts_per_frame = [df["ts_init"].max() for df in buffer.values()]
        min_ts = min(max_ts_per_frame)

        # Filter buffer dataframes based on min_timestamp
        batches = []
        for f in files:
            df = buffer[f.filename]
            ts_filter = df["ts_init"] <= min_ts
            batch = df[ts_filter]
            buffer[f.filename] = df[~ts_filter]
            # print(f"{f.filename} batch={len(batch)} buffer={len(buffer)}")
            batches.append(frame_to_nautilus(df=batch, cls=f.datatype))

        # Merge ticks
        values = list(heapq.merge(*batches, key=lambda x: x.ts_init))
        yield values
