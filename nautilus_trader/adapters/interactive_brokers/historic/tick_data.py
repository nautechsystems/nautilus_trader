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

from collections.abc import Callable
from itertools import product

import pandas as pd

# fmt: off
from nautilus_trader.adapters.interactive_brokers.historic.async_actor import AsyncActor
from nautilus_trader.common.actor import ActorConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId


# fmt: on


class TickDataDownloaderConfig(ActorConfig):
    """
    Configuration for `TickDataDownloader` instances.
    """

    start_iso_ts: str
    end_iso_ts: str
    instrument_ids: list[str]
    tick_types: list[str]
    handler: Callable
    freq: str = "1D"


class TickDataDownloader(AsyncActor):
    def __init__(self, config: TickDataDownloaderConfig):
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

        self.instrument_ids: list[str] = [
            InstrumentId.from_str(instrument_id) for instrument_id in config.instrument_ids
        ]
        if not set(config.tick_types).issubset({"BID_ASK", "TRADES"}):
            raise ValueError("`tick_type` must be either 'BID_ASK' or 'TRADES'")
        self.tick_types: list[str] = config.tick_types
        self.handler: Callable | None = config.handler
        self.freq: str = config.freq

    async def _on_start(self):
        for instrument_id in self.instrument_ids:
            request_id = self.request_instrument(instrument_id)
            await self.await_request(request_id)

        request_dates = list(pd.date_range(self.start_time, self.end_time, freq=self.freq))

        for request_date, instrument_id, tick_type in product(
            request_dates,
            self.instrument_ids,
            self.tick_types,
        ):
            if tick_type == "TRADES":
                request_id = self.request_trade_ticks(
                    instrument_id=instrument_id,
                    start=request_date,
                    end=request_date + pd.Timedelta(self.freq),
                )
            elif tick_type == "BID_ASK":
                request_id = self.request_quote_ticks(
                    instrument_id=instrument_id,
                    start=request_date,
                    end=request_date + pd.Timedelta(self.freq),
                )
            else:
                raise ValueError(
                    f"Tick type {tick_type} not supported or invalid.",
                )  # pragma: no cover

        await self.await_request(request_id)

        self.stop()

    def handle_ticks(self, ticks: list):
        """
        Handle the given historical tick data by handling each tick individually.

        Parameters
        ----------
        ticks : list[Union[QuoteTick, TradeTick]]
            The ticks to handle.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        PyCondition.not_none(ticks, "bars")  # Can be empty

        length = len(ticks)
        first: QuoteTick | TradeTick = ticks[0] if length > 0 else None
        last: QuoteTick | TradeTick = ticks[length - 1] if length > 0 else None

        if length > 0:
            self._log.info(f"Received <{type(first)}[{length}]> data.")
        else:
            self._log.error(f"Received <{type(first)}[{length}]> data for unknown tick type.")
            return

        if length > 0 and first.ts_init > last.ts_init:
            raise RuntimeError(f"cannot handle <{type(first)}[{length}]> data: incorrectly sorted")

        # Send Bars response as a whole to handler
        self.handler(ticks)
