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

import fsspec
import orjson
from fsspec.utils import infer_storage_options


PROCESSED_FILES_FN = ".processed_raw_files.json"
PARTITION_MAPPINGS_FN = "_partition_mappings.json"


def load_mappings(fs, path) -> Dict:
    if not fs.exists(f"{path}/{PARTITION_MAPPINGS_FN}"):
        return {}
    with fs.open(f"{path}/{PARTITION_MAPPINGS_FN}", "rb") as f:
        return orjson.loads(f.read())


def write_partition_column_mappings(fs, path, mappings) -> None:
    with fs.open(f"{path}/{PARTITION_MAPPINGS_FN}", "wb") as f:
        f.write(orjson.dumps(mappings))


# TODO(bm): We should save a hash of the contents alongside the filename to check for changes
def save_processed_raw_files(fs: fsspec.AbstractFileSystem, root: str, files: List[str]):
    existing = load_processed_raw_files(fs=fs)
    new = set(files + existing)
    with fs.open(f"{root}/{PROCESSED_FILES_FN}", "wb") as f:
        return f.write(orjson.dumps(sorted(new)))


def load_processed_raw_files(fs):
    if fs.exists(PROCESSED_FILES_FN):
        with fs.open(PROCESSED_FILES_FN, "rb") as f:
            return orjson.loads(f.read())
    else:
        return []


def _glob_path_to_fs(glob_path):
    inferred = infer_storage_options(glob_path)
    inferred.pop("path", None)
    return fsspec.filesystem(**inferred)
