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
from typing import Dict, List

from betfairlightweight import APIClient
from betfairlightweight.filters import market_filter
import pandas as pd

from nautilus_trader.model.instrument import BettingInstrument


VENUE = "betfair"


def filter_type(root, filter_value):
    for child in root["children"]:
        if child["type"] == filter_value:
            yield child
        elif "children" in child:
            yield from filter_type(child, filter_value)


def flatten_tree(y, **filters):
    results = []
    ignore_keys = ("type", "children")

    def flatten(dict_like, depth=None):
        depth = depth or 0
        node_type = dict_like["type"].lower()
        data = {
            f"{node_type}_{k}": v for k, v in dict_like.items() if k not in ignore_keys
        }
        if "children" in dict_like:
            for child in dict_like["children"]:
                for child_data in flatten(child, depth=depth + 1):
                    if depth == 0:
                        if all(child_data[k] == v for k, v in filters.items()):
                            results.append(child_data)
                    else:
                        yield {**data, **child_data}
        else:
            yield data

    list(flatten(y))
    return results


def load_markets(client, filter=None):
    navigation = client.navigation.list_navigation()
    return list(flatten_tree(navigation, **(filter or {})))


def chunk(list_like, n):
    """Yield successive n-sized chunks from l."""
    for i in range(0, len(list_like), n):
        yield list_like[i: i + n]


def load_markets_metadata(client: APIClient, markets: List[Dict]):
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


def make_instrument(market_definition):
    def _parse_date(s):
        # pd.Timestamp is ~5x faster than datetime.datetime.isoformat here.
        return pd.Timestamp(
            s, tz=market_definition["event"]["timezone"]
        ).to_pydatetime()

    # assert market_definition['event']['openDate'] == 'GMT'
    for runner in market_definition["runners"]:
        yield BettingInstrument(
            venue_name=VENUE,
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
            market_start_time=_parse_date(
                market_definition["description"]["marketTime"]
            ),
            market_type=market_definition["description"]["marketType"],
            selection_id=str(runner["selectionId"]),
            selection_name=runner.get("runnerName"),
            selection_handicap=str(runner.get("hc", runner.get("handicap", ""))),
            # info=market_definition,  # TODO We should probably store a copy of the raw input data
        )


def search(root, *terms):
    level_search, remaining_terms = terms[0], terms[1:]
    for child in root["children"]:
        if level_search.lower() in child["name"].lower():
            if not len(remaining_terms):
                yield child
            elif "children" in child:
                yield from search(child, *remaining_terms)


# def filter_markets(nav_results, *search_terms):
#     category = next(search(nav_results, *search_terms))
#     listed_games = filter_type(category, "EVENT")
#     game_markets = filter_type({"children": listed_games}, "MARKET")
