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

import hashlib
import inspect
from typing import Dict, Union

import cloudpickle


def freeze_dict(dict_like: Dict):
    return tuple(sorted(dict_like.items()))


def check_value(v):
    if isinstance(v, dict):
        return freeze_dict(dict_like=v)
    return v


def resolve_kwargs(func, *args, **kwargs):
    kw = inspect.getcallargs(func, *args, **kwargs)
    return {k: check_value(v) for k, v in kw.items()}


class Singleton(type):
    """
    The base class to ensure a singleton.
    """

    def __init__(cls, name, bases, dict_like):
        super(Singleton, cls).__init__(name, bases, dict_like)
        cls._instances = {}

    def __call__(cls, *args, **kw):
        full_kwargs = resolve_kwargs(cls.__init__, None, *args, **kw)
        if full_kwargs == {"self": None, "args": (), "kwargs": {}}:
            full_kwargs = {}
        full_kwargs.pop("self", None)
        key = tuple(full_kwargs.items())
        if key not in cls._instances:
            cls._instances[key] = super(Singleton, cls).__call__(*args, **kw)
        return cls._instances[key]


def clear_singleton_instances(cls: type):
    assert isinstance(cls, Singleton)
    cls._instances = {}


def tokenize(obj: object) -> str:
    value = cloudpickle.dumps(obj)
    return hashlib.sha256(value).hexdigest()


# Taken from https://github.com/dask/dask/blob/261bf174931580230717abca93fe172e166cc1e8/dask/utils.py
byte_sizes = {
    "kB": 10**3,
    "MB": 10**6,
    "GB": 10**9,
    "TB": 10**12,
    "PB": 10**15,
    "KiB": 2**10,
    "MiB": 2**20,
    "GiB": 2**30,
    "TiB": 2**40,
    "PiB": 2**50,
    "B": 1,
    "": 1,
}
byte_sizes = {k.lower(): v for k, v in byte_sizes.items()}
byte_sizes.update({k[0]: v for k, v in byte_sizes.items() if k and "i" not in k})
byte_sizes.update({k[:-1]: v for k, v in byte_sizes.items() if k and "i" in k})


def parse_bytes(s: Union[float, str]) -> int:
    if isinstance(s, (int, float)):
        return int(s)
    s = s.replace(" ", "")
    if not any(char.isdigit() for char in s):
        s = "1" + s

    for i in range(len(s) - 1, -1, -1):
        if not s[i].isalpha():
            break
    index = i + 1

    prefix = s[:index]
    suffix = s[index:]

    try:
        n = float(prefix)
    except ValueError as e:
        raise ValueError(f"Could not interpret '{prefix}' as a number") from e

    try:
        multiplier = byte_sizes[suffix.lower()]
    except KeyError as e:
        raise ValueError(f"Could not interpret '{suffix}' as a byte unit") from e

    result = n * multiplier
    return int(result)
