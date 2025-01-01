#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
import datetime
import os

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.gateway import DockerizedIBGateway
from nautilus_trader.adapters.interactive_brokers.historical import HistoricInteractiveBrokersClient
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.persistence.catalog import ParquetDataCatalog


async def main(
    host: str | None = None,
    port: int | None = None,
    dockerized_gateway: DockerizedIBGatewayConfig | None = None,
) -> None:
    if dockerized_gateway:
        PyCondition.none(host, "Ensure `host` is set to None when using DockerizedIBGatewayConfig.")
        PyCondition.none(port, "Ensure `port` is set to None when using DockerizedIBGatewayConfig.")
        PyCondition.type(dockerized_gateway, DockerizedIBGatewayConfig, "dockerized_gateway")
        gateway = DockerizedIBGateway(config=dockerized_gateway)
        gateway.start(dockerized_gateway.timeout)
        host = gateway.host
        port = gateway.port
    else:
        gateway = None
        PyCondition.not_none(
            host,
            "Please provide the `host` IP address for the IB TWS or Gateway.",
        )
        PyCondition.not_none(port, "Please provide the `port` for the IB TWS or Gateway.")

    contract = IBContract(
        secType="STK",
        symbol="AAPL",
        exchange="SMART",
        primaryExchange="NASDAQ",
    )
    instrument_id = "TSLA.NASDAQ"

    client = HistoricInteractiveBrokersClient(host=host, port=port, client_id=5)
    await client.connect()
    await asyncio.sleep(2)

    instruments = await client.request_instruments(
        contracts=[contract],
        instrument_ids=[instrument_id],
    )

    bars = await client.request_bars(
        bar_specifications=["1-HOUR-LAST", "30-MINUTE-MID"],
        start_date_time=datetime.datetime(2023, 11, 6, 9, 30),
        end_date_time=datetime.datetime(2023, 11, 6, 16, 30),
        tz_name="America/New_York",
        contracts=[contract],
        instrument_ids=[instrument_id],
    )

    trade_ticks = await client.request_ticks(
        "TRADES",
        start_date_time=datetime.datetime(2023, 11, 6, 10, 0),
        end_date_time=datetime.datetime(2023, 11, 6, 10, 1),
        tz_name="America/New_York",
        contracts=[contract],
        instrument_ids=[instrument_id],
    )

    quote_ticks = await client.request_ticks(
        "BID_ASK",
        start_date_time=datetime.datetime(2023, 11, 6, 10, 0),
        end_date_time=datetime.datetime(2023, 11, 6, 10, 1),
        tz_name="America/New_York",
        contracts=[contract],
        instrument_ids=[instrument_id],
    )

    if gateway:
        gateway.stop()

    catalog = ParquetDataCatalog("./catalog")
    catalog.write_data(instruments)
    catalog.write_data(bars)
    catalog.write_data(trade_ticks)
    catalog.write_data(quote_ticks)


if __name__ == "__main__":
    gateway_config = DockerizedIBGatewayConfig(
        username=os.environ["TWS_USERNAME"],
        password=os.environ["TWS_PASSWORD"],
        trading_mode="paper",
    )
    asyncio.run(main(dockerized_gateway=gateway_config))

    # To connect to an existing TWS or Gateway instance without the use of automated dockerized gateway,
    # follow this format:
    # asyncio.run(main(host="127.0.0.1", port=7497))
