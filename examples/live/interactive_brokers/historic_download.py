#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.historic import HistoricInteractiveBrokersClient
from nautilus_trader.persistence.catalog import ParquetDataCatalog


async def main():
    contract = IBContract(
        secType="STK",
        symbol="AAPL",
        exchange="SMART",
        primaryExchange="NASDAQ",
    )
    instrument_id = "TSLA.NASDAQ"

    client = HistoricInteractiveBrokersClient(port=4002, client_id=5)
    await client._connect()
    await asyncio.sleep(2)

    instruments = await client.request_instruments(
        contracts=[contract],
        instrument_ids=[instrument_id],
    )

    bars = await client.request_bars(
        bar_specifications=["1-HOUR-LAST", "30-MINUTE-MID"],
        end_date_time=datetime.datetime(2023, 11, 6, 16, 0),
        duration="1 D",
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

    catalog = ParquetDataCatalog("./catalog")
    catalog.write_data(instruments + bars + trade_ticks + quote_ticks)


if __name__ == "__main__":
    asyncio.run(main())
