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

from typing import Dict

import fsspec
import msgspec
from fsspec.utils import infer_storage_options


PARTITION_MAPPINGS_FN = "_partition_mappings.json"


def load_mappings(fs, path) -> Dict:
    if not fs.exists(f"{path}/{PARTITION_MAPPINGS_FN}"):
        return {}
    with fs.open(f"{path}/{PARTITION_MAPPINGS_FN}", "rb") as f:
        return msgspec.json.decode(f.read())


def write_partition_column_mappings(fs, path, mappings) -> None:
    with fs.open(f"{path}/{PARTITION_MAPPINGS_FN}", "wb") as f:
        f.write(msgspec.json.encode(mappings))


def _glob_path_to_fs(glob_path):
    inferred = infer_storage_options(glob_path)
    inferred.pop("path", None)
    return fsspec.filesystem(**inferred)
