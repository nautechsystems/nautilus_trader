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

from nautilus_trader.common.config import DatabaseConfig
from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import PositiveInt


class CacheConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``Cache`` instances.

    Parameters
    ----------
    database : DatabaseConfig, optional
        The configuration for the cache backing database.
    encoding : str, {'msgpack', 'json'}, default 'msgpack'
        The encoding for database operations, controls the type of serializer used.
    timestamps_as_iso8601 : bool, default False
        If timestamps should be persisted as ISO 8601 strings.
        If `False` then will persist as UNIX nanoseconds.
    persist_account_events : bool, default True
        If account state events are written to the backing database.
        Set to `False` in place of purging account state events.
    buffer_interval_ms : PositiveInt, optional
        The buffer interval (milliseconds) between pipelined/batched transactions.
        The recommended range if using buffered pipelining is [10, 1000] milliseconds,
        with a good compromise being 100 milliseconds.
    use_trader_prefix : bool, default True
        If a 'trader-' prefix is used for keys.
    use_instance_id : bool, default False
        If the traders instance ID is used for keys.
    flush_on_start : bool, default False
        If database should be flushed on start.
    drop_instruments_on_reset : bool, default True
        If instruments data should be dropped from the caches memory on reset.
    tick_capacity : PositiveInt, default 10_000
        The maximum length for internal tick dequeues.
    bar_capacity : PositiveInt, default 10_000
        The maximum length for internal bar dequeues.

    """

    database: DatabaseConfig | None = None
    encoding: str = "msgpack"
    timestamps_as_iso8601: bool = False
    persist_account_events: bool = True
    buffer_interval_ms: PositiveInt | None = None
    use_trader_prefix: bool = True
    use_instance_id: bool = False
    flush_on_start: bool = False
    drop_instruments_on_reset: bool = True
    tick_capacity: PositiveInt = 10_000
    bar_capacity: PositiveInt = 10_000
