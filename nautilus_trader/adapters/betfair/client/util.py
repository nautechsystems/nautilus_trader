# -------------------------------------------------------------------------------------------------
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


def parse_market_filter(market_filter):
    string_keys = ("textQuery",)
    bool_keys = ("bspOnly", "turnInPlayEnabled", "inPlayOnly")
    list_string_keys = (
        "exchangeIds",
        "eventTypeIds",
        "eventIds",
        "competitionIds",
        "marketIds",
        "venues",
        "marketBettingTypes",
        "marketCountries",
        "marketTypeCodes",
        "withOrders",
        "raceTypes",
    )
    for key in string_keys:
        if key not in market_filter:
            continue
        # Condition.type(market_filter[key], str, key)
        assert isinstance(market_filter[key], str), f"{key} should be type `str` not {type(key)}"
    for key in bool_keys:
        if key not in market_filter:
            continue
        # Condition.type(market_filter[key], bool, key)
        assert isinstance(market_filter[key], bool), f"{key} should be type `bool` not {type(key)}"
    for key in list_string_keys:
        if key not in market_filter:
            continue
        # Condition.list_type(market_filter[key], str, key)
        assert isinstance(market_filter[key], list), f"{key} should be type `list` not {type(key)}"
        for v in market_filter[key]:
            assert isinstance(v, str), f"{v} should be type `str` not {type(v)}"
    return market_filter


def snake_to_camel_case(s):
    """
    Convert a snakecase string to camel case.

    Examples
    --------
    >>> snake_to_camel_case('bet_status')
    'betStatus'

    >>> snake_to_camel_case("customer_strategy_refs")
    'customerStrategyRefs'

    >>> snake_to_camel_case("filter_")
    'filter'

    """
    components = s.split("_")
    return components[0] + "".join(x.title() for x in components[1:])


def parse_params(**kw):
    return {
        snake_to_camel_case(k): v for k, v in kw.items() if v is not None and k not in ("self",)
    }
