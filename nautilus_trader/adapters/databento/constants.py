# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Final

import pandas as pd

from nautilus_trader.model.identifiers import ClientId


DATABENTO: Final[str] = "DATABENTO"
DATABENTO_CLIENT_ID: Final[ClientId] = ClientId(DATABENTO)

ALL_SYMBOLS: Final[str] = "ALL_SYMBOLS"

ONE_DAY: Final[pd.Timedelta] = pd.Timedelta(days=1)
