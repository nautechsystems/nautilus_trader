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

import heapq
import itertools
import sys
from collections import namedtuple
from typing import Dict, Iterator, List, Set

import fsspec
import pandas as pd
import pyarrow.dataset as ds
import pyarrow.parquet as pq
from pyarrow.lib import ArrowInvalid

from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.funcs import parse_bytes
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.util import clean_key


FileMeta = namedtuple("FileMeta", "filename datatype instrument_id client_id start end")


def dataset_batches(
    file_meta: FileMeta,
    fs: fsspec.AbstractFileSystem,
    n_rows: int,
) -> Iterator[pd.DataFrame]:
    try:
        d: ds.Dataset = ds.dataset(file_meta.filename, filesystem=fs)
    except ArrowInvalid:
        return
    for fn in sorted(map(str, d.files)):
        f = pq.ParquetFile(fs.open(fn))
        for batch in f.iter_batches(batch_size=n_rows):
            if batch.num_rows == 0:
                break
            df = batch.to_pandas()
            df = df[(df["ts_init"] >= file_meta.start) & (df["ts_init"] <= file_meta.end)]
            if df.empty:
                continue
            if file_meta.instrument_id:
                df.loc[:, "instrument_id"] = file_meta.instrument_id
            yield df


def build_filenames(
    catalog: ParquetDataCatalog,
    data_configs: List[BacktestDataConfig],
) -> List[FileMeta]:
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


def frame_to_nautilus(df: pd.DataFrame, cls: type):
    return ParquetSerializer.deserialize(cls=cls, chunk=df.to_dict("records"))


def batch_files(  # noqa: C901
    catalog: ParquetDataCatalog,
    data_configs: List[BacktestDataConfig],
    read_num_rows: int = 10000,
    target_batch_size_bytes: int = parse_bytes("100mb"),  # noqa: B008,
):
    files = build_filenames(catalog=catalog, data_configs=data_configs)
    buffer = {fn.filename: pd.DataFrame() for fn in files}
    datasets = {
        f.filename: dataset_batches(file_meta=f, fs=catalog.fs, n_rows=read_num_rows) for f in files
    }
    completed: Set[str] = set()
    bytes_read = 0
    values = []
    sent_count = 0
    while set([f.filename for f in files]) != completed:
        # Fill buffer (if required)
        for fn in buffer:
            if len(buffer[fn]) < read_num_rows:
                next_buf = next(datasets[fn], None)
                if next_buf is None:
                    completed.add(fn)
                    continue
                buffer[fn] = pd.concat([buffer[fn], next_buf])

        # Determine minimum timestamp
        max_ts_per_frame = {fn: df["ts_init"].max() for fn, df in buffer.items() if not df.empty}
        if not max_ts_per_frame:
            continue
        min_ts = min(max_ts_per_frame.values())

        # Filter buffer dataframes based on min_timestamp
        batches = []
        for f in files:
            df = buffer[f.filename]
            if df.empty:
                continue
            ts_filter = df["ts_init"] <= min_ts  # min of max timestamps
            batch = df[ts_filter]
            buffer[f.filename] = df[~ts_filter]
            objs = frame_to_nautilus(df=batch, cls=f.datatype)
            batches.append(objs)
            bytes_read += sum([sys.getsizeof(x) for x in objs])

        # Merge ticks
        values.extend(list(heapq.merge(*batches, key=lambda x: x.ts_init)))
        if bytes_read > target_batch_size_bytes:
            yield values
            sent_count += len(values)
            bytes_read = 0
            values = []

    if values:
        yield values
        sent_count += len(values)

    if sent_count == 0:
        raise ValueError("No data found, check data_configs")


def groupby_datatype(data):
    def _groupby_key(x):
        return type(x).__name__

    return [
        {"type": type(v[0]), "data": v}
        for v in [
            list(v) for _, v in itertools.groupby(sorted(data, key=_groupby_key), key=_groupby_key)
        ]
    ]


def extract_generic_data_client_ids(data_configs: List[BacktestDataConfig]) -> Dict:
    """
    Extract a mapping of data_type : client_id from the list of `data_configs`.
    In the process of merging the streaming data, we lose the `client_id` for
    generic data, we need to inject this back in so the backtest engine can be
    correctly loaded.
    """
    data_client_ids = [
        (config.data_type, config.client_id) for config in data_configs if config.client_id
    ]
    assert len(set(data_client_ids)) == len(
        dict(data_client_ids)
    ), "data_type found with multiple client_ids"
    return dict(data_client_ids)
