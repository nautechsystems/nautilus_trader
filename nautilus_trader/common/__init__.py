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

"""
The `common` subpackage provides generic/common parts for assembling the frameworks various components.

More domain specific concepts are introduced above the `core` base layer. The
ID cache is implemented, a base `Clock` with `Test` and `Live`
implementations which can control many `Timer` instances.

Trading domain specific components for generating `Order` and `Identifier` objects.
Common logging components. A high performance `Queue`. Common `UUID4` factory.
"""

from enum import Enum
from enum import unique


@unique
class Environment(Enum):
    """
    Represents the environment context for a Nautilus system.
    """

    BACKTEST = "backtest"
    SANDBOX = "sandbox"
    LIVE = "live"
