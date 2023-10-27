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
import datetime
import logging
from typing import Literal

import pandas as pd
from ibapi.contract import Contract

from nautilus_trader.adapters.interactive_brokers.historic.client import HistoricInteractiveBrokersClient
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog


logger = logging.getLogger(__name__)


def generate_filename(
    catalog: ParquetDataCatalog,
    instrument_id: InstrumentId,
    kind: Literal["BID_ASK", "TRADES"],
    date: datetime.date,
) -> str:
    fn_kind = {"BID_ASK": "quote_tick", "TRADES": "trade_tick", "BARS": "bars"}[kind.split("-")[0]]
    return f"{catalog.path}/data/{fn_kind}.parquet/instrument_id={instrument_id.value}/{date:%Y%m%d}-0.parquet"


async def back_fill_catalog(
    catalog: ParquetDataCatalog,
    contracts: list[Contract],
    start_date: datetime.date,
    end_date: datetime.date,
    tz_name: str,
    kinds=("BID_ASK", "TRADES"),
):
    """
    Backfill the data catalog with market data from Interactive Brokers.

    Parameters
    ----------
    catalog : ParquetDataCatalog
        The data catalog to write the data to.
    contracts : list[Contract]
        The list of IB Contracts to collect data for.
    start_date : datetime.date
        The start_date for the backfill.
    end_date : datetime.date
        The end_date for the backfill.
    tz_name : str
        The timezone of the contracts
    kinds : tuple[str] (default: ('BID_ASK', 'TRADES')
        The kinds to query data for, can be any of:
        - BID_ASK
        - TRADES
        - A bar specification, i.e. BARS-1-MINUTE-LAST or BARS-5-SECOND-MID

    """
    client = HistoricInteractiveBrokersClient()
    for date in pd.bdate_range(start_date, end_date, tz=tz_name):
        for contract in contracts:
            # [details] = client.reqContractDetails(contract=contract)
            # instrument = parse_instrument(contract_details=details)

            # Check if this instrument exists in the catalog, if not, write it.
            # if not catalog.instruments(instrument_ids=[instrument.id.value]):
            #     catalog.write_data([instrument])
            #
            # for kind in kinds:
            #     fn = generate_filename(catalog, instrument_id=instrument.id, kind=kind, date=date)
            #     if catalog.fs.exists(fn):
            #         logger.info(
            #             f"file for {instrument.id.value} {kind} {date:%Y-%m-%d} exists, skipping",
            #         )
            #         continue
            #     logger.info(f"Fetching {instrument.id.value} {kind} for {date:%Y-%m-%d}")
            #
            data = await client.request_trade_ticks(
                contract=contract,  # typing: ignore
                date=date.date(),
                tz_name=tz_name,
            )
            if data is None:
                continue

            template = f"{date:%Y%m%d}" + "-{i}.parquet"
            catalog.write_data(data, basename_template=template)
