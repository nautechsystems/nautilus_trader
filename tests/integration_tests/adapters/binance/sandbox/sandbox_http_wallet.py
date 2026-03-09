import json
import os

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.spot.http.wallet import BinanceSpotWalletHttpAPI
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio
async def test_binance_spot_wallet_http_client():
    clock = LiveClock()

    client = get_cached_binance_http_client(
        clock=clock,
        account_type=BinanceAccountType.SPOT,
        api_key=os.getenv("BINANCE_API_KEY"),
        api_secret=os.getenv("BINANCE_API_SECRET"),
    )

    wallet = BinanceSpotWalletHttpAPI(clock=clock, client=client)
    response = await wallet.trade_fee_spot(symbol="BTCUSDT")
    print(json.dumps(response, indent=4))
