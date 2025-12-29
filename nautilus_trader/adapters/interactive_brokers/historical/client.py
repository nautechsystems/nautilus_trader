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

import datetime
import re
from typing import Literal

import msgspec
import pandas as pd
from ibapi.common import MarketDataTypeEnum

from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.data import InteractiveBrokersDataClient
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import ib_contract_to_instrument_id
from nautilus_trader.adapters.interactive_brokers.providers import InteractiveBrokersInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.cache.database import CacheDatabaseAdapter
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.component import log_level_from_str
from nautilus_trader.common.functions import get_event_loop
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.serialization.serializer import MsgSpecSerializer


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
        cache_config: CacheConfig | None = None,
        instrument_provider_config: InteractiveBrokersInstrumentProviderConfig | None = None,
    ) -> None:
        loop = get_event_loop()

        loop.set_debug(True)
        self._clock = LiveClock()

        self._log_guard = init_logging(level_stdout=log_level_from_str(log_level))

        self.log = Logger(name="HistoricInteractiveBrokersClient")
        trader_id = TraderId("historic_interactive_brokers_client-001")
        msgbus = MessageBus(
            trader_id,
            self._clock,
        )
        self.market_data_type = market_data_type
        if not cache_config or not cache_config.database:
            cache_db = None
        elif cache_config.database.type == "redis":
            encoding = cache_config.encoding.lower()
            cache_db = CacheDatabaseAdapter(
                trader_id=trader_id,
                instance_id=UUID4(),
                serializer=MsgSpecSerializer(
                    encoding=msgspec.msgpack if encoding == "msgpack" else msgspec.json,
                    timestamps_as_str=True,  # Hardcoded for now
                    timestamps_as_iso8601=cache_config.timestamps_as_iso8601,
                ),
                config=cache_config,
            )
        else:
            raise ValueError(
                f"Unrecognized `cache_config.database.type`, was '{cache_config.database.type}'. "
                "The only database type currently supported is 'redis', if you don't want a cache database backing "
                "then you can pass `None` for the `cache_config.database`",
            )

        self._client = InteractiveBrokersClient(
            loop=loop,
            msgbus=msgbus,
            cache=Cache(database=cache_db, config=cache_config) if cache_config else Cache(),
            clock=self._clock,
            host=host,
            port=port,
            client_id=client_id,
        )
        self._client.start()

        # Store instrument provider config and create provider once
        if instrument_provider_config is None:
            instrument_provider_config = InteractiveBrokersInstrumentProviderConfig()

        self._instrument_provider_config = instrument_provider_config
        instrument_provider = InteractiveBrokersInstrumentProvider(
            self._client,
            self._clock,
            instrument_provider_config,
        )

        self._data_client = InteractiveBrokersDataClient(
            loop=loop,
            client=self._client,
            msgbus=msgbus,
            cache=Cache(database=cache_db, config=cache_config) if cache_config else Cache(),
            clock=self._clock,
            instrument_provider=instrument_provider,
            ibg_client_id=client_id,
            config=InteractiveBrokersDataClientConfig(
                market_data_type=market_data_type,
            ),
        )

    async def connect(self) -> None:
        # Connect client
        await self._data_client._connect()

    async def request_instruments(
        self,
        instrument_ids: list[str | InstrumentId] | None = None,
        contracts: list[IBContract] | None = None,
    ) -> list[Instrument]:
        """
        Return Instruments given a list of IBContracts and/or InstrumentId strings.

        Parameters
        ----------
        instrument_ids : list[str | InstrumentId], default 'None'
            Instrument IDs (e.g. AAPL.NASDAQ) defining which instruments to retrieve.
            Can be strings or InstrumentId objects.
        contracts : list[IBContract], default 'None'
            IBContracts defining which instruments to retrieve.

        Returns
        -------
        list[Instrument]

        """
        # Convert string instrument_ids to InstrumentId objects
        converted_instrument_ids = [
            InstrumentId.from_str(instrument_id) if isinstance(instrument_id, str) else instrument_id
            for instrument_id in (instrument_ids or [])
        ]

        await self._data_client.instrument_provider.load_ids_async(converted_instrument_ids + (contracts or []))

        return list(self._data_client.instrument_provider._instruments.values())

    async def request_bars(
        self,
        bar_specifications: list[str],
        end_date_time: datetime.datetime,
        tz_name: str,
        start_date_time: datetime.datetime | None = None,
        duration: str | None = None,
        contracts: list[IBContract] | None = None,
        instrument_ids: list[str | InstrumentId] | None = None,
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
            Note that for continuous futures (CONTFUT), the downloaded data is always up to now.
        tz_name : str
            The timezone to use. (e.g. 'America/New_York', 'UTC')
        duration : str
            The amount of time to go back from the end_date_time.
            Valid values follow the pattern of an integer followed by S|D|W|M|Y
            for seconds, days, weeks, months, or years respectively.
        contracts : list[IBContract], default 'None'
            IBContracts defining which bars to retrieve.
        instrument_ids : list[str | InstrumentId], default 'None'
            Instrument IDs (e.g. AAPL.NASDAQ) defining which bars to retrieve.
            Can be strings or InstrumentId objects.
        use_rth : bool, default 'True'
            Whether to use regular trading hours.
        timeout : int, default 120
            The timeout (seconds) for each request.

        Returns
        -------
        list[Bar]

        """
        # Perform all necessary validations (merged from _prepare_request_bars_parameters)
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

        # Convert instrument_id strings or InstrumentId objects to IBContracts
        contracts.extend(
            [
                await self._data_client.instrument_provider.instrument_id_to_ib_contract(
                    InstrumentId.from_str(instrument_id) if isinstance(instrument_id, str) else instrument_id,
                )
                for instrument_id in instrument_ids
            ],
        )

        # Ensure instruments are fetched and cached
        await self._fetch_instruments_if_not_cached(contracts)
        data: list[Bar] = []

        for contract in contracts:
            for bar_spec in bar_specifications:
                venue = self._data_client.instrument_provider.determine_venue_from_contract(contract)
                instrument_id = ib_contract_to_instrument_id(
                    contract,
                    venue,
                    self._instrument_provider_config.symbology_method,
                )
                bar_type = BarType(
                    instrument_id,
                    BarSpecification.from_str(bar_spec),
                    AggregationSource.EXTERNAL,
                )

                bars = await self._data_client.get_historical_bars_chunked(
                    bar_type=bar_type,
                    contract=contract,
                    start_date_time=start_date_time,
                    end_date_time=end_date_time,
                    duration=duration,
                    use_rth=use_rth,
                    timeout=timeout,
                )

                if bars:
                    data.extend(bars)

        return sorted(data, key=lambda x: x.ts_init)

    async def request_ticks(
        self,
        tick_type: Literal["TRADES", "BID_ASK"],
        start_date_time: datetime.datetime,
        end_date_time: datetime.datetime,
        tz_name: str,
        contracts: list[IBContract] | None = None,
        instrument_ids: list[str | InstrumentId] | None = None,
        use_rth: bool = True,
        timeout: int = 60,
        limit: int = 0,
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
        instrument_ids : list[str | InstrumentId], default 'None'
            Instrument IDs (e.g. AAPL.NASDAQ) defining which ticks to retrieve.
            Can be strings or InstrumentId objects.
        use_rth : bool, default 'True'
            Whether to use regular trading hours.
        timeout : int, default 60
            The timeout (seconds) for each request.
        limit : int, default 0
            Maximum number of ticks to retrieve. If 0, no limit is applied.

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

        # Convert instrument_id strings or InstrumentId objects to IBContracts
        contracts.extend(
            [
                await self._data_client.instrument_provider.instrument_id_to_ib_contract(
                    InstrumentId.from_str(instrument_id) if isinstance(instrument_id, str) else instrument_id,
                )
                for instrument_id in instrument_ids
            ],
        )

        # Ensure instruments are fetched and cached
        await self._fetch_instruments_if_not_cached(contracts)
        data: list[TradeTick | QuoteTick] = []

        for contract in contracts:
            venue = self._data_client.instrument_provider.determine_venue_from_contract(contract)
            instrument_id = ib_contract_to_instrument_id(
                contract,
                venue,
                self._instrument_provider_config.symbology_method,
            )

            ticks = await self._data_client.get_historical_ticks_paged(
                instrument_id=instrument_id,
                contract=contract,
                tick_type=tick_type,
                start_date_time=start_date_time,
                end_date_time=end_date_time,
                limit=limit,
                use_rth=use_rth,
                timeout=timeout,
            )

            if ticks:
                data.extend(ticks)

        return sorted(data, key=lambda x: x.ts_init)

    async def _fetch_instruments_if_not_cached(
        self,
        contracts: list[IBContract],
    ) -> None:
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
            venue = self._data_client.instrument_provider.determine_venue_from_contract(contract)
            instrument_id = ib_contract_to_instrument_id(
                contract,
                venue,
                self._instrument_provider_config.symbology_method,
            )

            if not self._client._cache.instrument(instrument_id):
                self.log.info(f"Fetching Instrument for: {instrument_id}")
                await self.request_instruments(
                    contracts=[contract],
                )
