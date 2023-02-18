import inspect
import os
import re
import sys
from typing import Optional

import pandas as pd

from nautilus_trader.core.nautilus_pyo3.persistence import ParquetType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick


class Singleton(type):
    """
    The base class to ensure a singleton.
    """

    def __init__(cls, name, bases, dict_like):
        super().__init__(name, bases, dict_like)
        cls._instances = {}

    def __call__(cls, *args, **kw):
        full_kwargs = resolve_kwargs(cls.__init__, None, *args, **kw)
        if full_kwargs == {"self": None, "args": (), "kwargs": {}}:
            full_kwargs = {}
        full_kwargs.pop("self", None)
        key = tuple(full_kwargs.items())
        if key not in cls._instances:
            cls._instances[key] = super().__call__(*args, **kw)
        return cls._instances[key]


def clear_singleton_instances(cls: type):
    assert isinstance(cls, Singleton)
    cls._instances = {}


def resolve_kwargs(func, *args, **kwargs):
    kw = inspect.getcallargs(func, *args, **kwargs)
    return {k: check_value(v) for k, v in kw.items()}


def check_value(v):
    if isinstance(v, dict):
        return freeze_dict(dict_like=v)
    return v


def freeze_dict(dict_like: dict):
    return tuple(sorted(dict_like.items()))


def parse_filename(fn: str) -> tuple[Optional[int], Optional[int]]:
    match = re.match(r"\d{19}-\d{19}", fn)

    if match is None:
        return (None, None)

    parts = fn.split("-")
    return int(parts[0]), int(parts[1])


def is_filename_in_time_range(fn: str, start: Optional[int], end: Optional[int]) -> bool:
    """
    Return True if a filename is within a start and end timestamp range.
    """
    timestamps = parse_filename(fn)
    if timestamps == (None, None):
        return False  # invalid filename

    if start is None and end is None:
        return True

    if start is None:
        start = 0
    if end is None:
        end = sys.maxsize

    a, b = start, end
    x, y = timestamps

    no_overlap = y < a or b < x

    return not no_overlap


def parse_filename_start(fn: str) -> Optional[tuple[str, pd.Timestamp]]:
    """
    Parse start time by filename.

    >>> parse_filename('/data/test/sample.parquet/instrument_id=a/1577836800000000000-1578182400000000000-0.parquet')
    '1577836800000000000'

    >>> parse_filename(1546383600000000000-1577826000000000000-SIM-1-HOUR-BID-EXTERNAL-0.parquet)
    '1546383600000000000'

    >>> parse_filename('/data/test/sample.parquet/instrument_id=a/0648140b1fd7491a97983c0c6ece8d57.parquet')

    """
    instrument_id = re.findall(r"instrument_id\=(.*)\/", fn)[0] if "instrument_id" in fn else None

    start, _ = parse_filename(os.path.basename(fn))

    if start is None:
        return None

    start = pd.Timestamp(start)
    return instrument_id, start


def py_type_to_parquet_type(cls: type) -> ParquetType:
    if cls == QuoteTick:
        return ParquetType.QuoteTick
    elif cls == TradeTick:
        return ParquetType.TradeTick
    else:
        raise RuntimeError(f"Type {cls} not supported as a `ParquetType` yet.")
