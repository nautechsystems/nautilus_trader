import asyncio
import datetime
import re
from typing import Literal

import pandas as pd
from ibapi.common import MarketDataTypeEnum

from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import ib_contract_to_instrument_id
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import instrument_id_to_ib_contract
from nautilus_trader.adapters.interactive_brokers.providers import InteractiveBrokersInstrumentProvider
from nautilus_trader.adapters.interactive_brokers.providers import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.persistence.catalog import ParquetDataCatalog


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
    ):
        loop = asyncio.get_event_loop()
        loop.set_debug(True)
        clock = LiveClock()
        logger = Logger(clock)
        self.log = LoggerAdapter("HistoricInteractiveBrokersClient", logger)
        msgbus = MessageBus(
            TraderId("historic_interactive_brokers_client-001"),
            clock,
            logger,
        )
        cache = Cache(logger)
        self.market_data_type = market_data_type
        self._client = InteractiveBrokersClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            host=host,
            port=port,
            client_id=client_id,
        )

    async def _connect(self) -> None:
        # Connect client
        await self._client.is_running_async()
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
            Logger(LiveClock()),
        )
        await provider.load_all_async()
        return list(provider._instruments.values())

    async def request_bars(
        self,
        bar_specifications: list[str],
        end_date_time: datetime.datetime,
        duration: str,
        contracts: list[IBContract] | None = None,
        instrument_ids: list[str] | None = None,
        use_rth: bool = True,
    ) -> list[Bar]:
        """
        Return Bars for one or more bar specifications for a list of IBContracts and/or
        InstrumentId strings.

        Parameters
        ----------
        bar_specifications : list[str]
            BarSpecifications represented as strings defining which bars to retrieve.
            (e.g. '1-HOUR-LAST', '5-MINUTE-MID')
        end_date_time : datetime.datetime
            The end date time for the bars.
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

        Returns
        -------
        list[Bar]

        """
        pattern = r"^\d+\s[SDWMY]$"
        if not re.match(pattern, duration):
            raise ValueError("duration must be in format: 'int S|D|W|M|Y'")

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
        await self.fetch_instruments_if_not_cached(contracts)

        data: list[Bar] = []

        for contract in contracts:
            for bar_spec in bar_specifications:
                bar_type = BarType(
                    ib_contract_to_instrument_id(contract),
                    BarSpecification.from_str(bar_spec),
                    AggregationSource.EXTERNAL,
                )
                self.log.info(f"Requesting bars: {bar_type}")
                bars = await self._client.get_historical_bars(
                    bar_type,
                    contract,
                    use_rth,
                    end_date_time.strftime("%Y%m%d-%H:%M:%S"),
                    duration,
                )
                data.extend(bars)

        return data

    async def request_ticks(
        self,
        tick_type: Literal["TRADES", "BID_ASK"],
        start_date_time: datetime.datetime,
        end_date_time: datetime.datetime,
        tz_name: str,
        contracts: list[IBContract] | None = None,
        instrument_ids: list[str] | None = None,
        use_rth: bool = True,
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
            The timezone to use.
        contracts : list[IBContract], default 'None'
            IBContracts defining which ticks to retrieve.
        instrument_ids : list[str], default 'None'
            Instrument IDs (e.g. AAPL.NASDAQ) defining which ticks to retrieve.
        use_rth : bool, default 'True'
            Whether to use regular trading hours.

        Returns
        -------
        list[TradeTick | QuoteTick]

        """
        start_date_time = pd.Timestamp(start_date_time, tz=tz_name).tz_convert("UTC")
        end_date_time = pd.Timestamp(end_date_time, tz=tz_name).tz_convert("UTC")
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
        await self.fetch_instruments_if_not_cached(contracts)

        data: list[TradeTick | QuoteTick] = []
        for contract in contracts:
            self.log.info(f"Requesting ticks for: {ib_contract_to_instrument_id(contract)}")
            while True:
                self.log.info(f"Requesting ticks from: {start_date_time}")
                ticks = await self._client.get_historical_ticks(
                    contract=contract,
                    tick_type=tick_type,
                    start_date_time=start_date_time,
                    use_rth=use_rth,
                )
                self.log.info(f"Number of ticks retrieved: {len(ticks)}")
                if not ticks:
                    continue

                self.log.info(f"Number of ticks in data: {len(data)}")

                start_date_time, should_continue = self.handle_timestamp_iteration(
                    ticks,
                    start_date_time,
                    end_date_time,
                )

                if not should_continue:
                    break

                data.extend(ticks)

        return data

    def handle_timestamp_iteration(self, ticks, start_date_time, end_date_time):
        if not ticks:
            return start_date_time, False

        timestamps = [unix_nanos_to_dt(tick.ts_event) for tick in ticks]
        min_timestamp = min(timestamps)
        max_timestamp = max(timestamps)

        if min_timestamp.floor("S") == max_timestamp.floor("S"):
            max_timestamp = max_timestamp.floor("S") + pd.Timedelta(seconds=1)

        if max_timestamp >= end_date_time:
            return end_date_time, False

        return max_timestamp, True

    async def fetch_instruments_if_not_cached(self, contracts: list[IBContract]) -> None:
        for contract in contracts:
            instrument_id = ib_contract_to_instrument_id(contract)
            if not self._client._cache.instrument(instrument_id):
                self.log.info(f"Fetching Instrument for: {instrument_id}")
                await self.request_instruments(contracts=[contract])


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
