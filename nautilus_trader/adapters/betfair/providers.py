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
from typing import Dict, List, Optional, Set

import pandas as pd

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.client.enums import MarketProjection
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.common import EVENT_TYPE_TO_NAME
from nautilus_trader.adapters.betfair.parsing import parse_handicap
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
        filters: Optional[Dict] = None,
        config: Optional[InstrumentProviderConfig] = None,
    ):
        if config is None:
            config = InstrumentProviderConfig(
                load_all_on_start=True,
                load_ids_on_start=None,
                filters=filters,
            )
        super().__init__(
            venue=BETFAIR_VENUE,
            logger=logger,
            config=config,
        )

        self._client = client
        self._cache: Dict[InstrumentId, BettingInstrument] = {}
        self._account_currency = None
        self._missing_instruments: Set[BettingInstrument] = set()

    async def load_ids_async(
        self,
        instrument_ids: List[InstrumentId],
        filters: Optional[Dict] = None,
    ) -> None:
        raise NotImplementedError()

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: Optional[Dict] = None,
    ):
        raise NotImplementedError()

    @classmethod
    def from_instruments(
        cls,
        instruments: List[Instrument],
        logger: Optional[Logger] = None,
    ):
        logger = logger or Logger(LiveClock())
        instance = cls(client=None, logger=logger)
        instance.add_bulk(instruments)
        return instance

    async def load_all_async(self, market_filter: Optional[Dict] = None):
        currency = await self.get_account_currency()
        market_filter = market_filter or self._filters

        self._log.info(f"Loading markets with market_filter={market_filter}")
        markets = await load_markets(self._client, market_filter=market_filter)

        self._log.info(f"Found {len(markets)} markets, loading metadata")
        market_metadata = await load_markets_metadata(client=self._client, markets=markets)

        self._log.info("Creating instruments..")
        instruments = [
            instrument
            for metadata in market_metadata.values()
            for instrument in make_instruments(metadata, currency=currency)
        ]
        for instrument in instruments:
            self.add(instrument=instrument)

        self._log.info(f"{len(instruments)} Instruments created")

    def load_markets(self, market_filter: Optional[Dict] = None):
        """Search for betfair markets. Useful for debugging / interactive use"""
        return load_markets(client=self._client, market_filter=market_filter)

    def search_instruments(self, instrument_filter: Optional[Dict] = None):
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


def parse_market_definition(market_definition):
    if "marketDefinition" in market_definition:
        market_id = market_definition["id"]
        market_definition = market_definition["marketDefinition"]
        market_definition["marketId"] = market_id

    def _parse_grouped():
        """Parse a market where data is grouped by type (ie keys are {'competition': {'id': 1, 'name': 'NBA')"""
        return {
            "event_type_id": market_definition["eventType"]["id"],
            "event_type_name": market_definition["eventType"]["name"],
            "competition_name": market_definition.get("competition", {}).get("name", ""),
            "competition_id": market_definition.get("competition", {}).get("id", ""),
            "event_id": market_definition["event"]["id"],
            "event_name": market_definition["event"]["name"].strip(),
            "country_code": market_definition["event"].get("countryCode"),
            "event_open_date": pd.Timestamp(
                market_definition["event"]["openDate"], tz=market_definition["event"]["timezone"]
            ),
            "betting_type": market_definition["description"]["bettingType"],
            "market_type": market_definition["description"]["marketType"],
            "market_name": market_definition.get("marketName", ""),
            "market_start_time": pd.Timestamp(market_definition["description"]["marketTime"]),
            "market_id": market_definition["marketId"],
            "runners": [
                {
                    "name": r.get("runnerName") or "NO_NAME",
                    "selection_id": r["selectionId"],
                    "handicap": parse_handicap(r.get("hc", r.get("handicap"))),
                    "sort_priority": r.get("sortPriority"),
                    "runner_id": r.get("metadata", {}).get("runnerId")
                    if str(r.get("metadata", {}).get("runnerId")) != str(r["selectionId"])
                    else None,
                }
                for r in market_definition["runners"]
            ],
        }

    def _parse_top_level():
        """Parse a market where all data is contained at the top-level (ie keys are eventTypeId, competitionId)"""
        return {
            "event_type_id": market_definition["eventTypeId"],
            "event_type_name": market_definition.get(
                "eventTypeName", EVENT_TYPE_TO_NAME[market_definition["eventTypeId"]]
            ),
            "event_id": market_definition["eventId"],
            "event_name": market_definition.get("eventName", ""),
            "event_open_date": pd.Timestamp(
                market_definition["openDate"], tz=market_definition["timezone"]
            ),
            "betting_type": market_definition["bettingType"],
            "country_code": market_definition.get("countryCode"),
            "market_type": market_definition.get("marketType"),
            "market_name": market_definition.get("name", ""),
            "market_start_time": pd.Timestamp(
                market_definition["marketTime"], tz=market_definition["timezone"]
            ),
            "market_id": market_definition["marketId"],
            "runners": [
                {
                    "name": r.get("name") or "NO_NAME",
                    "selection_id": r["id"],
                    "handicap": parse_handicap(r.get("hc")),
                    "sort_priority": r.get("sortPriority"),
                }
                for r in market_definition["runners"]
            ],
        }

    if all(k in market_definition for k in ("eventType", "event")):
        return _parse_grouped()
    else:
        return _parse_top_level()


