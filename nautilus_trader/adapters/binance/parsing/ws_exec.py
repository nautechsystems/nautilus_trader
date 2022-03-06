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

from typing import Dict, List

from nautilus_trader.adapters.binance.parsing.common import parse_balances_futures
from nautilus_trader.adapters.binance.parsing.common import parse_balances_spot
from nautilus_trader.model.objects import AccountBalance


def parse_account_balances_spot_ws(raw_balances: List[Dict[str, str]]) -> List[AccountBalance]:
    return parse_balances_spot(raw_balances, "a", "f", "l")


def parse_account_balances_futures_ws(raw_balances: List[Dict[str, str]]) -> List[AccountBalance]:
    return parse_balances_futures(raw_balances, "a", "wb", "bc", "bc")  # TODO(cs): Implement
