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
import datetime
from functools import lru_cache
from typing import List, Optional, Union

import fsspec
import numpy as np
import pandas as pd
import pyarrow.dataset as ds
from dask.base import tokenize
from dask.utils import parse_bytes
from scipy.optimize import minimize

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.serialization.arrow.util import class_to_filename


def make_unix_ns(value: Union[str, datetime.datetime, pd.Timestamp]) -> int:
    ts = pd.Timestamp(value)  # type: ignore
    if not ts.tz:
        ts = ts.tz_localize("UTC")
    return int(ts.to_datetime64())


def _sample_column_widths(dataset: ds.Dataset, column_names, samples=100):
    scanner = dataset.scanner(columns=column_names, batch_size=samples)
    for batch in scanner.to_batches():
        df = batch.to_pandas()
        mem = df.memory_usage(index=False, deep=True) / batch.num_rows
        return mem.to_dict()


@lru_cache
def _get_schema_widths(path: str, fs: Optional[fsspec.AbstractFileSystem] = None, samples=1000):
    widths = {}
    variable_width_columns = []

    dataset = ds.dataset(path, filesystem=fs)
    schema = dataset.schema

    # Get fixed width types from schema
    for n in schema.names:
        try:
            widths[n] = schema.field(n).type.bit_width / 8
        except ValueError:
            variable_width_columns.append(n)

    # Read sample of parquet file to determine variable
    variable_widths = _sample_column_widths(
        dataset=dataset, column_names=variable_width_columns, samples=samples
    )
    widths.update(variable_widths)
    return widths


@lru_cache()
def _get_row_size(path: str, fs: Optional[fsspec.AbstractFileSystem] = None, samples=1000):
    schema_widths = _get_schema_widths(path=path, fs=fs, samples=samples)
    return sum(schema_widths.values())


def _calculate_instrument_data_type_size(
    root_path: str,
    fs: fsspec.AbstractFileSystem,
    instrument_id: str,
    data_type: type,
    start_time: int,
    end_time: int,
):
    fp = f"{root_path}/data/{class_to_filename(data_type)}.parquet/instrument_id={instrument_id}"
    try:
        dataset = ds.dataset(fp, filesystem=fs)
    except FileNotFoundError:
        return 0
    filters = (ds.field("ts_init") >= start_time) & (ds.field("ts_init") < end_time)
    table = dataset.to_table(filter=filters)
    return table.nbytes


def _calculate_data_type_size(
    root_path: str,
    fs: fsspec.AbstractFileSystem,
    instrument_ids: List[str],
    data_type: type,
    start_time: int,
    end_time: int,
):
    size = sum(
        _calculate_instrument_data_type_size(
            root_path, fs, instrument_id, data_type, start_time, end_time
        )
        for instrument_id in instrument_ids
    )
    return size


def calculate_data_size(
    root_path: str,
    fs: fsspec.AbstractFileSystem,
    instrument_ids: List[str],
    data_types: List[type],
    start_time: int,
    end_time: int,
):
    size = sum(
        _calculate_data_type_size(root_path, fs, instrument_ids, data_type, start_time, end_time)
        for data_type in data_types
    )
    return size


def search_data_size_timestamp(
    root_path: str,
    fs: fsspec.AbstractFileSystem,
    instrument_ids,
    data_types,
    start_time,
    target_size=10485760,
):
    def inner(end_time):
        actual_size = calculate_data_size(
            root_path=root_path,
            fs=fs,
            instrument_ids=instrument_ids,
            data_types=data_types,
            start_time=start_time,
            end_time=int(end_time[0]),
        )
        value = abs(target_size - actual_size)
        return value

    return inner


def calc_streaming_batches(
    catalog: "DataCatalog",
    instrument_ids: List[str],
    data_types: List[type],
    start_time: Union[str, datetime.datetime, pd.Timestamp],
    end_time: Union[str, datetime.datetime, pd.Timestamp],
    target_size=parse_bytes("100mib"),  # noqa: B008
    tolerance_pct=0.01,
    debug=False,
):
    """
    Calculate the chunks of data to load for a backtest, given a target chunk size
    """
    start_nanos: int = make_unix_ns(start_time)
    end_nanos: int = make_unix_ns(end_time)
    options = {"disp": True} if debug else {}
    last = (0, 0)
    while True:
        target_func = search_data_size_timestamp(
            root_path=str(catalog.path),
            fs=catalog.fs,
            instrument_ids=instrument_ids,
            data_types=data_types,
            start_time=start_nanos,
            target_size=target_size,
        )
        result = minimize(
            fun=target_func,
            x0=np.asarray([(start_nanos + end_nanos) / 2]),
            method="Powell",
            bounds=((start_nanos, end_nanos),),
            options=options,
            tol=target_size * tolerance_pct,
        )
        assert result.success, "Optimisation did not complete successfully - check inputs"
        end_nanos = int(result.x[0])
        if (start_nanos, end_nanos) == last or (start_nanos, end_nanos) == (last[1], last[1]):
            break
        yield start_nanos, end_nanos
        last = (start_nanos, end_nanos)
        start_nanos = end_nanos
        end_nanos = make_unix_ns(end_time)


def merge_data_configs_for_calc_streaming_chunks(data_configs: List[BacktestDataConfig]):
    instrument_ids = [c.instrument_id for c in data_configs]
    data_types = [c.data_type for c in data_configs]
    starts = [c.start_time for c in data_configs]
    ends = [c.end_time for c in data_configs]
    start = starts[0]
    end = ends[0]
    if len(set(starts)) > 1:
        print("Multiple start dates in data_configs, using min")
        start = min(starts)
    if len(set(ends)) > 1:
        print("Multiple end dates in data_configs, using max")
        end = max(ends)
    return {
        "instrument_ids": instrument_ids,
        "data_types": data_types,
        "start_time": start,
        "end_time": end,
    }


def _cache_batches(func):
    def inner(catalog, data_configs, **kw):
        key = tokenize(data_configs)
        cached = catalog._read_streaming_cache(key=key)
        if cached is not None:
            return cached
        data = func(catalog=catalog, data_configs=data_configs, **kw)
        catalog._write_streaming_cache(key=key, data=data)
        return data

    return inner


@_cache_batches
def generate_data_batches(catalog: DataCatalog, data_configs: List[BacktestDataConfig], batch_size):
    streaming_kw = merge_data_configs_for_calc_streaming_chunks(data_configs=data_configs)
    batches = list(calc_streaming_batches(catalog=catalog, target_size=batch_size, **streaming_kw))
    return batches
