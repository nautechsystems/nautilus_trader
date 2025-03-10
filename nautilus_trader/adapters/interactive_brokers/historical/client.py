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
import re
from typing import Literal

import pandas as pd
from ibapi.common import MarketDataTypeEnum

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import ib_contract_to_instrument_id
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import instrument_id_to_ib_contract
from nautilus_trader.adapters.interactive_brokers.providers import InteractiveBrokersInstrumentProvider
from nautilus_trader.adapters.interactive_brokers.providers import InteractiveBrokersInstrumentProviderConfig

# fmt: on
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.component import log_level_from_str
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import Instrument


class HistoricInteractiveBrokersClient:
    """
    Provides a means of requesting historical market data for backtesting.
    """

    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 7497,
        client_id: int = 1,
        market_data_type: MarketDataTypeEnum = MarketDataTypeEnum.REALTIME,
        log_level: str = "INFO",
    ) -> None:
        loop = asyncio.get_event_loop()
        loop.set_debug(True)
        clock = LiveClock()

        self._log_guard = init_logging(level_stdout=log_level_from_str(log_level))

        self.log = Logger(name="HistoricInteractiveBrokersClient")
        msgbus = MessageBus(
            TraderId("historic_interactive_brokers_client-001"),
            clock,
        )
        cache = Cache()
        self.market_data_type = market_data_type
        self._client = InteractiveBrokersClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            host=host,
            port=port,
            client_id=client_id,
        )
        self._client.start()

    async def connect(self) -> None:
        # Connect client
        await self._client.wait_until_ready()
        self._client.registered_nautilus_clients.add(1)

        # Set Market Data Type
        await self._client.set_market_data_type(self.market_data_type)

    async def request_instruments(
        self,
        instrument_provider_config: InteractiveBrokersInstrumentProviderConfig | None = None,
        contracts: list[IBContract] | None = None,
        instrument_ids: list[str] | None = None,
    ) -> list[Instrument]:
        """
        Return Instruments given either a InteractiveBrokersInstrumentProviderConfig or
        a list of IBContracts and/or InstrumentId strings.

        Parameters
        ----------
        instrument_provider_config : InteractiveBrokersInstrumentProviderConfig
            An instrument provider config defining which instruments to retrieve.
        contracts : list[IBContract], default 'None'
            IBContracts defining which instruments to retrieve.
        instrument_ids : list[str], default 'None'
            Instrument IDs (e.g. AAPL.NASDAQ) defining which instruments to retrieve.

        Returns
        -------
        list[Instrument]

        """
        if instrument_provider_config and (contracts or instrument_ids):
            raise ValueError(
                "Either instrument_provider_config or ib_contracts/instrument_ids should be provided, not both.",
            )
        if instrument_provider_config is None:
            instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
                load_contracts=frozenset(contracts) if contracts else None,
                load_ids=frozenset(instrument_ids) if instrument_ids else None,
            )
        provider = InteractiveBrokersInstrumentProvider(
            self._client,
            instrument_provider_config,
        )
        await provider.load_all_async()
        return list(provider._instruments.values())

    async def _prepare_request_bars_parameters(
        self,
        bar_specifications: list[str],
        end_date_time: datetime.datetime,
        tz_name: str,
        start_date_time: datetime.datetime | None = None,
        duration: str | None = None,
        contracts: list[IBContract] | None = None,
        instrument_ids: list[str] | None = None,
        use_rth: bool = True,
    ) -> tuple[list[IBContract], datetime.datetime, datetime.datetime]:
        """
        Prepare and validate parameters for requesting bars.

        Returns a tuple of (contracts, start_date_time, end_date_time).

        """
        # Perform all necessary validations
        if start_date_time and duration:
            raise ValueError("Either start_date_time or duration should be provided, not both.")

        # Adjust start and end time based on the timezone
        if start_date_time:
            start_date_time = pd.Timestamp(start_date_time, tz=tz_name).tz_convert("UTC")
        end_date_time = pd.Timestamp(end_date_time, tz=tz_name).tz_convert("UTC")

        if start_date_time and start_date_time >= end_date_time:
            raise ValueError("Start date must be before end date.")

        if duration:
            pattern = r"^\d+\s[SDWMY]$"
            if not re.match(pattern, duration):
                raise ValueError("duration must be in format: 'int S|D|W|M|Y'")

        # Prepare contracts and instrument_ids
        contracts = contracts or []
        instrument_ids = instrument_ids or []
        if not contracts and not instrument_ids:
            raise ValueError("Either contracts or instrument_ids must be provided")

        contracts.extend(
            [
                instrument_id_to_ib_contract(
                    InstrumentId.from_str(instrument_id),
                )
                for instrument_id in instrument_ids
            ],
        )

        return contracts, start_date_time, end_date_time

    async def request_bars(
        self,
        bar_specifications: list[str],
        end_date_time: datetime.datetime,
        tz_name: str,
        start_date_time: datetime.datetime | None = None,
        duration: str | None = None,
        contracts: list[IBContract] | None = None,
        instrument_ids: list[str] | None = None,
        use_rth: bool = True,
        timeout: int = 120,
    ) -> list[Bar]:
        """
        Return Bars for one or more bar specifications for a list of IBContracts and/or
        InstrumentId strings.

        Parameters
        ----------
        bar_specifications : list[str]
            BarSpecifications represented as strings defining which bars to retrieve.
            (e.g. '1-HOUR-LAST', '5-MINUTE-MID')
        start_date_time : datetime.datetime
            The start date time for the bars. If provided, duration is derived.
        end_date_time : datetime.datetime
            The end date time for the bars.
        tz_name : str
            The timezone to use. (e.g. 'America/New_York', 'UTC')
        duration : str
            The amount of time to go back from the end_date_time.
            Valid values follow the pattern of an integer followed by S|D|W|M|Y
            for seconds, days, weeks, months, or years respectively.
        contracts : list[IBContract], default 'None'
            IBContracts defining which bars to retrieve.
        instrument_ids : list[str], default 'None'
            Instrument IDs (e.g. AAPL.NASDAQ) defining which bars to retrieve.
        use_rth : bool, default 'True'
            Whether to use regular trading hours.
        timeout : int, default 120
            The timeout (seconds) for each request.

        Returns
        -------
        list[Bar]

        """
        contracts, start_date_time, end_date_time = await self._prepare_request_bars_parameters(
            bar_specifications,
            end_date_time,
            tz_name,
            start_date_time,
            duration,
            contracts,
            instrument_ids,
            use_rth,
        )

        # Ensure instruments are fetched and cached
        await self._fetch_instruments_if_not_cached(contracts)

        data: list[Bar] = []

        for contract in contracts:
            for bar_spec in bar_specifications:
                instrument_id = ib_contract_to_instrument_id(contract)
                bar_type = BarType(
                    instrument_id,
                    BarSpecification.from_str(bar_spec),
                    AggregationSource.EXTERNAL,
                )

                for segment_end_date_time, segment_duration in self._calculate_duration_segments(
                    start_date_time,
                    end_date_time,
                    duration,
                ):
                    self.log.info(
                        f"{instrument_id}: Requesting historical bars: {bar_type} ending on '{segment_end_date_time}' "
                        f"with duration '{segment_duration}'",
                    )

                    bars = await self._client.get_historical_bars(
                        bar_type,
                        contract,
                        use_rth,
                        segment_end_date_time,
                        segment_duration,
                        timeout=timeout,
                    )
                    if bars:
                        self.log.info(
                            f"{instrument_id}: Number of bars retrieved in batch: {len(bars)}",
                        )
                        data.extend(bars)
                        self.log.info(f"Total number of bars in data: {len(data)}")
                    else:
                        self.log.info(f"{instrument_id}: No bars retrieved for: {bar_type}")

        return sorted(data, key=lambda x: x.ts_init)

    async def request_ticks(
        self,
        tick_type: Literal["TRADES", "BID_ASK"],
        start_date_time: datetime.datetime,
        end_date_time: datetime.datetime,
        tz_name: str,
        contracts: list[IBContract] | None = None,
        instrument_ids: list[str] | None = None,
        use_rth: bool = True,
        timeout: int = 60,
    ) -> list[TradeTick | QuoteTick]:
        """
        Return TradeTicks or QuoteTicks for one or more bar specifications for a list of
        IBContracts and/or InstrumentId strings.

        Parameters
        ----------
        tick_type : Literal["TRADES", "BID_ASK"]
            The type of ticks to retrieve.
        start_date_time : datetime.date
            The start date for the ticks.
        end_date_time : datetime.date
            The end date for the ticks.
        tz_name : str
            The timezone to use. (e.g. 'America/New_York', 'UTC')
        contracts : list[IBContract], default 'None'
            IBContracts defining which ticks to retrieve.
        instrument_ids : list[str], default 'None'
            Instrument IDs (e.g. AAPL.NASDAQ) defining which ticks to retrieve.
        use_rth : bool, default 'True'
            Whether to use regular trading hours.
        timeout : int, default 60
            The timeout (seconds) for each request.

        Returns
        -------
        list[TradeTick | QuoteTick]

        """
        if tick_type not in ["TRADES", "BID_ASK"]:
            raise ValueError(
                "tick_type must be one of: 'TRADES' (for TradeTicks), 'BID_ASK' (for QuoteTicks)",
            )
        if start_date_time >= end_date_time:
            raise ValueError("Start date must be before end date.")
        start_date_time = pd.Timestamp(start_date_time, tz=tz_name).tz_convert("UTC")
        end_date_time = pd.Timestamp(end_date_time, tz=tz_name).tz_convert("UTC")
        if (end_date_time - start_date_time) > pd.Timedelta(days=1):
            self.log.warning(
                "Requesting tick data for more than 1 day may take a long time, particularly for liquid instruments. "
                "You may want to consider sourcing tick data elsewhere",
            )

        contracts = contracts or []
        instrument_ids = instrument_ids or []
        if not contracts and not instrument_ids:
            raise ValueError("Either contracts or instrument_ids must be provided")

        # Convert instrument_id strings to IBContracts
        contracts.extend(
            [
                instrument_id_to_ib_contract(
                    InstrumentId.from_str(instrument_id),
                )
                for instrument_id in instrument_ids
            ],
        )

        # Ensure instruments are fetched and cached
        await self._fetch_instruments_if_not_cached(contracts)

        data: list[TradeTick | QuoteTick] = []
        for contract in contracts:
            instrument_id = ib_contract_to_instrument_id(contract)
            current_start_date_time = start_date_time
            while True:
                self.log.info(
                    f"{instrument_id}: Requesting {tick_type} ticks from {current_start_date_time}",
                )

                ticks: list[TradeTick | QuoteTick] = await self._client.get_historical_ticks(
                    contract=contract,
                    tick_type=tick_type,
                    start_date_time=current_start_date_time,
                    use_rth=use_rth,
                    timeout=timeout,
                )

                if not ticks:
                    break

                self.log.info(
                    f"{instrument_id}: Number of {tick_type} ticks retrieved in batch: {len(ticks)}",
                )

                current_start_date_time, should_continue = self._handle_timestamp_iteration(
                    ticks,
                    end_date_time,
                )

                if not should_continue:
                    # Filter out ticks that are after the end_date_time
                    ticks = [
                        tick for tick in ticks if tick.ts_event <= dt_to_unix_nanos(end_date_time)
                    ]
                    data.extend(ticks)
                    self.log.info(f"Total number of {tick_type} ticks in data: {len(data)}")
                    break

                data.extend(ticks)
                self.log.info(f"Total number of {tick_type} ticks in data: {len(data)}")

        return sorted(data, key=lambda x: x.ts_init)

    def _handle_timestamp_iteration(
        self,
        ticks: list[TradeTick | QuoteTick],
        end_date_time: pd.Timestamp,
    ) -> tuple[pd.Timestamp | None, bool]:
        """
        Return the max timestamp from the given ticks and whether to continue iterating.
        If all timestamps occur in the same second, the max timestamp will be
        incremented by 1 second.

        Parameters
        ----------
        ticks : list[TradeTick | QuoteTick]
            The type of ticks to retrieve.
        end_date_time : datetime.date
            The end date for the ticks.

        Returns
        -------
        tuple[pd.Timestamp | None, bool]

        """
        if not ticks:
            return None, False

        timestamps = [unix_nanos_to_dt(tick.ts_event) for tick in ticks]
        min_timestamp = min(timestamps)
        max_timestamp = max(timestamps)

        if min_timestamp.floor("S") == max_timestamp.floor("S"):
            max_timestamp = max_timestamp.floor("S") + pd.Timedelta(seconds=1)
        if len(ticks) <= 50:
            max_timestamp = max_timestamp.floor("S") + pd.Timedelta(minutes=1)
        if max_timestamp >= end_date_time:
            return None, False

        return max_timestamp, True

    async def _fetch_instruments_if_not_cached(self, contracts: list[IBContract]) -> None:
        """
        Fetch and cache Instruments for the given IBContracts if they are not already
        cached.

        Parameters
        ----------
        contracts : list[IBContract]
            A list of IBContracts to fetch Instruments for.

        Returns
        -------
        None

        """
        for contract in contracts:
            instrument_id = ib_contract_to_instrument_id(contract)
            if not self._client._cache.instrument(instrument_id):
                self.log.info(f"Fetching Instrument for: {instrument_id}")
                await self.request_instruments(contracts=[contract])

    def _calculate_duration_segments(
        self,
        start_date: pd.Timestamp | None,
        end_date: pd.Timestamp,
        duration: str | None,
    ) -> list[tuple[pd.Timestamp, str]]:
        """
        Calculate the difference in years, days, and seconds between two dates for the
        purpose of requesting specific date ranges for historical bars.

        This function breaks down the time difference between two provided dates (start_date
        and end_date) into separate components: years, days, and seconds. It accounts for leap
        years in its calculation of years and considers detailed time components (hours, minutes,
        seconds) for precise calculation of seconds.

        Each component of the time difference (years, days, seconds) is represented as a
        tuple in the returned list.
        The first element is the date that indicates the end point of that time segment
        when moving from start_date to end_date. For example, if the function calculates 1
        year, the date for the year entry will be the end date after 1 year has passed
        from start_date. This helps in understanding the progression of time from start_date
        to end_date in segmented intervals.

        Parameters
        ----------
        start_date : pd.Timestamp | None
            The starting date and time.
        end_date : pd.Timestamp
            The ending date and time.
        duration : str
            The amount of time to go back from the end_date_time.
            Valid values follow the pattern of an integer followed by S|D|W|M|Y
            for seconds, days, weeks, months, or years respectively.

        Returns
        -------
        tuple[pd.Timestamp, str]: A list of tuples, each containing a date and a duration.
            The date represents the end point of each calculated time segment (year, day, second),
            and the duration is the length of the time segment as a string.

        """
        if duration:
            return [(end_date, duration)]

        total_delta = end_date - start_date

        # Calculate full years in the time delta
        years = total_delta.days // 365
        minus_years_date = end_date - pd.Timedelta(days=365 * years)

        # Calculate remaining days after subtracting full years
        days = (minus_years_date - start_date).days
        minus_days_date = minus_years_date - pd.Timedelta(days=days)

        # Calculate remaining time in seconds
        delta = minus_days_date - start_date
        subsecond = (
            1
            if delta.components.milliseconds > 0
            or delta.components.microseconds > 0
            or delta.components.nanoseconds > 0
            else 0
        )
        seconds = (
            delta.components.hours * 3600
            + delta.components.minutes * 60
            + delta.components.seconds
            + subsecond
        )

        results = []
        if years:
            results.append((end_date, f"{years} Y"))

        if days:
            results.append((minus_years_date, f"{days} D"))

        if seconds:
            results.append((minus_days_date, f"{seconds} S"))

        return results
