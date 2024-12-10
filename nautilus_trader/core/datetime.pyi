# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import datetime
from typing import Final

import pandas as pd

# Re-exports
from nautilus_trader.core.nautilus_pyo3 import micros_to_nanos as micros_to_nanos
from nautilus_trader.core.nautilus_pyo3 import millis_to_nanos as millis_to_nanos
from nautilus_trader.core.nautilus_pyo3 import nanos_to_micros as nanos_to_micros
from nautilus_trader.core.nautilus_pyo3 import nanos_to_millis as nanos_to_millis
from nautilus_trader.core.nautilus_pyo3 import nanos_to_secs as nanos_to_secs
from nautilus_trader.core.nautilus_pyo3 import secs_to_millis as secs_to_millis
from nautilus_trader.core.nautilus_pyo3 import secs_to_nanos as secs_to_nanos

# UNIX epoch is the UTC time at 00:00:00 on 1/1/1970
# https://en.wikipedia.org/wiki/Unix_time
UNIX_EPOCH: Final[pd.Timestamp]

def unix_nanos_to_dt(nanos: int) -> pd.Timestamp:
    """
    Return the datetime (UTC) from the given UNIX timestamp (nanoseconds).

    Parameters
    ----------
    nanos : int
        The UNIX timestamp (nanoseconds) to convert.

    Returns
    -------
    pd.Timestamp
    """

def dt_to_unix_nanos(dt: pd.Timestamp) -> int:
    """
    Return the UNIX timestamp (nanoseconds) from the given datetime (UTC).

    Parameters
    ----------
    dt : pd.Timestamp
        The datetime to convert.

    Returns
    -------
    int

    Warnings
    --------
    This function expects a pandas `Timestamp` as standard Python `datetime`
    objects are only accurate to 1 microsecond (μs).
    """

def unix_nanos_to_str(unix_nanos: int) -> str:
    """
    Convert the given `unix_nanos` to an ISO 8601 formatted string.

    Parameters
    ----------
    unix_nanos : int
        The UNIX timestamp (nanoseconds) to be converted.

    Returns
    -------
    str
    """

def maybe_unix_nanos_to_dt(nanos: int | None) -> pd.Timestamp | None:
    """
    Return the datetime (UTC) from the given UNIX timestamp (nanoseconds), or ``None``.

    If nanos is ``None``, then will return ``None``.

    Parameters
    ----------
    nanos : int, optional
        The UNIX timestamp (nanoseconds) to convert.

    Returns
    -------
    pd.Timestamp or ``None``
    """

def maybe_dt_to_unix_nanos(dt: pd.Timestamp | None) -> int | None:
    """
    Return the UNIX timestamp (nanoseconds) from the given datetime, or ``None``.

    If dt is ``None``, then will return ``None``.

    Parameters
    ----------
    dt : pd.Timestamp, optional
        The datetime to convert.

    Returns
    -------
    int or ``None``

    Warnings
    --------
    If the input is not ``None`` then this function expects a pandas `Timestamp`
    as standard Python `datetime` objects are only accurate to 1 microsecond (μs).
    """

def is_datetime_utc(dt: datetime) -> bool:
    """
    Return a value indicating whether the given timestamp is timezone aware UTC.

    Parameters
    ----------
    dt : datetime
        The datetime to check.

    Returns
    -------
    bool
        True if timezone aware UTC, else False.
    """

def is_tz_aware(time_object: datetime | pd.DataFrame) -> bool:
    """
    Return a value indicating whether the given object is timezone aware.

    Parameters
    ----------
    time_object : datetime, pd.Timestamp, pd.Series, pd.DataFrame
        The time object to check.

    Returns
    -------
    bool
        True if timezone aware, else False.
    """

def is_tz_naive(time_object: datetime | pd.DataFrame) -> bool:
    """
    Return a value indicating whether the given object is timezone naive.

    Parameters
    ----------
    time_object : datetime, pd.Timestamp, pd.DataFrame
        The time object to check.

    Returns
    -------
    bool
        True if object timezone naive, else False.
    """

def as_utc_timestamp(dt: datetime) -> datetime:
    """
    Ensure the given timestamp is tz-aware UTC.

    Parameters
    ----------
    dt : datetime
        The timestamp to check.

    Returns
    -------
    datetime
    """

def as_utc_index(data: pd.DataFrame) -> pd.DataFrame:
    """
    Ensure the given data has a DateTimeIndex which is tz-aware UTC.

    Parameters
    ----------
    data : pd.Series or pd.DataFrame
        The object to ensure is UTC.

    Returns
    -------
    pd.Series, pd.DataFrame or ``None``
    """

def time_object_to_dt(time_object: pd.Timestamp | str | int | None) -> datetime | None:
    """
    Return the datetime (UTC) from the given UNIX timestamp as integer (nanoseconds), string or pd.Timestamp.
    Returns None if the input is None.

    Parameters
    ----------
    time_object : pd.Timestamp | str | int | None
        The time object to convert.

    Returns
    -------
    pd.Timestamp
    """

def format_iso8601(dt: datetime) -> str:
    """
    Format the given datetime to a millisecond accurate ISO 8601 specification string.

    Parameters
    ----------
    dt : datetime
        The datetime to format.

    Returns
    -------
    str
    """

def max_date(
    date1: pd.Timestamp | str | int | None = None,
    date2: str | int | None = None,
) -> pd.Timestamp | None:
    """
    Return the maximum date as a datetime (UTC).

    Parameters
    ----------
    date1 : pd.Timestamp | str | int | None, optional
        The first date to compare. Can be a string, integer (timestamp), or None. Default is None.
    date2 : pd.Timestamp | str | int | None, optional
        The second date to compare. Can be a string, integer (timestamp), or None. Default is None.

    Returns
    -------
    pd.Timestamp | None
        The maximum date as an ISO 8601 formatted string, or None if both input dates are None.
    """

def min_date(
    date1: pd.Timestamp | str | int | None = None,
    date2: str | int | None = None,
) -> pd.Timestamp | None:
    """
    Return the minimum date as a datetime (UTC).

    Parameters
    ----------
    date1 : pd.Timestamp | str | int | None, optional
        The first date to compare. Can be a string, integer (timestamp), or None. Default is None.
    date2 : pd.Timestamp | str | int | None, optional
        The second date to compare. Can be a string, integer (timestamp), or None. Default is None.

    Returns
    -------
    pd.Timestamp | None
        The minimum date as an ISO 8601 formatted string, or None if both input dates are None.
    """
