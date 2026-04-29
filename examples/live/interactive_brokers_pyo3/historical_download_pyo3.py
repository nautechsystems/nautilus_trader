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
import datetime
import os
import tempfile

from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.gateway import DockerizedIBGateway
from nautilus_trader.adapters.interactive_brokers_pyo3 import HistoricalInteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersInstrumentProvider
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.examples.interactive_brokers import resolve_ib_endpoint
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.catalog import ParquetDataCatalog


async def main(
    host: str | None = None,
    port: int | None = None,
    dockerized_gateway: DockerizedIBGatewayConfig | None = None,
) -> None:
    if dockerized_gateway is not None:
        gateway = DockerizedIBGateway(config=dockerized_gateway)
        gateway.safe_start(wait=dockerized_gateway.timeout)
        host = gateway.host
        port = gateway.port
    else:
        gateway = None
        default_host, default_port = resolve_ib_endpoint("IB_PYO3_HOST", "IB_PYO3_PORT")
        host = host or default_host
        port = port or default_port

    tsla_id = InstrumentId.from_str("TSLA.NASDAQ")
    provider_config = InteractiveBrokersInstrumentProviderConfig(
        load_ids=frozenset({tsla_id}),
        build_options_chain=False,
        build_futures_chain=False,
    )
    provider = InteractiveBrokersInstrumentProvider(config=provider_config)

    client_config = InteractiveBrokersDataClientConfig(
        ibg_host=host,
        ibg_port=port,
        ibg_client_id=int(os.getenv("IB_PYO3_HIST_CLIENT_ID", "180")),
        use_regular_trading_hours=False,
        instrument_provider=provider_config,
    )
    client = HistoricalInteractiveBrokersClient(
        instrument_provider=provider,
        config=client_config,
    )

    print("Requesting instruments...", flush=True)
    instruments = await client.request_instruments(
        instrument_ids=["TSLA.NASDAQ"],
        contracts=[
            {
                "secType": "STK",
                "symbol": "AAPL",
                "exchange": "SMART",
                "primaryExchange": "NASDAQ",
            },
        ],
    )
    print(f"Loaded {len(instruments)} instrument(s)", flush=True)

    print("Requesting bars...", flush=True)
    bars = await client.request_bars(
        bar_specifications=["1-HOUR-LAST", "30-MINUTE-MID"],
        start_date_time=datetime.datetime(2025, 11, 6, 9, 30),
        end_date_time=datetime.datetime(2025, 11, 6, 16, 30),
        contracts=[
            {
                "secType": "STK",
                "symbol": "AAPL",
                "exchange": "SMART",
                "primaryExchange": "NASDAQ",
            },
        ],
        instrument_ids=["TSLA.NASDAQ"],
        use_rth=False,
        timeout=120,
    )
    print(f"Downloaded {len(bars)} bar(s)", flush=True)

    print("Requesting trade ticks...", flush=True)
    trade_ticks = await client.request_ticks(
        tick_type="TRADES",
        start_date_time=datetime.datetime(2025, 11, 6, 10, 0),
        end_date_time=datetime.datetime(2025, 11, 6, 10, 1),
        contracts=[
            {
                "secType": "STK",
                "symbol": "AAPL",
                "exchange": "SMART",
                "primaryExchange": "NASDAQ",
            },
        ],
        instrument_ids=["TSLA.NASDAQ"],
        use_rth=False,
        timeout=120,
    )
    print(f"Downloaded {len(trade_ticks)} trade tick(s)", flush=True)

    print("Requesting quote ticks...", flush=True)
    quote_ticks = await client.request_ticks(
        tick_type="BID_ASK",
        start_date_time=datetime.datetime(2025, 11, 6, 10, 0),
        end_date_time=datetime.datetime(2025, 11, 6, 10, 1),
        contracts=[
            {
                "secType": "STK",
                "symbol": "AAPL",
                "exchange": "SMART",
                "primaryExchange": "NASDAQ",
            },
        ],
        instrument_ids=["TSLA.NASDAQ"],
        use_rth=False,
        timeout=120,
    )
    print(f"Downloaded {len(quote_ticks)} quote tick(s)", flush=True)

    output_path = os.getenv(
        "IB_PYO3_CATALOG_PATH",
        os.path.join(tempfile.gettempdir(), "nautilus_ib_pyo3_catalog"),
    )
    catalog = ParquetDataCatalog(output_path)
    catalog.write_data(instruments)
    catalog.write_data(bars)
    catalog.write_data(trade_ticks)
    catalog.write_data(quote_ticks)
    print(f"Wrote data to {output_path}", flush=True)

    if gateway is not None:
        gateway.stop()


if __name__ == "__main__":
    use_dockerized_gateway = os.getenv("IB_PYO3_USE_DOCKERIZED_GATEWAY", "0") == "1"

    if use_dockerized_gateway and os.getenv("TWS_USERNAME") and os.getenv("TWS_PASSWORD"):
        gateway_config = DockerizedIBGatewayConfig(
            username=os.environ["TWS_USERNAME"],
            password=os.environ["TWS_PASSWORD"],
            trading_mode="paper",
        )
        asyncio.run(main(dockerized_gateway=gateway_config))
    else:
        asyncio.run(main())
