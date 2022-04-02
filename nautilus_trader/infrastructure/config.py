# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import pydantic


class CacheDatabaseConfig(pydantic.BaseModel):
    """
    Configuration for ``CacheDatabase`` instances.

    Parameters
    ----------
    type : str, {'in-memory', 'redis'}, default 'in-memory'
        The database type.
    host : str, default 'localhost'
        The database host address (default for Redis).
    port : int, default 6379
        The database port (default for Redis).
    flush : bool, default False
        If database should be flushed before start.
    """

    type: str = "in-memory"
    host: str = "localhost"
    port: int = 6379
    flush: bool = False
