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


def convert_series_to_dict(series: pd.Series) -> dict[int, float]:
    """
    Convert pandas Series to dict with unix nanoseconds (or integer keys).
    """
    if series.empty:
        return {}
    result = {}
    for idx, val in series.items():
        # Check if index is datetime (has .value attribute for nanoseconds)
        if hasattr(idx, "value"):
            key = idx.value  # Direct nanosecond value, no float precision loss
        else:
            # Use integer index directly (convert to nanoseconds for consistency)
            key = int(idx) * 1_000_000_000
        result[key] = float(val)
    return result
