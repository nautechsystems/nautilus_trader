from nautilus_trader.adapters.bybit.common.enums import BybitAccountType


def get_category_from_account_type(account_type: BybitAccountType) -> str:
    if account_type == BybitAccountType.SPOT:
        return "spot"
    elif account_type == BybitAccountType.LINEAR:
        return "linear"
    elif account_type == BybitAccountType.INVERSE:
        return "inverse"
    else:
        raise ValueError(f"Unknown account type: {account_type}")