# TODO: handle short hand market def
def make_instruments(market_definition, currency):
    instruments = []
    market_definition = parse_market_definition(market_definition)

    # assert market_definition['event']['openDate'] == 'GMT'
    for runner in market_definition["runners"]:
        instrument = BettingInstrument(
            venue_name=BETFAIR_VENUE.value,
            event_type_id=market_definition["event_type_id"],
            event_type_name=market_definition["event_type_name"],
            competition_id=market_definition.get("competition_id", ""),
            competition_name=market_definition.get("competition_name", ""),
            event_id=market_definition["event_id"],
            event_name=(market_definition.get("event_name") or "").strip(),
            event_country_code=market_definition.get("country_code") or "",
            event_open_date=market_definition["event_open_date"],
            betting_type=market_definition["betting_type"],
            market_id=market_definition["market_id"],
            market_name=market_definition["market_name"],
            market_start_time=market_definition["market_start_time"],
            market_type=market_definition["market_type"],
            selection_id=str(runner["selection_id"]),
            selection_name=runner["name"],
            selection_handicap=parse_handicap(runner.get("hc", runner.get("handicap"))),
            currency=currency,
            # TODO - Add the provider, use clock
            ts_event=time.time_ns(),  # TODO(bm): Duplicate timestamps for now
            ts_init=time.time_ns(),
            # info=market_definition,  # TODO We should probably store a copy of the raw input data
        )
        instruments.append(instrument)
    return instruments


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


async def load_markets(client: BetfairClient, market_filter: Optional[Dict] = None):
    if isinstance(market_filter, dict):
        # This code gets called from search instruments which may pass selection_id/handicap which don't exist here,
        # only the market_id is relevant, so we just drop these two fields
        market_filter = {
            k: v
            for k, v in market_filter.items()
            if k not in ("selection_id", "selection_handicap")
        }
    assert all((k in VALID_MARKET_FILTER_KEYS for k in (market_filter or [])))
    navigation = await client.list_navigation()
    return list(flatten_tree(navigation, **(market_filter or {})))


async def load_markets_metadata(client: BetfairClient, markets: List[Dict]) -> Dict:
    all_results = {}
    for market_id_chunk in chunk(list(set([m["market_id"] for m in markets])), 50):
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
        all_results.update({r["marketId"]: r for r in results})
    return all_results


def get_market_book(client, market_ids):
    resp = client.betting.list_market_book(
        market_ids=market_ids, price_projection={"priceData": ["EX_TRADED"]}
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
                }
            )
    return pd.DataFrame(data)
