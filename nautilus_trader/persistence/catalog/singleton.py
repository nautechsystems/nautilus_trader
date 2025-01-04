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

from __future__ import annotations

import inspect
from typing import Any


class Singleton(type):
    """
    The base class to ensure a singleton.
    """

    def __init__(cls, name, bases, dict_like):
        super().__init__(name, bases, dict_like)
        cls._instances = {}

    def __call__(cls, *args, **kw):
        full_kwargs = resolve_kwargs(cls.__init__, None, *args, **kw)
        if full_kwargs == {"self": None, "args": (), "kwargs": {}}:
            full_kwargs = {}
        full_kwargs.pop("self", None)
        key = tuple(full_kwargs.items())
        if key not in cls._instances:
            cls._instances[key] = super().__call__(*args, **kw)
        return cls._instances[key]


def clear_singleton_instances(cls: type) -> None:
    assert isinstance(cls, Singleton)
    cls._instances = {}


def resolve_kwargs(func, *args, **kwargs):
    kw = inspect.getcallargs(func, *args, **kwargs)
    return {k: check_value(v) for k, v in kw.items()}


def check_value(v: Any) -> Any:
    if isinstance(v, dict):
        return freeze_dict(dict_like=v)
    return v


def freeze_dict(dict_like: dict) -> tuple:
    return tuple(sorted((k, check_value(v)) for k, v in dict_like.items()))
