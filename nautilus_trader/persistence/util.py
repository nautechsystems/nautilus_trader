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
import inspect


# import os
# import pathlib
# from functools import lru_cache
#
# import fsspec.utils

# KEY = "NAUTILUS_CATALOG"
# _CATALOGS = {}


# def _path() -> str:
#     if KEY not in os.environ:
#         raise KeyError(f"`{KEY}` env variable not set")
#     return os.environ[KEY]
#
#
# @lru_cache(1)
# def get_catalog_fs() -> fsspec.AbstractFileSystem:
#     url = _path()
#     protocol = fsspec.utils.get_protocol(url)
#     return fsspec.filesystem(
#         protocol=protocol,
#     )
#
#
# def get_catalog_root() -> pathlib.Path:
#     fs = get_catalog_fs()
#     url = _path()
#     protocol = fsspec.utils.get_protocol(url)
#     root = pathlib.Path(url.replace(f"{protocol}://", ""))
#     assert fs.exists(str(root))
#     for folder in ("data",):
#         fs.mkdirs(path=f"{root}/{folder}", exist_ok=True)
#     return root


def resolve_kwargs(func, *args, **kwargs):
    sig = inspect.signature(func)
    bound_args = sig.bind(*args, **kwargs)
    bound_args.apply_defaults()
    return bound_args.kwargs


class Singleton(type):
    def __init__(cls, name, bases, dict):
        super(Singleton, cls).__init__(name, bases, dict)
        cls._instances = {}

    def __call__(cls, *args, **kw):
        full_kwargs = resolve_kwargs(cls, *args, **kw)
        key = tuple(full_kwargs.items())
        if key not in cls._instances:
            cls._instances[key] = super(Singleton, cls).__call__(*args, **kw)
        return cls._instances[key]
