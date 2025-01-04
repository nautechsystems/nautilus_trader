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

from py_clob_client.client import BalanceAllowanceParams
from py_clob_client.clob_types import AssetType

from nautilus_trader.adapters.polymarket.common.conversion import usdce_from_units
from nautilus_trader.adapters.polymarket.factories import get_polymarket_http_client


def get_allowances() -> None:
    http_client = get_polymarket_http_client()

    # Check USDC wallet balance
    params = BalanceAllowanceParams(
        asset_type=AssetType.COLLATERAL,
    )
    response = http_client.get_balance_allowance(params)
    balance_usdc = usdce_from_units(int(response["balance"]))
    print(f"Wallet: {balance_usdc}")

    # Check Trump 'Yes' position
    token_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
    params = BalanceAllowanceParams(
        asset_type=AssetType.CONDITIONAL,
        token_id=token_id,
    )
    response = http_client.get_balance_allowance(params)
    balance_usdc = usdce_from_units(int(response["balance"]))
    print(f"Balance {token_id}: {balance_usdc}")

    # Check Trump 'No' position
    token_id = "48331043336612883890938759509493159234755048973500640148014422747788308965732"
    params = BalanceAllowanceParams(
        asset_type=AssetType.CONDITIONAL,
        token_id=token_id,
    )
    response = http_client.get_balance_allowance(params)
    balance_usdc = usdce_from_units(int(response["balance"]))
    print(f"Balance {token_id}: {balance_usdc}")

    # Check Kamala 'Yes' position
    token_id = "69236923620077691027083946871148646972011131466059644796654161903044970987404"
    params = BalanceAllowanceParams(
        asset_type=AssetType.CONDITIONAL,
        token_id=token_id,
    )
    response = http_client.get_balance_allowance(params)
    balance_usdc = usdce_from_units(int(response["balance"]))
    print(f"Balance {token_id}: {balance_usdc}")

    # Check Kamala 'No' position
    token_id = "87584955359245246404952128082451897287778571240979823316620093987046202296181"
    params = BalanceAllowanceParams(
        asset_type=AssetType.CONDITIONAL,
        token_id=token_id,
    )
    response = http_client.get_balance_allowance(params)
    balance_usdc = usdce_from_units(int(response["balance"]))
    print(f"Balance {token_id}: {balance_usdc}")


if __name__ == "__main__":
    get_allowances()
