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
from typing import Dict, Generator, List, Optional

import numpy as np
import pandas as pd
import pyarrow.dataset as ds
import pyarrow.parquet as pq
from pyarrow.lib import ArrowInvalid

from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.catalog.rust.reader import ParquetFileReader
from nautilus_trader.persistence.funcs import parse_bytes
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.util import clean_key


def frame_to_nautilus(df: pd.DataFrame, cls: type):
    return ParquetSerializer.deserialize(cls=cls, chunk=df.to_dict("records"))


def generate_batches(  # noqa: C901
    catalog: ParquetDataCatalog, config: BacktestDataConfig, n_rows: int, use_rust: bool = False
) -> Optional[Generator]:

    datatype = config.data_type
    start_time = config.start_time_nanos
    end_time = config.end_time_nanos
    instrument_id = config.instrument_id

    # Get folder in the catalog
    folder = catalog._make_path(cls=config.data_type)
    if config.instrument_id:
        folder += f"/instrument_id={clean_key(config.instrument_id)}"

    if not catalog.fs.exists(folder):
        return None  # no batches available

    # Get files
    try:
        dataset: ds.Dataset = ds.dataset(folder, filesystem=catalog.fs)
    except ArrowInvalid:
        return None  # no batches available

    files = sorted(map(str, dataset.files))

    for fn in files:
        if use_rust and datatype in (QuoteTick, TradeTick):
            yield from ParquetFileReader(parquet_type=datatype, file_path=fn, chunk_size=n_rows)
        else:
            f = pq.ParquetFile(catalog.fs.open(fn))
            for batch in f.iter_batches(batch_size=n_rows):
                if batch.num_rows == 0:
                    break
                df = batch.to_pandas()
                df = df[(df["ts_init"] >= start_time) & (df["ts_init"] <= end_time)]
                if df.empty:
                    continue
                if instrument_id:
                    df.loc[:, "instrument_id"] = instrument_id
                objs = frame_to_nautilus(df=df, cls=datatype)

                yield objs


class Buffer:
    """A buffer that yields batches of nautilus objects. Supports trimming from the front by timestamp"""

    def __init__(
        self, batches: Generator, start_timestamp: int = 0, end_timestamp: int = sys.maxsize
    ):
        self.is_complete = False
        self._batches = batches

        self._buffer: list = []
        self._index: list = []

        self._start_timestamp = start_timestamp
        self._end_timestamp = end_timestamp

    @property
    def max_timestamp(self):
        return self._buffer[-1].ts_init if self._buffer else None

    @property
    def min_timestamp(self):
        return self._buffer[0].ts_init if self._buffer else None

    def __len__(self):
        return len(self._buffer)

    def update(self):

        next_buf = next(self._batches, None)
        if next_buf is None:
            self.is_complete = True
            return

        self._index.extend([x.ts_init for x in next_buf])
        self._buffer.extend(next_buf)

    def pop(self, timestamp_ns: int) -> list:
        has_started = timestamp_ns >= self._start_timestamp
        if not has_started:
            return []

        has_ended = timestamp_ns > self._end_timestamp
        if has_ended:
            timestamp_ns = self._end_timestamp - 1  # -1 = exclusive end
            self.is_complete = True

        # Trim batch start to start_timestamp
        if self.min_timestamp and self.min_timestamp < self._start_timestamp:
            i = self._get_index(self._start_timestamp)
            self._index = self._index[i:]
            self._buffer = self._buffer[i:]

        return self._pop(timestamp_ns)

    def _pop(self, timestamp_ns: int) -> list:
        i = self._get_index(timestamp_ns)
        if i:
            removed = self._buffer[:i]
            self._buffer = self._buffer[i:]
            self._index = self._index[i:]
            assert len(self._buffer) == len(self._index)
            assert self._buffer[0].ts_init == self._index[0]
        else:
            removed = self._buffer
            self._reset()

        return removed

    def _get_index(self, timestamp_ns) -> Optional[int]:
        index = pd.Index(self._index, dtype=np.uint64)
        ts_filter = index > timestamp_ns
        indices = np.where(ts_filter)[0]

        if len(indices):
            return indices[0]
        else:
            return None

    def _reset(self):
        self._buffer: list = []
        self._index: list = []


def batch_files(  # noqa: C901
    catalog: ParquetDataCatalog,
    data_configs: List[BacktestDataConfig],
    read_num_rows: int = 10000,
    target_batch_size_bytes: int = parse_bytes("100mb"),  # noqa: B008,
    use_rust=False,
):
    # Setup buffers
    buffers = []
    for config in data_configs:
        batch_generator = generate_batches(
            catalog=catalog, config=config, n_rows=read_num_rows, use_rust=use_rust
        )
        buffer = Buffer(
            batches=batch_generator,
            start_timestamp=config.start_time_nanos,
            end_timestamp=config.end_time_nanos,
        )
        buffers.append(buffer)

    sent_count = 0
    bytes_read = 0
    values = []
    while buffers:

        # Fill buffer (if required)
        for buffer in buffers:
            if len(buffer) < read_num_rows:
                buffer.update()

        # Update buffers
        buffers = [x for x in buffers if not x.is_complete]

        # Find timestamp
        max_timestamps = list(filter(None, [buffer.max_timestamp for buffer in buffers]))
        if not max_timestamps:
            continue
        min_timestamp = min(max_timestamps)

        # Trim buffers
        batches = [buffer.pop(min_timestamp) for buffer in buffers if len(buffer)]

        # Merge
        values.extend(list(heapq.merge(*batches, key=lambda x: x.ts_init)))

        bytes_read += sum([sys.getsizeof(x) for x in values])
        if bytes_read > target_batch_size_bytes:
            yield values
            sent_count += len(values)
            bytes_read = 0
            values = []


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
