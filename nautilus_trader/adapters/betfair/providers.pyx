# -------------------------------------------------------------------------------------------------
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

from nautilus_trader.common.providers cimport InstrumentProvider

from typing import Dict, List

from betfairlightweight import APIClient
from betfairlightweight.filters import market_filter
import pandas as pd

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.util import chunk
from nautilus_trader.adapters.betfair.util import flatten_tree
from nautilus_trader.model.instrument import BettingInstrument


logger = logging.getLogger(__name__)



cdef class BetfairInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects from a unified CCXT exchange.
    """

    def __init__(self, client not None: APIClient, bint load_all=True, dict market_filter=None):
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
        self.venue = BETFAIR_VENUE
        self._instruments = {}

        if load_all:
            self.load_all()

    cpdef void load_all(self) except *:
        """
        Load all instruments for the venue.
        """
        self._load_instruments()

    cdef void _load_instruments(self) except *:
        """
        Load available BettingInstruments from Betfair. The full list of fields available are:

        :param market_filters: A list of filters to apply before requesting instrument metadata.
            Example:
                _load_instruments(market_filters={"event_type_name": "Basketball", "betting_type": "MATCH_ODDS"})
            The full list of fields available are:
                - event_type_name
                - event_type_id
                - event_name
                - event_id
                - event_countryCode
                - market_name
                - market_id
                - market_exchangeId
                - market_marketType
                - market_marketStartTime
                - market_numberOfWinners
        :return:
        """
        cdef str k
        cdef dict v
        cdef list instruments = load_instruments(client=self._client, market_filter=self.market_filter)

        for ins in instruments:
            self._instruments[ins.id] = ins

    cpdef void _assert_loaded_instruments(self) except *:
        assert self._instruments, "Instruments empty, has `load_all()` been called?"

    cpdef list search_markets(self, dict market_filter=None):
        """ Search for betfair markets. Useful for debugging / interactive use """
        return load_markets(client=self._client, market_filter=market_filter)

    cpdef list search_instruments(self, dict instrument_filter=None):
        """ Search for instruments within the cache. Useful for debugging / interactive use """
        self._assert_loaded_instruments()
        return [
            ins for ins in self.list_instruments() if all([getattr(ins, k) == v for k, v in instrument_filter.items()])
        ]

    cpdef list list_instruments(self):
        self._assert_loaded_instruments()
        return list(self._instruments.values())


def load_markets(client: APIClient, market_filter=None):
    navigation = client.navigation.list_navigation()
    return list(flatten_tree(navigation, **(market_filter or {})))


def load_markets_metadata(client: APIClient, markets: List[Dict]) -> Dict:
    all_results = {}
    for market__id_chunk in chunk([m["market_id"] for m in markets], 50):
        logger.info("Loading ")
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


def make_instrument(market_definition: Dict) -> BettingInstrument:
    def _parse_date(s):
        # pd.Timestamp is ~5x faster than datetime.datetime.isoformat here.
        return pd.Timestamp(
            s, tz=market_definition["event"]["timezone"]
        ).to_pydatetime()

    # assert market_definition['event']['openDate'] == 'GMT'
    for runner in market_definition["runners"]:
        yield BettingInstrument(
            venue_name=BETFAIR_VENUE.value,
            event_type_id=market_definition["eventType"]["id"],
            event_type_name=market_definition["eventType"]["name"],
            competition_id=market_definition.get("competition", {}).get("id", ""),
            competition_name=market_definition.get("competition", {}).get("name", ""),
            event_id=market_definition["event"]["id"],
            event_name=market_definition["event"]["name"].strip(),
            event_country_code=market_definition["event"].get("countryCode", ""),
            event_open_date=_parse_date(market_definition["event"]["openDate"]),
            betting_type=market_definition["description"]["bettingType"],
            market_id=market_definition["marketId"],
            market_name=market_definition["marketName"],
            market_start_time=_parse_date(market_definition["description"]["marketTime"]),
            market_type=market_definition["description"]["marketType"],
            selection_id=str(runner["selectionId"]),
            selection_name=runner.get("runnerName"),
            selection_handicap=str(runner.get("hc", runner.get("handicap", ""))),
            # info=market_definition,  # TODO We should probably store a copy of the raw input data
        )


def load_instruments(client: APIClient, market_filter=None):
    markets = load_markets(client, market_filter=market_filter)
    market_metadata = load_markets_metadata(client=client, markets=markets)
    return [
        instrument
        for metadata in market_metadata.values()
        for instrument in make_instrument(metadata)
    ]
