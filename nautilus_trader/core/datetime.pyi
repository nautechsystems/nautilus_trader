# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import datetime as dt
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


# UNIX epoch is the UTC time at midnight on 1970-01-01
UNIX_EPOCH: Final[pd.Timestamp]

def unix_nanos_to_dt(nanos: int) -> pd.Timestamp: ...
def dt_to_unix_nanos(dt: pd.Timestamp | str | int) -> int: ...
def unix_nanos_to_iso8601(unix_nanos: int, nanos_precision: bool = True) -> str: ...
def format_iso8601(dt: dt.datetime, nanos_precision: bool = True) -> str: ...
def format_optional_iso8601(dt: dt.datetime | None, nanos_precision: bool = True) -> str: ...
def maybe_unix_nanos_to_dt(nanos: int | None) -> pd.Timestamp | None: ...
def maybe_dt_to_unix_nanos(dt: pd.Timestamp | None) -> int | None: ...
def is_datetime_utc(dt: dt.datetime) -> bool: ...
def is_tz_aware(time_object: dt.datetime | pd.DataFrame) -> bool: ...
def is_tz_naive(time_object: dt.datetime | pd.DataFrame) -> bool: ...
def as_utc_timestamp(dt: dt.datetime) -> dt.datetime: ...
def as_utc_index(data: pd.DataFrame) -> pd.DataFrame: ...
def time_object_to_dt(time_object: pd.Timestamp | str | int | None) -> dt.datetime | None: ...
def max_date(
    date1: pd.Timestamp | str | int | None = None,
    date2: str | int | None = None,
) -> pd.Timestamp | None: ...
def min_date(
    date1: pd.Timestamp | str | int | None = None,
    date2: str | int | None = None,
) -> pd.Timestamp | None: ...
def ensure_pydatetime_utc(timestamp: pd.Timestamp | None) -> dt.datetime | None: ...
