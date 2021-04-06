# -----------------------------------book--------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import logging
from typing import Dict, List

from betfairlightweight import APIClient
from betfairlightweight.filters import market_filter
import pandas as pd

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.core.time cimport unix_timestamp_ns

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.util import chunk
from nautilus_trader.adapters.betfair.util import flatten_tree
from nautilus_trader.model.instrument import BettingInstrument


logger = logging.getLogger(__name__)



cdef class BetfairInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `BettingInstruments` from the Betfair APIClient.
    """

    def __init__(self, client not None: APIClient, logger: Logger, bint load_all=True, dict market_filter=None):
        """
        Initialize a new instance of the `CCXTInstrumentProvider` class.

        Parameters
        ----------
        client : APIClient
            The client for the provider.
        load_all : bool, optional
            If all instruments should be loaded at instantiation.

        """
        super().__init__()

        self._client = client
        self.market_filter = market_filter or {}
        self._log = LoggerAdapter("BetfairInstrumentProvider", logger)
        self.venue = BETFAIR_VENUE
        self._instruments = {}
        self._cache = {}
        self._searched_filters = set()

        if load_all:
            self._load_instruments()

    cpdef void load_all(self) except *:
        """
        Load all instruments for the venue.
        """
        self._load_instruments()

    cdef void _load_instruments(self, market_filter=None) except *:
        markets = load_markets(self._client, market_filter=market_filter or self.market_filter)
        self._log.info(f"Found {len(markets)} markets with filter: {market_filter}")
        self._log.info(f"Loading metadata for {len(markets)} markets..")
        market_metadata = load_markets_metadata(client=self._client, markets=markets)
        self._log.info(f"Creating {len(market_metadata)} instruments")

        cdef list instruments = [
            instrument
            for metadata in market_metadata.values()
            for instrument in make_instrument(metadata, currency=self.get_account_currency())
        ]
        self._log.info(f"Instruments created")

        for ins in instruments:
            self._instruments[ins.id] = ins

    cpdef void _assert_loaded_instruments(self) except *:
        assert self._instruments, "Instruments empty, has `load_all()` been called?"

    cpdef list search_markets(self, dict market_filter=None):
        """ Search for betfair markets. Useful for debugging / interactive use """
        return load_markets(client=self._client, market_filter=market_filter)

    cpdef list search_instruments(self, dict instrument_filter=None, bint load=True):
        """ Search for instruments within the cache. Useful for debugging / interactive use """
        key = tuple((instrument_filter or {}).items())
        if key not in self._searched_filters and load:
            self._log.info(f"Searching for instruments with filter: {instrument_filter}")
            self._load_instruments(market_filter=instrument_filter)
            self._searched_filters.add(key)
        instruments = [
            ins for ins in self.list_instruments() if all([getattr(ins, k) == v for k, v in instrument_filter.items()])
        ]
        for ins in instruments:
            self._log.debug(f"Found instrument: {ins}")
        return instruments

    cpdef BettingInstrument get_betting_instrument(self, str market_id, str selection_id, str handicap):
        """ Performance friendly instrument lookup """
        key = (market_id, selection_id, handicap)
        if key not in self._cache:
            instrument_filter = {'market_id': market_id, 'selection_id': selection_id, 'selection_handicap': handicap}
            instruments = self.search_instruments(instrument_filter=instrument_filter, load=False)
            count = len(instruments)
            if count < 1:
                self._log.warning(f"Found 0 instrument for filter: {instrument_filter}")
                return
            # assert count == 1, f"Wrong number of instruments: {len(instruments)} for filter: {instrument_filter}"
            self._cache[key] = instruments[0]
        return self._cache[key]

    cpdef list list_instruments(self):
        self._assert_loaded_instruments()
        return list(self._instruments.values())

    cpdef str get_account_currency(self):
        if self._account_currency is None:
            detail = self._client.account.get_account_details()
            self._account_currency = detail['currencyCode']
        return self._account_currency


def _parse_date(s, tz):
    # pd.Timestamp is ~5x faster than datetime.datetime.isoformat here.
    return pd.Timestamp(s, tz=tz).to_pydatetime()


cpdef list make_instrument(dict market_definition, str currency):
    cdef list instruments = []

    # assert market_definition['event']['openDate'] == 'GMT'
    for runner in market_definition["runners"]:
        instrument = BettingInstrument(
            venue_name=BETFAIR_VENUE.value,
            event_type_id=market_definition["eventType"]["id"],
            event_type_name=market_definition["eventType"]["name"],
            competition_id=market_definition.get("competition", {}).get("id", ""),
            competition_name=market_definition.get("competition", {}).get("name", ""),
            event_id=market_definition["event"]["id"],
            event_name=market_definition["event"]["name"].strip(),
            event_country_code=market_definition["event"].get("countryCode", ""),
            event_open_date=_parse_date(
                market_definition["event"]["openDate"], tz=market_definition["event"]["timezone"]
            ),
            betting_type=market_definition["description"]["bettingType"],
            market_id=market_definition["marketId"],
            market_name=market_definition["marketName"],
            market_start_time=_parse_date(
                market_definition["description"]["marketTime"], tz=market_definition["event"]["timezone"]
            ),
            market_type=market_definition["description"]["marketType"],
            selection_id=str(runner["selectionId"]),
            selection_name=runner.get("runnerName"),
            selection_handicap=str(runner.get("hc", runner.get("handicap", ""))),
            currency=currency,
            # TODO - Add the provider, use clock
            timestamp_ns=unix_timestamp_ns()
            # info=market_definition,  # TODO We should probably store a copy of the raw input data
        )
        instruments.append(instrument)
    return instruments


VALID_MARKET_FILTER_KEYS = (
    'event_type_name', 'event_type_id', 'event_name', 'event_id', 'event_countryCode', 'market_name', 'market_id',
    'market_exchangeId', 'market_marketType', 'market_marketStartTime', 'market_numberOfWinners'
)


def load_markets(client: APIClient, market_filter=None):
    if isinstance(market_filter, dict):
        # This code gets called from search instruments which may pass selection_id/handicap which don't exist here,
        # only the market_id is relevant, so we just drop these two fields
        market_filter = {k: v for k, v in market_filter.items() if k not in ("selection_id", "selection_handicap")}
    assert all((k in VALID_MARKET_FILTER_KEYS for k in (market_filter or [])))
    navigation = client.navigation.list_navigation()
    return list(flatten_tree(navigation, **(market_filter or {})))


def load_markets_metadata(client: APIClient, markets: List[Dict]) -> Dict:
    all_results = {}
    for market__id_chunk in chunk([m["market_id"] for m in markets], 50):
        results = client.betting.list_market_catalogue(
            market_projection=[
                "EVENT_TYPE",
                "EVENT",
                "COMPETITION",
                "MARKET_DESCRIPTION",
                "RUNNER_METADATA",
                "RUNNER_DESCRIPTION",
                "MARKET_START_TIME",
            ],
            filter=market_filter(market_ids=market__id_chunk),
            lightweight=True,
            max_results=len(market__id_chunk),
        )
        all_results.update({r["marketId"]: r for r in results})
    return all_results
