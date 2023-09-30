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

from typing import Callable, Optional

import pandas as pd

# fmt: off
from nautilus_trader.adapters.interactive_brokers.historic.async_actor import AsyncActor
from nautilus_trader.common.actor import ActorConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarType


# fmt: on


class BarDataDownloaderConfig(ActorConfig):
    """
    Configuration for `BarDataDownloader` instances.
    """

    start_iso_ts: str
    end_iso_ts: str
    bar_types: list[str]
    handler: Callable
    freq: str = "1W"


class BarDataDownloader(AsyncActor):
    def __init__(self, config: BarDataDownloaderConfig):
        super().__init__(config)
        try:
            self.start_time: pd.Timestamp = pd.to_datetime(
                config.start_iso_ts,
                format="%Y-%m-%dT%H:%M:%S%z",
            )
            self.end_time: pd.Timestamp = pd.to_datetime(
                config.end_iso_ts,
                format="%Y-%m-%dT%H:%M:%S%z",
            )
        except ValueError:
            raise ValueError("`start_iso_ts` and `end_iso_ts` must be like '%Y-%m-%dT%H:%M:%S%z'")

        self.bar_types: list[BarType] = []
        for bar_type in config.bar_types:
            self.bar_types.append(BarType.from_str(bar_type))

        self.handler: Optional[Callable] = config.handler
        self.freq: str = config.freq

    async def _on_start(self):
        instrument_ids = {bar_type.instrument_id for bar_type in self.bar_types}
        for instrument_id in instrument_ids:
            request_id = self.request_instrument(instrument_id)
            await self.await_request(request_id)

        request_dates = list(pd.date_range(self.start_time, self.end_time, freq=self.freq))

        for request_date in request_dates:
            for bar_type in self.bar_types:
                request_id = self.request_bars(
                    bar_type=bar_type,
                    start=request_date,
                    end=request_date + pd.Timedelta(self.freq),
                )
                await self.await_request(request_id)

        self.stop()

    def handle_bars(self, bars: list):
        """
        Handle the given historical bar data by handling each bar individually.

        Parameters
        ----------
        bars : list[Bar]
            The bars to handle.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        PyCondition.not_none(bars, "bars")  # Can be empty

        length = len(bars)
        first: Bar = bars[0] if length > 0 else None
        last: Bar = bars[length - 1] if length > 0 else None

        if length > 0:
            self._log.info(f"Received <Bar[{length}]> data for {first.bar_type}.")
        else:
            self._log.error(f"Received <Bar[{length}]> data for unknown bar type.")
            return

        if length > 0 and first.ts_init > last.ts_init:
            raise RuntimeError(f"cannot handle <Bar[{length}]> data: incorrectly sorted")

        # Send Bars response as a whole to handler
        self.handler(bars)
