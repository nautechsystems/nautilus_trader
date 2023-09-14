import json
import os

import msgspec
import pytest

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.endpoints.market.instruments_info import BybitInstrumentsInfoEndpoint, \
    BybitInstrumentsInfoGetParameters
from nautilus_trader.adapters.bybit.endpoints.market.server_time import BybitServerTimeEndpoint
from nautilus_trader.adapters.bybit.factories import get_cached_bybit_http_client
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger

from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrumentLinear
from nautilus_trader.adapters.bybit.utils import msgspec_bybit_item_save
from nautilus_trader.utils.save_struct_to_file import save_struct_to_file


@pytest.mark.asyncio()
async def test_bybit_market_http_client():
    clock = LiveClock()

    client = get_cached_bybit_http_client(
        clock=clock,
        logger=Logger(clock=clock),
        is_testnet=True,
    )
    force_create = True if 'FORCE_CREATE' in os.environ else False

    base_path = "../resources/http_responses/"
    base_endpoint = '/v5/market/'
    ################################################################################
    # Server time
    ################################################################################
    time_endpoint = BybitServerTimeEndpoint(client=client,base_endpoint=base_endpoint)
    server_time = await time_endpoint.get()
    save_struct_to_file(base_path+"server_time.json", server_time,force_create)

    ################################################################################
    # Instruments
    ################################################################################
    # --- Spot ---
    instruments_spot_endpoint = BybitInstrumentsInfoEndpoint(client,base_endpoint,BybitInstrumentType.SPOT)
    instruments_spot = await instruments_spot_endpoint.get(BybitInstrumentsInfoGetParameters(category='spot'))
    # extract only BTCUSDT and ETHUSDT
    instruments_spot.result.list = [item for item in instruments_spot.result.list if item.symbol in ["BTCUSDT", "ETHUSDT"]]
    save_struct_to_file(base_path+"spot/"+"instruments.json", instruments_spot,force_create)


    # --- Linear ---
    instruments_linear_endpoint= BybitInstrumentsInfoEndpoint(client,base_endpoint,BybitInstrumentType.LINEAR)
    instruments_linear = await instruments_linear_endpoint.get(BybitInstrumentsInfoGetParameters(category='linear'))
    # extract only BTCUSDT and ETHUSDT
    instruments_linear.result.list = [item for item in instruments_linear.result.list if item.symbol in ["BTCUSDT", "ETHUSDT"]]
    save_struct_to_file(base_path+"linear/"+"instruments.json", instruments_linear,force_create)

    # --- Option ---
    instruments_option_endpoint = BybitInstrumentsInfoEndpoint(client,base_endpoint,BybitInstrumentType.OPTION)
    instruments_options = await instruments_option_endpoint.get(BybitInstrumentsInfoGetParameters(category='option'))
    # take first few items
    instruments_options.result.list = instruments_options.result.list[:2]
    save_struct_to_file(base_path+"option/"+"instruments.json", instruments_options,force_create)

    # target_instruments = ["BTCUSDT", "ETHUSDT"]
    # for item in instruments:
    #     if item.symbol in target_instruments:
    #         print(json.dumps(msgspec.to_builtins(item), indent=4))
