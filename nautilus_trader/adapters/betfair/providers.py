# -----------------------------------book--------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from collections.abc import Iterable

import msgspec
import pandas as pd
from betfair_parser.spec.betting.enums import MarketProjection
from betfair_parser.spec.betting.type_definitions import MarketCatalogue
from betfair_parser.spec.betting.type_definitions import MarketFilter
from betfair_parser.spec.common import decode as bf_decode
from betfair_parser.spec.common import encode as bf_encode
from betfair_parser.spec.navigation import FlattenedMarket
from betfair_parser.spec.navigation import Navigation
from betfair_parser.spec.navigation import flatten_nav_tree
from betfair_parser.spec.streaming import MarketDefinition

from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.adapters.betfair.common import BETFAIR_TICK_SCHEME
from nautilus_trader.adapters.betfair.constants import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.parsing.common import chunk
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.instruments.betting import null_handicap


class BetfairInstrumentProviderConfig(InstrumentProviderConfig, frozen=True, kw_only=True):
    account_currency: str
    event_type_ids: list[str] | None = None
    event_ids: list[str] | None = None
    market_ids: list[str] | None = None
    country_codes: list[str] | None = None
    market_types: list[str] | None = None
    event_type_names: list[str] | None = None


class BetfairInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `BettingInstruments` from the Betfair APIClient.

    Parameters
    ----------
    client : BetfairClient, optional
        The client for the provider.
    config : InstrumentProviderConfig, optional
        The configuration for the provider.

    """

    def __init__(
        self,
        client: BetfairHttpClient | None,
        config: BetfairInstrumentProviderConfig,
    ):
        assert config is not None, "Must pass config to BetfairInstrumentProvider"
        super().__init__(config=config)

        self._config = config
        self._client = client
        self._account_currency = config.account_currency

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        raise NotImplementedError

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ):
        raise NotImplementedError

    async def load_all_async(self, filters: dict | None = None):
        currency = await self.get_account_currency()
        filters = filters or {}

        self._log.info(f"Loading markets with market_filter={self._config}")
        markets: list[FlattenedMarket] = await load_markets(
            self._client,
            event_type_ids=filters.get("event_type_ids") or self._config.event_type_ids,
            event_ids=filters.get("event_ids") or self._config.event_ids,
            market_ids=filters.get("market_ids") or self._config.market_ids,
            event_country_codes=filters.get("country_codes") or self._config.country_codes,
            market_market_types=filters.get("market_types") or self._config.market_types,
            event_type_names=filters.get("event_type_names") or self._config.event_type_names,
        )

        self._log.info(f"Found {len(markets)} markets, loading metadata")
        market_metadata = await load_markets_metadata(client=self._client, markets=markets)

        self._log.info("Creating instruments..")
        instruments = [
            instrument
            for metadata in market_metadata
            for instrument in make_instruments(metadata, currency=currency, ts_event=0, ts_init=0)
        ]
        for instrument in instruments:
            self.add(instrument=instrument)

        self._log.info(f"{len(instruments)} Instruments created")

    async def get_account_currency(self) -> str:
        if self._account_currency is None:
            detail = await self._client.get_account_details()
            self._account_currency = detail.currency_code
        return self._account_currency


def _parse_date(s, tz):
    # pd.Timestamp is ~5x faster than datetime.datetime.isoformat here.
    return pd.Timestamp(s, tz=tz)


def market_catalog_to_instruments(
    market_catalog: MarketCatalogue,
    currency: str,
    ts_event: int,
    ts_init: int,
) -> list[BettingInstrument]:
    instruments: list[BettingInstrument] = []
    for runner in market_catalog.runners:
        instrument = BettingInstrument(
            venue_name=BETFAIR_VENUE.value,
            event_type_id=market_catalog.event_type.id,
            event_type_name=market_catalog.event_type.name,
            competition_id=market_catalog.competition.id if market_catalog.competition else 0,
            competition_name=market_catalog.competition.name if market_catalog.competition else "",
            event_id=market_catalog.event.id,
            event_name=market_catalog.event.name,
            event_country_code=market_catalog.event.country_code or "",
            event_open_date=pd.Timestamp(market_catalog.event.open_date),
            betting_type=market_catalog.description.betting_type.name,
            market_id=market_catalog.market_id,
            market_name=market_catalog.market_name,
            market_start_time=pd.Timestamp(market_catalog.market_start_time),
            market_type=market_catalog.description.market_type,
            selection_id=runner.selection_id,
            selection_name=runner.runner_name,
            selection_handicap=runner.handicap or null_handicap(),
            currency=currency,
            tick_scheme_name=BETFAIR_TICK_SCHEME.name,
            price_precision=BETFAIR_PRICE_PRECISION,
            size_precision=BETFAIR_QUANTITY_PRECISION,
            ts_event=ts_event,
            ts_init=ts_init,
            info=msgspec.json.decode(bf_encode(market_catalog).decode()),
        )
        instruments.append(instrument)
    return instruments


def market_definition_to_instruments(
    market_definition: MarketDefinition,
    currency: str,
    ts_event: int,
    ts_init: int,
) -> list[BettingInstrument]:
    instruments: list[BettingInstrument] = []
    for runner in market_definition.runners:
        instrument = BettingInstrument(
            venue_name=BETFAIR_VENUE.value,
            event_type_id=market_definition.event_type_id.value,
            event_type_name=market_definition.event_type_name,
            competition_id=market_definition.competition_id or 0,
            competition_name=market_definition.competition_name or "",
            event_id=market_definition.event_id,
            event_name=market_definition.event_name or "",
            event_country_code=market_definition.country_code,
            event_open_date=pd.Timestamp(market_definition.open_date),
            betting_type=market_definition.betting_type.name,
            market_id=market_definition.market_id,
            market_name=market_definition.market_name or "",
            market_start_time=(
                pd.Timestamp(market_definition.market_time)
                if market_definition.market_time
                else pd.Timestamp(0, tz="UTC")
            ),
            market_type=market_definition.market_type,
            selection_id=runner.id,
            selection_name=runner.name or "",
            selection_handicap=runner.hc or null_handicap(),
            tick_scheme_name=BETFAIR_TICK_SCHEME.name,
            currency=currency,
            price_precision=BETFAIR_PRICE_PRECISION,
            size_precision=BETFAIR_QUANTITY_PRECISION,
            ts_event=ts_event,
            ts_init=ts_init,
            info=msgspec.json.decode(msgspec.json.encode(market_definition)),
        )
        instruments.append(instrument)
    return instruments


def make_instruments(
    market: MarketCatalogue | MarketDefinition,
    currency: str,
    ts_event: int,
    ts_init: int,
) -> list[BettingInstrument]:
    if isinstance(market, MarketCatalogue):
        return market_catalog_to_instruments(
            market,
            currency=currency,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif isinstance(market, MarketDefinition):
        return market_definition_to_instruments(
            market,
            currency=currency,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    else:
        raise TypeError(type(market))


VALID_MARKET_FILTER_KEYS = (
    "event_type_name",
    "event_type_id",
    "event_name",
    "event_id",
    "event_country_code",
    "market_name",
    "market_id",
    "market_exchange_id",
    "market_market_type",
    "market_market_start_time",
    "market_number_of_winners",
)


def check_market_filter_keys(keys: Iterable[str]) -> None:
    for key in keys:
        if key not in VALID_MARKET_FILTER_KEYS:
            raise ValueError(f"Invalid market filter key: {key}")


async def load_markets(
    client: BetfairHttpClient,
    event_type_ids: list[str] | None = None,
    event_ids: list[str] | None = None,
    market_ids: list[str] | None = None,
    event_country_codes: list[str] | None = None,
    market_market_types: list[str] | None = None,
    event_type_names: list[str] | None = None,
) -> list[FlattenedMarket]:
    market_filter = {
        "event_type_id": event_type_ids,
        "event_id": event_ids,
        "market_id": market_ids,
        "market_market_type": market_market_types,
        "event_country_code": event_country_codes,
        "event_type_name": event_type_names,
    }
    market_filter = {k: v for k, v in market_filter.items() if v is not None}
    check_market_filter_keys(market_filter.keys())
    navigation: Navigation = await client.list_navigation()
    markets = flatten_nav_tree(navigation, **market_filter)
    return markets


def parse_market_catalog(catalog: list[dict]) -> list[MarketCatalogue]:
    raw = msgspec.json.encode(catalog)
    return bf_decode(raw, type=list[MarketCatalogue])


async def load_markets_metadata(
    client: BetfairHttpClient,
    markets: list[FlattenedMarket],
) -> list[MarketCatalogue]:
    all_results: list[MarketCatalogue] = []
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
            filter_=MarketFilter(market_ids=market_id_chunk),
            max_results=len(market_id_chunk),
        )
        all_results.extend(results)
    return all_results


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
