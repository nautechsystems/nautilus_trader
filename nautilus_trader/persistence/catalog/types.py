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

from dataclasses import dataclass
from enum import Enum
from enum import unique

from nautilus_trader.core.data import Data
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.instruments import Instrument


@dataclass(frozen=True)
class CatalogDataResult:
    """
    Represents a catalog data query result.
    """

    data_cls: type
    data: list[Data]
    instruments: list[Instrument] | None = None
    client_id: ClientId | None = None


@unique
class CatalogWriteMode(Enum):
    """
    Represents a catalog write mode.
    """

    APPEND = 1
    PREPEND = 2
    OVERWRITE = 3
    NEWFILE = 4
