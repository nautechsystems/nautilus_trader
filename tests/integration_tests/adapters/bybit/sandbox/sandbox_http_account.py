import json

import msgspec
import pytest

from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.adapters.bybit.factories import get_cached_bybit_http_client
from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


@pytest.mark.asyncio()
async def test_bybit_account_http_client():
    clock = LiveClock()

    client = get_cached_bybit_http_client(
        clock=clock,
        logger=Logger(clock=clock),
        is_testnet=True,
    )

    http_account = BybitAccountHttpAPI(
        clock=clock,
        client=client,
        account_type=BybitAccountType.LINEAR,
    )

    ################################################################################
    # Account balance
    ################################################################################
    account_balance = await http_account.query_wallet_balance()
    for item in account_balance:
        print(json.dumps(msgspec.to_builtins(item), indent=4))
