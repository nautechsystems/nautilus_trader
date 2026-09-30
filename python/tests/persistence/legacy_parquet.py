# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Builders of legacy Parquet catalog files for migration tests.
"""

import json
from pathlib import Path

import pyarrow as pa


def legacy_table_from_staged_feather(path: Path) -> pa.Table:
    """
    Return a staged Feather file as a legacy Parquet catalog table.

    Legacy files keep the instrument ID in the schema metadata next to the precisions a
    staged file stores per row, and have no identifier column.

    """
    staged = pa.ipc.open_stream(pa.py_buffer(path.read_bytes())).read_all()
    [metadata_json] = set(staged.column("nautilus_metadata_json").to_pylist())
    [identifier] = set(staged.column("identifier").to_pylist())
    table = staged.drop_columns(["identifier", "nautilus_metadata_id", "nautilus_metadata_json"])
    return table.replace_schema_metadata({**json.loads(metadata_json), "instrument_id": identifier})
