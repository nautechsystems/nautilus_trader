# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import json
import os
import os.path
import time
from typing import Any

import msgspec


def save_struct_to_file(filepath, obj, force_create=False):
    item = msgspec.to_builtins(obj)
    item_json = json.dumps(item, indent=4)
    # Check if the file already exists, if exists, do not overwrite
    if not force_create and os.path.isfile(filepath):
        return
    with open(filepath, "w", encoding="utf-8") as f:
        f.write(item_json)


def msgspec_bybit_item_save(filename: str, obj: Any) -> None:
    item = msgspec.to_builtins(obj)
    timestamp = round(time.time() * 1000)
    item_json = json.dumps(
        {"retCode": 0, "retMsg": "success", "time": timestamp, "result": item},
        indent=4,
    )
    # Check if the file already exists, if exists, do not overwrite
    if os.path.isfile(filename):
        return
    with open(filename, "w", encoding="utf-8") as f:
        f.write(item_json)
