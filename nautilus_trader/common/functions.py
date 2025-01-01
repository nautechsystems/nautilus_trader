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

import pandas as pd

from nautilus_trader.core.datetime import format_iso8601


def format_utc_timerange(start: pd.Timestamp | None, end: pd.Timestamp | None) -> str:
    """
    Return a formatted time range string based on start and end timestamps (UTC).

    Parameters
    ----------
    start : pd.Timestamp | None
        The start timestamp (UTC).
    end : pd.Timestamp | None
        The end timestamp (UTC).

    Returns
    -------
    str

    """
    if start and end:
        return f" from {format_iso8601(start)} to {format_iso8601(end)}"
    elif start:
        return f" from {format_iso8601(start)}"
    elif end:
        return f" to {format_iso8601(end)}"
    else:
        return ""
