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

from pathlib import Path

import msgspec

from nautilus_trader.adapters.polymarket.factories import get_polymarket_http_client


def save_historical_trades() -> None:
    client = get_polymarket_http_client()

    response = client.get_trades()
    print(response)

    path = Path("trades_history.json")
    path.write_bytes(msgspec.json.encode(response))


if __name__ == "__main__":
    save_historical_trades()
