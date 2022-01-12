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
    type : str
        The database type.
    host : str
        The database host address.
    port : int
        The database port.
    flush : bool
        If database should be flushed before start.
    """

    type: str = "redis"
    host: str = "localhost"
    port: int = 6379
    flush: bool = False
