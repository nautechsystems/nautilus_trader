# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import List

import pandas as pd
from ib_insync import IB
from ib_insync import Contract


logger = logging.getLogger(__name__)


# def back_fill_catalog(ib: IB, contract: Contract, start: datetime.datetime, end: datetime.datetime):
#     """
#     Back fill the data catalog with market data for `contract` between `start` and `end`
#     """
#     pass


def _request_historical_ticks(ib: IB, contract: Contract, start_time: str, what="BID_ASK"):
    return ib.reqHistoricalTicks(
        contract=contract,
        startDateTime=start_time,
        endDateTime="",
        numberOfTicks=1000,
        whatToShow=what,
        useRth=False,
    )


def _determine_next_timestamp(timestamps: List[pd.Timestamp], date: datetime.date, tz_name: str):
    """
    While looping over available data, it is possible for very liquid products that a 1s period may contain 1000 ticks,
    at which point we need to step the time forward to avoid getting stuck when iterating.
    """
    if timestamps is None:
        return pd.Timestamp(date, tz=tz_name).tz_convert("UTC")
    unique_values = set(timestamps)
    if len(unique_values) == 1:
        timestamp = timestamps[-1]
        return timestamp + pd.Timedelta(seconds=1)
    else:
        return timestamps[-1]


def fetch_market_data(
    contract: Contract, date: datetime.date, kind: str, tz_name: str, ib=None
) -> List:
    data: List = []

    while True:
        start_time = _determine_next_timestamp(date=date, timestamps=data, tz_name=tz_name)
        logger.info(f"Using start_time: {start_time}")

        ticks = _request_historical_ticks(
            ib=ib,
            contract=contract,
            start_time=start_time.strftime("%Y%m%d %H:%M:%S %Z"),
            what=kind,
        )
        if not ticks or ticks[0].time < start_time:
            break

        logger.debug(f"Received {len(ticks)} ticks")

        # TODO - Load into catalog

        last_timestamp = ticks[-1]
        last_date = last_timestamp.astimezone(tz_name).date()

        if last_date != date:
            # May contain data from next date, filter this out
            data.extend([tick for tick in ticks if pd.to_datetime(tick)])
            break
        else:
            data.extend(ticks)
    return data
