#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import asyncio
import os

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.historical import HistoricInteractiveBrokersClient
from nautilus_trader.examples.interactive_brokers import resolve_ib_endpoint


async def main() -> None:
    host, port = resolve_ib_endpoint("IB_EXAMPLE_HOST", "IB_EXAMPLE_PORT")
    client = HistoricInteractiveBrokersClient(
        host=host,
        port=port,
        client_id=int(os.getenv("IB_EXAMPLE_CONTRACT_CLIENT_ID", "1181")),
        log_level="INFO",
    )
    await client.connect()
    await asyncio.sleep(1)

    print("Requesting contracts...", flush=True)
    instruments = await client.request_instruments(
        contracts=[
            IBContract(
                secType="STK",
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
            ),
            IBContract(
                secType="STK",
                symbol="MSFT",
                exchange="SMART",
                primaryExchange="NASDAQ",
            ),
            IBContract(
                secType="STK",
                symbol="TSLA",
                exchange="SMART",
                primaryExchange="NASDAQ",
            ),
        ],
    )

    print(f"Loaded {len(instruments)} instrument(s)", flush=True)
    for instrument in instruments[:20]:
        print(instrument.id, flush=True)


if __name__ == "__main__":
    asyncio.run(main())
