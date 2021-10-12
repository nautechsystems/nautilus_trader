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
#
#  Heavily refactored from MIT licensed github.com/binance/binance-connector-python
#  Original author: Jeremy https://github.com/2pd
# -------------------------------------------------------------------------------------------------

import json
from urllib.parse import urlencode


def clean_none_value(d) -> dict:
    out = {}
    for k in d.keys():
        if d[k] is not None:
            out[k] = d[k]
    return out


def encoded_string(query):
    return urlencode(query, True).replace("%40", "@").replace("%2F", "/")


def convert_list_to_json_array(symbols):
    if symbols is None:
        return symbols
    res = json.dumps(symbols)
    return res.replace(" ", "")
