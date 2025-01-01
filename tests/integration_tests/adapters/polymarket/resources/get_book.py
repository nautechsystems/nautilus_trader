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


def main():
    print("Requesting book")
    client = get_polymarket_http_client()
    token_id = 23360939988679364027624185518382759743328544433592111535569478055890815567848
    response = client.get_order_book(token_id)

    data = msgspec.json.encode(response)
    Path("http_responses/book.json").write_bytes(data)


if __name__ == "__main__":
    main()
