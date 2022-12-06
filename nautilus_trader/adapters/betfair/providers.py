# -----------------------------------book--------------------------------------------------------------
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
import time
from typing import Optional, Union

import msgspec.json
import pandas as pd
from betfair_parser.spec.api.markets import MarketCatalog
from betfair_parser.spec.api.markets import NavigationMarket
from betfair_parser.spec.streaming.mcm import MarketDefinition

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.client.enums import MarketProjection
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.parsing.requests import parse_handicap
from nautilus_trader.adapters.betfair.util import chunk
from nautilus_trader.adapters.betfair.util import flatten_tree
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.betting import BettingInstrument


class BetfairInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `BettingInstruments` from the Betfair APIClient.

    Parameters
    ----------
    client : BetfairClient, optional
        The client for the provider.
    logger : Logger
        The logger for the provider.
    config : InstrumentProviderConfig, optional
        The configuration for the provider.
    """

    def __init__(
        self,
        client: Optional[BetfairClient],
        logger: Logger,
        filters: Optional[dict] = None,
        config: Optional[InstrumentProviderConfig] = None,
    ):
        if config is None:
            config = InstrumentProviderConfig(
                load_all=True,
                filters=filters,
            )
        super().__init__(
            venue=BETFAIR_VENUE,
            logger=logger,
            config=config,
        )

        self._client = client
        self._cache: dict[InstrumentId, BettingInstrument] = {}
        self._account_currency = None
        self._missing_instruments: set[BettingInstrument] = set()

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: Optional[dict] = None,
    ) -> None:
        raise NotImplementedError()

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: Optional[dict] = None,
    ):
        raise NotImplementedError()

    @classmethod
    def from_instruments(
        cls,
        instruments: list[Instrument],
        logger: Optional[Logger] = None,
    ):
        logger = logger or Logger(LiveClock())
        instance = cls(client=None, logger=logger)
        instance.add_bulk(instruments)
        return instance

    async def load_all_async(self, market_filter: Optional[dict] = None):
        currency = await self.get_account_currency()
        market_filter = market_filter or self._filters

        self._log.info(f"Loading markets with market_filter={market_filter}")
        markets = await load_markets(self._client, market_filter=market_filter)

        self._log.info(f"Found {len(markets)} markets, loading metadata")
        market_metadata = await load_markets_metadata(client=self._client, markets=markets)

        self._log.info("Creating instruments..")
        instruments = [
            instrument
            for metadata in market_metadata
            for instrument in make_instruments(metadata, currency=currency)
        ]
        for instrument in instruments:
            self.add(instrument=instrument)

        self._log.info(f"{len(instruments)} Instruments created")

    def load_markets(self, market_filter: Optional[dict] = None):
        """Search for betfair markets. Useful for debugging / interactive use"""
        return load_markets(client=self._client, market_filter=market_filter)

    def search_instruments(self, instrument_filter: Optional[dict] = None):
        """Search for instruments within the cache. Useful for debugging / interactive use"""
        instruments = self.list_all()
        if instrument_filter:
            instruments = [
                ins
                for ins in instruments
                if all([getattr(ins, k) == v for k, v in instrument_filter.items()])
            ]
        return instruments

    def get_betting_instrument(
        self,
        market_id: str,
        selection_id: str,
        handicap: str,
    ) -> BettingInstrument:
        """Return a betting instrument with performance friendly lookup."""
        key = (market_id, selection_id, handicap)
        if key not in self._cache:
            instrument_filter = {
                "market_id": market_id,
                "selection_id": selection_id,
                "selection_handicap": parse_handicap(handicap),
            }
            instruments = self.search_instruments(instrument_filter=instrument_filter)
            count = len(instruments)
            if count < 1:
                key = (market_id, selection_id, parse_handicap(handicap))
                if key not in self._missing_instruments:
                    self._log.warning(f"Found 0 instrument for filter: {instrument_filter}")
                    self._missing_instruments.add(key)
                return
            # assert count == 1, f"Wrong number of instruments: {len(instruments)} for filter: {instrument_filter}"
            self._cache[key] = instruments[0]
        return self._cache[key]

    async def get_account_currency(self) -> str:
        if self._account_currency is None:
            detail = await self._client.get_account_details()
            self._account_currency = detail["currencyCode"]
        return self._account_currency


def _parse_date(s, tz):
    # pd.Timestamp is ~5x faster than datetime.datetime.isoformat here.
    return pd.Timestamp(s, tz=tz)


def market_catalog_to_instruments(
    market_catalog: MarketCatalog,
    currency: str,
) -> list[BettingInstrument]:
    instruments: list[BettingInstrument] = []
    for runner in market_catalog.runners:
        instrument = BettingInstrument(
            venue_name=BETFAIR_VENUE.value,
            event_type_id=market_catalog.eventType.id,
            event_type_name=market_catalog.eventType.name,
            competition_id=market_catalog.competition_id,
            competition_name=market_catalog.competition_name,
            event_id=market_catalog.event.id,
            event_name=market_catalog.event.name,
            event_country_code=market_catalog.event.countryCode or "",
            event_open_date=pd.Timestamp(market_catalog.event.openDate),
            betting_type=market_catalog.description.bettingType,
            market_id=market_catalog.marketId,
            market_name=market_catalog.marketName,
            market_start_time=pd.Timestamp(market_catalog.marketStartTime),
            market_type=market_catalog.description.marketType,
            selection_id=str(runner.runner_id),
            selection_name=runner.runner_name,
            selection_handicap=parse_handicap(runner.handicap),
            currency=currency,
            ts_event=time.time_ns(),
            ts_init=time.time_ns(),
            info=msgspec.json.decode(msgspec.json.encode(market_catalog)),
        )
        instruments.append(instrument)
    return instruments


def market_definition_to_instruments(
    market_definition: MarketDefinition,
    currency: str,
) -> list[BettingInstrument]:
    instruments: list[BettingInstrument] = []
    for runner in market_definition.runners:
        instrument = BettingInstrument(
            venue_name=BETFAIR_VENUE.value,
            event_type_id=market_definition.eventTypeId,
            event_type_name=market_definition.event_type_name,
            competition_id=market_definition.competitionId,
            competition_name=market_definition.competitionName,
            event_id=market_definition.eventId,
            event_name=market_definition.eventName,
            event_country_code=market_definition.countryCode,
            event_open_date=pd.Timestamp(market_definition.openDate),
            betting_type=market_definition.bettingType,
            market_id=market_definition.marketId,
            market_name=market_definition.marketName,
            market_start_time=pd.Timestamp(market_definition.marketStartTime)
            if market_definition.marketStartTime
            else pd.Timestamp(0, tz="UTC"),
            market_type=market_definition.marketType,
            selection_id=str(runner.selectionId or runner.id),
            selection_name=runner.name or "",
            selection_handicap=parse_handicap(runner.hc),
            currency=currency,
            ts_event=time.time_ns(),
            ts_init=time.time_ns(),
            info=msgspec.json.decode(msgspec.json.encode(market_definition)),
        )
        instruments.append(instrument)
    return instruments


def make_instruments(
    market: Union[MarketCatalog, MarketDefinition],
    currency: str,
) -> list[BettingInstrument]:
    if isinstance(market, MarketCatalog):
        return market_catalog_to_instruments(market, currency)
    elif isinstance(market, MarketDefinition):
        return market_definition_to_instruments(market, currency)
    else:
        raise TypeError(type(market))


VALID_MARKET_FILTER_KEYS = (
    "event_type_name",
    "event_type_id",
    "event_name",
    "event_id",
    "event_countryCode",
    "market_name",
    "market_id",
    "market_exchangeId",
    "market_marketType",
    "market_marketStartTime",
    "market_numberOfWinners",
)


async def load_markets(
    client: BetfairClient,
    market_filter: Optional[dict] = None,
) -> list[NavigationMarket]:
    if isinstance(market_filter, dict):
        # This code gets called from search instruments which may pass selection_id/handicap which don't exist here,
        # only the market_id is relevant, so we just drop these two fields
        market_filter = {
            k: v
            for k, v in market_filter.items()
            if k not in ("selection_id", "selection_handicap")
        }
    assert all(k in VALID_MARKET_FILTER_KEYS for k in (market_filter or []))
    navigation = await client.list_navigation()
    markets = list(flatten_tree(navigation, **(market_filter or {})))
    return [
        msgspec.json.decode(msgspec.json.encode(market), type=NavigationMarket)
        for market in markets
    ]


def parse_market_catalog(catalog: list[dict]) -> list[MarketCatalog]:
    return [msgspec.json.decode(msgspec.json.encode(r), type=MarketCatalog) for r in catalog]


async def load_markets_metadata(
    client: BetfairClient,
    markets: list[NavigationMarket],
) -> list[MarketCatalog]:
    all_results = []
    for market_id_chunk in chunk(list({m.market_id for m in markets}), 50):
        results = await client.list_market_catalogue(
            market_projection=[
                MarketProjection.EVENT_TYPE,
                MarketProjection.EVENT,
                MarketProjection.COMPETITION,
                MarketProjection.MARKET_DESCRIPTION,
                MarketProjection.RUNNER_METADATA,
                MarketProjection.RUNNER_DESCRIPTION,
                MarketProjection.MARKET_START_TIME,
            ],
            filter_={"marketIds": market_id_chunk},
            max_results=len(market_id_chunk),
        )
        all_results.extend(results)
    return parse_market_catalog(all_results)


def get_market_book(client, market_ids):
    resp = client.betting.list_market_book(
        market_ids=market_ids,
        price_projection={"priceData": ["EX_TRADED"]},
    )
    data = []
    for market in resp:
        for runner in market["runners"]:
            data.append(
                {
                    "market_id": market["marketId"],
                    "selection_id": runner["selectionId"],
                    "market_matched": market["totalMatched"],
                    "market_status": market["status"],
                    "selection_status": runner["status"],
                    "selection_matched": runner.get("totalMatched"),
                    "selection_last_price": runner.get("lastPriceTraded"),
                },
            )
    return pd.DataFrame(data)
