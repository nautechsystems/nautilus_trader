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
import re
from typing import Dict, List, Optional

import pandas as pd

from nautilus_trader.model.data.base import Data


INVALID_WINDOWS_CHARS = r'<>:"/\|?* '

GENERIC_DATA_PREFIX = "genericdata_"


def list_dicts_to_dict_lists(dicts, keys=None):
    """
    Convert a list of dictionaries into a dictionary of lists
    """
    result = {}
    for d in dicts:
        for k in keys or d:
            if k not in result:
                result[k] = [d.get(k)]
            else:
                result[k].append(d.get(k))
    return result


def identity(x):
    """
    The identity function
    """
    return x


def maybe_list(obj):
    if isinstance(obj, dict):
        return [obj]
    return obj


def is_nautilus_class(cls):
    """
    Determine whether a class belongs to nautilus_trader
    """
    is_nautilus_paths = cls.__module__.startswith("nautilus_trader.")
    if not is_nautilus_paths:
        # This object is defined outside of nautilus, definitely custom
        return False
    else:
        is_data_subclass = issubclass(cls, Data)
        is_nautilus_builtin = any(
            (cls.__module__.startswith(p) for p in ("nautilus_trader.model",))
        )
        return is_data_subclass and is_nautilus_builtin


def check_partition_columns(
    df: pd.DataFrame, partition_columns: Optional[List[str]]
) -> Dict[str, Dict[str, str]]:
    """
    When writing a parquet dataset, parquet uses the values in `partition_columns` as part of the filename. The values
    in `df` could potentially contain illegal characters. This function generates a mapping of {illegal: legal} that is
    used to "clean" the values before they are written to the filename (and also saving this mapping for reversing the
    process on reload)
    """
    if partition_columns:
        missing = [c for c in partition_columns if c not in df.columns]
        assert (
            not missing
        ), f"Missing `partition_columns`: {missing} in dataframe columns: {df.columns}"

    mappings = {}
    for col in partition_columns or []:
        values = list(map(str, df[col].unique()))
        invalid_values = {val for val in values if any(x in val for x in INVALID_WINDOWS_CHARS)}
        if invalid_values:
            if col == "instrument_id":
                # We have control over how instrument_ids are retrieved from the cache, so we can do this replacement
                val_map = {k: clean_key(k) for k in values}
                mappings[col] = val_map
            else:
                # We would be arbitrarily replacing values here which could break queries, we should not do this.
                raise ValueError(
                    f"Some values in partition column [{col}] contain invalid characters: {invalid_values}"
                )

    return mappings


def clean_partition_cols(df, mappings: Dict[str, Dict[str, str]]):
    """
    The values in `partition_cols` may have characters that are illegal in filenames. Strip them out and return a
    dataframe we can write into a parquet file.
    """
    for col, val_map in mappings.items():
        df.loc[:, col] = df[col].map(val_map)
    return df


def clean_key(s):
    """
    Clean characters that are illegal on windows from the string `s`
    """
    for ch in INVALID_WINDOWS_CHARS:
        if ch in s:
            s = s.replace(ch, "-")
    return s


def camel_to_snake_case(s):
    return re.sub(r"((?<=[a-z0-9])[A-Z]|(?!^)[A-Z](?=[a-z]))", r"_\1", s).lower()


def class_to_filename(cls):
    name = f"{camel_to_snake_case(cls.__name__)}"
    if not is_nautilus_class(cls):
        name = f"{GENERIC_DATA_PREFIX}{camel_to_snake_case(cls.__name__)}"
    return name
