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

import heapq
import itertools
import sys
from collections.abc import Generator

import fsspec
import numpy as np

from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.core.data import Data
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.funcs import parse_bytes
from nautilus_trader.persistence.streaming.batching import generate_batches
from nautilus_trader.persistence.streaming.batching import generate_batches_rust


class _StreamingBuffer:
    def __init__(self, batches: Generator):
        self._data: list = []
        self._is_complete = False
        self._batches = batches
        self._size = 10_000

    @property
    def is_complete(self) -> bool:
        return self._is_complete and len(self) == 0

    def remove_front(self, timestamp_ns: int) -> list:
        if len(self) == 0 or timestamp_ns < self._data[0].ts_init:
            return []  # nothing to remove

        timestamps = np.array([x.ts_init for x in self._data])
        mask = timestamps <= timestamp_ns
        removed = list(itertools.compress(self._data, mask))
        self._data = list(itertools.compress(self._data, np.invert(mask)))
        return removed

    def add_data(self) -> None:
        if len(self) >= self._size:
            return  # buffer filled already

        objs = next(self._batches, None)
        if objs is None:
            self._is_complete = True
        else:
            self._data.extend(objs)

    @property
    def max_timestamp(self) -> int:
        return self._data[-1].ts_init

    def __len__(self) -> int:
        return len(self._data)

    def __repr__(self):
        return f"{self.__class__.__name__}({len(self)})"


class _BufferIterator:
    """
    Streams merged batches of nautilus objects from _StreamingBuffer objects
    """

    def __init__(
        self,
        buffers: list[_StreamingBuffer],
        target_batch_size_bytes: int = parse_bytes("100mb"),  # noqa: B008,
    ):
        self._buffers = buffers
        self._target_batch_size_bytes = target_batch_size_bytes

    def __iter__(self) -> Generator[list[Data], None, None]:
        yield from self._iterate_batches_to_target_memory()

    def _iterate_batches_to_target_memory(self) -> Generator[list[Data], None, None]:
        bytes_read = 0
        values = []

        for objs in self._iterate_batches():
            values.extend(objs)

            bytes_read += sum([sys.getsizeof(x) for x in values])

            if bytes_read > self._target_batch_size_bytes:
                yield values
                bytes_read = 0
                values = []

        if values:  # yield remaining values
            yield values

    def _iterate_batches(self) -> Generator[list[Data], None, None]:
        while True:
            for buffer in self._buffers:
                buffer.add_data()

            self._remove_completed()

            if len(self._buffers) == 0:
                return  # stop iterating

            yield self._remove_front()

            self._remove_completed()

    def _remove_front(self) -> list[Data]:
        # Get the timestamp to trim at (the minimum of the maximum timestamps)
        trim_timestamp = min(buffer.max_timestamp for buffer in self._buffers if len(buffer) > 0)

        # Trim front of buffers by timestamp
        chunks = []
        for buffer in self._buffers:
            chunk = buffer.remove_front(trim_timestamp)
            if chunk == []:
                continue
            chunks.append(chunk)

        if not chunks:
            return []

        # Merge chunks together
        objs = list(heapq.merge(*chunks, key=lambda x: x.ts_init))
        return objs

    def _remove_completed(self) -> None:
        self._buffers = [b for b in self._buffers if not b.is_complete]


class StreamingEngine(_BufferIterator):
    """
    Streams merged batches of nautilus objects from BacktestDataConfig objects

    """

    def __init__(
        self,
        data_configs: list[BacktestDataConfig],
        target_batch_size_bytes: int = parse_bytes("100mb"),  # noqa: B008,
    ):
        # Sort configs (larger time_aggregated bar specifications first)
        # Define the order of objects with the same timestamp.
        # Larger bar aggregations first. H4 > H1
        def _sort_larger_specifications_first(config) -> tuple[int, int]:
            if config.bar_spec is None:
                return sys.maxsize, sys.maxsize  # last
            else:
                spec = BarSpecification.from_str(config.bar_spec)
                return spec.aggregation * -1, spec.step * -1

        self._configs = sorted(data_configs, key=_sort_larger_specifications_first)

        buffers = list(map(self._config_to_buffer, data_configs))

        super().__init__(
            buffers=buffers,
            target_batch_size_bytes=target_batch_size_bytes,
        )

    @staticmethod
    def _config_to_buffer(config: BacktestDataConfig) -> _StreamingBuffer:
        if config.data_type is Bar:
            assert config.bar_spec

        files = config.catalog().get_files(
            cls=config.data_type,
            instrument_id=config.instrument_id,
            start_nanos=config.start_time_nanos,
            end_nanos=config.end_time_nanos,
            bar_spec=BarSpecification.from_str(config.bar_spec) if config.bar_spec else None,
        )
        assert files, f"No files found for {config}"
        if config.use_rust:
            batches = generate_batches_rust(
                files=files,
                cls=config.data_type,
                batch_size=config.batch_size,
                start_nanos=config.start_time_nanos,
                end_nanos=config.end_time_nanos,
            )
        else:
            batches = generate_batches(
                files=files,
                cls=config.data_type,
                instrument_id=InstrumentId.from_str(config.instrument_id)
                if config.instrument_id
                else None,
                fs=fsspec.filesystem(config.catalog_fs_protocol or "file"),
                batch_size=config.batch_size,
                start_nanos=config.start_time_nanos,
                end_nanos=config.end_time_nanos,
            )

        return _StreamingBuffer(batches=batches)


def extract_generic_data_client_ids(data_configs: list["BacktestDataConfig"]) -> dict:
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
        dict(data_client_ids),
    ), "data_type found with multiple client_ids"
    return dict(data_client_ids)


def groupby_datatype(data):
    def _groupby_key(x):
        return type(x).__name__

    return [
        {"type": type(v[0]), "data": v}
        for v in [
            list(v) for _, v in itertools.groupby(sorted(data, key=_groupby_key), key=_groupby_key)
        ]
    ]
