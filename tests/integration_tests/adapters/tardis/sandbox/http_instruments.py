import asyncio

import pandas as pd

from nautilus_trader.adapters.tardis.factories import get_tardis_http_client
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.model.instruments import CryptoPerpetual


async def run():
    _guard = init_logging(level_stdout=LogLevel.TRACE)

    http_client = get_tardis_http_client()

    # pyo3_instrument = await http_client.instrument("okex", "ETH-USDT")
    # print(f"Received: {pyo3_instrument[0].id}")

    pyo3_instruments = await http_client.instruments(
        "bitmex",
        base_currency=["BTC"],
        quote_currency=["USD"],
        instrument_type=["perpetual"],
        # active=True,
        # start=pd.Timestamp("2021-01-01").value,
        # end=pd.Timestamp("2022-01-01").value,
        effective=pd.Timestamp("2020-08-01 08:00:00").value,
    )

    for pyo3_inst in pyo3_instruments:
        inst = CryptoPerpetual.from_pyo3(pyo3_inst)  # Remove/change this if not filtering for perps
        print(repr(inst))
        print(pd.Timestamp(inst.ts_event))

    print(f"Received: {len(pyo3_instruments)} instruments")


if __name__ == "__main__":
    asyncio.run(run())
