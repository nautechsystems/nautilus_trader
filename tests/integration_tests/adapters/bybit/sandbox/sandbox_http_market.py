import json

import msgspec
import pytest

from nautilus_trader.common.clock import LiveClock

from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.adapters.bybit.factories import get_cached_bybit_http_client
from nautilus_trader.common.logging import Logger

from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI


@pytest.mark.asyncio()
async def test_bybit_market_http_client():
    clock = LiveClock()

    client = get_cached_bybit_http_client(
        clock=clock,
        logger=Logger(clock=clock),
        is_testnet=True,
    )

    http_account_linear = BybitMarketHttpAPI(
        clock=clock,
        client=client,
        account_type=BybitAccountType.LINEAR,
    )

    ################################################################################
    # Instruments - Linear
    ################################################################################
    instruments = await http_account_linear.fetch_instruments()
    target_instruments = ['BTCUSDT','ETHUSDT']
    for item in instruments:
        if item.symbol in target_instruments:
            print(json.dumps(msgspec.to_builtins(item), indent=4))

