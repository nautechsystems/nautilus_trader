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

from __future__ import annotations

from nautilus_trader.common.config import NautilusConfig


class DataEngineConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``DataEngine`` instances.

    Parameters
    ----------
    time_bars_build_with_no_updates : bool, default True
        If time bar aggregators will build and emit bars with no new market updates.
    time_bars_timestamp_on_close : bool, default True
        If time bar aggregators will timestamp `ts_event` on bar close.
        If False then will timestamp on bar open.
    time_bars_interval_type : str, default 'left-open'
        Determines the type of interval used for time aggregation.
        - 'left-open': start time is excluded and end time is included (default).
        - 'right-open': start time is included and end time is excluded.
    validate_data_sequence : bool, default False
        If data objects timestamp sequencing will be validated and handled.
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    time_bars_build_with_no_updates: bool = True
    time_bars_timestamp_on_close: bool = True
    time_bars_interval_type: str = "left-open"
    validate_data_sequence: bool = False
    debug: bool = False
