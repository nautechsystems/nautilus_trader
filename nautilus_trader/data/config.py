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

from __future__ import annotations

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.model.identifiers import ClientId


class DataEngineConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``DataEngine`` instances.

    Parameters
    ----------
    time_bars_interval_type : str, default 'left-open'
        Determines the type of interval used for time aggregation.
        - 'left-open': start time is excluded and end time is included (default).
        - 'right-open': start time is included and end time is excluded.
    time_bars_timestamp_on_close : bool, default True
        If time bar aggregators will timestamp `ts_event` on bar close.
        If False, then will timestamp on bar open.
    time_bars_skip_first_non_full_bar : bool, default False
        If time bar aggregators will skip emitting a bar if the aggregation starts mid-interval.
    time_bars_build_with_no_updates : bool, default True
        If time bar aggregators will build and emit bars with no new market updates.
    time_bars_origins : dict[BarAggregation, pd.Timedelta | pd.DateOffset], optional
        A dictionary mapping time bar aggregations to their origin time offsets.
    validate_data_sequence : bool, default False
        If data objects timestamp sequencing will be validated and handled.
    buffer_deltas : bool, default False
        If order book deltas should be buffered until the F_LAST flag is set for a delta.
    external_clients : list[ClientId], optional
        The client IDs declared for external stream processing.
        The data engine will not attempt to send data commands to these client IDs.
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    time_bars_interval_type: str = "left-open"
    time_bars_timestamp_on_close: bool = True
    time_bars_skip_first_non_full_bar: bool = False
    time_bars_build_with_no_updates: bool = True
    time_bars_origins: dict | None = None
    validate_data_sequence: bool = False
    buffer_deltas: bool = False
    external_clients: list[ClientId] | None = None
    debug: bool = False
