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

import hashlib
from typing import Dict

import orjson


def flatten_tree(y: Dict, **filters):
    """
    Flatten a nested dict into a list of dicts with each nested level combined
    into a single dict.

    :param y: Dict
    :param filters: Filter keys
    :return:
    """

    results = []
    ignore_keys = ("type", "children")

    def flatten(dict_like, depth=None):
        def _filter(k, v):
            if isinstance(v, str):
                return k == v
            elif isinstance(v, list):
                return k in v
            else:
                raise TypeError

        depth = depth or 0
        node_type = dict_like["type"].lower()
        data = {f"{node_type}_{k}": v for k, v in dict_like.items() if k not in ignore_keys}
        if "children" in dict_like:
            for child in dict_like["children"]:
                for child_data in flatten(child, depth=depth + 1):
                    if depth == 0:
                        if all(_filter(child_data[k], v) for k, v in filters.items()):
                            results.append(child_data)
                    else:
                        yield {**data, **child_data}
        else:
            yield data

    list(flatten(y))
    return results


def chunk(list_like, n):
    """
    Yield successive n-sized chunks from l.
    """
    for i in range(0, len(list_like), n):
        yield list_like[i : i + n]


def hash_json(data):
    h = hashlib.sha256(orjson.dumps(data))
    return h.hexdigest()


def one(iterable):
    """
    Stolen from more_itertools.one()
    """
    it = iter(iterable)

    try:
        first_value = next(it)
    except StopIteration as e:
        raise (ValueError("too few items in iterable (expected 1)")) from e

    try:
        second_value = next(it)
    except StopIteration:
        pass
    else:
        msg = f"Expected exactly one item in iterable, but got {first_value}, {second_value}, and perhaps more."
        raise ValueError(msg)

    return first_value


def historical_instrument_provider_loader(instrument_provider, line):
    from nautilus_trader.adapters.betfair.providers import make_instruments

    if instrument_provider is None:
        return

    data = orjson.loads(line)
    # Find instruments in data
    for mc in data.get("mc", []):
        if "marketDefinition" in mc:
            market_def = {**mc["marketDefinition"], **{"marketId": mc["id"]}}
            instruments = make_instruments(market_definition=market_def, currency="GBP")
            instrument_provider.add_instruments(instruments)

    # By this point we should always have some instruments loaded from historical data.
    if not instrument_provider.list_instruments():
        # TODO - Need to add historical search
        raise Exception("No instruments found")
