# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from pydantic import PositiveInt


class CacheConfig(pydantic.BaseModel):
    """
    Configuration for ``Cache`` instances.

    Parameters
    ----------
    tick_capacity : int
        The maximum length for internal tick deques.
    bar_capacity : int
        The maximum length for internal bar deques.
    """

    tick_capacity: PositiveInt = 1000
    bar_capacity: PositiveInt = 1000
