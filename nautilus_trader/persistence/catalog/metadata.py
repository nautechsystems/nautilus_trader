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

import orjson

from nautilus_trader.persistence.util import get_catalog_fs


PROCESSED_FILES_FN = ".processed_raw_files.json"


def _load_mappings(fs, fn) -> Dict:
    if not fs.exists(fn):
        return {}
    with fs.open(str(fn / "_partition_mappings.json"), "rb") as f:
        return orjson.loads(f.read())


def _write_mappings(fs, fn, mappings) -> None:
    with fs.open(str(fn / "_partition_mappings.json"), "wb") as f:
        f.write(orjson.dumps(mappings))


# TODO(bm): We should save a hash of the contents alongside the filename to check for changes
def save_processed_raw_files(files: List[str]):
    fs = get_catalog_fs()
    existing = load_processed_raw_files()
    new = set(files + existing)
    with fs.open(PROCESSED_FILES_FN, "wb") as f:
        return f.write(orjson.dumps(sorted(new)))


def load_processed_raw_files():
    fs = get_catalog_fs()
    if fs.exists(PROCESSED_FILES_FN):
        with fs.open(PROCESSED_FILES_FN, "rb") as f:
            return orjson.loads(f.read())
    else:
        return []
