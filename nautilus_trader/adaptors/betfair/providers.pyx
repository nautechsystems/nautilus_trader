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


from betfairlightweight import APIClient

from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.model.identifiers cimport Venue

from adaptors.betfair.parsing import load_markets, load_instruments

VENUE = "betfair"


cdef class BetfairInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects from a unified CCXT exchange.
    """

    def __init__(self, client not None: APIClient, bint load_all=False, dict market_filter=None):
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
        self.venue = Venue(VENUE)


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

    cpdef list search_markets(self, dict market_filter=None):
        """ Search for betfair markets. Useful for debugging / interactive use """
        return load_markets(client=self._client, market_filter=market_filter)
