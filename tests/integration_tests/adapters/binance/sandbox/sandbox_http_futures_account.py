import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.futures.http.account import BinanceFuturesAccountHttpAPI
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio
async def test_binance_futures_account_http_client():
    clock = LiveClock()

    client = get_cached_binance_http_client(
        clock=clock,
        account_type=BinanceAccountType.USDT_FUTURES,
    )

    http_account = BinanceFuturesAccountHttpAPI(
        clock=clock,
        client=client,
        account_type=BinanceAccountType.USDT_FUTURES,
    )

    ############################################################################
    # ACCOUNT STATUS
    ############################################################################
    response = await http_account.account(recv_window=5000)
    print(response)

    ############################################################################
    # NEW ORDER (MARKET)
    ############################################################################
    # response = await http_account.new_order(
    #     symbol="ETHUSDT",
    #     side="BUY",
    #     type="MARKET",
    #     quantity="0.01",
    #     # stop_price="4200",
    #     # new_client_order_id="O-20211120-021300-001-001-1",
    #     # recv_window=5000,
    # )
    # print(json.dumps(response, indent=4))

    ############################################################################
    # NEW ORDER (LIMIT)
    ############################################################################
    # response = await http_account.new_order(
    #     symbol="ETHUSDT",
    #     side="BUY",
    #     type="LIMIT",
    #     quantity="0.01",
    #     time_in_force="GTC",
    #     price="1000",
    #     # stop_price="4200",
    #     # new_client_order_id="O-20211120-021300-001-001-1",
    #     # recv_window=5000,
    # )
    # print(json.dumps(response, indent=4))

    ############################################################################
    # CANCEL ORDER
    ############################################################################
    # response = await http_account.cancel_order(
    #     symbol="ETHUSDT",
    #     orig_client_order_id="fxSU6k85PZlwQEDFODh4Ad",
    #     #new_client_order_id=str(uuid.uuid4()),
    #     #recv_window=5000,
    # )
    # print(json.dumps(response, indent=4))

    ############################################################################
    # CANCEL ALL ORDERS
    ############################################################################
    # response = await http_account.cancel_open_orders(symbol="ETHUSDT")
    # print(json.dumps(response, indent=4))

    ############################################################################
    # OPEN ORDERS
    ############################################################################
    orders = await http_account.get_open_orders()
    print(orders)

    ############################################################################
    # POSITIONS
    ############################################################################
    positions = await http_account.get_position_risk()
    print(positions)

    await client.disconnect()
