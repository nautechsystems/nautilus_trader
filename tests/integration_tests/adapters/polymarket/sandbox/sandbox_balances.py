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

    token_id = "3642309182816755995211647069086230404892359515361325090555875625429003317932"
    params = BalanceAllowanceParams(
        asset_type=AssetType.CONDITIONAL,
        token_id=token_id,
    )
    response = http_client.get_balance_allowance(params)
    balance_usdc = usdce_from_units(int(response["balance"]))
    print(f"Balance {token_id}: {balance_usdc}")


if __name__ == "__main__":
    get_allowances()
