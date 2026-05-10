#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from __future__ import annotations

import asyncio
import os

from nautilus_trader.adapters.interactive_brokers_pyo3 import HistoricalInteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersInstrumentProvider
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.examples.interactive_brokers import resolve_ib_endpoint


async def main() -> None:
    host, port = resolve_ib_endpoint("IB_PYO3_HOST", "IB_PYO3_PORT")
    provider_config = InteractiveBrokersInstrumentProviderConfig(
        build_options_chain=False,
        build_futures_chain=False,
    )
    provider = InteractiveBrokersInstrumentProvider(config=provider_config)

    client_config = InteractiveBrokersDataClientConfig(
        ibg_host=host,
        ibg_port=port,
        ibg_client_id=int(os.getenv("IB_PYO3_CONTRACT_CLIENT_ID", "181")),
        instrument_provider=provider_config,
    )
    client = HistoricalInteractiveBrokersClient(
        instrument_provider=provider,
        config=client_config,
    )

    print("Requesting contracts...", flush=True)
    instruments = await client.request_instruments(
        contracts=[
            {
                "secType": "STK",
                "symbol": "AAPL",
                "exchange": "SMART",
                "primaryExchange": "NASDAQ",
            },
            {
                "secType": "STK",
                "symbol": "MSFT",
                "exchange": "SMART",
                "primaryExchange": "NASDAQ",
            },
            {
                "secType": "STK",
                "symbol": "TSLA",
                "exchange": "SMART",
                "primaryExchange": "NASDAQ",
            },
        ],
    )

    print(f"Loaded {len(instruments)} instrument(s)", flush=True)
    for instrument in instruments[:20]:
        print(instrument.id, flush=True)


if __name__ == "__main__":
    asyncio.run(main())
